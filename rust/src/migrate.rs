//! Converts old workbooks, whose sheets carried the field name, type and scope in rows 1-3.
//!
//! * Each table sheet's header becomes `<Table>.schema.xlsx`.
//! * Each `<enum>Name` sheet becomes `<Enum>.enum.xlsx` in the enum folder, its rows the enum
//!   values. Extra columns (other than Id, Value and Comment) become a `<Enum>Info` table: a
//!   schema keyed by the enum and a new data workbook holding its rows.
//!
//! Existing data workbooks are never modified; only new files are written.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rust_xlsxwriter::Workbook;

use crate::errors::ValidationErrors;
use crate::reader::{read_workbook, Grid};
use crate::schema::{DATA_ROW, NAME_ROW, SCOPE_ROW, TYPE_ROW};
use crate::schemafile::{render_xlsx, write_cell, Schema, SCHEMA_SUFFIXES};
use crate::sources::{enum_sheet_name, find_files, is_identifier, strip_sheet_comment, table_name_of};
use crate::value::Cell;

/// (0-based column, name, type, scope) of an old header.
type Header = Vec<(usize, Cell, Cell, Cell)>;

/// Writes schema files (and <Enum>Info data workbooks) for the old sheets under `input`.
/// The first sheet of a table, in file and sheet order, defines its schema. Existing files
/// are kept unless `overwrite` is set. Returns the files written.
pub fn extract_schemas(input: &Path, schema_dir: &Path, enum_dir: &Path, overwrite: bool) -> Result<Vec<PathBuf>, ValidationErrors> {
    let mut written = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (path, _) in find_files(input, &[".xlsx"], false)? {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if SCHEMA_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            continue;
        }
        let Ok(sheets) = read_workbook(&path, |_| true) else { continue };
        let data_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        for sheet in sheets {
            let stripped = strip_sheet_comment(&sheet.title);
            let enum_name = enum_sheet_name(stripped);
            let name = match enum_name {
                Some(name) => name.to_string(),
                None => match table_name_of(&sheet.title) {
                    Some(name) => name.to_string(),
                    None => continue,
                },
            };
            let key = if enum_name.is_some() { format!("<enum>{name}") } else { name.clone() };
            if name.is_empty() || !is_identifier(&name) || seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            let Some(grid) = &sheet.grid else { continue };
            let header = old_header(grid);
            if header.is_empty() {
                continue;
            }
            if enum_name.is_some() {
                written.extend(convert_enum(grid, &name, &header, schema_dir, enum_dir, &data_dir, overwrite));
            } else {
                let rows = header.iter().map(|(_, n, t, s)| (0, n.clone(), t.clone(), s.clone(), Cell::Empty)).collect();
                written.extend(write_schema(&Schema { file: String::new(), name, rows, is_enum: false, title: String::new() }, schema_dir, overwrite));
            }
        }
    }
    Ok(written)
}

fn convert_enum(
    grid: &Grid,
    name: &str,
    header: &Header,
    schema_dir: &Path,
    enum_dir: &Path,
    data_dir: &Path,
    overwrite: bool,
) -> Vec<PathBuf> {
    let key_column = header
        .iter()
        .find(|(_, _, t, _)| t.py_str().trim().to_lowercase().starts_with("id<"))
        .map(|h| h.0)
        .unwrap_or(0);
    let by_name = |wanted: &str| header.iter().find(|(_, n, _, _)| n.py_str().trim() == wanted).map(|h| h.0);
    let value_column = by_name("Value");
    let comment_column = by_name("Comment");
    let extras: Vec<&(usize, Cell, Cell, Cell)> = header
        .iter()
        .filter(|(index, _, _, scope)| {
            *index != key_column && Some(*index) != value_column && Some(*index) != comment_column && scope.py_str().trim() != "#"
        })
        .collect();
    let cell = |row: usize, column: Option<usize>| -> Cell {
        column.map(|c| grid.value(row, c + 1).clone()).unwrap_or(Cell::Empty)
    };
    let data_rows: Vec<usize> = (DATA_ROW..=grid.max_row()).filter(|&row| !cell(row, Some(key_column)).is_blank()).collect();
    let values = data_rows
        .iter()
        .map(|&row| (0, cell(row, Some(key_column)), cell(row, value_column), Cell::Empty, cell(row, comment_column)))
        .collect();
    let mut written = write_schema(&Schema { file: String::new(), name: name.to_string(), rows: values, is_enum: true, title: String::new() }, enum_dir, overwrite);
    if !extras.is_empty() {
        let info = format!("{name}Info");
        let mut fields = vec![(0, Cell::Str("Id".into()), Cell::Str(format!("ID<E{name}>")), Cell::Str("all".into()), Cell::Empty)];
        fields.extend(extras.iter().map(|(_, n, t, s)| (0, n.clone(), t.clone(), s.clone(), Cell::Empty)));
        written.extend(write_schema(&Schema { file: String::new(), name: info.clone(), rows: fields, is_enum: false, title: String::new() }, schema_dir, overwrite));
        let target = data_dir.join(format!("{info}.xlsx"));
        if overwrite || !target.exists() {
            let mut workbook = Workbook::new();
            let filled = (|| -> Result<(), String> {
                let sheet = workbook.add_worksheet();
                sheet.set_name(&info).map_err(|e| e.to_string())?;
                sheet.write_string(0, 0, "Id").map_err(|e| e.to_string())?;
                for (position, (_, field_name, _, _)) in extras.iter().enumerate() {
                    sheet.write_string(0, position as u16 + 1, field_name.py_str()).map_err(|e| e.to_string())?;
                }
                for (index, &row) in data_rows.iter().enumerate() {
                    let out_row = index as u32 + 3;
                    write_cell(sheet, out_row, 0, &cell(row, Some(key_column)))?;
                    for (position, (column, _, _, _)) in extras.iter().enumerate() {
                        write_cell(sheet, out_row, position as u16 + 1, &cell(row, Some(*column)))?;
                    }
                }
                Ok(())
            })();
            let saved = filled.and_then(|_| workbook.save(&target).map_err(|e| e.to_string()));
            if saved.is_ok() {
                written.push(target);
            }
        }
    }
    written
}

fn write_schema(schema: &Schema, folder: &Path, overwrite: bool) -> Vec<PathBuf> {
    let kind = if schema.is_enum { "enum" } else { "schema" };
    let target = folder.join(format!("{}.{kind}.xlsx", schema.name));
    if target.exists() && !overwrite {
        return Vec::new();
    }
    if std::fs::create_dir_all(folder).is_err() {
        return Vec::new();
    }
    match render_xlsx(schema).map(|bytes| std::fs::write(&target, bytes)) {
        Ok(Ok(())) => vec![target],
        _ => Vec::new(),
    }
}

/// The old rows 1-3, up to the first empty name.
fn old_header(grid: &Grid) -> Header {
    let mut fields = Vec::new();
    for column in 1..=grid.max_column() {
        let name = grid.value(NAME_ROW, column);
        if name.is_blank() {
            break;
        }
        fields.push((column - 1, name.clone(), grid.value(TYPE_ROW, column).clone(), grid.value(SCOPE_ROW, column).clone()));
    }
    fields
}
