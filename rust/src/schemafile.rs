//! Schema files: where tables and enums are defined, apart from the data.
//!
//! * Table schemas: `<Table>.schema.xlsx` in the schema folder.
//! * Enum schemas: `<Enum>.enum.xlsx` in the enum folder (default `<schema folder>/Enums`).
//!
//! Table schema: one sheet named after the table; row 1 labels, from row 2 one field per row
//! (A name, B type, C scope, D comment). Enum schema: one sheet named after the enum; from
//! row 2 one enumerator per row (A name, B value, C comment).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rust_xlsxwriter::{DocProperties, Workbook};

use crate::errors::{ErrorCollector, ValidationErrors};
use crate::i18n::tr;
use crate::reader::read_workbook;
use crate::schema::RawColumn;
use crate::sources::{find_files, is_identifier};
use crate::value::Cell;

pub const TABLE_SUFFIXES: [&str; 1] = [".schema.xlsx"];
pub const ENUM_SUFFIXES: [&str; 1] = [".enum.xlsx"];
pub const SCHEMA_SUFFIXES: [&str; 2] = [".schema.xlsx", ".enum.xlsx"];
pub const TABLE_LABELS: [&str; 4] = ["Field", "Type", "Scope", "Comment"];
pub const ENUM_LABELS: [&str; 3] = ["Name", "Value", "Comment"];
pub const DEFAULT_ENUM_FOLDER: &str = "Enums";

/// One definition row: (row, name, type or value, scope, comment).
pub type Row = (usize, Cell, Cell, Cell, Cell);

#[derive(Clone, Debug)]
pub struct Schema {
    pub file: String,
    pub name: String,
    pub rows: Vec<Row>,
    pub is_enum: bool,
    /// Sheet name as written.
    pub title: String,
}

impl Schema {
    pub fn where_(&self) -> String {
        format!("[{}]{}", self.file, if self.title.is_empty() { &self.name } else { &self.title })
    }

    pub fn cell(row: usize, column: usize) -> String {
        format!("{}{row}", ["A", "B", "C", "D"][column - 1])
    }

    pub fn at(&self, row: usize) -> String {
        format!("{}!{}", self.where_(), Self::cell(row, 1))
    }

    pub fn raw_columns(&self) -> Vec<RawColumn> {
        self.rows.iter().map(|(row, name, type_, scope, _)| (*row, name.clone(), type_.clone(), scope.clone())).collect()
    }

    /// (row, name, value, comment) of every enumerator.
    pub fn enumerators(&self) -> Vec<(usize, Cell, Cell, Cell)> {
        self.rows.iter().map(|(row, name, value, _, comment)| (*row, name.clone(), value.clone(), comment.clone())).collect()
    }
}

#[derive(Default)]
pub struct Schemas {
    pub tables: BTreeMap<String, Schema>,
    pub enums: BTreeMap<String, Schema>,
}

impl Schemas {
    pub fn all(&self) -> impl Iterator<Item = &Schema> {
        self.tables.values().chain(self.enums.values())
    }
}

/// The folder that holds `path`: itself when it is a folder (or does not exist yet).
pub fn folder_of(path: &Path) -> PathBuf {
    if path.is_file() {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        path.to_path_buf()
    }
}

pub fn enum_folder(schema_root: &Path, enum_root: Option<&Path>) -> PathBuf {
    match enum_root {
        Some(root) => root.to_path_buf(),
        None => folder_of(schema_root).join(DEFAULT_ENUM_FOLDER),
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| absolute(path))
}

/// An absolute path without resolving symlinks (for paths that may not exist).
pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

fn inside(path: &Path, folder: &Path) -> bool {
    canonical(path).starts_with(folder)
}

/// Table schemas under schema_root (outside the enum folder) and enum schemas under enum_root.
pub fn load_schemas(
    schema_root: &Path,
    enum_root: &Path,
    errors: &mut ErrorCollector,
) -> Result<Schemas, ValidationErrors> {
    let mut result = Schemas::default();
    let enum_resolved = canonical(enum_root);
    let table_files: Vec<(PathBuf, String)> = find_files(schema_root, &TABLE_SUFFIXES, true)?
        .into_iter()
        .filter(|(path, _)| !inside(path, &enum_resolved))
        .collect();
    for (path, relative) in find_files(schema_root, &ENUM_SUFFIXES, true)? {
        if !inside(&path, &enum_resolved) {
            errors.add(&format!("[{relative}]"), "A1", tr(
                format!("열거형 스키마는 열거형 폴더({})에 두어야 합니다", enum_root.display()),
                format!("enum schemas belong in the enum folder ({})", enum_root.display()),
            ));
        }
    }
    let mut enum_files = Vec::new();
    if enum_root.exists() {
        let base = canonical(&folder_of(schema_root));
        for (path, _) in find_files(enum_root, &SCHEMA_SUFFIXES, true)? {
            let relative = match canonical(&path).strip_prefix(&base) {
                Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/"),
                Err(_) => path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            };
            if path.to_string_lossy().to_lowercase().ends_with(".schema.xlsx") {
                errors.add(&format!("[{relative}]"), "A1", tr(
                    "열거형 폴더에는 열거형 스키마(.enum.xlsx)만 둡니다",
                    "the enum folder holds enum schemas (.enum.xlsx) only",
                ));
            } else {
                enum_files.push((path, relative));
            }
        }
    }
    collect(&table_files, ".schema.xlsx", false, &mut result.tables, errors);
    collect(&enum_files, ".enum.xlsx", true, &mut result.enums, errors);
    Ok(result)
}

