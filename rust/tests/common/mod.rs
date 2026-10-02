//! Shared helpers for the black-box tests of the `drtable` executable.
//!
//! Most tests describe a table compactly, as one sheet with the field name, type and scope in
//! rows 1-3 (enums in `<enum>` sheets). [`Book::save`] writes such a workbook and also its schema
//! files next to it (tables) and under `Enums/` (enums), so each test keeps its fixture in one
//! readable place. Tests that write schema files themselves use [`Book::save_plain`].
//!
//! Messages are asserted in Korean (`DRTABLE_LANG=ko`); English is covered by i18n.rs.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_xlsxwriter::{DocProperties, ExcelDateTime, Workbook};

/// A cell value as a test writes it (`E` is an empty cell, Python's None).
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    S(String),
    I(i64),
    F(f64),
    B(bool),
    /// An Excel date cell: (year, month, day, hour, minute, second).
    Date(u16, u8, u8, u16, u8, f64),
    /// An Excel time-of-day / duration cell, as a fraction of a day, formatted [h]:mm:ss.
    Time(f64),
    E,
}

impl From<&str> for V {
    fn from(value: &str) -> Self {
        V::S(value.to_string())
    }
}
impl From<String> for V {
    fn from(value: String) -> Self {
        V::S(value)
    }
}
impl From<i32> for V {
    fn from(value: i32) -> Self {
        V::I(value.into())
    }
}
impl From<i64> for V {
    fn from(value: i64) -> Self {
        V::I(value)
    }
}
impl From<f64> for V {
    fn from(value: f64) -> Self {
        V::F(value)
    }
}
impl From<bool> for V {
    fn from(value: bool) -> Self {
        V::B(value)
    }
}
impl From<()> for V {
    fn from(_: ()) -> Self {
        V::E
    }
}

impl V {
    fn is_blank(&self) -> bool {
        matches!(self, V::E) || *self == V::S(String::new())
    }

    /// Python's `str(value).strip()` for names and types.
    fn text(&self) -> String {
        match self {
            V::S(text) => text.trim().to_string(),
            V::I(value) => value.to_string(),
            V::F(value) => value.to_string(),
            V::B(true) => "True".into(),
            V::B(false) => "False".into(),
            V::Date(..) | V::Time(_) => "date".into(),
            V::E => "None".into(),
        }
    }
}

/// A row of cell values: `row![1001, "Burn", (), 12.5]` (`()` is an empty cell).
macro_rules! row {
    ($($value:expr),* $(,)?) => { vec![$($crate::common::V::from($value)),*] };
}

pub type Row = Vec<V>;

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Row>,
}

impl Sheet {
    pub fn append(&mut self, row: Row) {
        self.rows.push(row);
    }

    /// Sets one cell by its A1 name, growing the sheet as needed.
    pub fn set(&mut self, cell: &str, value: impl Into<V>) {
        let letters: String = cell.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let row: usize = cell[letters.len()..].parse().expect("cell row");
        let column = letters.bytes().fold(0usize, |acc, b| acc * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize) - 1;
        if self.rows.len() < row {
            self.rows.resize(row, Vec::new());
        }
        let cells = &mut self.rows[row - 1];
        if cells.len() <= column {
            cells.resize(column + 1, V::E);
        }
        cells[column] = value.into();
    }

