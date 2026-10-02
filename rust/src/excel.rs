//! Reads schema files and data workbooks into a validated data model.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use regex::Regex;

use crate::errors::{ErrorCollector, ValidationErrors};
use crate::headers::link_warnings;
use crate::i18n::tr;
use crate::reader::{read_workbook, Grid, Sheet};
use crate::schema::{
    build_columns, calculate_schema_hash, cell_name, parse_type, ArrayShape, ColumnSchema, EnumSchema, EnumValue, Enums,
    TableKeys, TableSchema, TableSource, DATA_ROW, KEY_ROLE_PREFIX_RE, NAME_ROW, TYPE_ROW,
};
use crate::schemafile::{
    canonical, enum_folder, folder_of, load_schemas, strings_folder, Schema, SCHEMA_SUFFIXES, STRING_TABLE_SUFFIX,
};
use crate::sources::{find_files, is_identifier, table_name_of};
use crate::value::{Cell, Value};
use crate::values::convert_value;

/// A type alias with its resolved type (`ItemID` -> `int32`, through `Ref<Items>`).
#[derive(Clone, Debug)]
pub struct AliasType {
    pub name: String,
    pub type_name: String,
    pub comment: String,
}

pub struct DataModel {
    pub source_files: Vec<String>,
    pub enums: Vec<EnumSchema>,
    pub aliases: Vec<AliasType>,
    pub tables: Vec<TableSchema>,
    pub warnings: Vec<String>,
}

impl DataModel {
    pub fn enum_map(&self) -> Enums {
        self.enums.iter().map(|e| (e.name.clone(), e.clone())).collect()
    }

    /// Tables with row structs and assets (every table except the string tables).
    pub fn data_tables(&self) -> impl Iterator<Item = &TableSchema> {
        self.tables.iter().filter(|t| !t.is_strings())
    }

    pub fn string_tables(&self) -> impl Iterator<Item = &TableSchema> {
        self.tables.iter().filter(|t| t.is_strings())
    }
}

struct SheetRef<'a> {
    file: &'a str,
    grid: &'a Grid,
}

/// Format arguments such as {0} or {Name} in a string.
static PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^{}]*\}").unwrap());
/// `Reward[2]`: one element column of an array field in a data sheet.
static ELEMENT_COLUMN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9_]*)\[(\d+)\]$").unwrap());

impl SheetRef<'_> {
    fn where_(&self) -> String {
        format!("[{}]{}", self.file, self.grid.title)
    }
}

/// Shared across the parts of one table so duplicates are found between files too.
#[derive(Default)]
struct RowState {
    used_keys: HashMap<String, String>,
    /// (field, lowercase value) -> (first spelling, location).
    name_spellings: HashMap<(String, String), (String, String)>,
    /// String tables: empty translations filled from the base language, per language
    /// (count, first cell).
    filled: BTreeMap<String, (usize, String)>,
    warnings: Vec<String>,
}

