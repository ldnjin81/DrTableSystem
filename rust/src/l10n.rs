//! 번역 작업용 원본 셀 분석과 미사용 키 보고.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::path::{Component, PathBuf};

use rust_xlsxwriter::{DocProperties, ExcelDateTime, Workbook};

use crate::excel::{self, DataModel};
use crate::i18n::tr;
use crate::reader::read_workbook;
use crate::schema::TableSchema;
use crate::value::Value;
use crate::value::Cell;

#[derive(Clone, Debug)]
pub struct Entry {
    pub table: String,
    pub key: String,
    pub base: String,
    pub translation: String,
    pub language: String,
    pub source: String,
    pub sheet: String,
    pub row: usize,
    pub memo: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Count {
    pub total: usize,
    pub filled: usize,
    pub empty: usize,
    pub same_as_base: usize,
}

fn error(file: &str, sheet: &str, row: usize, message: String) -> String {
    format!("[{file}]{sheet}!A{row}: {message}")
}

pub fn load(input: &Path) -> Result<(DataModel, Vec<Entry>), String> {
    let model = excel::load_model(input, None, None, None)
        .map_err(|errors| errors.0.join("\n"))?;
    let mut entries = Vec::new();
    for table in model.string_tables() {
        let Some(base) = table.base_language() else { continue };
        for source in &table.sources {
            let path = input.join(&source.file);
            let sheets = read_workbook(&path, |name| name == source.sheet)
                .map_err(|e| error(&source.file, &source.sheet, 1, tr(format!("xlsx를 읽을 수 없습니다: {e}"), format!("cannot read xlsx: {e}"))))?;
            let Some(grid) = sheets.iter().find(|s| s.title == source.sheet).and_then(|s| s.grid.as_ref()) else {
                return Err(error(&source.file, &source.sheet, 1, tr("시트를 찾을 수 없습니다", "sheet not found")));
            };
            let header = |name: &str| (1..=grid.max_column()).find(|&c| grid.value(1, c).text_or_empty() == name);
            let key_col = header(&table.primary_key().name).ok_or_else(|| error(&source.file, &source.sheet, 1, tr("키 열이 없습니다", "key column missing")))?;
            let base_col = header(&base.name).ok_or_else(|| error(&source.file, &source.sheet, 1, tr("기준 언어 열이 없습니다", "base language column missing")))?;
            for row in crate::schema::DATA_ROW..=grid.max_row() {
                let key = grid.value(row, key_col).text_or_empty();
                if key.is_empty() { continue; }
                let base_text = grid.value(row, base_col).text_or_empty();
                for lang in table.languages() {
                    let col = header(&lang.name).ok_or_else(|| error(&source.file, &source.sheet, 1, tr("언어 열이 없습니다", "language column missing")))?;
                    entries.push(Entry {
                        table: table.name.clone(), key: key.clone(), base: base_text.clone(),
                        translation: grid.value(row, col).text_or_empty(), language: lang.culture(),
                        source: source.file.clone(), sheet: source.sheet.clone(), row,
                        memo: (1..=grid.max_column()).find(|&c| grid.value(1, c).text_or_empty().starts_with('#'))
                            .map(|c| grid.value(row, c).text_or_empty()).unwrap_or_default(),
                    });
                }
            }
        }
    }
    entries.sort_by(|a, b| (&a.table, &a.language, &a.key, &a.source, &a.sheet, a.row)
        .cmp(&(&b.table, &b.language, &b.key, &b.source, &b.sheet, b.row)));
    Ok((model, entries))
}

fn xlsx_error(path: &Path, message: impl std::fmt::Display) -> String {
    format!("[{}]Workbook!A1: {message}", path.display())
}

fn workbook() -> Workbook {
    let mut book = Workbook::new();
    let date = ExcelDateTime::from_ymd(2000, 1, 1).unwrap();
    book.set_properties(&DocProperties::new().set_creation_datetime(&date));
    book
}

pub fn export(entries: &[Entry], language: &str, output: &Path) -> Result<usize, String> {
    if output.exists() { return Err(xlsx_error(output, tr("출력 파일이 이미 있습니다", "output file already exists"))); }
    let selected: Vec<&Entry> = entries.iter().filter(|e| e.language == language &&
        (e.translation.is_empty() || e.translation == e.base)).collect();
    if !entries.iter().any(|e| e.language == language) {
        return Err(xlsx_error(output, tr(format!("언어 '{language}'가 없습니다"), format!("language '{language}' not found"))));
    }
    let mut book = workbook();
    let sheet = book.add_worksheet();
    sheet.set_name("Translations").map_err(|e| xlsx_error(output, e))?;
    for (col, name) in ["Key", "Base", "Translation", "Memo", "Source", "Sheet", "Table", "Language"].iter().enumerate() {
        sheet.write_string(0, col as u16, *name).map_err(|e| xlsx_error(output, e))?;
    }
    for (index, entry) in selected.iter().enumerate() {
        for (col, value) in [&entry.key, &entry.base, &entry.translation, &entry.memo, &entry.source, &entry.sheet, &entry.table, &entry.language].iter().enumerate() {
            sheet.write_string((index + 1) as u32, col as u16, *value).map_err(|e| xlsx_error(output, e))?;
        }
    }
    if let Some(parent) = output.parent() { std::fs::create_dir_all(parent).map_err(|e| xlsx_error(output, e))?; }
    book.save(output).map_err(|e| xlsx_error(output, e))?;
    Ok(selected.len())
}

fn safe_relative(text: &str) -> Option<PathBuf> {
    let path = Path::new(text);
    if path.components().all(|part| matches!(part, Component::Normal(_)))
        && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"))
        && path.components().next().is_some_and(|part| part.as_os_str().to_string_lossy() == "Strings") {
        Some(path.to_path_buf())
    } else { None }
}

fn write_cell(sheet: &mut rust_xlsxwriter::Worksheet, row: u32, col: u16, cell: &Cell) -> Result<(), rust_xlsxwriter::XlsxError> {
    match cell {
        Cell::Empty => Ok(()),
        Cell::Str(value) | Cell::Date(value) => sheet.write_string(row, col, value).map(|_| ()),
        Cell::Int(value) => sheet.write_number(row, col, *value as f64).map(|_| ()),
        Cell::Float(value) => sheet.write_number(row, col, *value).map(|_| ()),
        Cell::Bool(value) => sheet.write_boolean(row, col, *value).map(|_| ()),
    }
}

pub fn import(received: &Path, input: &Path, output: &Path) -> Result<(usize, Vec<String>), String> {
    let model = excel::load_model(input, None, None, None).map_err(|e| e.0.join("\n"))?;
    let bases: BTreeMap<String, String> = model.string_tables().filter_map(|t| Some((t.name.clone(), t.base_language()?.name.clone()))).collect();
    let sheets = read_workbook(received, |_| true).map_err(|e| xlsx_error(received, e))?;
    let grid = sheets.iter().find(|s| s.title == "Translations").and_then(|s| s.grid.as_ref())
        .ok_or_else(|| xlsx_error(received, tr("Translations 시트가 없습니다", "Translations sheet missing")))?;
    let expected = ["Key", "Base", "Translation", "Memo", "Source", "Sheet", "Table", "Language"];
    if expected.iter().enumerate().any(|(c, name)| grid.value(1, c + 1).text_or_empty() != *name) {
        return Err(xlsx_error(received, tr("번역 파일 머리글이 다릅니다", "translation workbook header mismatch")));
    }
    let input_abs = crate::schemafile::absolute(input);
    let output_abs = crate::schemafile::absolute(output);
    if output_abs.starts_with(&input_abs) || input_abs.starts_with(&output_abs) {
        return Err(xlsx_error(output, tr("출력 폴더는 입력 폴더와 분리해야 합니다", "output folder must be separate from input folder")));
    }
    let mut requested: BTreeMap<PathBuf, Vec<(usize, String, String, String, String, String, String)>> = BTreeMap::new();
    for row in 2..=grid.max_row() {
        let translation = grid.value(row, 3).text_or_empty();
        if translation.is_empty() { continue; }
        let source = grid.value(row, 5).text_or_empty();
        let relative = safe_relative(&source).ok_or_else(|| error(&received.display().to_string(), "Translations", row, tr("출처 경로가 잘못되었습니다", "invalid source path")))?;
        requested.entry(relative).or_default().push((row, grid.value(row, 1).text_or_empty(), grid.value(row, 2).text_or_empty(), translation,
            grid.value(row, 6).text_or_empty(), grid.value(row, 7).text_or_empty(), grid.value(row, 8).text_or_empty()));
    }
    for relative in requested.keys() {
        let destination = output.join(relative);
        if destination.exists() { return Err(xlsx_error(&destination, tr("출력 파일이 이미 있습니다", "output file already exists"))); }
        if !input.join(relative).is_file() { return Err(xlsx_error(&input.join(relative), tr("원본 파일이 없습니다", "source file missing"))); }
    }
    let mut warnings = Vec::new();
    let mut count = 0;
    for (relative, rows) in requested {
        let source = input.join(&relative);
        let destination = output.join(&relative);
        let sheets = read_workbook(&source, |_| true).map_err(|e| xlsx_error(&source, e))?;
        let mut book = workbook();
        let mut handled = BTreeSet::new();
        for original in sheets {
            let target = book.add_worksheet();
            target.set_name(&original.title).map_err(|e| xlsx_error(&destination, e))?;
            let Some(grid) = original.grid else { continue };
            let header = |name: &str| (1..=grid.max_column()).find(|&c| grid.value(1, c).text_or_empty() == name);
            let mut changes: BTreeMap<(usize, usize), String> = BTreeMap::new();
            for (received_row, key, old_base, translated, sheet_name, table_name, language) in &rows {
                if *sheet_name != original.title { continue; }
                handled.insert(*received_row);
                if crate::sources::strip_sheet_comment(&original.title) != table_name {
                    warnings.push(error(&relative.to_string_lossy(), &original.title, *received_row,
                        tr(format!("테이블 '{table_name}'이 출처 시트와 다릅니다"), format!("table '{table_name}' does not match source sheet"))));
                    continue;
                }
                let Some(key_col) = header("Id") else { continue };
                let Some(lang_col) = header(language) else { continue };
                let found = (crate::schema::DATA_ROW..=grid.max_row()).find(|&r| grid.value(r, key_col).text_or_empty() == *key);
                let Some(source_row) = found else {
                    warnings.push(error(&relative.to_string_lossy(), &original.title, *received_row,
                        tr(format!("원본에 없는 키 '{key}'"), format!("key '{key}' missing from source"))));
                    continue;
                };
                let base_name = bases.get(table_name).cloned().unwrap_or_default();
                if let Some(base_col) = header(&base_name) {
                    let current_base = grid.value(source_row, base_col).text_or_empty();
                    if current_base != *old_base {
                        warnings.push(error(&relative.to_string_lossy(), &original.title, source_row,
                            tr(format!("키 '{key}'의 기준 문구가 바뀜"), format!("base text changed for key '{key}'"))));
                    }
                }
                changes.insert((source_row, lang_col), translated.clone());
                count += 1;
            }
            for row in 1..=grid.max_row() {
                for col in 1..=grid.max_column() {
                    let value = changes.get(&(row, col)).map(|s| Cell::Str(s.clone())).unwrap_or_else(|| grid.value(row, col).clone());
                    write_cell(target, (row - 1) as u32, (col - 1) as u16, &value).map_err(|e| xlsx_error(&destination, e))?;
                }
            }
        }
        for (received_row, key, _, _, sheet_name, _, _) in &rows {
            if !handled.contains(received_row) {
                warnings.push(error(&relative.to_string_lossy(), sheet_name, *received_row,
                    tr(format!("원본에 없는 키 '{key}' (시트를 찾을 수 없음)"), format!("key '{key}' missing from source (sheet not found)"))));
            }
        }
        if let Some(parent) = destination.parent() { std::fs::create_dir_all(parent).map_err(|e| xlsx_error(&destination, e))?; }
        book.save(&destination).map_err(|e| xlsx_error(&destination, e))?;
    }
    Ok((count, warnings))
}

pub fn status(entries: &[Entry], format: &str) -> String {
    let mut counts: BTreeMap<(String, String), Count> = BTreeMap::new();
    for entry in entries {
        let count = counts.entry((entry.table.clone(), entry.language.clone())).or_default();
        count.total += 1;
        if entry.translation.is_empty() { count.empty += 1; }
        else {
            count.filled += 1;
            if entry.translation == entry.base { count.same_as_base += 1; }
        }
    }
    match format {
        "json" => {
            let rows: Vec<_> = counts.iter().map(|((table, language), c)| serde_json::json!({
                "table": table, "language": language, "total": c.total, "filled": c.filled,
                "empty": c.empty, "same_as_base": c.same_as_base,
            })).collect();
            format!("{}\n", serde_json::to_string_pretty(&rows).unwrap())
        }
        "markdown" => {
            let mut output = format!("| {} | {} | {} | {} | {} | {} |\n|---|---|---:|---:|---:|---:|\n",
                tr("테이블", "Table"), tr("언어", "Language"), tr("전체", "Total"),
                tr("채움", "Filled"), tr("비어 있음", "Empty"), tr("기준과 같음", "Same as base"));
            for ((table, language), c) in counts {
                output.push_str(&format!("| {table} | {language} | {} | {} | {} | {} |\n", c.total, c.filled, c.empty, c.same_as_base));
            }
            output
        }
        _ => counts.into_iter().map(|((table, language), c)| format!("{table} {language}: {}={} {}={} {}={} {}={}\n",
            tr("전체", "total"), c.total, tr("채움", "filled"), c.filled,
            tr("비어 있음", "empty"), c.empty, tr("기준과 같음", "same as base"), c.same_as_base)).collect(),
    }
}

fn strings(value: &Value) -> Vec<&str> {
    match value {
        Value::Str(s) => vec![s.as_str()],
        Value::List(items) => items.iter().flat_map(strings).collect(),
        _ => Vec::new(),
    }
}

pub fn unused(model: &DataModel) -> Vec<(String, String)> {
    let mut used: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let tables: Vec<&TableSchema> = model.string_tables().collect();
    for table in model.data_tables() {
        for (index, column) in table.columns.iter().enumerate() {
            if column.ref_strings {
                if let Some(target) = &column.ref_target {
                    for row in &table.rows {
                        used.entry(target.clone()).or_default().extend(strings(&row[index]).into_iter().map(str::to_string));
                    }
                }
            } else if column.type_name == "text" {
                for row in &table.rows {
                    for value in strings(&row[index]) {
                        for target in &tables {
                            used.entry(target.name.clone()).or_default().insert(value.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut result = Vec::new();
    for table in tables {
        let key_index = table.columns.iter().position(|c| c.is_role("id")).unwrap();
        for row in &table.rows {
            if let Some(key) = row[key_index].as_str()
                && !used.get(&table.name).is_some_and(|keys| keys.contains(key)) {
                    result.push((table.name.clone(), key.to_string()));
                }
        }
    }
    result.sort();
    result
}
