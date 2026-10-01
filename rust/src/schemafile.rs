//! Schema files: where tables and enums are defined, apart from the data.
//!
//! * Table schemas: `<Table>.schema.xlsx` in the schema folder.
//! * Enum schemas: `<Enum>.enum.xlsx` in the enum folder (default: `Enums` next to the schema folder).
//! * String table schemas: `<Name>.string.xlsx` in the schema folder. They define the table
//!   `<Name>String`: an `Id` name key and one column per language.
//!
//! Table schema: one sheet named after the table; row 1 labels, from row 2 one field per row
//! (A name, B type, C scope, D comment). Enum schema: one sheet named after the enum; from
//! row 2 one enumerator per row (A name, B value, C comment). String table schema: one sheet
//! named `<Name>`; from row 2 one language per row (A language code, B base language mark,
//! C scope (default client), D comment).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::errors::{ErrorCollector, ValidationErrors};
use crate::i18n::tr;
use crate::reader::read_workbook;
use crate::schema::RawColumn;
use crate::sources::{find_files, is_identifier};
use crate::value::Cell;

pub const TABLE_SUFFIXES: [&str; 1] = [".schema.xlsx"];
pub const ENUM_SUFFIXES: [&str; 1] = [".enum.xlsx"];
pub const STRING_SUFFIXES: [&str; 1] = [".string.xlsx"];
pub const SCHEMA_SUFFIXES: [&str; 3] = [".schema.xlsx", ".enum.xlsx", ".string.xlsx"];
/// A string table `UI` (UI.string.xlsx, data sheets `UI`) is the table `UIString`.
pub const STRING_TABLE_SUFFIX: &str = "String";
pub const DEFAULT_ENUM_FOLDER: &str = "Enums";
pub const DEFAULT_STRINGS_FOLDER: &str = "Strings";

/// One definition row: (row, name, type or value, scope, comment).
pub type Row = (usize, Cell, Cell, Cell, Cell);

#[derive(Clone, Debug)]
pub struct Schema {
    pub file: String,
    pub name: String,
    pub rows: Vec<Row>,
    pub is_enum: bool,
    /// A string table schema (`.string.xlsx`); `name` is `<title>String`.
    pub is_strings: bool,
    /// Sheet name as written.
    pub title: String,
    /// The file on disk.
    pub path: PathBuf,
}

impl Schema {
    pub fn where_(&self) -> String {
        format!("[{}]{}", self.file, if self.title.is_empty() { &self.name } else { &self.title })
    }

