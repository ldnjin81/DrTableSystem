//! Reference headers: rows 2 and 3 of a data sheet show the schema through Excel formulas.
//!
//! The build never reads rows 2 and 3. `drtable new` creates a data workbook with the
//! formulas; the tool never rewrites an existing data workbook (designers may be editing it).
//!
//! Excel keeps a link relative only when the schema file sits in the data file's folder or
//! below it. Otherwise the link stores the absolute path of whoever saved the workbook, and on
//! another machine the header keeps showing the values from that save; `link_warnings`
//! reports that case.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use rust_xlsxwriter::Workbook;

use crate::errors::ErrorCollector;
use crate::i18n::tr;
use crate::schema::{column_letter, NAME_ROW, SCOPE_ROW, TYPE_ROW};
use crate::schemafile::{Schemas, SCHEMA_SUFFIXES, STRING_TABLE_SUFFIX};

/// Creates a data workbook for `table`: field names in row 1, reference formulas in rows
/// 2-3. Refuses to touch an existing file. Returns true when the workbook was written.
pub fn new_workbook(target: &Path, table: &str, schemas: &Schemas, errors: &mut ErrorCollector) -> bool {
    let label = format!("[{}]", target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    // A string table can be named as in its schema file (UI) or as the table (UIString).
    let string_table = format!("{table}{STRING_TABLE_SUFFIX}");
    let found = schemas.tables.get(table).or_else(|| schemas.tables.get(&string_table).filter(|s| s.is_strings));
    let Some(schema) = found else {
        errors.add(&label, "A1", tr(format!("테이블 '{table}'의 스키마가 없습니다"), format!("table '{table}' has no schema")));
        return false;
    };
    if target.exists() {
        errors.add(&label, "A1", tr(
            "이미 있는 파일입니다. drtable은 기존 데이터 파일을 고치지 않습니다",
            "the file already exists; drtable never changes existing data workbooks",
        ));
        return false;
    }
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    if std::fs::create_dir_all(&parent).is_err() {
        errors.add(&label, "A1", tr("폴더를 만들 수 없습니다", "cannot create the folder"));
        return false;
    }
    let schema_path = schema.path.clone();
    let link_target = relative_path(&schema_path, &canonical(&parent));
    let sheet_name = if schema.title.is_empty() { schema.name.clone() } else { schema.title.clone() };
    let reference = format!("'[1]{sheet_name}'");
    let missing = tr("(스키마에 없음)", "(not in schema)");

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let written = (|| -> Result<(), rust_xlsxwriter::XlsxError> {
        // A string table's data sheet is named like its schema sheet (UI), not the table (UIString).
        sheet.set_name(&sheet_name)?;
        for (index, (schema_row, name, _, _)) in schema.raw_columns().iter().enumerate() {
            let column = index as u16;
            let letter = column_letter(index + 1);
            if schema.is_strings && *schema_row == 1 {
                // The implicit Id key has no row in a string table schema.
                sheet.write_string((NAME_ROW - 1) as u32, column, "Id")?;
                continue;
            }
            let header = if schema.is_strings { name.py_str().replace('_', "-") } else { name.py_str() };
            sheet.write_string((NAME_ROW - 1) as u32, column, header)?;
            for (row, source) in [(TYPE_ROW, "B"), (SCOPE_ROW, "C")] {
                let lookup = |key: &str| format!("INDEX({reference}!${source}:${source},MATCH({key},{reference}!$A:$A,0))&\"\"");
                let formula = if schema.is_strings {
                    // Language codes may be written zh-Hans or zh_Hans in the schema.
                    format!(
                        "=IFERROR({},IFERROR({},\"{missing}\"))",
                        lookup(&format!("{letter}${NAME_ROW}")),
                        lookup(&format!("SUBSTITUTE({letter}${NAME_ROW},\"-\",\"_\")"))
                    )
                } else {
                    format!(
                        "=IFERROR(INDEX({reference}!${source}:${source},MATCH({letter}${NAME_ROW},{reference}!$A:$A,0)),\"{missing}\")"
                    )
                };
                sheet.write_formula((row - 1) as u32, column, formula.as_str())?;
            }
        }
        Ok(())
    })();
    let bytes = written.and_then(|_| workbook.save_to_buffer()).map_err(|e| e.to_string());
    match bytes.and_then(|bytes| add_external_link(&bytes, &link_target, &sheet_name)) {
        Ok(bytes) => {
            if std::fs::write(target, bytes).is_err() {
                errors.add(&label, "A1", tr("파일을 쓸 수 없습니다", "cannot write the file"));
                return false;
            }
            true
        }
        Err(error) => {
            errors.add(&label, "A1", error);
            false
        }
    }
}

/// Adds external link 1 (to `target`, sheet `sheet_name`) to a saved workbook.
fn add_external_link(bytes: &[u8], target: &str, sheet_name: &str) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|e| e.to_string())?;
        parts.push((file.name().to_string(), data));
    }
    let escape = |text: &str| text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
    let link_rid = "rIdExternal1";
    for (name, data) in parts.iter_mut() {
        let text = String::from_utf8_lossy(data).to_string();
        let updated = match name.as_str() {
            "[Content_Types].xml" => text.replace(
                "</Types>",
                "<Override PartName=\"/xl/externalLinks/externalLink1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml\"/></Types>",
            ),
            "xl/_rels/workbook.xml.rels" => text.replace(
                "</Relationships>",
                &format!("<Relationship Id=\"{link_rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink\" Target=\"externalLinks/externalLink1.xml\"/></Relationships>"),
            ),
            "xl/workbook.xml" => text.replace(
                "</sheets>",
                &format!("</sheets><externalReferences><externalReference r:id=\"{link_rid}\"/></externalReferences>"),
            ),
            _ => continue,
        };
        *data = updated.into_bytes();
    }
    parts.push((
        "xl/externalLinks/externalLink1.xml".to_string(),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<externalLink xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><externalBook xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:id=\"rId1\"><sheetNames><sheetName val=\"{}\"/></sheetNames></externalBook></externalLink>",
            escape(sheet_name)
        )
        .into_bytes(),
    ));
    parts.push((
        "xl/externalLinks/_rels/externalLink1.xml.rels".to_string(),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLinkPath\" Target=\"{}\" TargetMode=\"External\"/></Relationships>",
            escape(target)
        )
        .into_bytes(),
    ));
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (name, data) in parts {
        output.start_file(name, options).map_err(|e| e.to_string())?;
        output.write_all(&data).map_err(|e| e.to_string())?;
    }
    Ok(output.finish().map_err(|e| e.to_string())?.into_inner())
}

