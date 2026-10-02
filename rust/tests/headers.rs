//! Reference headers (formulas in rows 2-3 of a data sheet) and link warnings. Excel's results
//! for the formulas were checked separately with Excel on the PC.
//!
//! The tool never edits existing data files; `drtable new` puts the formulas in new files only.

mod common;

use calamine::{Reader, Xlsx, open_workbook};
use common::*;
use std::fs;
use std::path::Path;

fn schemas(tmp: &Path) -> std::path::PathBuf {
    let folder = tmp.join("Tables/Schemas");
    write_schema(&folder.join("Items.schema.xlsx"), "Items", &fields(&[("Id", "ID<int32>", "all"), ("Cost", "int32", "server")]));
    folder
}

fn new(tmp: &Path, table: &str, out: &Path) -> Output {
    drtable(["new", "--table", table, "--out", &s(out), "--schema", &s(&tmp.join("Tables/Schemas"))])
}

fn build_tables(tmp: &Path) -> Output {
    let out = tmp.join("out");
    drtable([
        "build", "--input", &s(&tmp.join("Tables")), "--schema", &s(&tmp.join("Tables/Schemas")),
        "--out-cpp", &s(&out.join("cpp")), "--out-client", &s(&out.join("c")), "--out-server", &s(&out.join("s")),
    ])
}

fn link_targets(path: &Path) -> Vec<String> {
    zip_parts(path, |name| name.ends_with(".xml.rels") && name.contains("externalLink"))
}

#[test]
fn new_creates_a_workbook_with_reference_formulas() {
    let tmp = Tmp::new();
    schemas(&tmp);
    let out = tmp.join("Tables/Items.xlsx");
    assert_eq!(new(&tmp, "Items", &out).code, 0);
    let mut workbook: Xlsx<_> = open_workbook(&out).unwrap();
    assert_eq!(workbook.sheet_names(), ["Items"]);
    let values = workbook.worksheet_range("Items").unwrap();
    let names: Vec<String> = (0..2).map(|c| values.get_value((0, c)).unwrap().to_string()).collect();
    assert_eq!(names, ["Id", "Cost"]);
    let formulas = workbook.worksheet_formula("Items").unwrap();
    let formula = |row: u32, column: u32| formulas.get_value((row, column)).cloned().unwrap_or_default();
    assert_eq!(
        formula(1, 0).trim_start_matches('='),
        // An array's element columns (Reward[0], ...) look up the field name before "[".
        "IFERROR(INDEX('[1]Items'!$B:$B,MATCH(IFERROR(LEFT(A$1,FIND(\"[\",A$1)-1),A$1),'[1]Items'!$A:$A,0)),\"(스키마에 없음)\")",
    );
    assert!(formula(2, 1).trim_start_matches('=').starts_with("IFERROR(INDEX('[1]Items'!$C:$C,MATCH(IFERROR(LEFT(B$1,"));
    assert!(link_targets(&out)[0].contains("Target=\"Schemas/Items.schema.xlsx\""));
    // The build does not read rows 2-3, so this is an empty table.
    assert_eq!(build_tables(&tmp).code, 0);
}

#[test]
fn new_never_overwrites() {
    let tmp = Tmp::new();
    schemas(&tmp);
    let out = tmp.join("Tables/Items.xlsx");
    fs::write(&out, b"designer data").unwrap();
    let result = new(&tmp, "Items", &out);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("기존 데이터 파일을 고치지 않습니다"), "{}", result.stderr);
    assert_eq!(fs::read(&out).unwrap(), b"designer data");
}

#[test]
fn new_needs_a_schema() {
    let tmp = Tmp::new();
    schemas(&tmp);
    let result = new(&tmp, "Nope", &tmp.join("Tables/Nope.xlsx"));
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("'Nope'의 스키마가 없습니다"), "{}", result.stderr);
}

#[test]
fn build_warns_when_the_link_path_is_missing_here() {
    let tmp = Tmp::new();
    schemas(&tmp);
    let path = tmp.join("Tables/Items.xlsx");
    assert_eq!(new(&tmp, "Items", &path).code, 0);
    // As if saved on another PC: the link points to an absolute path that is not here.
    rewrite_zip(&path, |name, bytes| {
        if name.contains("externalLink") && name.ends_with(".rels") {
            let text = String::from_utf8(bytes).unwrap();
            text.replace("Target=\"Schemas/Items.schema.xlsx\"", "Target=\"file:///D:/Other/Schemas/Items.schema.xlsx\"").into_bytes()
        } else {
            bytes
        }
    });
    let result = build_tables(&tmp);
    assert_eq!(result.code, 0);
    assert!(
        result.stderr.contains("참고 헤더가 이 PC에 없는 경로를 가리킵니다(file:///D:/Other/Schemas/Items.schema.xlsx)"),
        "{}",
        result.stderr
    );
    // A file with a working link gives no warning.
    fs::remove_file(&path).unwrap();
    assert_eq!(new(&tmp, "Items", &path).code, 0);
    let result = build_tables(&tmp);
    assert_eq!(result.code, 0);
    assert!(!result.stderr.contains("참고 헤더"));
}
