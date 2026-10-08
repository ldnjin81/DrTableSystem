//! 생성된 JSON 출력 폴더 두 개를 기본키 기준으로 비교한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::errors::ErrorCollector;
use crate::{excel, schema, schemafile};
use crate::i18n::tr;

#[derive(Clone, Copy)]
pub enum Format {
    Text,
    Markdown,
    Json,
}

#[derive(Clone)]
struct Change {
    kind: &'static str,
    table: String,
    key: Option<Value>,
    field: Option<String>,
    index: Option<usize>,
    before: Option<Value>,
    after: Option<Value>,
}

type FixedScales = BTreeMap<String, BTreeMap<String, i64>>;

fn fixed_scales(folder: &Path) -> Result<FixedScales, String> {
    let mut errors = ErrorCollector::default();
    let enums = schemafile::enum_folder(folder, None, true);
    let mut schemas = schemafile::load_schemas(folder, &enums, &mut errors)
        .map_err(|e| e.0.join("\n"))?;
    excel::expand_aliases(&mut schemas, &mut errors);
    if !errors.messages.is_empty() { return Err(errors.messages.join("\n")); }
    let mut result = FixedScales::new();
    for (table, definition) in &schemas.tables {
        if definition.is_strings { continue; }
        let mut fields = BTreeMap::new();
        for (row, name, type_cell, scope, _) in &definition.rows {
            if scope.text_or_empty().trim() == "#" { continue; }
            let Some(parsed) = schema::parse_type(type_cell, &definition.where_(), &format!("B{row}"), &mut errors, None, None) else { continue; };
            if let Some((scale, _)) = schema::fixed_of(&parsed.type_name) {
                fields.insert(name.text_or_empty().trim().to_string(), scale);
            }
        }
        result.insert(table.clone(), fields);
    }
    if !errors.messages.is_empty() { return Err(errors.messages.join("\n")); }
    Ok(result)
}

fn decimal(value: &Value, scale: i64) -> Value {
    let Some(raw) = value.as_i64().map(i128::from).or_else(|| value.as_u64().map(i128::from)) else { return value.clone(); };
    let scale = i128::from(scale);
    let digits = scale.to_string().len() - 1;
    let sign = if raw < 0 { "-" } else { "" };
    let whole = raw.abs() / scale;
    let fraction = raw.abs() % scale;
    let mut text = format!("{sign}{whole}.{fraction:0digits$}");
    while text.ends_with('0') { text.pop(); }
    if text.ends_with('.') { text.pop(); }
    Value::String(text)
}

fn display_value(value: &Value, scale: Option<i64>) -> Value {
    match (value, scale) {
        (Value::Array(items), Some(scale)) => Value::Array(items.iter().map(|item| decimal(item, scale)).collect()),
        (_, Some(scale)) => decimal(value, scale),
        _ => value.clone(),
    }
}

fn display_row(row: &Map<String, Value>, scales: &BTreeMap<String, i64>) -> Map<String, Value> {
    row.iter().map(|(field, value)| (field.clone(), display_value(value, scales.get(field).copied()))).collect()
}

impl Change {
    fn new(kind: &'static str, table: &str) -> Self {
        Self { kind, table: table.into(), key: None, field: None, index: None, before: None, after: None }
    }

    fn json(&self) -> Value {
        let mut entry = Map::new();
        entry.insert("kind".into(), json!(self.kind));
        entry.insert("table".into(), json!(self.table));
        if let Some(key) = &self.key { entry.insert("key".into(), key.clone()); }
        if let Some(field) = &self.field { entry.insert("field".into(), json!(field)); }
        if let Some(index) = self.index { entry.insert("index".into(), json!(index)); }
        if let Some(before) = &self.before { entry.insert("before".into(), before.clone()); }
        if let Some(after) = &self.after { entry.insert("after".into(), after.clone()); }
        Value::Object(entry)
    }
}

fn error(path: &Path, message: impl AsRef<str>) -> String {
    format!("[{}]JSON!A1: {}", path.display(), message.as_ref())
}

fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| error(path, tr(format!("파일을 읽을 수 없습니다: {e}"), format!("cannot read file: {e}"))))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| error(path, tr(format!("JSON을 읽을 수 없습니다: {e}"), format!("cannot read JSON: {e}"))))?;
    if !value.is_object() { return Err(error(path, tr("JSON 최상위 값은 객체여야 합니다", "the top-level JSON value must be an object"))); }
    Ok(value)
}