    fn cell(&self, row: usize, column: usize) -> &V {
        self.rows.get(row).and_then(|cells| cells.get(column)).unwrap_or(&V::E)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Book {
    pub sheets: Vec<Sheet>,
}

impl Book {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a sheet (builder style).
    pub fn with(mut self, name: &str, rows: Vec<Row>) -> Self {
        self.add(name, rows);
        self
    }

    pub fn add(&mut self, name: &str, rows: Vec<Row>) -> &mut Sheet {
        self.sheets.push(Sheet { name: name.to_string(), rows });
        self.sheets.last_mut().unwrap()
    }

    pub fn sheet(&mut self, name: &str) -> &mut Sheet {
        self.sheets.iter_mut().find(|sheet| sheet.name == name).expect("sheet")
    }

    /// Writes the workbook as it is: no schema files.
    pub fn save_plain(&self, path: &Path) {
        write_xlsx(&self.sheets, path);
    }

    /// Writes the workbook. When it is in the compact layout (some sheet declares a key type
    /// such as `ID<int32>` in row 2) this also writes its schema files, and the data workbook
    /// drops its `<enum>` sheets (deleted when nothing is left).
    pub fn save(&self, path: &Path) {
        self.save_plain(path);
        if !self.has_compact_headers() {
            return;
        }
        let folder = path.parent().unwrap();
        self.extract_schemas(folder, &folder.join("Enums"));
        if self.sheets.iter().any(|sheet| sheet.name.starts_with("<enum>")) {
            let kept: Vec<Sheet> =
                self.sheets.iter().filter(|sheet| !sheet.name.starts_with("<enum>")).cloned().collect();
            if kept.is_empty() {
                fs::remove_file(path).unwrap();
            } else {
                write_xlsx(&kept, path);
            }
        }
    }

    fn has_compact_headers(&self) -> bool {
        self.sheets.iter().any(|sheet| {
            sheet.rows.get(1).is_some_and(|cells| {
                cells.iter().any(|cell| !matches!(cell, V::E) && cell.text().to_lowercase().starts_with("id<"))
            })
        })
    }

    /// The first sheet of each table defines its schema; each `<enum>Name` sheet becomes
    /// `<Name>.enum.xlsx`, its extra columns a `<Name>Info` table (schema plus data workbook).
    fn extract_schemas(&self, schema_dir: &Path, enum_dir: &Path) {
        let mut seen = BTreeSet::new();
        for sheet in &self.sheets {
            let title = strip_sheet_comment(&sheet.name);
            let (name, is_enum) = match title.strip_prefix("<enum>") {
                Some(name) => (name.to_string(), true),
                None if sheet.name.starts_with('#') => continue,
                None => (title.clone(), false),
            };
            let key = if is_enum { format!("<enum>{name}") } else { name.clone() };
            if !is_identifier(&name) || !seen.insert(key) {
                continue;
            }
            let header = header(sheet);
            if header.is_empty() {
                continue;
            }
            if is_enum {
                convert_enum(sheet, &name, &header, schema_dir, enum_dir);
            } else {
                let fields: Vec<_> = header.into_iter().map(|(_, n, t, s)| (n, t, s)).collect();
                write_schema(&schema_dir.join(format!("{name}.schema.xlsx")), &name, &fields);
            }
        }
    }
}

type Field = (usize, V, V, V);

/// (column, name, type, scope) from rows 1-3, up to the first empty name.
fn header(sheet: &Sheet) -> Vec<Field> {
    let names = sheet.rows.first().cloned().unwrap_or_default();
    let mut fields = Vec::new();
    for (index, name) in names.into_iter().enumerate() {
        if name.is_blank() {
            break;
        }
        fields.push((index, name, sheet.cell(1, index).clone(), sheet.cell(2, index).clone()));
    }
    fields
}

fn convert_enum(sheet: &Sheet, name: &str, header: &[Field], schema_dir: &Path, enum_dir: &Path) {
    let key_column = header
        .iter()
        .find(|(_, _, kind, _)| kind.text().to_lowercase().starts_with("id<"))
        .map_or(0, |field| field.0);
    let column_of = |wanted: &str| header.iter().find(|field| field.1.text() == wanted).map(|field| field.0);
    let (value_column, comment_column) = (column_of("Value"), column_of("Comment"));
    let extras: Vec<&Field> = header
        .iter()
        .filter(|(index, _, _, scope)| {
            *index != key_column
                && Some(*index) != value_column
                && Some(*index) != comment_column
                && scope.text() != "#"
        })
        .collect();
    let data: Vec<&Row> = sheet.rows.iter().skip(3).filter(|row| row.get(key_column).is_some_and(|key| !key.is_blank())).collect();
    let cell = |row: &Row, index: Option<usize>| index.and_then(|i| row.get(i).cloned()).unwrap_or(V::E);
    let values: Vec<_> = data
        .iter()
        .map(|row| (cell(row, Some(key_column)), cell(row, value_column), cell(row, comment_column)))
        .collect();
    write_enum(&enum_dir.join(format!("{name}.enum.xlsx")), name, &values);
    if extras.is_empty() {
        return;
    }
    let info = format!("{name}Info");
    let mut fields = vec![(V::from("Id"), V::from(format!("ID<E{name}>")), V::from("all"))];
    fields.extend(extras.iter().map(|(_, n, t, s)| (n.clone(), t.clone(), s.clone())));
    write_schema(&schema_dir.join(format!("{info}.schema.xlsx")), &info, &fields);
    let mut rows = vec![std::iter::once(V::from("Id")).chain(extras.iter().map(|f| f.1.clone())).collect(), vec![], vec![]];
    for row in data {
        rows.push(std::iter::once(cell(row, Some(key_column))).chain(extras.iter().map(|f| cell(row, Some(f.0)))).collect());
    }
    write_xlsx(&[Sheet { name: info.clone(), rows }], &schema_dir.join(format!("{info}.xlsx")));
}

fn strip_sheet_comment(title: &str) -> String {
    title.split('#').next().unwrap().trim().to_string()
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A table schema workbook: one (field, type, scope) per row under Field/Type/Scope/Comment.
pub fn write_schema(path: &Path, name: &str, fields: &[(V, V, V)]) {
    let mut rows = vec![row!["Field", "Type", "Scope", "Comment"]];
    rows.extend(fields.iter().map(|(n, t, s)| vec![n.clone(), t.clone(), s.clone()]));
    write_xlsx(&[Sheet { name: name.to_string(), rows }], path);
}

/// An enum schema workbook: one (name, value, comment) per row under Name/Value/Comment.
pub fn write_enum(path: &Path, name: &str, values: &[(V, V, V)]) {
    let mut rows = vec![row!["Name", "Value", "Comment"]];
    rows.extend(values.iter().map(|(n, v, c)| vec![n.clone(), v.clone(), c.clone()]));
    write_xlsx(&[Sheet { name: name.to_string(), rows }], path);
}

/// Table fields given as plain text.
pub fn fields(items: &[(&str, &str, &str)]) -> Vec<(V, V, V)> {
    items.iter().map(|(n, t, s)| (V::from(*n), V::from(*t), V::from(*s))).collect()
}

fn write_xlsx(sheets: &[Sheet], path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut workbook = Workbook::new();
    let fixed = ExcelDateTime::from_ymd(2000, 1, 1).unwrap();
    workbook.set_properties(&DocProperties::new().set_creation_datetime(&fixed));
    for sheet in sheets {
        let target = workbook.add_worksheet();
        target.set_name(&sheet.name).unwrap();
        for (r, cells) in sheet.rows.iter().enumerate() {
            for (c, value) in cells.iter().enumerate() {
                let (r, c) = (r as u32, c as u16);
                match value {
                    V::S(text) => target.write_string(r, c, text).map(|_| ()),
                    V::I(number) => target.write_number(r, c, *number as f64).map(|_| ()),
                    V::F(number) => target.write_number(r, c, *number).map(|_| ()),
                    V::B(flag) => target.write_boolean(r, c, *flag).map(|_| ()),
                    V::Date(year, month, day, hour, minute, second) => {
                        let date = ExcelDateTime::from_ymd(*year, *month, *day).unwrap().and_hms(*hour, *minute, *second).unwrap();
                        let format = rust_xlsxwriter::Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
                        target.write_datetime_with_format(r, c, &date, &format).map(|_| ())
                    }
                    V::Time(days) => {
                        let format = rust_xlsxwriter::Format::new().set_num_format("[h]:mm:ss");
                        target.write_number_with_format(r, c, *days, &format).map(|_| ())
                    }
                    V::E => Ok(()),
                }
                .unwrap();
            }
        }
    }
    workbook.save(path).unwrap();
}

/// Rewrites the parts of an xlsx (zip) file.
pub fn rewrite_zip(path: &Path, mut edit: impl FnMut(&str, Vec<u8>) -> Vec<u8>) {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut parts = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        parts.push((entry.name().to_string(), data));
    }
    let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, data) in parts {
        let data = edit(&name, data);
        writer.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(&data).unwrap();
    }
    writer.finish().unwrap();
}

/// The text of every part of an xlsx file whose name matches.
pub fn zip_parts(path: &Path, matches: impl Fn(&str) -> bool) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut found = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        if matches(entry.name()) {
            let mut text = String::new();
            entry.read_to_string(&mut text).unwrap();
            found.push(text);
        }
    }
    found
}

