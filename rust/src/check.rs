//! Checks the referential integrity of generated JSON (`drtable check`). Reads only the JSON
//! and manifest written by `drtable build`, so it can run in CI on the build output.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{Map, Value as J};

use crate::i18n::tr;
use crate::value::py_str_repr;

/// The check input (manifest or table JSON) is malformed.
pub struct CheckInputError(pub String);

fn read_json(path: &Path) -> Result<Map<String, J>, CheckInputError> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let text = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        // The wording this tool has always used for a missing file.
        std::io::ErrorKind::NotFound => format!("[Errno 2] No such file or directory: {}", py_str_repr(&path.display().to_string())),
        _ => e.to_string(),
    });
    let data = text.and_then(|t| serde_json::from_str::<J>(&t).map_err(|e| e.to_string()));
    match data {
        Err(error) => Err(CheckInputError(tr(
            format!("{name}!A1: JSON을 읽을 수 없습니다: {error}"),
            format!("{name}!A1: cannot read JSON: {error}"),
        ))),
        Ok(J::Object(map)) => Ok(map),
        Ok(_) => Err(CheckInputError(tr(
            format!("{name}!A1: JSON 최상위 값은 객체여야 합니다"),
            format!("{name}!A1: the top-level JSON value must be an object"),
        ))),
    }
}

fn input_error(file: &str, ko: impl Into<String>, en: impl Into<String>) -> CheckInputError {
    CheckInputError(format!("{file}!A1: {}", tr(ko, en)))
}

/// A key value as it can appear in a table: an integer or a string.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    Str(String),
}

impl Key {
    fn of(value: &J) -> Option<Key> {
        match value {
            J::Number(n) if n.is_i64() => Some(Key::Int(n.as_i64()?)),
            J::Number(n) if n.is_u64() => Some(Key::Int(n.as_u64()? as i64)),
            J::String(s) => Some(Key::Str(s.clone())),
            _ => None,
        }
    }

    fn text(&self) -> String {
        match self {
            Key::Int(i) => i.to_string(),
            Key::Str(s) => s.clone(),
        }
    }
}