/// Tables and enums from their schema files, rows from the data workbooks under `input`.
/// The schema folder defaults to the input folder; the enum folder to `Enums` and the string
/// table data folder to `Strings`, both next to the schema folder (inside the input folder when
/// no schema folder is given).
pub fn load_model(
    input: &Path,
    schema_path: Option<&Path>,
    enum_path: Option<&Path>,
    strings_path: Option<&Path>,
) -> Result<DataModel, ValidationErrors> {
    let schema_root = match schema_path {
        Some(path) => path.to_path_buf(),
        None => folder_of(input),
    };
    let mut errors = ErrorCollector::default();
    let mut warnings: Vec<String> = Vec::new();
    // Enums sit next to an explicit schema folder, or inside the data folder when that is the schema folder.
    let mut schemas = load_schemas(&schema_root, &enum_folder(&schema_root, enum_path, schema_path.is_some()), &mut errors)?;
    // Type aliases are replaced in the table schemas before anything reads a type.
    let alias_names = expand_aliases(&mut schemas, &mut errors);
    let not_schema = |(path, _): &(PathBuf, String)| {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        !SCHEMA_SUFFIXES.iter().any(|s| name.ends_with(s))
    };
    let mut files: Vec<(PathBuf, String)> = find_files(input, &[".xlsx"], true)?.into_iter().filter(not_schema).collect();
    // String table data lives in its own folder. Inside the input folder it is found by the scan
    // above; elsewhere it is read too, named relative to the folder that holds it ("Strings/UI.xlsx").
    let strings_root = strings_folder(&schema_root, strings_path, schema_path.is_some());
    let strings_resolved = canonical(&strings_root);
    let in_strings = |path: &Path| canonical(path).starts_with(&strings_resolved);
    let scanned = input.is_dir() && strings_resolved.starts_with(canonical(input));
    if strings_root.is_dir() && !scanned {
        let label = strings_root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        for (path, relative) in find_files(&strings_root, &[".xlsx"], true)?.into_iter().filter(not_schema) {
            files.push((path, format!("{label}/{relative}")));
        }
    }
    // An input folder with nothing in it is almost always a wrong path; building it would
    // replace the generated code with an empty set.
    if files.is_empty() && schemas.all().next().is_none() && errors.messages.is_empty() {
        return Err(ValidationErrors(vec![tr(
            format!("입력!A1: 스키마도 데이터 파일도 없습니다: {}", input.display()),
            format!("input!A1: no schema or data workbook found: {}", input.display()),
        )]));
    }
    let read: Vec<Result<Vec<Sheet>, String>> = files
        .par_iter()
        .map(|(path, _)| read_workbook(path, |title| !title.starts_with('#')))
        .collect();
    let mut workbooks: Vec<(&str, Vec<Sheet>)> = Vec::new();
    for ((_, relative), result) in files.iter().zip(read) {
        match result {
            Ok(sheets) => workbooks.push((relative, sheets)),
            Err(error) => errors.add(&format!("[{relative}]"), "A1", tr(
                format!("xlsx 파일을 읽을 수 없습니다: {error}"),
                format!("cannot read the xlsx file: {error}"),
            )),
        }
    }
    errors.raise_if_any()?;

    // Reference headers that point at a schema path missing on this machine stop updating.
    let schema_files: HashSet<PathBuf> = schemas.all().map(|s| s.path.clone()).collect();
    for (path, relative) in &files {
        warnings.extend(link_warnings(path, relative, &schema_files));
    }

    let strings_files: HashSet<&str> =
        files.iter().filter(|(path, _)| in_strings(path)).map(|(_, relative)| relative.as_str()).collect();
    let mut table_parts: HashMap<String, Vec<SheetRef>> = HashMap::new();
    for (relative, sheets) in &workbooks {
        let in_strings_folder = strings_files.contains(relative);
        for sheet in sheets {
            let where_ = format!("[{relative}]{}", sheet.title);
            if sheet.title.starts_with('#') {
                continue;
            }
            let name = table_name_of(&sheet.title).unwrap_or("");
            if name.is_empty() || !is_identifier(name) {
                errors.add(&where_, "A1", tr(
                    format!("올바르지 않은 테이블 이름 '{}'", sheet.title),
                    format!("invalid table name '{}'", sheet.title),
                ));
                continue;
            }
            // The sheet name is the table name, in the strings folder too (UIString).
            let table = name;
            let folder = strings_root.display();
            let wrong_place = |is_strings: bool| if is_strings {
                tr(
                    format!("스트링테이블 '{name}'의 데이터는 스트링 폴더({folder})에 두어야 합니다"),
                    format!("string table '{name}' data belongs in the strings folder ({folder})"),
                )
            } else {
                tr(
                    format!("일반 테이블 '{name}'의 데이터는 스트링 폴더({folder})에 둘 수 없습니다"),
                    format!("table '{name}' is not a string table; its data cannot be in the strings folder ({folder})"),
                )
            };
            let full_name = format!("{name}{STRING_TABLE_SUFFIX}");
            match schemas.tables.get(table) {
                Some(schema) if schema.is_strings == in_strings_folder => {
                    if let Some(grid) = &sheet.grid {
                        table_parts.entry(table.to_string()).or_default().push(SheetRef { file: relative, grid });
                    }
                }
                Some(schema) => errors.add(&where_, "A1", wrong_place(schema.is_strings)),
                None if in_strings_folder && schemas.tables.get(&full_name).is_some_and(|s| s.is_strings) => errors.add(&where_, "A1", tr(
                    format!("시트 이름은 테이블 이름 그대로 '{full_name}'으로 쓰세요"),
                    format!("name the sheet after the table: '{full_name}'"),
                )),
                None if in_strings_folder => {
                    let schema_name = if name.ends_with(STRING_TABLE_SUFFIX) { name.to_string() } else { full_name.clone() };
                    errors.add(&where_, "A1", tr(
                        format!("스트링테이블 스키마가 없습니다. 언어 목록을 '{schema_name}.string.xlsx'에 정의하세요"),
                        format!("no string table schema. List the languages in '{schema_name}.string.xlsx'"),
                    ))
                }
                None => errors.add(&where_, "A1", tr(
                    format!("스키마가 없습니다. '{name}.schema.xlsx'에 필드를 정의하세요"),
                    format!("no schema. Define the fields in '{name}.schema.xlsx'"),
                )),
            }
        }
    }

    let mut enums: Enums = HashMap::new();
    for (name, schema) in &schemas.enums {
        if let Some(enum_schema) = parse_enum(schema, &mut errors) {
            enums.insert(name.clone(), enum_schema);
        }
    }

    // Resolve every table's key types before reading rows (Ref<T> needs them).
    let mut table_keys: TableKeys = HashMap::new();
    for (name, schema) in &schemas.tables {
        for (row, raw_name, raw_type, raw_scope) in schema.raw_columns() {
            let scope = raw_scope.py_str().trim().to_lowercase();
            if scope == "#" {
                continue;
            }
            let type_text = raw_type.py_str();
            if !KEY_ROLE_PREFIX_RE.is_match(type_text.trim()) {
                if is_identifier(&raw_name.py_str()) {
                    table_keys.insert(format!("{name}.{}", raw_name.py_str()), (type_text, "field".into(), scope));
                }
                continue;
            }
            let Some(parsed) = parse_type(&raw_type, &schema.where_(), &Schema::cell(row, TYPE_ROW), &mut errors, None, None) else {
                continue;
            };
            let field_key = format!("{name}.{}", raw_name.py_str());
            match parsed.role.as_deref() {
                Some("id") => {
                    table_keys.insert(name.clone(), (parsed.type_name.clone(), "id".into(), scope.clone()));
                    table_keys.insert(field_key, (parsed.type_name, "id".into(), scope));
                }
                Some("subkey") => {
                    table_keys.insert(field_key, (parsed.type_name, "subkey".into(), scope));
                }
                _ => {}
            }
        }
    }

    let mut tables = Vec::new();
    for (name, schema) in &schemas.tables {
        let parts = table_parts.remove(name).unwrap_or_default();
        if let Some(mut table) = parse_table(name, schema, &parts, &enums, &table_keys, &mut errors, &mut warnings) {
            for column in &mut table.columns {
                // Schema rows are recorded per column at build time (source_columns are data columns after binding).
                column.alias = column.header_cells.first().and_then(|cell| {
                    let row: usize = cell.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse().ok()?;
                    alias_names.get(&(name.clone(), row)).cloned()
                });
            }
            tables.push(table);
        }
    }

    let mut aliases = Vec::new();
    for alias in schemas.aliases.values() {
        let Some(text) = resolve_alias_text(&alias.name, &schemas.aliases) else { continue };
        let cell = format!("B{}", alias.row);
        if let Some(parsed) = parse_type(&Cell::Str(text), &alias.location, &cell, &mut errors, Some(&table_keys), None) {
            if parsed.role.is_some() {
                errors.add(&alias.location, &cell, tr(
                    "별칭에는 키 역할(ID<…>, SubKey<…>)을 넣지 않습니다. 필드에서 ID<별칭>으로 쓰세요",
                    "an alias carries no key role (ID<…>, SubKey<…>); write ID<Alias> in the field",
                ));
                continue;
            }
            aliases.push(AliasType { name: alias.name.clone(), type_name: parsed.type_name, comment: alias.comment.clone() });
        }
    }

    errors.raise_if_any()?;
    let string_tables: HashSet<String> = tables.iter().filter(|t| t.is_strings()).map(|t| t.name.clone()).collect();
    for table in &mut tables {
        for column in &mut table.columns {
            column.ref_strings = column.ref_target.as_ref().is_some_and(|target| string_tables.contains(target));
        }
    }
    let mut enum_list: Vec<EnumSchema> = enums.into_values().collect();
    enum_list.sort_by(|a, b| a.name.cmp(&b.name));
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(DataModel {
        source_files: workbooks.iter().map(|(relative, _)| relative.to_string()).collect(),
        enums: enum_list,
        aliases,
        tables,
        warnings,
    })
}