fn entries<'a>(manifest: &'a Value, field: &str, path: &Path) -> Result<BTreeMap<String, &'a Value>, String> {
    let Some(array) = manifest.get(field).and_then(Value::as_array).map(Vec::as_slice).or_else(|| if field == "string_tables" { Some(&[]) } else { None }) else {
        return Err(error(path, tr(format!("{field} 배열이 필요합니다"), format!("a '{field}' array is required"))));
    };
    let mut result = BTreeMap::new();
    for item in array {
        let Some(name) = item.get("name").and_then(Value::as_str) else { return Err(error(path, tr(format!("{field} 항목에 이름이 없습니다"), format!("a '{field}' entry has no name")))); };
        if result.insert(name.to_string(), item).is_some() { return Err(error(path, tr(format!("{field}에 '{name}'이 중복되었습니다"), format!("duplicate '{name}' in '{field}'")))); }
    }
    Ok(result)
}

fn key_id(value: &Value) -> String {
    format!("{}:{}", if value.is_string() { "s" } else { "n" }, value)
}

fn rows(payload: &Value, path: &Path) -> Result<BTreeMap<String, (Value, Map<String, Value>)>, String> {
    let Some(primary) = payload.get("primary_key").and_then(Value::as_str) else { return Err(error(path, tr("기본키가 없습니다", "primary_key is missing"))); };
    let Some(source) = payload.get("rows").and_then(Value::as_array) else { return Err(error(path, tr("rows 배열이 없습니다", "rows array is missing"))); };
    let pools: BTreeMap<String, Vec<Value>> = payload.get("arrays").and_then(Value::as_array).into_iter().flatten().filter_map(|entry| {
        Some((entry.get("field")?.as_str()?.to_string(), entry.get("pool")?.as_array()?.clone()))
    }).collect();
    let mut result = BTreeMap::new();
    for row in source {
        let Some(fields) = row.as_object() else { return Err(error(path, tr("잘못된 행 객체입니다", "invalid row object"))); };
        let Some(key) = fields.get(primary).filter(|v| v.is_string() || v.is_i64() || v.is_u64()) else { return Err(error(path, tr("기본키 값이 없거나 잘못되었습니다", "missing or invalid primary key value"))); };
        let mut normalized = fields.clone();
        for (field, pool) in &pools {
            let (start_name, num_name) = (format!("{field}_Start"), format!("{field}_Num"));
            let (Some(start), Some(num)) = (normalized.remove(&start_name).and_then(|v| v.as_u64()), normalized.remove(&num_name).and_then(|v| v.as_u64())) else {
                return Err(error(path, tr(format!("{field} 배열 범위가 없습니다"), format!("array range for '{field}' is missing"))));
            };
            let (start, num) = (start as usize, num as usize);
            let Some(end) = start.checked_add(num).filter(|end| *end <= pool.len()) else { return Err(error(path, tr(format!("{field} 배열 범위가 잘못되었습니다"), format!("array range for '{field}' is invalid")))); };
            normalized.insert(field.clone(), Value::Array(pool[start..end].to_vec()));
        }
        if result.insert(key_id(key), (key.clone(), normalized)).is_some() { return Err(error(path, tr("기본키가 중복되었습니다", "duplicate primary key"))); }
    }
    Ok(result)
}