/// Returns (failures, warnings) for one output directory (client or server).
pub fn check_directory(directory: &Path) -> Result<(Vec<String>, Vec<String>), CheckInputError> {
    let manifest = read_json(&directory.join("manifest.json"))?;
    let (Some(J::Array(tables)), Some(J::Array(references))) = (manifest.get("tables"), manifest.get("references")) else {
        return Err(input_error("manifest.json", "tables와 references 배열이 필요합니다", "'tables' and 'references' arrays are required"));
    };
    let mut names: Vec<String> = Vec::new();
    let mut payloads: Vec<(String, Map<String, J>)> = Vec::new();
    for entry in tables {
        let Some(name) = entry.as_object().and_then(|e| e.get("name")).and_then(J::as_str) else {
            return Err(input_error("manifest.json", "잘못된 tables 항목입니다", "invalid 'tables' entry"));
        };
        if names.iter().any(|n| n == name) {
            return Err(input_error("manifest.json", format!("테이블 '{name}'이 중복되었습니다"), format!("table '{name}' appears twice")));
        }
        names.push(name.to_string());
        let payload = read_json(&directory.join(format!("{name}.json")))?;
        if payload.get("table").and_then(J::as_str) != Some(name) || !payload.get("primary_key").is_some_and(J::is_string) {
            return Err(input_error(&format!("{name}.json"), "테이블 이름 또는 기본키가 잘못되었습니다", "wrong table name or primary key"));
        }
        if !payload.get("rows").is_some_and(J::is_array) {
            return Err(input_error(&format!("{name}.json"), "rows 배열이 필요합니다", "a 'rows' array is required"));
        }
        payloads.push((name.to_string(), payload));
    }
    // String tables: their keys (from one language file) are reference targets like primary keys.
    if let Some(J::Array(string_tables)) = manifest.get("string_tables") {
        for entry in string_tables {
            let entry = entry.as_object();
            let name = entry.and_then(|e| e.get("name")).and_then(J::as_str);
            let languages: Vec<&str> =
                entry.and_then(|e| e.get("languages")).and_then(J::as_array).map(|l| l.iter().filter_map(J::as_str).collect()).unwrap_or_default();
            let (Some(name), Some(language)) = (name, languages.first()) else {
                return Err(input_error("manifest.json", "잘못된 string_tables 항목입니다", "invalid 'string_tables' entry"));
            };
            let file = format!("Strings/{language}/{name}.json");
            let payload = read_json(&directory.join(&file))?;
            let Some(J::Array(string_keys)) = payload.get("keys") else {
                return Err(input_error(&file, "keys 배열이 필요합니다", "a 'keys' array is required"));
            };
            let rows = string_keys.iter().map(|key| J::Object(Map::from_iter([("Id".to_string(), key.clone())]))).collect();
            let table = Map::from_iter([
                ("table".to_string(), J::from(name)),
                ("primary_key".to_string(), J::from("Id")),
                ("rows".to_string(), J::Array(rows)),
            ]);
            payloads.push((name.to_string(), table));
        }
    }
    let get = |name: &str| payloads.iter().find(|(n, _)| n == name).map(|(_, p)| p);
    let rows_of = |payload: &Map<String, J>| payload["rows"].as_array().cloned().unwrap_or_default();

    let mut keys: Vec<(String, HashSet<Key>)> = Vec::new();
    for (name, payload) in &payloads {
        let primary = payload["primary_key"].as_str().unwrap_or("").to_string();
        let rows = rows_of(payload);
        if rows.iter().any(|row| !row.as_object().is_some_and(|r| r.contains_key(&primary))) {
            return Err(input_error(&format!("{name}.json"), "기본키가 빠진 행이 있습니다", "a row has no primary key"));
        }
        let mut set = HashSet::new();
        for row in &rows {
            match Key::of(&row[&primary]) {
                Some(key) => {
                    set.insert(key);
                }
                None => return Err(input_error(&format!("{name}.json"), "기본키 자료형이 잘못되었습니다", "wrong primary key type")),
            }
        }
        keys.push((name.clone(), set));
    }

    let mut failures = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for reference in references {
        let fields: Option<Vec<&str>> = reference.as_object().and_then(|r| {
            ["table", "field", "target", "key_type"].iter().map(|f| r.get(*f).and_then(J::as_str)).collect()
        });
        let Some(fields) = fields else {
            return Err(input_error("manifest.json", "잘못된 references 항목입니다", "invalid 'references' entry"));
        };
        let reference = reference.as_object().unwrap();
        let (source, field, target, key_type) = (fields[0], fields[1], fields[2], fields[3]);
        let (Some(source_payload), Some(target_payload)) = (get(source), get(target)) else {
            return Err(input_error("manifest.json", format!("참조 '{source}.{field}'의 테이블이 없습니다"), format!("a table of reference '{source}.{field}' is missing")));
        };
        let is_array = match reference.get("array") {
            None => false,
            Some(J::Bool(b)) => *b,
            Some(_) => return Err(input_error("manifest.json", "array가 잘못되었습니다", "invalid array")),
        };
        let target_key = reference.get("target_key").filter(|v| !v.is_null());
        let cardinality = reference.get("cardinality").and_then(J::as_str);
        if !matches!(cardinality, Some("one") | Some("many")) || target_key.is_none() != (cardinality == Some("one")) {
            return Err(input_error("manifest.json", "cardinality가 target_key와 맞지 않습니다", "cardinality does not match target_key"));
        }
        let target_rows = rows_of(target_payload);
        let (target_keys, target_label, sub_key) = match target_key {
            Some(value) => {
                let Some(key_name) = value.as_str().filter(|s| !s.is_empty()) else {
                    return Err(input_error("manifest.json", "target_key가 잘못되었습니다", "invalid target_key"));
                };
                let has_index = target_payload.get("sub_keys").and_then(J::as_array).is_some_and(|entries| {
                    entries.iter().any(|e| e.as_object().and_then(|e| e.get("field")).and_then(J::as_str) == Some(key_name))
                });
                if !has_index {
                    return Err(input_error(&format!("{target}.json"), format!("'{key_name}' 서브키 인덱스가 없습니다"), format!("no sub key index for '{key_name}'")));
                }
                if target_rows.iter().any(|row| !row.as_object().is_some_and(|r| r.contains_key(key_name))) {
                    return Err(input_error(&format!("{target}.json"), format!("서브키 '{key_name}'가 빠진 행이 있습니다"), format!("a row has no sub key '{key_name}'")));
                }
                let mut set = HashSet::new();
                for row in &target_rows {
                    match Key::of(&row[key_name]) {
                        Some(key) => {
                            set.insert(key);
                        }
                        None => return Err(input_error(&format!("{target}.json"), format!("서브키 '{key_name}'의 자료형이 잘못되었습니다"), format!("wrong type for sub key '{key_name}'"))),
                    }
                }
                (set, format!("{target}.{key_name}"), true)
            }
            None => (keys.iter().find(|(n, _)| n == target).map(|(_, s)| s.clone()).unwrap_or_default(), target.to_string(), false),
        };
        // Empty cells of a reference column mean "no reference": 0 for numbers, "" for names.
        let absent = match key_type {
            "name" => Some(Key::Str(String::new())),
            "int32" | "int64" => Some(Key::Int(0)),
            _ => None,
        };
        if let Some(absent_key) = &absent
            && target_keys.contains(absent_key) {
                let shown = match absent_key {
                    Key::Str(s) => py_str_repr(s),
                    Key::Int(i) => i.to_string(),
                };
                let warning = tr(
                    format!("{target_label}: 키 {shown}이 참조 없음 값과 충돌합니다"),
                    format!("{target_label}: key {shown} collides with the 'no reference' value"),
                );
                if !warnings.contains(&warning) {
                    warnings.push(warning);
                }
            }
        let primary = source_payload["primary_key"].as_str().unwrap_or("").to_string();
        // Client JSON keeps array elements in a pool per field: rows hold <field>_Start and _Num.
        let pool = source_payload
            .get("arrays")
            .and_then(J::as_array)
            .and_then(|arrays| arrays.iter().find(|a| a.get("field").and_then(J::as_str) == Some(field)))
            .and_then(|a| a.get("pool"))
            .and_then(J::as_array);
        for row in rows_of(source_payload) {
            let object = row.as_object();
            let values: Vec<(Option<usize>, J)> = match (object.and_then(|r| r.get(field)), pool) {
                (Some(value), _) if is_array => match value.as_array() {
                    Some(items) => items.iter().cloned().enumerate().map(|(i, v)| (Some(i), v)).collect(),
                    None => return Err(input_error(&format!("{source}.json"), format!("필드 '{field}'가 배열이 아닙니다"), format!("field '{field}' is not an array"))),
                },
                (Some(value), _) => vec![(None, value.clone())],
                (None, Some(pool)) if is_array => {
                    let number = |suffix: &str| object.and_then(|r| r.get(&format!("{field}{suffix}"))).and_then(J::as_u64).map(|n| n as usize);
                    match (number("_Start"), number("_Num")) {
                        (Some(start), Some(count)) if start + count <= pool.len() => pool[start..start + count].iter().cloned().enumerate().map(|(i, v)| (Some(i), v)).collect(),
                        _ => return Err(input_error(&format!("{source}.json"), format!("필드 '{field}'의 배열 위치가 잘못되었습니다"), format!("field '{field}' has an invalid array range"))),
                    }
                }
                _ => return Err(input_error(&format!("{source}.json"), format!("필드 '{field}'가 없습니다"), format!("field '{field}' is missing"))),
            };
            for (index, item) in values {
                let Some(key) = Key::of(&item) else {
                    return Err(input_error(&format!("{source}.json"), format!("필드 '{field}'의 값 자료형이 잘못되었습니다"), format!("wrong value type in field '{field}'")));
                };
                if Some(&key) == absent.as_ref() {
                    continue;
                }
                if !target_keys.contains(&key) {
                    let suffix = index.map(|i| format!("({i})")).unwrap_or_default();
                    let missing = if sub_key {
                        tr("에 해당 값 없음", ": no row has this value")
                    } else {
                        tr(" 테이블에 없음", ": not in the table")
                    };
                    let row_key = Key::of(&row[&primary]).map(|k| k.text()).unwrap_or_default();
                    failures.push(format!("{source}.{field}[{row_key}]{suffix} = {} → {target_label}{missing}", key.text()));
                }
            }
        }
    }
    Ok((failures, warnings))
}
