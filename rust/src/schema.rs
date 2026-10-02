//! Schema types and validation rules.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::errors::ErrorCollector;
use crate::i18n::tr;
use crate::sources::is_identifier;
use crate::value::{object, Cell, Json, Value};
use crate::values::convert_value;

pub const PRIMITIVES: [&str; 13] =
    ["int32", "int64", "float", "double", "bool", "name", "string", "text", "tag", "path", "lang", "datetime", "duration"];
pub const KEY_PRIMITIVES: [&str; 3] = ["int32", "int64", "name"];
pub const SCOPES: [&str; 4] = ["all", "client", "server", "#"];
pub const CLIENT_SCOPES: [&str; 2] = ["all", "client"];
pub const SERVER_SCOPES: [&str; 2] = ["all", "server"];
/// Header layout of data sheets: row 1 field name, rows 2-3 a view, data from row 4.
pub const NAME_ROW: usize = 1;
pub const TYPE_ROW: usize = 2;
pub const SCOPE_ROW: usize = 3;
pub const DATA_ROW: usize = 4;

static ARRAY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9_]*)\[(\d+)\]$").unwrap());
static ROLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(id|subkey)\s*<\s*([^<>]+)\s*>$").unwrap());
static REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^ref\s*<\s*([^<>\s]+)\s*>$").unwrap());
static REF_ROLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(id|subkey)\s*<\s*(ref\s*<[^<>]+>)\s*>$").unwrap());
/// `Base<lang>`: the base language column of a string table.
static BASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^base\s*<\s*lang\s*>$").unwrap());
pub static KEY_ROLE_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(id|subkey)\s*<").unwrap());

/// `fixed<10000>` (int32) or `fixed64<10000>` (int64): a decimal stored as an integer
/// count of 1/scale units, so clients and servers compute with exact integers.
static FIXED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^fixed(64)?\s*<\s*(\d+)\s*>$").unwrap());

/// `datetime<+09:00>`: a point in time written in that UTC offset (plain `datetime` is UTC).
static DATETIME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^datetime\s*<\s*([+-])(\d{1,2}):?(\d{2})\s*>$").unwrap());

/// The UTC offset in minutes of a `datetime` or `datetime<±HH:MM>` type name.
pub fn datetime_offset(type_name: &str) -> Option<i32> {
    if type_name == "datetime" {
        return Some(0);
    }
    let captures = DATETIME_RE.captures(type_name)?;
    let minutes = captures[2].parse::<i32>().ok()? * 60 + captures[3].parse::<i32>().ok()?;
    Some(if &captures[1] == "-" { -minutes } else { minutes })
}

/// (scale, 64-bit) of a fixed-point type name such as `fixed<10000>`.
pub fn fixed_of(type_name: &str) -> Option<(i64, bool)> {
    let captures = FIXED_RE.captures(type_name)?;
    let scale = captures[2].parse::<i64>().ok()?;
    Some((scale, captures.get(1).is_some()))
}

/// Whether a schema type cell declares a language column (`lang` or `Base<lang>`).
pub fn is_lang_type(text: &str) -> bool {
    let text = text.split('=').next().unwrap_or("").trim();
    text.eq_ignore_ascii_case("lang") || BASE_RE.is_match(text)
}

/// The enum name of an `E<Name>` type.
pub fn enum_of(type_name: &str) -> Option<&str> {
    let name = type_name.strip_prefix('E')?;
    is_identifier(name).then_some(name)
}

pub fn in_scopes(scope: &str, scopes: &[&str]) -> bool {
    scopes.contains(&scope)
}

#[derive(Clone, Debug)]
pub struct EnumValue {
    pub name: String,
    pub value: i64,
    pub comment: String,
}

#[derive(Clone, Debug)]
pub struct EnumSchema {
    pub name: String,
    pub sheet: String,
    pub source_name: String,
    pub values: Vec<EnumValue>,
}

pub type Enums = HashMap<String, EnumSchema>;