static LINK_PART_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^xl/externalLinks/externalLink\d+\.xml$").unwrap());
static BOOK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<externalBook[^>]*\br:id="([^"]+)""#).unwrap());
static TARGET_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"Target="([^"]+)""#).unwrap());

/// Warns when a data workbook links to a schema file that is not at the linked path here.
pub fn link_warnings(path: &Path, relative: &str, schema_files: &HashSet<PathBuf>) -> Vec<String> {
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let Ok(mut archive) = zip::ZipArchive::new(file) else { return Vec::new() };
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let read = |archive: &mut zip::ZipArchive<std::fs::File>, name: &str| -> Option<String> {
        let mut entry = archive.by_name(name).ok()?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data).ok()?;
        Some(String::from_utf8_lossy(&data).to_string())
    };
    let mut messages = Vec::new();
    for name in names.iter().filter(|n| LINK_PART_RE.is_match(n)) {
        let Some(xml) = read(&mut archive, name) else { continue };
        let Some(book) = BOOK_RE.captures(&xml).map(|c| c[1].to_string()) else { continue };
        let (folder, base) = name.rsplit_once('/').unwrap_or(("", name));
        let rels_name = format!("{folder}/_rels/{base}.rels");
        if !names.contains(&rels_name) {
            continue;
        }
        let Some(rels) = read(&mut archive, &rels_name) else { continue };
        let relationship = Regex::new(&format!(r#"<Relationship[^>]*Id="{}"[^>]*/>"#, regex::escape(&book))).ok();
        let target = relationship
            .and_then(|re| re.find(&rels).map(|m| m.as_str().to_string()))
            .and_then(|found| TARGET_RE.captures(&found).map(|c| c[1].to_string()))
            .unwrap_or_default();
        if !SCHEMA_SUFFIXES.iter().any(|s| target.to_lowercase().ends_with(s)) {
            continue;
        }
        let decoded = percent_decode(&target);
        let resolved = resolve_target(&decoded, path.parent().unwrap_or(Path::new(".")));
        if resolved.is_none_or(|r| !schema_files.contains(&r)) {
            messages.push(tr(
                format!("[{relative}]: 참고 헤더가 이 PC에 없는 경로를 가리킵니다({decoded}). 헤더가 스키마 변경을 따라가지 않습니다. 엑셀의 데이터 > 링크 편집에서 원본을 바꾸거나 drtable new로 만든 파일의 2·3행을 복사해 넣으세요"),
                format!("[{relative}]: the reference headers link to a path that does not exist here ({decoded}), so they do not follow schema changes; fix it in Excel (Data > Edit Links > Change Source) or paste rows 2-3 from a workbook made by drtable new"),
            ));
        }
    }
    messages
}

fn resolve_target(text: &str, folder: &Path) -> Option<PathBuf> {
    let mut text = text.to_string();
    if text.to_lowercase().starts_with("file:///") {
        text = text[8..].to_string();
    }
    let normalized = text.replace('\\', "/");
    let bytes = normalized.as_bytes();
    let is_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let candidate = if is_drive || text.starts_with('/') || text.starts_with('\\') {
        PathBuf::from(&normalized)
    } else {
        folder.join(&normalized)
    };
    if candidate.exists() { std::fs::canonicalize(candidate).ok() } else { None }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 3 <= bytes.len()
            && let Some(value) = std::str::from_utf8(&bytes[index + 1..index + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                index += 3;
                continue;
            }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `os.path.relpath(target, start)` for absolute paths, with forward slashes.
pub fn relative_path(target: &Path, start: &Path) -> String {
    let target: Vec<Component> = target.components().collect();
    let start: Vec<Component> = start.components().collect();
    let common = target.iter().zip(start.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); start.len() - common];
    parts.extend(target[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}
