//! Reading workbooks with calamine into in-memory grids.

use std::path::Path;

use calamine::{open_workbook, Data, Reader, SheetType, Xlsx};

use crate::value::Cell;

/// A worksheet's values, addressed by 1-based (row, column) like Excel.
pub struct Grid {
    pub title: String,
    rows: Vec<Vec<Cell>>,
}

static EMPTY: Cell = Cell::Empty;

impl Grid {
    pub fn value(&self, row: usize, column: usize) -> &Cell {
        if row == 0 || column == 0 {
            return &EMPTY;
        }
        self.rows.get(row - 1).and_then(|r| r.get(column - 1)).unwrap_or(&EMPTY)
    }

    pub fn max_row(&self) -> usize {
        self.rows.len()
    }

    pub fn max_column(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }
}

/// A worksheet of a workbook: its title and, when it was read, its values.
pub struct Sheet {
    pub title: String,
    pub grid: Option<Grid>,
}

/// Opens an xlsx workbook and reads the worksheets that `want` selects by title.
/// Chart sheets are not worksheets and are left out, like openpyxl's `worksheets`.
pub fn read_workbook(path: &Path, want: impl Fn(&str) -> bool) -> Result<Vec<Sheet>, String> {
    let mut workbook: Xlsx<std::io::BufReader<std::fs::File>> =
        open_workbook(path).map_err(|e: calamine::XlsxError| e.to_string())?;
    let titles: Vec<String> = workbook
        .sheets_metadata()
        .iter()
        .filter(|sheet| sheet.typ == SheetType::WorkSheet)
        .map(|sheet| sheet.name.clone())
        .collect();
    let mut sheets = Vec::with_capacity(titles.len());
    for title in titles {
        let grid = if want(&title) {
            let range = workbook.worksheet_range(&title).map_err(|e| e.to_string())?;
            Some(to_grid(&title, &range))
        } else {
            None
        };
        sheets.push(Sheet { title, grid });
    }
    Ok(sheets)
}

fn to_grid(title: &str, range: &calamine::Range<Data>) -> Grid {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    if let Some((start_row, start_column)) = range.start() {
        let start_row = start_row as usize;
        let start_column = start_column as usize;
        rows.resize_with(start_row, Vec::new);
        for row in range.rows() {
            let mut cells: Vec<Cell> = Vec::with_capacity(start_column + row.len());
            cells.resize(start_column, Cell::Empty);
            cells.extend(row.iter().map(to_cell));
            while matches!(cells.last(), Some(Cell::Empty)) {
                cells.pop();
            }
            rows.push(cells);
        }
    }
    Grid { title: title.to_string(), rows }
}

/// calamine gives every number as f64; openpyxl gives integers for integral numbers, so
/// integral values (that f64 holds exactly) become Int.
pub fn to_cell(data: &Data) -> Cell {
    match data {
        Data::Empty => Cell::Empty,
        Data::Int(i) => Cell::Int(*i),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                Cell::Int(*f as i64)
            } else {
                Cell::Float(*f)
            }
        }
        Data::String(s) => Cell::Str(s.clone()),
        Data::Bool(b) => Cell::Bool(*b),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(value) => Cell::Date(value.format("%Y-%m-%d %H:%M:%S").to_string()),
            None => Cell::Float(dt.as_f64()),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Str(s.clone()),
        Data::Error(e) => Cell::Str(e.to_string()),
    }
}