#[derive(Clone, Debug)]
pub struct ColumnSchema {
    pub name: String,
    pub type_name: String,
    pub role: Option<String>,
    pub scope: String,
    pub source_columns: Vec<usize>,
    pub header_cells: Vec<String>,
    pub array_size: Option<usize>,
    pub default_values: Vec<Option<Value>>,
    pub ref_target: Option<String>,
    pub ref_key: Option<String>,
    /// The reference points at a string table (its accessor returns the text in the current language).
    pub ref_strings: bool,
    /// The type alias the schema used (`ItemID`), if any.
    pub alias: Option<String>,
}

impl ColumnSchema {
    pub fn is_array(&self) -> bool {
        self.array_size.is_some()
    }

    pub fn is_role(&self, role: &str) -> bool {
        self.role.as_deref() == Some(role)
    }

    /// A language column of a string table (`lang` or `Base<lang>`).
    pub fn is_lang(&self) -> bool {
        self.type_name == "lang"
    }

    /// The culture code a language column stands for: its name with '_' read as '-' (zh_Hans -> zh-Hans).
    pub fn culture(&self) -> String {
        self.name.replace('_', "-")
    }
}

#[derive(Clone, Debug)]
pub struct TableSource {
    pub file: String,
    pub sheet: String,
    pub rows: usize,
}

#[derive(Clone, Debug)]
pub struct TableSchema {
    pub name: String,
    pub sheet: String,
    pub source_name: String,
    pub columns: Vec<ColumnSchema>,
    /// Converted rows, one value per column (in `columns` order).
    pub rows: Vec<Vec<Value>>,
    pub schema_hash: String,
    pub sources: Vec<TableSource>,
    /// Where the fields are defined, e.g. "[Items.schema.xlsx]Items".
    pub schema_location: String,
    /// The schema file; generated code names it instead of data files.
    pub schema_file: String,
}

impl TableSchema {
    pub fn header_location(&self) -> String {
        if self.schema_location.is_empty() { self.location() } else { self.schema_location.clone() }
    }

    pub fn location(&self) -> String {
        match self.sources.first() {
            Some(part) => format!("[{}]{}", part.file, part.sheet),
            None => format!("[{}]{}", self.source_name, self.sheet),
        }
    }

    pub fn source_list(&self) -> Vec<String> {
        if self.sources.is_empty() {
            vec![format!("{} / {}", self.source_name, self.sheet)]
        } else {
            self.sources.iter().map(|p| format!("{} / {}", p.file, p.sheet)).collect()
        }
    }

    pub fn primary_key(&self) -> &ColumnSchema {
        self.columns.iter().find(|c| c.is_role("id")).expect("a table has a primary key")
    }

    pub fn sub_keys(&self) -> Vec<&ColumnSchema> {
        self.columns.iter().filter(|c| c.is_role("subkey")).collect()
    }

    /// A string table has language columns; it is loaded one language at a time.
    pub fn is_strings(&self) -> bool {
        self.columns.iter().any(ColumnSchema::is_lang)
    }

    /// The language columns, in schema order.
    pub fn languages(&self) -> Vec<&ColumnSchema> {
        self.columns.iter().filter(|c| c.is_lang()).collect()
    }

    pub fn base_language(&self) -> Option<&ColumnSchema> {
        self.columns.iter().find(|c| c.is_lang() && c.is_role("base"))
    }
}

#[derive(Clone, Debug)]
pub struct ParsedType {
    pub type_name: String,
    pub role: Option<String>,
    pub default_text: Option<String>,
    pub ref_target: Option<String>,
    pub ref_key: Option<String>,
    pub ref_scope: Option<String>,
}

/// (resolved type, role "id"/"subkey"/"field", scope) of every key and field, by "Table" and
/// "Table.Field".
pub type TableKeys = HashMap<String, (String, String, String)>;