/// Points the schema's fields at this sheet's columns, found by the names in row 1. Rows 2
/// and 3 are only a view and are never read; columns whose name starts with '#' are notes.
fn bind(schema: &Schema, columns: &[ColumnSchema], part: &SheetRef, errors: &mut ErrorCollector) -> Option<Vec<ColumnSchema>> {
    let where_ = part.where_();
    let mut ok = true;
    // Schema row of each array field: its data columns may be Name[0], Name[1], ... or one cell.
    let array_rows: HashSet<usize> = columns.iter().filter(|c| c.is_array()).map(|c| c.source_columns[0]).collect();
    let mut defined: Vec<(String, (usize, String))> = Vec::new();
    for (row, raw_name, _, raw_scope) in schema.raw_columns() {
        let name = raw_name.py_str().trim().to_string();
        // Old per-element schema names (Reward[0]) are reported by the schema; not again here.
        if ELEMENT_COLUMN_RE.is_match(&name) {
            continue;
        }
        let value = (row, raw_scope.py_str().trim().to_lowercase());
        match defined.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => defined.push((name, value)),
        }
    }
    let is_array_field = |name: &str| defined.iter().any(|(n, (row, _))| n == name && array_rows.contains(row));

    // Plain headers by name; element headers (Name[i]) by field: (index, column, cell).
    type ElementColumns = Vec<(usize, usize, String)>;
    let mut header: Vec<(String, usize)> = Vec::new();
    let mut elements: Vec<(String, ElementColumns)> = Vec::new();
    for (index, raw_name) in header_names(part.grid) {
        let mut name = raw_name.py_str().trim().to_string();
        if name.starts_with('#') {
            continue;
        }
        if schema.is_strings {
            name = name.replace('-', "_");
        }
        let cell = cell_name(index, NAME_ROW);
        if let Some(captures) = ELEMENT_COLUMN_RE.captures(&name) {
            let field = captures[1].to_string();
            if !is_array_field(&field) {
                let (ko, en) = if defined.iter().any(|(n, _)| *n == field) {
                    (format!("필드 '{field}'은 배열이 아니라 '{name}' 열을 쓸 수 없습니다"), format!("field '{field}' is not an array, so there is no '{name}' column"))
                } else {
                    (format!("필드 '{field}'이 스키마 {}에 없습니다. 필드 추가는 스키마에서 합니다", schema.where_()), format!("field '{field}' is not in the schema {}; fields are added in the schema", schema.where_()))
                };
                errors.add(&where_, &cell, tr(ko, en));
                ok = false;
                continue;
            }
            let position: usize = captures[2].parse().unwrap_or(usize::MAX);
            match elements.iter_mut().find(|(n, _)| *n == field) {
                Some((_, list)) => {
                    if list.iter().any(|(p, _, _)| *p == position) {
                        errors.add(&where_, &cell, tr(format!("열 '{name}'이 중복되었습니다"), format!("duplicate column '{name}'")));
                        ok = false;
                        continue;
                    }
                    list.push((position, index, cell));
                }
                None => elements.push((field, vec![(position, index, cell)])),
            }
            continue;
        }
        if header.iter().any(|(n, _)| *n == name) {
            errors.add(&where_, &cell, tr(format!("필드명 '{name}'이 중복되었습니다"), format!("duplicate field name '{name}'")));
            ok = false;
            continue;
        }
        header.push((name, index));
    }
    for (field, list) in &mut elements {
        list.sort();
        if header.iter().any(|(n, _)| n == field) {
            errors.add(&where_, &list[0].2, tr(
                format!("배열 '{field}'을 한 칸('{field}' 열)과 원소 열('{field}[0]'…)로 함께 쓸 수 없습니다. 한 가지만 쓰세요"),
                format!("array '{field}' cannot have both a single-cell column ('{field}') and element columns ('{field}[0]', ...); use one"),
            ));
            ok = false;
        }
        if !list.iter().map(|(p, _, _)| *p).eq(0..list.len()) {
            errors.add(&where_, &list[0].2, tr(
                format!("배열 '{field}'의 열 번호는 0부터 빈틈없이 이어져야 합니다 ({field}[0], {field}[1], ...)"),
                format!("array '{field}' columns must be numbered from 0 without gaps ({field}[0], {field}[1], ...)"),
            ));
            ok = false;
        }
    }
    for (name, index) in &header {
        if !defined.iter().any(|(n, _)| n == name) {
            errors.add(&where_, &cell_name(*index, NAME_ROW), tr(
                format!("필드 '{name}'이 스키마 {}에 없습니다. 필드 추가는 스키마에서 합니다", schema.where_()),
                format!("field '{name}' is not in the schema {}; fields are added in the schema", schema.where_()),
            ));
            ok = false;
        }
    }
    for (name, (row, scope)) in &defined {
        let present = header.iter().any(|(n, _)| n == name) || elements.iter().any(|(n, _)| n == name);
        if scope != "#" && !present {
            errors.add(&where_, "A1", tr(
                format!("필드 '{name}'의 열이 없습니다 (스키마 {})", schema.at(*row)),
                format!("no column for field '{name}' (schema {})", schema.at(*row)),
            ));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    // Schema row -> (data columns, written as one cell).
    let by_row: HashMap<usize, (Vec<usize>, bool)> = defined
        .iter()
        .filter_map(|(name, (row, _))| {
            if let Some((_, index)) = header.iter().find(|(n, _)| n == name) {
                return Some((*row, (vec![*index], array_rows.contains(row))));
            }
            elements.iter().find(|(n, _)| n == name).map(|(_, list)| (*row, (list.iter().map(|(_, index, _)| *index).collect(), false)))
        })
        .collect();
    Some(
        columns
            .iter()
            .map(|column| {
                let (sources, cells) = by_row.get(&column.source_columns[0]).cloned().unwrap_or((vec![0], false));
                ColumnSchema {
                    source_columns: sources,
                    array: column.array.as_ref().map(|shape| ArrayShape { cells, ..shape.clone() }),
                    ..column.clone()
                }
            })
            .collect(),
    )
}

/// An enum's values from its schema: name, value (default: previous + 1) and comment.
fn parse_enum(schema: &Schema, errors: &mut ErrorCollector) -> Option<EnumSchema> {
    let where_ = schema.where_();
    let mut values: Vec<EnumValue> = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();
    let mut used_values: HashSet<i64> = HashSet::new();
    let mut next_value: i64 = 0;
    for (row, raw_name, raw_value, raw_comment) in schema.enumerators() {
        let item_name = raw_name.py_str().trim().to_string();
        if !is_identifier(&item_name) {
            errors.add(&where_, &Schema::cell(row, 1), tr(
                format!("올바르지 않은 열거자 이름 '{item_name}'"),
                format!("invalid enumerator name '{item_name}'"),
            ));
            continue;
        }
        if used_names.contains(&item_name) {
            errors.add(&where_, &Schema::cell(row, 1), tr(
                format!("열거자 이름 '{item_name}'이 중복되었습니다"),
                format!("enumerator '{item_name}' appears twice"),
            ));
        }
        used_names.insert(item_name.clone());
        let value = if raw_value.is_blank() {
            next_value
        } else {
            let parsed = match &raw_value {
                Cell::Int(i) => Some(*i),
                Cell::Float(f) if f.fract() == 0.0 && f.is_finite() => Some(*f as i64),
                Cell::Bool(b) => Some(*b as i64),
                Cell::Str(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            match parsed {
                Some(v) => v,
                None => {
                    errors.add(&where_, &Schema::cell(row, 2), tr(
                        format!("열거형 값 '{}'은 정수가 아닙니다", raw_value.py_str()),
                        format!("enum value '{}' is not an integer", raw_value.py_str()),
                    ));
                    continue;
                }
            }
        };
        if !(0..=255).contains(&value) {
            errors.add(&where_, &Schema::cell(row, 2), tr("열거형 값은 uint8 범위(0~255)여야 합니다", "enum values must fit in uint8 (0-255)"));
        }
        if used_values.contains(&value) {
            errors.add(&where_, &Schema::cell(row, 2), tr(
                format!("열거형 값 {value}가 중복되었습니다"),
                format!("enum value {value} appears twice"),
            ));
        }
        used_values.insert(value);
        next_value = value + 1;
        let comment = if matches!(raw_comment, Cell::Empty) { String::new() } else { raw_comment.py_str() };
        values.push(EnumValue { name: item_name, value, comment });
    }
    if values.is_empty() {
        errors.add(&where_, "A2", tr("열거형에는 항목이 하나 이상 필요합니다", "an enum needs at least one value"));
        return None;
    }
    let sheet = if schema.title.is_empty() { schema.name.clone() } else { schema.title.clone() };
    Some(EnumSchema { name: schema.name.clone(), sheet, source_name: schema.file.clone(), values })
}

/// A table's fields come from its schema file; its rows from every data sheet of that name.
fn parse_table(
    name: &str,
    schema: &Schema,
    parts: &[SheetRef],
    enums: &Enums,
    table_keys: &TableKeys,
    errors: &mut ErrorCollector,
    warnings: &mut Vec<String>,
) -> Option<TableSchema> {
    let columns = build_columns(&schema.raw_columns(), &schema.where_(), enums, errors, Some(table_keys), &Schema::cell);
    if columns.is_empty() || !columns.iter().any(|c| c.is_role("id")) {
        return None;
    }
    if columns.iter().any(ColumnSchema::is_lang) && !check_string_schema(schema, &columns, errors) {
        return None;
    }
    let (source_name, sheet) = match parts.first() {
        Some(part) => (part.file.to_string(), part.grid.title.clone()),
        None => (schema.file.clone(), schema.title.clone()),
    };
    let mut table = TableSchema {
        name: name.to_string(),
        sheet,
        source_name,
        columns: columns.clone(),
        rows: Vec::new(),
        schema_hash: String::new(),
        sources: Vec::new(),
        schema_location: schema.where_(),
        schema_file: schema.file.clone(),
    };
    let mut state = RowState::default();
    for part in parts {
        let Some(bound) = bind(schema, &columns, part, errors) else { continue };
        let rows = read_rows(&mut table, part, &bound, enums, errors, &mut state);
        table.sources.push(TableSource { file: part.file.to_string(), sheet: part.grid.title.clone(), rows });
    }
    table.schema_hash = calculate_schema_hash(&columns, Some(enums));
    warnings.append(&mut state.warnings);
    if let Some(base) = table.base_language() {
        for (language, (count, first)) in &state.filled {
            let base = base.culture();
            warnings.push(tr(
                format!("{}: {language} 번역 {count}칸이 비어 기준 언어({base}) 값으로 채웠습니다 (처음: {first})", schema.where_()),
                format!("{}: {count} empty {language} translation(s) filled from the base language ({base}) (first at {first})", schema.where_()),
            ));
        }
    }
    Some(table)
}

/// A string table holds a name primary key and language columns, one of them the base language.
fn check_string_schema(schema: &Schema, columns: &[ColumnSchema], errors: &mut ErrorCollector) -> bool {
    let where_ = schema.where_();
    let before = errors.messages.len();
    let type_cell = |column: &ColumnSchema| Schema::cell(column.source_columns[0], TYPE_ROW);
    let mut bases = Vec::new();
    for column in columns {
        if column.is_role("id") {
            if column.type_name != "name" {
                errors.add(&where_, &type_cell(column), tr(
                    "스트링테이블의 기본키는 ID<name>이어야 합니다",
                    "a string table's primary key must be ID<name>",
                ));
            }
        } else if !column.is_lang() {
            errors.add(&where_, &type_cell(column), tr(
                "스트링테이블에는 기본키와 언어(lang) 열만 둘 수 있습니다",
                "a string table holds only its primary key and language (lang) columns",
            ));
        } else if column.is_array() || column.default_values.iter().any(Option::is_some) {
            errors.add(&where_, &type_cell(column), tr(
                "언어(lang) 열은 배열이나 기본값을 쓸 수 없습니다",
                "a language (lang) column cannot be an array or have a default value",
            ));
        } else if column.is_role("base") {
            bases.push(column);
        }
    }
    let first_language = columns.iter().find(|c| c.is_lang()).expect("a language column");
    if bases.is_empty() {
        errors.add(&where_, &type_cell(first_language), tr(
            "기준 언어를 하나 표시하세요(B열 Base에 ✓ 등)",
            "mark one base language (column B, Base)",
        ));
    }
    for extra in bases.iter().skip(1) {
        errors.add(&where_, &type_cell(extra), tr(
            format!("기준 언어는 하나만 표시할 수 있습니다 (처음: {})", bases[0].culture()),
            format!("only one base language can be marked (first: {})", bases[0].culture()),
        ));
    }
    errors.messages.len() == before
}

/// String table rows: the base language must be filled; an empty translation takes the base
/// text; a translation whose format arguments differ from the base text is reported.
fn check_string_row(columns: &[ColumnSchema], values: &mut [Value], row: usize, where_: &str, errors: &mut ErrorCollector, state: &mut RowState) {
    let Some(base_index) = columns.iter().position(|c| c.is_lang() && c.is_role("base")) else { return };
    let base = &columns[base_index];
    let base_text = values[base_index].as_str().unwrap_or("").to_string();
    if base_text.is_empty() {
        errors.add(where_, &cell_name(base.source_columns[0], row), tr(
            format!("기준 언어({}) 칸이 비어 있습니다", base.culture()),
            format!("the base language ({}) cell is empty", base.culture()),
        ));
        return;
    }
    let arguments = |text: &str| -> BTreeSet<String> { PLACEHOLDER_RE.find_iter(text).map(|m| m.as_str().to_string()).collect() };
    let base_arguments = arguments(&base_text);
    for (index, column) in columns.iter().enumerate() {
        if !column.is_lang() || index == base_index {
            continue;
        }
        let cell = cell_name(column.source_columns[0], row);
        let text = values[index].as_str().unwrap_or("").to_string();
        if text.is_empty() {
            let entry = state.filled.entry(column.culture()).or_insert((0, format!("{where_}!{cell}")));
            entry.0 += 1;
            values[index] = Value::Str(base_text.clone());
            continue;
        }
        let found = arguments(&text);
        if found != base_arguments {
            let list = |set: &BTreeSet<String>| if set.is_empty() { "-".to_string() } else { set.iter().cloned().collect::<Vec<_>>().join(" ") };
            state.warnings.push(tr(
                format!("{where_}!{cell}: {} 번역의 서식 인자가 기준 언어({})와 다릅니다: {} / {}", column.culture(), base.culture(), list(&found), list(&base_arguments)),
                format!("{where_}!{cell}: the {} translation's format arguments differ from the base language ({}): {} / {}", column.culture(), base.culture(), list(&found), list(&base_arguments)),
            ));
        }
    }
}

/// Appends the rows of one sheet to the table and returns how many were read.
fn read_rows(
    table: &mut TableSchema,
    part: &SheetRef,
    columns: &[ColumnSchema],
    enums: &Enums,
    errors: &mut ErrorCollector,
    state: &mut RowState,
) -> usize {
    let grid = part.grid;
    let where_ = part.where_();
    let primary_index = columns.iter().position(|c| c.is_role("id")).expect("a primary key");
    let primary = &columns[primary_index];
    let mut data_columns: Vec<usize> = columns.iter().flat_map(|c| c.source_columns.iter().copied()).collect();
    data_columns.sort_unstable();
    data_columns.dedup();
    let empty_reference = || tr("열거형 기본키를 참조하는 셀은 비울 수 없습니다", "a reference to an enum-keyed table cannot be empty");
    let mut count = 0;
    for row in DATA_ROW..=grid.max_row() {
        if data_columns.iter().all(|&c| grid.value(row, c).is_blank()) {
            continue;
        }
        let mut converted: Vec<Value> = Vec::with_capacity(columns.len());
        for column in columns {
            let references_enum = column.ref_target.is_some() && column.type_name.starts_with('E');
            if let Some(shape) = &column.array {
                converted.push(Value::List(read_array(grid, row, column, shape, enums, &where_, errors)));
            } else {
                let source = column.source_columns[0];
                let raw_value = grid.value(row, source);
                if references_enum && raw_value.is_blank() {
                    errors.add(&where_, &cell_name(source, row), empty_reference());
                }
                if column.is_role("id") && raw_value.is_blank() {
                    errors.add(&where_, &cell_name(source, row), tr("기본키 값이 비어 있습니다", "the primary key is empty"));
                }
                converted.push(convert_cell(raw_value, &column.default_values[0], &column.type_name, enums, &where_, &cell_name(source, row), errors));
            }
        }
        if table.columns.iter().any(ColumnSchema::is_lang) {
            check_string_row(columns, &mut converted, row, &where_, errors, state);
        }
        let key = &converted[primary_index];
        let key_cell = cell_name(primary.source_columns[0], row);
        let prefix = format!("{where_}!");
        match state.used_keys.get(&key.key()) {
            Some(seen) => {
                let (ko_suffix, en_suffix) = if seen.starts_with(&prefix) {
                    (String::new(), String::new())
                } else {
                    (format!(" (처음: {seen})"), format!(" (first at {seen})"))
                };
                errors.add(&where_, &key_cell, tr(
                    format!("기본키 값 '{}'이 중복되었습니다{ko_suffix}", key.py_str()),
                    format!("duplicate primary key '{}'{en_suffix}", key.py_str()),
                ));
            }
            None => {
                state.used_keys.insert(key.key(), format!("{where_}!{key_cell}"));
                if primary.type_name == "name" {
                    check_name_case(&where_, &primary.name, key, &key_cell, &mut state.name_spellings, errors);
                }
            }
        }
        for (index, column) in columns.iter().enumerate() {
            if column.is_role("subkey") && column.type_name == "name" && column.name != primary.name {
                let cell = cell_name(column.source_columns[0], row);
                check_name_case(&where_, &column.name, &converted[index], &cell, &mut state.name_spellings, errors);
            }
        }
        table.rows.push(converted);
        count += 1;
    }
    count
}

/// Reports name key values that differ from an earlier value only by case.
fn check_name_case(
    sheet: &str,
    field: &str,
    value: &Value,
    cell: &str,
    spellings: &mut HashMap<(String, String), (String, String)>,
    errors: &mut ErrorCollector,
) {
    let Some(text) = value.as_str() else { return };
    if text.is_empty() {
        return;
    }
    let folded = (field.to_string(), text.to_lowercase());
    match spellings.get(&folded) {
        None => {
            spellings.insert(folded, (text.to_string(), format!("{sheet}!{cell}")));
        }
        Some((spelling, location)) if spelling != text => {
            errors.add(sheet, cell, tr(
                format!("name 키 '{text}'이 {location}의 '{spelling}'과 대소문자만 다릅니다. 언리얼 FName은 대소문자를 구분하지 않아 같은 값이 됩니다"),
                format!("name key '{text}' differs from '{spelling}' at {location} only by case. Unreal FName is case-insensitive, so they would be the same value"),
            ));
        }
        Some(_) => {}
    }
}

/// A cell's value; an empty cell takes the declared default, which is already converted
/// (converting it again would scale a fixed-point default twice).
fn convert_cell(
    value: &Cell,
    declared: &Option<Value>,
    type_name: &str,
    enums: &Enums,
    sheet: &str,
    cell: &str,
    errors: &mut ErrorCollector,
) -> Value {
    match declared {
        Some(default) if value.is_blank() => default.clone(),
        _ => convert_value(value, type_name, enums, sheet, cell, errors, true).unwrap_or(Value::Null),
    }
}

/// One row's elements of an array field. Columns (`Reward[0]`, `Reward[1]`, ...): empty cells at
/// the end are not elements, an empty cell before a filled one is an error. One cell: elements
/// separated by commas (`10, 20, 30`), an empty cell is no element.
fn read_array(grid: &Grid, row: usize, column: &ColumnSchema, shape: &ArrayShape, enums: &Enums, where_: &str, errors: &mut ErrorCollector) -> Vec<Value> {
    let mut items = Vec::new();
    let first_cell = cell_name(column.source_columns[0], row);
    if shape.cells {
        let source = column.source_columns[0];
        let value = grid.value(row, source);
        match value {
            _ if value.is_blank() => {}
            Cell::Str(text) => {
                for (position, piece) in text.split(',').enumerate() {
                    let piece = piece.trim();
                    if piece.is_empty() {
                        errors.add(where_, &first_cell, tr(
                            format!("배열 '{}'의 {}번째 원소가 비어 있습니다 ('{text}')", column.name, position + 1),
                            format!("element {} of array '{}' is empty ('{text}')", position + 1, column.name),
                        ));
                        continue;
                    }
                    if let Some(v) = convert_value(&Cell::Str(piece.to_string()), &column.type_name, enums, where_, &first_cell, errors, false) {
                        items.push(v);
                    }
                }
            }
            // A number or date typed into the cell is one element.
            other => items.extend(convert_value(other, &column.type_name, enums, where_, &first_cell, errors, false)),
        }
    } else {
        let last = column.source_columns.iter().rposition(|&source| !grid.value(row, source).is_blank());
        if let Some(last) = last {
            for &source in &column.source_columns[..=last] {
                let value = grid.value(row, source);
                if value.is_blank() {
                    errors.add(where_, &cell_name(source, row), tr(
                        format!("배열 '{}' 중간의 칸이 비어 있습니다. 원소는 앞에서부터 빈칸 없이 채웁니다", column.name),
                        format!("an empty cell in the middle of array '{}'; fill elements from the first without gaps", column.name),
                    ));
                    continue;
                }
                items.extend(convert_value(value, &column.type_name, enums, where_, &cell_name(source, row), errors, false));
            }
        }
    }
    if let Some(max) = shape.max
        && items.len() > max {
            errors.add(where_, &first_cell, tr(
                format!("배열 '{}'의 원소가 {}개로 최대 {max}개를 넘습니다", column.name, items.len()),
                format!("array '{}' has {} elements, more than its maximum {max}", column.name, items.len()),
            ));
        }
    items
}

/// (column, field name) from row 1, up to the first empty cell.
fn header_names(grid: &Grid) -> Vec<(usize, Cell)> {
    let mut names = Vec::new();
    for column in 1..=grid.max_column() {
        let name = grid.value(NAME_ROW, column);
        if name.is_blank() {
            break;
        }
        names.push((column, name.clone()));
    }
    names
}

static KEY_ROLE_WRAP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(id|subkey)\s*<\s*([A-Za-z][A-Za-z0-9_]*)\s*>$").unwrap());

/// The fully expanded type text of an alias (aliases of aliases followed; the nearest default
/// wins), or None when it loops (reported by expand_aliases).
fn resolve_alias_text(name: &str, aliases: &std::collections::BTreeMap<String, crate::schemafile::Alias>) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut current = name;
    let mut default: Option<String> = None;
    loop {
        if seen.contains(&current) {
            return None;
        }
        seen.push(current);
        let text = aliases.get(current)?.type_cell.text_or_empty();
        let (head, tail) = match text.split_once('=') {
            Some((h, d)) => (h.trim().to_string(), Some(d.trim().to_string())),
            None => (text.trim().to_string(), None),
        };
        if default.is_none() {
            default = tail;
        }
        match aliases.get(head.as_str()) {
            Some(next) => current = &next.name,
            None => {
                return Some(match default {
                    Some(d) => format!("{head}={d}"),
                    None => head,
                });
            }
        }
    }
}

/// Replaces alias names in table schema types (`ItemID`, `ItemID=3`, `ID<ItemID>`) with their
/// types and returns which alias each (table, schema row) used. Checks alias names and loops.
fn expand_aliases(schemas: &mut crate::schemafile::Schemas, errors: &mut ErrorCollector) -> HashMap<(String, usize), String> {
    let mut used = HashMap::new();
    if schemas.aliases.is_empty() {
        return used;
    }
    let mut valid: HashMap<String, String> = HashMap::new();
    for alias in schemas.aliases.values() {
        let cell = format!("A{}", alias.row);
        let lower = alias.name.to_lowercase();
        if crate::schema::PRIMITIVES.contains(&lower.as_str()) || crate::schema::fixed_of(&alias.name).is_some() || lower == "ref" || lower == "id" || lower == "subkey" {
            errors.add(&alias.location, &cell, tr(
                format!("'{}'은 자료형 이름이라 별칭으로 쓸 수 없습니다", alias.name),
                format!("'{}' is a type name and cannot be an alias", alias.name),
            ));
            continue;
        }
        if schemas.tables.contains_key(&alias.name) {
            errors.add(&alias.location, &cell, tr(
                format!("별칭 '{}'이 같은 이름의 테이블과 겹칩니다", alias.name),
                format!("alias '{}' has the same name as a table", alias.name),
            ));
            continue;
        }
        if alias.name.strip_prefix('E').is_some_and(|e| schemas.enums.contains_key(e)) {
            errors.add(&alias.location, &cell, tr(
                format!("별칭 '{}'이 열거형 자료형 이름과 겹칩니다", alias.name),
                format!("alias '{}' has the same name as an enum type", alias.name),
            ));
            continue;
        }
        match resolve_alias_text(&alias.name, &schemas.aliases) {
            Some(text) if crate::schema::TYPE_ARRAY_RE.is_match(text.split('=').next().unwrap_or("").trim()) => errors.add(&alias.location, &format!("B{}", alias.row), tr(
                format!("별칭 '{}'은 배열이 될 수 없습니다. 원소 자료형으로 정의하고 필드에서 {}[]로 씁니다", alias.name, alias.name),
                format!("alias '{}' cannot be an array; define the element type and write {}[] in the field", alias.name, alias.name),
            )),
            Some(text) => {
                valid.insert(alias.name.clone(), text);
            }
            None => errors.add(&alias.location, &format!("B{}", alias.row), tr(
                format!("별칭 '{}'이 자기 자신을 거쳐 돌아옵니다", alias.name),
                format!("alias '{}' refers back to itself", alias.name),
            )),
        }
    }
    for (table, schema) in schemas.tables.iter_mut() {
        if schema.is_strings {
            continue;
        }
        for (row, _, type_cell, _, _) in schema.rows.iter_mut() {
            let text = type_cell.text_or_empty();
            let (head, field_default) = match text.split_once('=') {
                Some((h, d)) => (h.trim().to_string(), Some(d.trim().to_string())),
                None => (text.trim().to_string(), None),
            };
            let (role, inner) = match KEY_ROLE_WRAP_RE.captures(&head) {
                Some(c) => (Some(c[1].to_string()), c[2].to_string()),
                None => (None, head.clone()),
            };
            // `ItemID[]` / `ItemID[4]`: an array of the alias's type.
            let (inner, array_suffix) = match crate::schema::TYPE_ARRAY_RE.captures(&inner) {
                Some(c) => (c[1].trim().to_string(), format!("[{}]", &c[2])),
                None => (inner, String::new()),
            };
            let Some(expanded) = valid.get(&inner) else { continue };
            let (base, alias_default) = match expanded.split_once('=') {
                Some((h, d)) => (h.to_string(), Some(d.to_string())),
                None => (expanded.clone(), None),
            };
            let mut result = match role {
                Some(role) => format!("{role}<{base}>"),
                None => format!("{base}{array_suffix}"),
            };
            if let Some(default) = field_default.or(alias_default) {
                result = format!("{result}={default}");
            }
            *type_cell = Cell::Str(result);
            used.insert((table.clone(), *row), inner);
        }
    }
    used
}