fn table_changes(old_dir: &Path, new_dir: &Path, name: &str, old: &Value, new: &Value, scales: Option<&BTreeMap<String, i64>>, changes: &mut Vec<Change>) -> Result<(), String> {
    let old_path = old_dir.join(format!("{name}.json"));
    let new_path = new_dir.join(format!("{name}.json"));
    let old_rows = rows(&read(&old_path)?, &old_path)?;
    let new_rows = rows(&read(&new_path)?, &new_path)?;
    if old.get("schema_hash") != new.get("schema_hash") {
        let mut change = Change::new("schema_changed", name);
        change.before = old.get("schema_hash").cloned();
        change.after = new.get("schema_hash").cloned();
        changes.push(change);
    }
    for id in old_rows.keys().chain(new_rows.keys()).collect::<BTreeSet<_>>() {
        match (old_rows.get(id), new_rows.get(id)) {
            (None, Some((key, row))) | (Some((key, row)), None) => {
                let added = !old_rows.contains_key(id);
                let mut change = Change::new(if added { "row_added" } else { "row_removed" }, name);
                change.key = Some(key.clone());
                let shown = scales.map(|s| display_row(row, s)).unwrap_or_else(|| row.clone());
                if added { change.after = Some(Value::Object(shown)); } else { change.before = Some(Value::Object(shown)); }
                changes.push(change);
            }
            (Some((key, before)), Some((_, after))) => {
                for field in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
                    if *field == old.get("primary_key").and_then(Value::as_str).unwrap_or("") { continue; }
                    let a = before.get(field);
                    let b = after.get(field);
                    if a == b { continue; }
                    if let (Some(Value::Array(left)), Some(Value::Array(right))) = (a, b) {
                        for index in 0..left.len().max(right.len()) {
                            if left.get(index) == right.get(index) { continue; }
                            let mut change = Change::new("field_changed", name);
                            change.key = Some(key.clone()); change.field = Some((*field).clone()); change.index = Some(index);
                            let scale = scales.and_then(|s| s.get(field)).copied();
                            change.before = left.get(index).map(|v| display_value(v, scale)); change.after = right.get(index).map(|v| display_value(v, scale));
                            changes.push(change);
                        }
                    } else {
                        let mut change = Change::new("field_changed", name);
                        change.key = Some(key.clone()); change.field = Some((*field).clone());
                        let scale = scales.and_then(|s| s.get(field)).copied();
                        change.before = a.map(|v| display_value(v, scale)); change.after = b.map(|v| display_value(v, scale));
                        changes.push(change);
                    }
                }
            }
            (None, None) => unreachable!(),
        }
    }
    Ok(())
}

fn string_rows(dir: &Path, table: &str, lang: &str) -> Result<BTreeMap<String, (Value, Value)>, String> {
    let path = dir.join("Strings").join(lang).join(format!("{table}.json"));
    let data = read(&path)?;
    let (Some(keys), Some(values)) = (data.get("keys").and_then(Value::as_array), data.get("values").and_then(Value::as_array)) else { return Err(error(&path, tr("keys와 values 배열이 필요합니다", "keys and values arrays are required"))); };
    if keys.len() != values.len() { return Err(error(&path, tr("keys와 values 길이가 다릅니다", "keys and values have different lengths"))); }
    let mut result = BTreeMap::new();
    for (key, value) in keys.iter().zip(values) {
        if result.insert(key_id(key), (key.clone(), value.clone())).is_some() { return Err(error(&path, tr("문자열 키가 중복되었습니다", "duplicate string key"))); }
    }
    Ok(result)
}

fn string_changes(old_dir: &Path, new_dir: &Path, table: &str, old: &Value, new: &Value, changes: &mut Vec<Change>) -> Result<(), String> {
    if old.get("schema_hash") != new.get("schema_hash") {
        let mut change = Change::new("schema_changed", table);
        change.before = old.get("schema_hash").cloned(); change.after = new.get("schema_hash").cloned(); changes.push(change);
    }
    let languages = |entry: &Value| -> BTreeSet<String> { entry.get("languages").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect() };
    let (old_langs, new_langs) = (languages(old), languages(new));
    for lang in old_langs.union(&new_langs) {
        if !old_langs.contains(lang) || !new_langs.contains(lang) {
            let mut change = Change::new(if new_langs.contains(lang) { "language_added" } else { "language_removed" }, table);
            change.field = Some(lang.clone()); changes.push(change); continue;
        }
        let (before, after) = (string_rows(old_dir, table, lang)?, string_rows(new_dir, table, lang)?);
        for id in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
            let a = before.get(id); let b = after.get(id);
            if a.map(|(_, v)| v) == b.map(|(_, v)| v) { continue; }
            let mut change = Change::new("string_changed", table);
            change.field = Some(lang.clone()); change.key = a.or(b).map(|(key, _)| key.clone());
            change.before = a.map(|(_, value)| value.clone()); change.after = b.map(|(_, value)| value.clone());
            changes.push(change);
        }
    }
    Ok(())
}