#[allow(clippy::too_many_arguments)]
pub fn parse_type(
    value: &Cell,
    sheet: &str,
    cell: &str,
    errors: &mut ErrorCollector,
    table_keys: Option<&TableKeys>,
    resolved: Option<&mut HashMap<String, String>>,
) -> Option<ParsedType> {
    let mut text = value.text_or_empty();
    let mut default_text: Option<String> = None;
    if let Some((head, tail)) = text.clone().split_once('=') {
        text = head.trim().to_string();
        default_text = Some(tail.trim().to_string());
    }
    let mut role: Option<String> = None;
    if BASE_RE.is_match(&text) {
        role = Some("base".into());
        text = "lang".into();
    }
    let captures = REF_ROLE_RE.captures(&text).or_else(|| ROLE_RE.captures(&text));
    if let Some(captures) = captures {
        role = Some(captures[1].to_lowercase());
        text = captures[2].trim().to_string();
    }
    let ref_spec = REF_RE.captures(&text).map(|c| c[1].to_string());
    let (ref_target, ref_key) = match &ref_spec {
        Some(spec) => match spec.split_once('.') {
            Some((t, k)) => (Some(t.to_string()), if k.is_empty() { None } else { Some(k.to_string()) }),
            None => (Some(spec.clone()), None),
        },
        None => (None, None),
    };
    let mut ref_scope: Option<String> = None;
    if let (Some(target), Some(spec)) = (&ref_target, &ref_spec) {
        if role.as_deref() == Some("id") {
            errors.add(sheet, cell, tr("ID<Ref<T>>는 기본키로 사용할 수 없습니다", "ID<Ref<T>> cannot be a primary key"));
            return None;
        }
        if default_text.is_some() {
            errors.add(sheet, cell, tr("Ref<T>에는 기본값을 지정할 수 없습니다", "Ref<T> cannot have a default value"));
            return None;
        }
        if let Some(keys) = table_keys {
            if !keys.contains_key(target) {
                errors.add(sheet, cell, tr(
                    format!("참조 대상 테이블 '{target}'이 없습니다"),
                    format!("referenced table '{target}' does not exist"),
                ));
                return None;
            }
            if ref_key.is_some() {
                let Some((_, target_role, scope)) = keys.get(spec) else {
                    errors.add(sheet, cell, tr(
                        format!("참조 대상 필드 '{spec}'이 없습니다"),
                        format!("referenced field '{spec}' does not exist"),
                    ));
                    return None;
                };
                ref_scope = Some(scope.clone());
                if target_role == "id" {
                    errors.add(sheet, cell, tr(
                        format!("기본키 '{spec}'에는 Ref<{target}>를 쓰세요"),
                        format!("'{spec}' is the primary key; use Ref<{target}>"),
                    ));
                    return None;
                }
                if target_role != "subkey" {
                    errors.add(sheet, cell, tr(
                        format!("'{spec}'을 SubKey로 선언하세요"),
                        format!("declare '{spec}' as a SubKey to reference it"),
                    ));
                    return None;
                }
            } else {
                ref_scope = Some(keys[target].2.clone());
            }
            let mut local = HashMap::new();
            let cache = match resolved {
                Some(cache) => cache,
                None => &mut local,
            };
            text = resolve(spec, &mut Vec::new(), keys, cache, sheet, cell, errors)?;
        } else {
            text = format!("Ref<{spec}>");
        }
    }
    if FIXED_RE.is_match(&text) {
        let (scale, wide) = fixed_of(&text)?;
        let limit: i64 = if wide { 1_000_000_000_000_000_000 } else { 1_000_000_000 };
        let mut power = 10i64;
        while power < scale && power < limit {
            power *= 10;
        }
        if scale < 10 || power != scale || scale > limit {
            errors.add(sheet, cell, tr(
                format!("고정소수점 배율은 10, 100, 1000 …처럼 10의 거듭제곱이어야 합니다(최대 {limit}): '{text}'"),
                format!("a fixed-point scale must be a power of ten such as 10, 100, 1000 (at most {limit}): '{text}'"),
            ));
            return None;
        }
        text = format!("fixed{}<{scale}>", if wide { "64" } else { "" });
    }
    if text.eq_ignore_ascii_case("datetime") || text.eq_ignore_ascii_case("duration") {
        text = text.to_lowercase();
    } else if DATETIME_RE.is_match(&text) {
        let offset = datetime_offset(&text).unwrap_or(i32::MAX);
        if offset.abs() > 14 * 60 {
            errors.add(sheet, cell, tr(
                format!("시간대는 -14:00 ~ +14:00이어야 합니다: '{text}'"),
                format!("the UTC offset must be between -14:00 and +14:00: '{text}'"),
            ));
            return None;
        }
        // Canonical form: datetime for UTC, datetime<+09:00> otherwise.
        let sign = if offset < 0 { '-' } else { '+' };
        text = if offset == 0 { "datetime".into() } else { format!("datetime<{sign}{:02}:{:02}>", offset.abs() / 60, offset.abs() % 60) };
    }
    if !PRIMITIVES.contains(&text.as_str()) && enum_of(&text).is_none() && ref_target.is_none() && fixed_of(&text).is_none() && datetime_offset(&text).is_none() {
        errors.add(sheet, cell, tr(format!("알 수 없는 자료형 '{text}'"), format!("unknown type '{text}'")));
        return None;
    }
    if role.is_some()
        && role.as_deref() != Some("base")
        && !KEY_PRIMITIVES.contains(&text.as_str())
        && enum_of(&text).is_none()
        && ref_target.is_none()
    {
        errors.add(sheet, cell, tr(
            format!("{text} 자료형은 기본키나 서브키로 사용할 수 없습니다. 키에는 int32, int64, name, 열거형(E*)만 사용할 수 있습니다"),
            format!("{text} cannot be a primary key or sub key. Keys must be int32, int64, name or an enum (E*)"),
        ));
        return None;
    }
    if role.is_some() && default_text.is_some() {
        errors.add(sheet, cell, tr("기본키와 서브키에는 기본값을 지정할 수 없습니다", "primary keys and sub keys cannot have a default value"));
        default_text = None;
    }
    Some(ParsedType { type_name: text, role, default_text, ref_target, ref_key, ref_scope })
}

