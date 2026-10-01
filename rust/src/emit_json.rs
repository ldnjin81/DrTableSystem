//! Client and server JSON output.

use std::path::Path;

use crate::excel::DataModel;
use crate::schema::{enum_of, in_scopes, sha256_hex, ColumnSchema, Enums, TableSchema, CLIENT_SCOPES, SERVER_SCOPES};
use crate::value::{object, py_cmp, Json, Value};

pub fn emit_json(
    model: &DataModel,
    client_output: &Path,
    server_output: &Path,
    stamp: Option<&str>,
    prefix: &str,
    asset_name: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(client_output)?;
    std::fs::create_dir_all(server_output)?;
    let enums = model.enum_map();
    let sides: [(&str, &[&str], &Path, bool); 2] =
        [("client", &CLIENT_SCOPES, client_output, true), ("server", &SERVER_SCOPES, server_output, false)];
    let mut content_hashes: Vec<[String; 2]> = Vec::new();
    for table in model.data_tables() {
        let rows = sorted_rows(table, &enums);
        let mut hashes = [String::new(), String::new()];
        for (position, (_, scopes, output, with_index)) in sides.iter().enumerate() {
            let (payload, hash) = table_payload(table, &rows, scopes, &enums, *with_index);
            hashes[position] = hash;
            write_json(&output.join(format!("{}.json", table.name)), &payload)?;
        }
        content_hashes.push(hashes);
    }
    for (position, (_, scopes, output, _)) in sides.iter().enumerate() {
        let mut references: Vec<(String, String, Json)> = Vec::new();
        for table in &model.tables {
            for column in &table.columns {
                let Some(target) = &column.ref_target else { continue };
                if !in_scopes(&column.scope, scopes) {
                    continue;
                }
                let entry = object([
                    ("table", Json::from(table.name.as_str())),
                    ("field", Json::from(column.name.as_str())),
                    ("target", Json::from(target.as_str())),
                    ("target_key", column.ref_key.as_deref().map(Json::from).unwrap_or(Json::Null)),
                    ("cardinality", Json::from(if column.ref_key.is_some() { "many" } else { "one" })),
                    ("key_type", Json::from(column.type_name.as_str())),
                    ("array_length", Json::Int(column.array_size.filter(|&n| n != 0).unwrap_or(1) as i64)),
                    ("subkey", Json::Bool(column.is_role("subkey"))),
                ]);
                references.push((table.name.clone(), column.name.clone(), entry));
            }
        }
        references.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        let tables = model
            .data_tables()
            .zip(&content_hashes)
            .map(|(table, hashes)| {
                object([
                    ("name", Json::from(table.name.as_str())),
                    ("rows", Json::Int(table.rows.len() as i64)),
                    ("schema_hash", Json::from(table.schema_hash.as_str())),
                    ("content_hash", Json::from(hashes[position].as_str())),
                    ("schema", Json::from(table.schema_file.as_str())),
                    (
                        "sources",
                        Json::List(
                            table
                                .sources
                                .iter()
                                .map(|part| {
                                    object([
                                        ("file", Json::from(part.file.as_str())),
                                        ("sheet", Json::from(part.sheet.as_str())),
                                        ("rows", Json::Int(part.rows as i64)),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect();
        let mut manifest = vec![
            ("source_files".to_string(), Json::List(model.source_files.iter().map(|f| Json::from(f.as_str())).collect())),
            ("cpp_prefix".to_string(), Json::from(prefix)),
            ("asset_name".to_string(), Json::from(asset_name)),
            ("tables".to_string(), Json::List(tables)),
            (
                "enums".to_string(),
                Json::List(
                    model
                        .enums
                        .iter()
                        .map(|e| object([("name", Json::from(e.name.as_str())), ("values", Json::Int(e.values.len() as i64))]))
                        .collect(),
                ),
            ),
            ("references".to_string(), Json::List(references.into_iter().map(|(_, _, e)| e).collect())),
        ];
        let strings = write_strings(model, scopes, output, &enums)?;
        if !strings.is_empty() {
            manifest.push(("string_tables".to_string(), Json::List(strings)));
        }
        if let Some(stamp) = stamp {
            manifest.push(("generated_at".to_string(), Json::from(stamp)));
        }
        write_json(&output.join("manifest.json"), &Json::Object(manifest))?;
    }
    Ok(())
}

/// Sort key. Enums sort by value, not name, to match the C++ comparison.
fn sort_value(column: &ColumnSchema, value: &Value, enums: &Enums) -> Value {
    if column.type_name.starts_with('E') {
        if let Some(e) = enum_of(&column.type_name).and_then(|name| enums.get(name))
            && let Some(item) = e.values.iter().find(|item| Some(item.name.as_str()) == value.as_str()) {
                return Value::Int(item.value);
            }
        return Value::Int(0);
    }
    value.clone()
}

/// Rows sorted by primary key; the asset's PrimaryKeys array follows the same order.
fn sorted_rows<'a>(table: &'a TableSchema, enums: &Enums) -> Vec<&'a Vec<Value>> {
    let index = table.columns.iter().position(|c| c.is_role("id")).expect("a primary key");
    let primary = &table.columns[index];
    let mut keyed: Vec<(Value, &Vec<Value>)> =
        table.rows.iter().map(|row| (sort_value(primary, &row[index], enums), row)).collect();
    keyed.sort_by(|a, b| py_cmp(&a.0, &b.0));
    keyed.into_iter().map(|(_, row)| row).collect()
}

/// CSR index. Buckets are sorted by key so the output never depends on hash order.
fn sub_key_index(column: &ColumnSchema, index: usize, rows: &[&Vec<Value>], enums: &Enums) -> Vec<(String, Json)> {
    let mut buckets: Vec<(Value, Vec<i64>)> = Vec::new();
    let mut positions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (row_index, row) in rows.iter().enumerate() {
        let value = &row[index];
        let slot = *positions.entry(value.key()).or_insert_with(|| {
            buckets.push((value.clone(), Vec::new()));
            buckets.len() - 1
        });
        buckets[slot].1.push(row_index as i64);
    }
    let mut keyed: Vec<(Value, &(Value, Vec<i64>))> =
        buckets.iter().map(|bucket| (sort_value(column, &bucket.0, enums), bucket)).collect();
    keyed.sort_by(|a, b| py_cmp(&a.0, &b.0));
    let mut offsets = vec![Json::Int(0)];
    let mut indices = Vec::new();
    let mut keys = Vec::new();
    for (_, (key, members)) in keyed {
        keys.push(Json::from(key));
        indices.extend(members.iter().map(|i| Json::Int(*i)));
        offsets.push(Json::Int(indices.len() as i64));
    }
    vec![
        ("keys".to_string(), Json::List(keys)),
        ("offsets".to_string(), Json::List(offsets)),
        ("indices".to_string(), Json::List(indices)),
    ]
}

/// The table's JSON document and its content hash (of the rows and indices it carries).
fn table_payload(
    table: &TableSchema,
    rows: &[&Vec<Value>],
    scopes: &[&str],
    enums: &Enums,
    with_index: bool,
) -> (Json, String) {
    let primary_index = table.columns.iter().position(|c| c.is_role("id")).expect("a primary key");
    let primary = &table.columns[primary_index];
    let mut content: Vec<(String, Json)> = vec![("primary_key".to_string(), Json::from(primary.name.as_str()))];
    if with_index {
        content.push(("primary_keys".to_string(), Json::List(rows.iter().map(|row| Json::from(&row[primary_index])).collect())));
    }
    let mut sub_keys = Vec::new();
    for (index, column) in table.columns.iter().enumerate() {
        if !column.is_role("subkey") || !in_scopes(&column.scope, scopes) {
            continue;
        }
        let mut entry = vec![
            ("name".to_string(), Json::from(column.name.as_str())),
            ("field".to_string(), Json::from(column.name.as_str())),
        ];
        if with_index {
            entry.extend(sub_key_index(column, index, rows, enums));
        }
        sub_keys.push(Json::Object(entry));
    }
    content.push(("sub_keys".to_string(), Json::List(sub_keys)));
    let included: Vec<usize> =
        (0..table.columns.len()).filter(|&i| in_scopes(&table.columns[i].scope, scopes)).collect();
    let json_rows = rows
        .iter()
        .map(|row| Json::Object(included.iter().map(|&i| (table.columns[i].name.clone(), Json::from(&row[i]))).collect()))
        .collect();
    content.push(("rows".to_string(), Json::List(json_rows)));
    let content = Json::Object(content);
    let hash = format!("sha256:{}", sha256_hex(content.compact_sorted().as_bytes()));
    let Json::Object(entries) = content else { unreachable!() };
    let mut ordered = vec![
        ("table".to_string(), Json::from(table.name.as_str())),
        ("schema_hash".to_string(), Json::from(table.schema_hash.as_str())),
        ("content_hash".to_string(), Json::from(hash.as_str())),
    ];
    ordered.extend(entries);
    (Json::Object(ordered), hash)
}

/// One JSON file per string table and language in this output (Strings/<culture>/<Table>.json),
/// and the manifest entries describing them. Keys are sorted like primary keys.
fn write_strings(model: &DataModel, scopes: &[&str], output: &Path, enums: &Enums) -> std::io::Result<Vec<Json>> {
    let mut entries = Vec::new();
    for table in model.string_tables() {
        let languages: Vec<(usize, &ColumnSchema)> = table
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_lang() && in_scopes(&c.scope, scopes))
            .collect();
        if languages.is_empty() {
            continue;
        }
        let rows = sorted_rows(table, enums);
        let key_index = table.columns.iter().position(|c| c.is_role("id")).expect("a primary key");
        let keys: Vec<Json> = rows.iter().map(|row| Json::from(&row[key_index])).collect();
        let base = table.base_language().map(|c| c.culture()).unwrap_or_default();
        let mut hashes = Vec::new();
        for (index, column) in &languages {
            let culture = column.culture();
            let content = object([
                ("language", Json::from(culture.as_str())),
                ("keys", Json::List(keys.clone())),
                ("values", Json::List(rows.iter().map(|row| Json::from(&row[*index])).collect())),
            ]);
            let hash = format!("sha256:{}", sha256_hex(content.compact_sorted().as_bytes()));
            let Json::Object(content) = content else { unreachable!() };
            let mut payload = vec![
                ("table".to_string(), Json::from(table.name.as_str())),
                ("base_language".to_string(), Json::from(base.as_str())),
                ("schema_hash".to_string(), Json::from(table.schema_hash.as_str())),
                ("content_hash".to_string(), Json::from(hash.as_str())),
            ];
            payload.extend(content);
            let folder = output.join("Strings").join(&culture);
            std::fs::create_dir_all(&folder)?;
            write_json(&folder.join(format!("{}.json", table.name)), &Json::Object(payload))?;
            hashes.push((culture, Json::Str(hash)));
        }
        entries.push(object([
            ("name", Json::from(table.name.as_str())),
            ("rows", Json::Int(table.rows.len() as i64)),
            ("base_language", Json::from(base.as_str())),
            ("languages", Json::List(hashes.iter().map(|(c, _)| Json::from(c.as_str())).collect())),
            ("schema_hash", Json::from(table.schema_hash.as_str())),
            ("content_hashes", Json::Object(hashes)),
            ("schema", Json::from(table.schema_file.as_str())),
            (
                "sources",
                Json::List(
                    table
                        .sources
                        .iter()
                        .map(|part| {
                            object([
                                ("file", Json::from(part.file.as_str())),
                                ("sheet", Json::from(part.sheet.as_str())),
                                ("rows", Json::Int(part.rows as i64)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]));
    }
    Ok(entries)
}

fn write_json(path: &Path, payload: &Json) -> std::io::Result<()> {
    std::fs::write(path, payload.pretty() + "\n")
}