// --- Running the executable ----------------------------------------------------------------

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// A command line usage error (exit code 2 with a "drtable: error:" line).
    pub fn is_usage_error(&self) -> bool {
        self.code == 2 && self.stderr.contains("drtable: error:")
    }
}

/// Runs `drtable args...` with Korean messages.
pub fn drtable<S: AsRef<OsStr>>(args: impl IntoIterator<Item = S>) -> Output {
    drtable_lang("ko", args)
}

pub fn drtable_lang<S: AsRef<OsStr>>(language: &str, args: impl IntoIterator<Item = S>) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_drtable"))
        .args(args)
        .env("DRTABLE_LANG", language)
        .output()
        .expect("run drtable");
    Output {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// `drtable build --input source` with the outputs under root (cpp, client, server).
pub fn build(source: &Path, root: &Path, extra: &[&str]) -> Output {
    let mut args: Vec<String> = vec!["build".into(), "--input".into(), s(source)];
    for (option, folder) in [("--out-cpp", "cpp"), ("--out-client", "client"), ("--out-server", "server")] {
        args.push(option.into());
        args.push(s(&root.join(folder)));
    }
    args.extend(extra.iter().map(|arg| arg.to_string()));
    drtable(args)
}

pub fn s(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// --- Files -----------------------------------------------------------------------------------

/// A temporary folder removed when the test ends.
pub struct Tmp(PathBuf);

impl Tmp {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("drtable-test-{}-{index}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Tmp(path)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Deref for Tmp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

pub fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

pub fn json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&read(path)).unwrap()
}

pub fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_string(value).unwrap()).unwrap();
}

/// File names in a folder.
pub fn names(folder: &Path) -> BTreeSet<String> {
    fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

/// Every file under root (relative path → bytes).
pub fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    collect(root, root, &mut found);
    found
}

fn collect(root: &Path, folder: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, found);
        } else {
            let relative = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            found.insert(relative, fs::read(&path).unwrap());
        }
    }
}