    /// The name as written in the file (`UI` for the string table `UIString`).
    pub fn defined_name(&self) -> String {
        if self.is_strings {
            self.name.strip_suffix(STRING_TABLE_SUFFIX).unwrap_or(&self.name).to_string()
        } else {
            self.name.clone()
        }
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

/// The enum folder: `enum_root` when given, otherwise "Enums" next to the schema folder
/// (Table/Schema -> Table/Enums). When the schema folder is the data folder itself
/// (`beside` false), "Enums" inside it (Table -> Table/Enums).
pub fn enum_folder(schema_root: &Path, enum_root: Option<&Path>, beside: bool) -> PathBuf {
    sibling_folder(schema_root, enum_root, beside, DEFAULT_ENUM_FOLDER)
}

/// The string table data folder, placed like the enum folder (Table/Schema -> Table/Strings).
pub fn strings_folder(schema_root: &Path, strings_root: Option<&Path>, beside: bool) -> PathBuf {
    sibling_folder(schema_root, strings_root, beside, DEFAULT_STRINGS_FOLDER)
}

fn sibling_folder(schema_root: &Path, given: Option<&Path>, beside: bool, name: &str) -> PathBuf {
    match given {
        Some(root) => root.to_path_buf(),
        None => {
            let folder = folder_of(schema_root);
            let base = if beside { absolute(&folder).parent().map(Path::to_path_buf).unwrap_or(folder) } else { folder };
            base.join(name)
        }
    }
}

pub fn canonical(path: &Path) -> PathBuf {
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
        // Enum files are named relative to the folder that holds the enum folder ("Enums/Kind.enum.xlsx").
        let base = canonical(&absolute(enum_root).parent().map(Path::to_path_buf).unwrap_or_default());
        for (path, _) in find_files(enum_root, &SCHEMA_SUFFIXES, true)? {
            let relative = match canonical(&path).strip_prefix(&base) {
                Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/"),
                Err(_) => path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            };
            let lower = path.to_string_lossy().to_lowercase();
            if lower.ends_with(".schema.xlsx") || lower.ends_with(".string.xlsx") {
                errors.add(&format!("[{relative}]"), "A1", tr(
                    "열거형 폴더에는 열거형 스키마(.enum.xlsx)만 둡니다",
                    "the enum folder holds enum schemas (.enum.xlsx) only",
                ));
            } else {
                enum_files.push((path, relative));
            }
        }
    }
    let string_files: Vec<(PathBuf, String)> = find_files(schema_root, &STRING_SUFFIXES, true)?
        .into_iter()
        .filter(|(path, _)| !inside(path, &enum_resolved))
        .collect();
    collect(&table_files, ".schema.xlsx", Kind::Table, &mut result.tables, errors);
    collect(&string_files, ".string.xlsx", Kind::Strings, &mut result.tables, errors);
    collect(&enum_files, ".enum.xlsx", Kind::Enum, &mut result.enums, errors);
    Ok(result)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Table,
    Enum,
    Strings,
}

fn collect(
    files: &[(PathBuf, String)],
    suffix: &str,
    kind: Kind,
    into: &mut BTreeMap<String, Schema>,
    errors: &mut ErrorCollector,
) {
    for (path, relative) in files {
        let Some(schema) = read_schema(path, relative, kind, errors) else { continue };
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem = &file_name[..file_name.len() - suffix.len()];
        let defined = schema.defined_name();
        if defined != stem {
            errors.add(&schema.where_(), "A1", tr(
                format!("이름 '{defined}'과 파일 이름 '{file_name}'이 다릅니다. 파일 이름은 '{defined}{suffix}'여야 합니다"),
                format!("name '{defined}' does not match the file name '{file_name}'; name the file '{defined}{suffix}'"),
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

fn read_schema(path: &Path, relative: &str, kind: Kind, errors: &mut ErrorCollector) -> Option<Schema> {
    let is_enum = kind == Kind::Enum;
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
    let is_strings = kind == Kind::Strings;
    let mut schema = Schema { file: relative.to_string(), name, rows: Vec::new(), is_enum, is_strings, title: sheet.title.clone(), path: canonical(path) };
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
        } else if is_strings {
            let fourth = values.next().unwrap();
            // A language row is a lang column; the marked one is the base language. The code
            // may be written with '-' (zh-Hans); the field name uses '_' (zh_Hans).
            let code = Cell::Str(first.py_str().trim().replace('-', "_"));
            let kind = Cell::Str(if is_marked(&second) { "Base<lang>" } else { "lang" }.into());
            let scope = if third.is_blank() { Cell::Str("client".into()) } else { third };
            schema.rows.push((row, code, kind, scope, fourth));
        } else {
            if crate::schema::is_lang_type(&second.text_or_empty()) {
                errors.add(&schema.where_(), &format!("B{row}"), tr(
                    "언어 열은 스트링테이블 스키마(<이름>.string.xlsx)에 정의합니다",
                    "language columns are defined in a string table schema (<Name>.string.xlsx)",
                ));
                continue;
            }
            let fourth = values.next().unwrap();
            schema.rows.push((row, first, second, third, fourth));
        }
    }
    if is_strings {
        // The key is implicit: Id, a name, in every output.
        schema.rows.insert(0, (1, Cell::Str("Id".into()), Cell::Str("ID<name>".into()), Cell::Str("all".into()), Cell::Empty));
        schema.name = format!("{}{STRING_TABLE_SUFFIX}", schema.name);
    }
    Some(schema)
}

/// The base language mark: any value except an empty cell, FALSE or 0.
fn is_marked(cell: &Cell) -> bool {
    match cell {
        Cell::Empty => false,
        Cell::Bool(b) => *b,
        Cell::Int(i) => *i != 0,
        Cell::Float(f) => *f != 0.0,
        other => {
            let text = other.text_or_empty().to_lowercase();
            !text.is_empty() && text != "false" && text != "0"
        }
    }
}