fn collect(
    files: &[(PathBuf, String)],
    suffix: &str,
    is_enum: bool,
    into: &mut BTreeMap<String, Schema>,
    errors: &mut ErrorCollector,
) {
    for (path, relative) in files {
        let Some(schema) = read_schema(path, relative, is_enum, errors) else { continue };
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem = &file_name[..file_name.len() - suffix.len()];
        if schema.name != stem {
            errors.add(&schema.where_(), "A1", tr(
                format!("이름 '{}'과 파일 이름 '{file_name}'이 다릅니다. 파일 이름은 '{}{suffix}'여야 합니다", schema.name, schema.name),
                format!("name '{}' does not match the file name '{file_name}'; name the file '{}{suffix}'", schema.name, schema.name),
            ));
            continue;
        }
        if let Some(existing) = into.get(&schema.name) {
            errors.add(&schema.where_(), "A1", tr(
                format!("'{}'의 스키마가 {}에도 있습니다", schema.name, existing.where_()),
                format!("'{}' also has a schema in {}", schema.name, existing.where_()),
            ));
            continue;
        }
        into.insert(schema.name.clone(), schema);
    }
}

fn read_schema(path: &Path, relative: &str, is_enum: bool, errors: &mut ErrorCollector) -> Option<Schema> {
    let sheets = match read_workbook(path, |title| !title.starts_with('#')) {
        Ok(sheets) => sheets,
        Err(error) => {
            errors.add(&format!("[{relative}]"), "A1", tr(
                format!("xlsx 파일을 읽을 수 없습니다: {error}"),
                format!("cannot read the xlsx file: {error}"),
            ));
            return None;
        }
    };
    let definitions: Vec<_> = sheets.into_iter().filter(|s| !s.title.starts_with('#')).collect();
    if definitions.len() != 1 {
        errors.add(&format!("[{relative}]"), "A1", tr(
            "스키마 파일에는 정의 시트가 하나만 있어야 합니다('#' 메모 시트 제외)",
            "a schema file needs exactly one definition sheet ('#' note sheets aside)",
        ));
        return None;
    }
    let sheet = definitions.into_iter().next()?;
    let name = sheet.title.split('#').next().unwrap_or("").trim().to_string();
    let mut schema = Schema { file: relative.to_string(), name, rows: Vec::new(), is_enum, title: sheet.title.clone() };
    if !is_identifier(&schema.name) {
        errors.add(&schema.where_(), "A1", tr(
            format!("올바르지 않은 이름 '{}'", schema.name),
            format!("invalid name '{}'", schema.name),
        ));
        return None;
    }
    let grid = sheet.grid?;
    let width = if is_enum { 3 } else { 4 };
    for row in 2..=grid.max_row() {
        let values: Vec<Cell> = (1..=width).map(|c| grid.value(row, c).clone()).collect();
        if values[..width - 1].iter().all(Cell::is_blank) {
            continue;
        }
        if values[0].is_blank() {
            errors.add(&schema.where_(), &format!("A{row}"), tr("이름이 비어 있습니다", "the name is empty"));
            continue;
        }
        let mut values = values.into_iter();
        let first = values.next().unwrap();
        let second = values.next().unwrap();
        let third = values.next().unwrap();
        if is_enum {
            schema.rows.push((row, first, second, Cell::Empty, third));
        } else {
            let fourth = values.next().unwrap();
            schema.rows.push((row, first, second, third, fourth));
        }
    }
    Some(schema)
}

/// A schema workbook. The creation time is pinned so the same definition gives the same file.
pub fn render_xlsx(schema: &Schema) -> Result<Vec<u8>, String> {
    let mut workbook = Workbook::new();
    let properties = DocProperties::new()
        .set_author("DrTableSystem")
        .set_creation_datetime(&rust_xlsxwriter::ExcelDateTime::from_ymd(2000, 1, 1).map_err(|e| e.to_string())?);
    workbook.set_properties(&properties);
    let sheet = workbook.add_worksheet();
    sheet.set_name(&schema.name).map_err(|e| e.to_string())?;
    let (labels, widths): (&[&str], &[f64]) = if schema.is_enum {
        (&ENUM_LABELS, &[28.0, 10.0, 40.0])
    } else {
        (&TABLE_LABELS, &[24.0, 28.0, 10.0, 40.0])
    };
    for (column, label) in labels.iter().enumerate() {
        sheet.write_string(0, column as u16, *label).map_err(|e| e.to_string())?;
    }
    for (column, width) in widths.iter().enumerate() {
        sheet.set_column_width(column as u16, *width).map_err(|e| e.to_string())?;
    }
    for (index, (_, name, second, third, comment)) in schema.rows.iter().enumerate() {
        let row = index as u32 + 1;
        let values: Vec<&Cell> = if schema.is_enum { vec![name, second, comment] } else { vec![name, second, third, comment] };
        for (column, value) in values.into_iter().enumerate() {
            write_cell(sheet, row, column as u16, value)?;
        }
    }
    workbook.save_to_buffer().map_err(|e| e.to_string())
}

pub fn write_cell(sheet: &mut rust_xlsxwriter::Worksheet, row: u32, column: u16, value: &Cell) -> Result<(), String> {
    let result = match value {
        Cell::Empty => return Ok(()),
        Cell::Int(i) => sheet.write_number(row, column, *i as f64).map(|_| ()),
        Cell::Float(f) => sheet.write_number(row, column, *f).map(|_| ()),
        Cell::Str(s) | Cell::Date(s) => sheet.write_string(row, column, s).map(|_| ()),
        Cell::Bool(b) => sheet.write_boolean(row, column, *b).map(|_| ()),
    };
    result.map_err(|e| e.to_string())
}