fn resolve(
    spec: &str,
    path: &mut Vec<String>,
    keys: &TableKeys,
    cache: &mut HashMap<String, String>,
    sheet: &str,
    cell: &str,
    errors: &mut ErrorCollector,
) -> Option<String> {
    if let Some(found) = cache.get(spec) {
        return Some(found.clone());
    }
    if let Some(position) = path.iter().position(|p| p == spec) {
        let mut cycle: Vec<String> = path[position..].to_vec();
        cycle.push(spec.to_string());
        let joined = cycle.join(" → ");
        errors.add(sheet, cell, tr(format!("자료형 순환: {joined}"), format!("type cycle: {joined}")));
        return None;
    }
    let Some((candidate, _, _)) = keys.get(spec) else {
        errors.add(sheet, cell, tr(
            format!("참조 '{spec}'의 자료형을 결정할 수 없습니다"),
            format!("cannot resolve the type of reference '{spec}'"),
        ));
        return None;
    };
    let mut candidate = candidate.clone();
    if candidate.starts_with("Ref<") && candidate.ends_with('>') {
        path.push(spec.to_string());
        let inner = candidate[4..candidate.len() - 1].to_string();
        let result = resolve(&inner, path, keys, cache, sheet, cell, errors);
        path.pop();
        candidate = result?;
    }
    cache.insert(spec.to_string(), candidate.clone());
    Some(candidate)
}

pub fn validate_enum_type(type_name: &str, enums: &Enums, sheet: &str, cell: &str, errors: &mut ErrorCollector) {
    if let Some(name) = enum_of(type_name)
        && !enums.contains_key(name) {
            errors.add(sheet, cell, tr(format!("정의되지 않은 열거형 '{name}'"), format!("undefined enum '{name}'")));
        }
}

/// (index, name, type, scope) of one field definition.
pub type RawColumn = (usize, Cell, Cell, Cell);