fn shown(value: Option<&Value>) -> String {
    match value {
        None => "∅".into(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

fn label(kind: &str) -> String {
    match kind {
        "table_added" => tr("테이블 추가", "Table added"),
        "table_removed" => tr("테이블 삭제", "Table removed"),
        "schema_changed" => tr("스키마 변경", "Schema changed"),
        "row_added" => tr("행 추가", "Row added"),
        "row_removed" => tr("행 삭제", "Row removed"),
        "field_changed" => tr("필드 변경", "Field changed"),
        "string_changed" => tr("문구 변경", "Text changed"),
        "language_added" => tr("언어 추가", "Language added"),
        "language_removed" => tr("언어 삭제", "Language removed"),
        _ => kind.into(),
    }
}

fn md(text: &str) -> String { text.replace('|', "\\|").replace('\n', "<br>") }

pub fn compare(old_dir: &Path, new_dir: &Path, format: Format, table: Option<&str>, schema_dir: Option<&Path>) -> Result<(String, bool), String> {
    let old_path = old_dir.join("manifest.json"); let new_path = new_dir.join("manifest.json");
    let (old, new) = (read(&old_path)?, read(&new_path)?);
    let scales = schema_dir.map(fixed_scales).transpose()?;
    let mut changes = Vec::new();
    for field in ["tables", "string_tables"] {
        let (before, after) = (entries(&old, field, &old_path)?, entries(&new, field, &new_path)?);
        for name in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
            if table.is_some_and(|filter| filter != *name) { continue; }
            match (before.get(name), after.get(name)) {
                (None, Some(_)) => changes.push(Change::new("table_added", name)),
                (Some(_), None) => changes.push(Change::new("table_removed", name)),
                (Some(a), Some(b)) if field == "tables" => table_changes(old_dir, new_dir, name, a, b, scales.as_ref().and_then(|s| s.get(name)), &mut changes)?,
                (Some(a), Some(b)) => string_changes(old_dir, new_dir, name, a, b, &mut changes)?,
                _ => unreachable!(),
            }
        }
    }
    let changed = !changes.is_empty();
    let note = if schema_dir.is_none() { Some(tr("fixed 배율을 알 수 없습니다. --schema <스키마 폴더>를 주면 소수로 표시합니다.", "Fixed-point scales are unknown. Pass --schema <schema folder> to display decimals.")) } else { None };
    let output = match format {
        Format::Json => {
            let mut report = Map::new();
            if let Some(note) = &note { report.insert("note".into(), json!(note)); }
            report.insert("changes".into(), json!(changes.iter().map(Change::json).collect::<Vec<_>>()));
            serde_json::to_string_pretty(&Value::Object(report)).unwrap() + "\n"
        },
        Format::Text if !changed => format!("{}\n", tr("변경 없음", "No changes")),
        Format::Markdown if !changed => format!("{}\n", tr("변경 없음", "No changes")),
        Format::Text => changes.iter().map(|c| {
            let key = c.key.as_ref().map(|v| format!(" [{}]", shown(Some(v)))).unwrap_or_default();
            let field = c.field.as_ref().map(|f| format!(" {f}{}", c.index.map(|i| format!("[{i}]")).unwrap_or_default())).unwrap_or_default();
            format!("{}: {}{}{}: {} → {}\n", label(c.kind), c.table, key, field, shown(c.before.as_ref()), shown(c.after.as_ref()))
        }).collect(),
        Format::Markdown => {
            let mut out = format!("| {} | {} | {} | {} | {} | {} |\n|---|---|---|---|---|---|\n", tr("변경", "Change"), tr("테이블", "Table"), tr("키", "Key"), tr("필드", "Field"), tr("이전", "Before"), tr("이후", "After"));
            for c in &changes {
                let field = c.field.as_ref().map(|f| format!("{f}{}", c.index.map(|i| format!("[{i}]")).unwrap_or_default())).unwrap_or_default();
                out.push_str(&format!("| {} | {} | {} | {} | {} | {} |\n", md(&label(c.kind)), md(&c.table), md(&shown(c.key.as_ref())), md(&field), md(&shown(c.before.as_ref())), md(&shown(c.after.as_ref()))));
            }
            out
        }
    };
    let output = if let Some(note) = note {
        match format {
            Format::Json => output,
            Format::Markdown => format!("{note}\n\n{output}"),
            Format::Text => format!("{note}\n{output}"),
        }
    } else { output };
    Ok((output, changed))
}
