//! Reads schema files and data workbooks into a validated data model.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::errors::{ErrorCollector, ValidationErrors};
use crate::headers::link_warnings;
use crate::i18n::tr;
use crate::reader::{read_workbook, Grid, Sheet};
use crate::schema::{
    build_columns, calculate_schema_hash, cell_name, parse_type, ColumnSchema, EnumSchema, EnumValue, Enums,
    TableKeys, TableSchema, TableSource, DATA_ROW, KEY_ROLE_PREFIX_RE, NAME_ROW, TYPE_ROW,
};
use crate::schemafile::{enum_folder, folder_of, load_schemas, Schema, SCHEMA_SUFFIXES};
use crate::sources::{find_files, is_identifier, table_name_of};
use crate::value::{Cell, Value};
use crate::values::{convert_value, value_to_cell};

pub struct DataModel {
    pub source_files: Vec<String>,
    pub enums: Vec<EnumSchema>,
    pub tables: Vec<TableSchema>,
    pub warnings: Vec<String>,
}

impl DataModel {
    pub fn enum_map(&self) -> Enums {
        self.enums.iter().map(|e| (e.name.clone(), e.clone())).collect()
    }
}

struct SheetRef<'a> {
    file: &'a str,
    grid: &'a Grid,
}

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
}

/// Tables and enums from their schema files, rows from the data workbooks under `input`.
/// The schema folder defaults to the input folder and the enum folder to `<schema>/Enums`.
pub fn load_model(input: &Path, schema_path: Option<&Path>, enum_path: Option<&Path>) -> Result<DataModel, ValidationErrors> {
    let schema_root = match schema_path {
        Some(path) => path.to_path_buf(),
        None => folder_of(input),
    };
    let mut errors = ErrorCollector::default();
    let mut warnings: Vec<String> = Vec::new();
    let schemas = load_schemas(&schema_root, &enum_folder(&schema_root, enum_path), &mut errors)?;
    let files: Vec<(PathBuf, String)> = find_files(input, &[".xlsx"], true)?
        .into_iter()
        .filter(|(path, _)| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            !SCHEMA_SUFFIXES.iter().any(|s| name.ends_with(s))
        })
        .collect();
    let read: Vec<Result<Vec<Sheet>, String>> = files
        .par_iter()
        .map(|(path, _)| read_workbook(path, |title| !title.starts_with('#') && !title.starts_with("<enum>")))
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
    let base = folder_of(&schema_root);
    let schema_files: HashSet<PathBuf> =
        schemas.all().filter_map(|s| std::fs::canonicalize(base.join(&s.file)).ok()).collect();
    for (path, relative) in &files {
        warnings.extend(link_warnings(path, relative, &schema_files));
    }

    let mut table_parts: HashMap<String, Vec<SheetRef>> = HashMap::new();
    for (relative, sheets) in &workbooks {
        for sheet in sheets {
            let where_ = format!("[{relative}]{}", sheet.title);
            if sheet.title.starts_with('#') {
                continue;
            }
            if sheet.title.starts_with("<enum>") {
                warnings.push(tr(
                    format!("{where_}: 열거형 값은 이제 열거형 스키마(.enum.xlsx)에 정의합니다. 이 시트는 읽지 않으니 지워도 됩니다(옮기기: drtable migrate)"),
                    format!("{where_}: enum values are now defined in enum schemas (.enum.xlsx). This sheet is not read and can be deleted (to convert: drtable migrate)"),
                ));
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
            if !schemas.tables.contains_key(name) {
                errors.add(&where_, "A1", tr(
                    format!("스키마가 없습니다. '{name}.schema.xlsx'에 필드를 정의하세요"),
                    format!("no schema. Define the fields in '{name}.schema.xlsx'"),
                ));
                continue;
            }
            if let Some(grid) = &sheet.grid {
                table_parts.entry(name.to_string()).or_default().push(SheetRef { file: relative, grid });
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
        if let Some(table) = parse_table(name, schema, &parts, &enums, &table_keys, &mut errors) {
            tables.push(table);
        }
    }

    errors.raise_if_any()?;
    let mut enum_list: Vec<EnumSchema> = enums.into_values().collect();
    enum_list.sort_by(|a, b| a.name.cmp(&b.name));
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(DataModel {
        source_files: workbooks.iter().map(|(relative, _)| relative.to_string()).collect(),
        enums: enum_list,
        tables,
        warnings,
    })
}

/// Points the schema's fields at this sheet's columns, found by the names in row 1. Rows 2
/// and 3 are only a view and are never read; columns whose name starts with '#' are notes.
fn bind(schema: &Schema, columns: &[ColumnSchema], part: &SheetRef, errors: &mut ErrorCollector) -> Option<Vec<ColumnSchema>> {
    let where_ = part.where_();
    let mut ok = true;
    let mut header: Vec<(String, usize)> = Vec::new();
    for (index, raw_name) in header_names(part.grid) {
        let name = raw_name.py_str().trim().to_string();
        if name.starts_with('#') {
            continue;
        }
        if header.iter().any(|(n, _)| *n == name) {
            errors.add(&where_, &cell_name(index, NAME_ROW), tr(
                format!("필드명 '{name}'이 중복되었습니다"),
                format!("duplicate field name '{name}'"),
            ));
            ok = false;
            continue;
        }
        header.push((name, index));
    }
    let mut defined: Vec<(String, (usize, String))> = Vec::new();
    for (row, raw_name, _, raw_scope) in schema.raw_columns() {
        let name = raw_name.py_str().trim().to_string();
        let value = (row, raw_scope.py_str().trim().to_lowercase());
        match defined.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => defined.push((name, value)),
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
        if scope != "#" && !header.iter().any(|(n, _)| n == name) {
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
    let by_row: HashMap<usize, usize> = defined
        .iter()
        .filter_map(|(name, (row, _))| header.iter().find(|(n, _)| n == name).map(|(_, index)| (*row, *index)))
        .collect();
    Some(
        columns
            .iter()
            .map(|column| ColumnSchema {
                source_columns: column.source_columns.iter().map(|row| by_row.get(row).copied().unwrap_or(0)).collect(),
                ..column.clone()
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
) -> Option<TableSchema> {
    let columns = build_columns(&schema.raw_columns(), &schema.where_(), enums, errors, Some(table_keys), &Schema::cell);
    if columns.is_empty() || !columns.iter().any(|c| c.is_role("id")) {
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
    Some(table)
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
            if column.is_array() {
                for &source in &column.source_columns {
                    if references_enum && grid.value(row, source).is_blank() {
                        errors.add(&where_, &cell_name(source, row), empty_reference());
                    }
                }
                let items = column
                    .source_columns
                    .iter()
                    .enumerate()
                    .map(|(position, &source)| {
                        let raw = with_default(grid.value(row, source), &column.default_values[position]);
                        convert_value(&raw, &column.type_name, enums, &where_, &cell_name(source, row), errors, true)
                            .unwrap_or(Value::Null)
                    })
                    .collect();
                converted.push(Value::List(items));
            } else {
                let source = column.source_columns[0];
                let raw_value = grid.value(row, source);
                if references_enum && raw_value.is_blank() {
                    errors.add(&where_, &cell_name(source, row), empty_reference());
                }
                if column.is_role("id") && raw_value.is_blank() {
                    errors.add(&where_, &cell_name(source, row), tr("기본키 값이 비어 있습니다", "the primary key is empty"));
                }
                let raw = with_default(raw_value, &column.default_values[0]);
                converted.push(
                    convert_value(&raw, &column.type_name, enums, &where_, &cell_name(source, row), errors, true)
                        .unwrap_or(Value::Null),
                );
            }
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

fn with_default(value: &Cell, declared: &Option<Value>) -> Cell {
    match declared {
        Some(default) if value.is_blank() => value_to_cell(default),
        _ => value.clone(),
    }
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