/// Turns field definitions into logical fields, grouping Name[0], Name[1], ... into arrays.
/// `cell_of(index, header_row)` gives the cell to report for a definition.
pub fn build_columns(
    raw_columns: &[RawColumn],
    sheet: &str,
    enums: &Enums,
    errors: &mut ErrorCollector,
    table_keys: Option<&TableKeys>,
    cell_of: &dyn Fn(usize, usize) -> String,
) -> Vec<ColumnSchema> {
    struct ArrayPart {
        column: usize,
        index: usize,
        parsed: Option<ParsedType>,
        scope: String,
        cell: String,
    }
    let mut scalars: Vec<(usize, ColumnSchema)> = Vec::new();
    let mut arrays: Vec<(String, Vec<ArrayPart>)> = Vec::new();
    let mut scalar_headers: HashMap<String, String> = HashMap::new();
    let mut resolved: HashMap<String, String> = HashMap::new();

    for (column_index, raw_name, raw_type, raw_scope) in raw_columns {
        let column_index = *column_index;
        let header_cell = cell_of(column_index, NAME_ROW);
        let type_cell = cell_of(column_index, TYPE_ROW);
        let scope_cell = cell_of(column_index, SCOPE_ROW);
        let name = raw_name.py_str().trim().to_string();
        let raw_scope_text = raw_scope.text_or_empty();
        let lowered = raw_scope_text.to_lowercase();
        if !SCOPES.contains(&lowered.as_str()) {
            errors.add(sheet, &scope_cell, tr(
                format!("범위는 all, client, server, # 중 하나여야 합니다: '{raw_scope_text}'"),
                format!("scope must be one of all, client, server, #: '{raw_scope_text}'"),
            ));
            continue;
        }
        let scope = lowered;
        if scope == "#" {
            continue;
        }
        let array_match = ARRAY_RE.captures(&name);
        let base_name = array_match.as_ref().map(|c| c[1].to_string()).unwrap_or_else(|| name.clone());
        if array_match.is_none() && !is_identifier(&name) {
            errors.add(sheet, &header_cell, tr(format!("올바르지 않은 필드명 '{name}'"), format!("invalid field name '{name}'")));
            continue;
        }
        let parsed = parse_type(raw_type, sheet, &type_cell, errors, table_keys, Some(&mut resolved));
        if let Some(parsed) = &parsed {
            validate_enum_type(&parsed.type_name, enums, sheet, &type_cell, errors);
            if let (Some(_), Some(ref_scope)) = (&parsed.ref_key, &parsed.ref_scope) {
                let outputs = |s: &str| -> Vec<&'static str> {
                    match s {
                        "all" => vec!["client", "server"],
                        "client" => vec!["client"],
                        "server" => vec!["server"],
                        _ => vec![],
                    }
                };
                let target = outputs(ref_scope);
                if !outputs(&scope).iter().all(|s| target.contains(s)) {
                    errors.add(sheet, &type_cell, tr(
                        format!("참조 범위 {scope}가 대상 서브키 범위 {ref_scope}보다 넓습니다"),
                        format!("reference scope {scope} is wider than the target sub key scope {ref_scope}"),
                    ));
                }
            }
        }
        if let Some(captures) = &array_match {
            let index: usize = captures[2].parse().unwrap_or(usize::MAX);
            let part = ArrayPart { column: column_index, index, parsed, scope: scope.clone(), cell: header_cell.clone() };
            match arrays.iter_mut().find(|(n, _)| *n == base_name) {
                Some((_, parts)) => parts.push(part),
                None => arrays.push((base_name.clone(), vec![part])),
            }
        } else if let Some(parsed) = &parsed {
            let default_value = convert_default(parsed, enums, sheet, &type_cell, errors);
            scalars.push((
                column_index,
                ColumnSchema {
                    name: name.clone(),
                    type_name: parsed.type_name.clone(),
                    role: parsed.role.clone(),
                    scope: scope.clone(),
                    source_columns: vec![column_index],
                    header_cells: vec![header_cell.clone()],
                    array_size: None,
                    default_values: vec![default_value],
                    ref_target: parsed.ref_target.clone(),
                    ref_key: parsed.ref_key.clone(),
                    ref_strings: false,
                    alias: None,
                },
            ));
        }
        if array_match.is_none() {
            if scalar_headers.contains_key(&base_name) {
                errors.add(sheet, &header_cell, tr(
                    format!("필드명 '{base_name}'이 중복되었습니다"),
                    format!("duplicate field name '{base_name}'"),
                ));
            } else {
                scalar_headers.insert(base_name.clone(), header_cell.clone());
            }
        }
    }

    let mut grouped: Vec<(usize, ColumnSchema)> = Vec::new();
    let scalar_names: BTreeSet<String> = scalars.iter().map(|(_, c)| c.name.clone()).collect();
    for (name, parts) in &arrays {
        let mut by_position: Vec<&ArrayPart> = parts.iter().collect();
        by_position.sort_by_key(|p| p.column);
        let mut by_index: Vec<&ArrayPart> = parts.iter().collect();
        by_index.sort_by_key(|p| (p.index, p.column));
        let first = by_index[0];
        let group_cell = by_position[0].cell.clone();
        if scalar_names.contains(name) {
            errors.add(sheet, &first.cell, tr(
                format!("필드명 '{name}'이 스칼라와 배열로 중복되었습니다"),
                format!("field '{name}' is used both as a scalar and as an array"),
            ));
        }
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        for part in &by_position {
            if seen.contains(&part.index) {
                errors.add(sheet, &part.cell, tr(
                    format!("배열 '{name}'의 인덱스 {}가 중복되었습니다", part.index),
                    format!("array '{name}' has index {} twice", part.index),
                ));
            }
            seen.insert(part.index);
        }
        if !seen.iter().copied().eq(0..seen.len()) {
            errors.add(sheet, &group_cell, tr(
                format!("배열 '{name}'의 인덱스는 0부터 연속이어야 합니다"),
                format!("array '{name}' indices must start at 0 without gaps"),
            ));
        }
        let first_type = first.parsed.as_ref();
        if first_type.is_some_and(|t| t.role.is_some()) {
            errors.add(sheet, &cell_of(first.column, TYPE_ROW), tr(
                format!("배열 '{name}'은 키로 지정할 수 없습니다"),
                format!("array '{name}' cannot be a key"),
            ));
        }
        for part in &by_index[1..] {
            if let (Some(parsed), Some(first_type)) = (&part.parsed, first_type)
                && (&parsed.type_name, &parsed.ref_target, &parsed.ref_key)
                    != (&first_type.type_name, &first_type.ref_target, &first_type.ref_key)
                {
                    errors.add(sheet, &cell_of(part.column, TYPE_ROW), tr(
                        format!("배열 '{name}'의 자료형이 일치하지 않습니다"),
                        format!("array '{name}' elements have different types"),
                    ));
                }
            if part.parsed.as_ref().is_some_and(|p| p.role.is_some()) {
                errors.add(sheet, &cell_of(part.column, TYPE_ROW), tr(
                    format!("배열 '{name}'은 키로 지정할 수 없습니다"),
                    format!("array '{name}' cannot be a key"),
                ));
            }
            if part.scope != first.scope {
                errors.add(sheet, &cell_of(part.column, SCOPE_ROW), tr(
                    format!("배열 '{name}'의 범위가 일치하지 않습니다"),
                    format!("array '{name}' elements have different scopes"),
                ));
            }
        }
        if let Some(first_type) = first_type {
            let defaults = by_index
                .iter()
                .map(|part| match &part.parsed {
                    Some(parsed) => convert_default(parsed, enums, sheet, &cell_of(part.column, TYPE_ROW), errors),
                    None => None,
                })
                .collect();
            grouped.push((
                parts.iter().map(|p| p.column).min().unwrap_or(0),
                ColumnSchema {
                    name: name.clone(),
                    type_name: first_type.type_name.clone(),
                    role: None,
                    scope: first.scope.clone(),
                    source_columns: by_index.iter().map(|p| p.column).collect(),
                    header_cells: by_index.iter().map(|p| p.cell.clone()).collect(),
                    array_size: Some(seen.len()),
                    default_values: defaults,
                    ref_target: first_type.ref_target.clone(),
                    ref_key: first_type.ref_key.clone(),
                    ref_strings: false,
                    alias: None,
                },
            ));
        }
    }

    let mut all: Vec<(usize, ColumnSchema)> = scalars.into_iter().chain(grouped).collect();
    all.sort_by_key(|(index, _)| *index);
    let columns: Vec<ColumnSchema> = all.into_iter().map(|(_, c)| c).collect();
    let ids: Vec<&ColumnSchema> = columns.iter().filter(|c| c.is_role("id")).collect();
    if ids.len() != 1 {
        let first = raw_columns.first().map(|c| c.0).unwrap_or(1);
        errors.add(sheet, &cell_of(first, TYPE_ROW), tr(
            format!("기본키는 정확히 1개여야 합니다(현재 {}개)", ids.len()),
            format!("exactly one primary key is required (found {})", ids.len()),
        ));
    } else if ids[0].scope != "all" {
        errors.add(sheet, &cell_of(ids[0].source_columns[0], SCOPE_ROW), tr(
            "기본키 범위는 all이어야 합니다",
            "the primary key scope must be all",
        ));
    }
    columns
}

fn convert_default(
    parsed: &ParsedType,
    enums: &Enums,
    sheet: &str,
    cell: &str,
    errors: &mut ErrorCollector,
) -> Option<Value> {
    let text = parsed.default_text.as_ref()?;
    convert_value(&Cell::Str(text.clone()), &parsed.type_name, enums, sheet, cell, errors, false)
}

/// Structure hash: fields, key roles, scopes, defaults and the (name, value) list of every
/// enum the table uses (baked enum key indices are sorted by value).
pub fn calculate_schema_hash(columns: &[ColumnSchema], enums: Option<&Enums>) -> String {
    let mut entries = Vec::new();
    for column in columns {
        let type_text = match (&column.ref_target, &column.ref_key) {
            (Some(target), Some(key)) => format!("Ref<{target}.{key}>"),
            (Some(target), None) => format!("Ref<{target}>"),
            _ => column.type_name.clone(),
        };
        let mut entry = vec![
            ("name".to_string(), Json::from(column.name.as_str())),
            ("type".to_string(), Json::Str(type_text)),
            ("role".to_string(), column.role.as_deref().map(Json::from).unwrap_or(Json::Null)),
            ("scope".to_string(), Json::from(column.scope.as_str())),
            ("array_size".to_string(), column.array_size.map(|n| Json::Int(n as i64)).unwrap_or(Json::Null)),
        ];
        if column.default_values.iter().any(Option::is_some) {
            let defaults = column
                .default_values
                .iter()
                .map(|value| {
                    object([
                        ("declared", Json::Bool(value.is_some())),
                        ("value", value.as_ref().map(Json::from).unwrap_or(Json::Null)),
                    ])
                })
                .collect();
            entry.push(("defaults".to_string(), Json::List(defaults)));
        }
        entries.push(Json::Object(entry));
    }
    let used: BTreeSet<&str> = columns.iter().filter_map(|c| enum_of(&c.type_name)).collect();
    let payload = match enums {
        Some(enums) if !used.is_empty() => object([
            ("columns", Json::List(entries)),
            (
                "enums",
                Json::Object(
                    used.iter()
                        .filter_map(|name| enums.get(*name).map(|e| (name.to_string(), e)))
                        .map(|(name, e)| {
                            let values = e
                                .values
                                .iter()
                                .map(|v| Json::List(vec![Json::from(v.name.as_str()), Json::Int(v.value)]))
                                .collect();
                            (name, Json::List(values))
                        })
                        .collect(),
                ),
            ),
        ]),
        _ => Json::List(entries),
    };
    format!("sha256:{}", sha256_hex(payload.compact_sorted().as_bytes()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Excel column letters for a 1-based column index, then the row: (28, 3) -> "AB3".
pub fn cell_name(column: usize, row: usize) -> String {
    format!("{}{row}", column_letter(column))
}

pub fn column_letter(mut column: usize) -> String {
    let mut letters = Vec::new();
    while column > 0 {
        let remainder = (column - 1) % 26;
        letters.push((b'A' + remainder as u8) as char);
        column = (column - 1) / 26;
    }
    letters.iter().rev().collect()
}
