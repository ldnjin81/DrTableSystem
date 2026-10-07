//! 새 데이터 파일의 엑셀 유효성은 XLSX 내부 XML에서 확인한다.

#[macro_use]
mod common;

use common::*;
use std::fs;

fn create(columns: &[(&str, &str, &str)], enum_names: &[String]) -> (Tmp, String) {
    let tmp = Tmp::new();
    let schema_dir = tmp.join("Tables/Schemas");
    let mut rows = vec![row!["Field", "Type", "Scope", "Comment", "Rule"]];
    rows.push(row!["Id", "ID<int32>", "all", "", ""]);
    for (name, type_name, rule) in columns {
        rows.push(row![*name, *type_name, "all", "", *rule]);
    }
    Book::new().with("Items", rows).save_plain(&schema_dir.join("Items.schema.xlsx"));
    if !enum_names.is_empty() {
        let values: Vec<_> = enum_names.iter().enumerate().map(|(index, name)| (V::from(name.as_str()), V::from(index as i32), V::E)).collect();
        write_enum(&tmp.join("Tables/Enums/Kind.enum.xlsx"), "Kind", &values);
    }
    let out = tmp.join("Tables/Items.xlsx");
    let result = drtable(["new", "--table", "Items", "--out", &s(&out), "--schema", &s(&schema_dir)]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let xml = zip_parts(&out, |name| name == "xl/worksheets/sheet1.xml").pop().unwrap();
    (tmp, xml)
}

#[test]
fn short_enum_list() {
    let (_, xml) = create(&[("Kind", "EKind", "")], &["Red".into(), "Blue".into()]);
    assert!(xml.contains("type=\"list\""));
    assert!(xml.contains("sqref=\"B4:B1048576\""));
    assert!(xml.contains("Red,Blue"));
}

#[test]
fn long_enum_uses_hidden_sheet() {
    let names: Vec<String> = (0..40).map(|n| format!("Choice_{n:03}")).collect();
    let (tmp, xml) = create(&[("Kind", "EKind", "")], &names);
    assert!(xml.contains("#Lists"));
    let workbook = zip_parts(&tmp.join("Tables/Items.xlsx"), |name| name == "xl/workbook.xml").pop().unwrap();
    assert!(workbook.contains("state=\"hidden\""));
}

#[test]
fn inclusive_integer_range() {
    let (_, xml) = create(&[("Score", "int32", "0..100")], &[]);
    assert!(xml.contains("type=\"whole\""));
    assert!(xml.contains("<formula1>0</formula1><formula2>100</formula2>"));
}

#[test]
fn lower_integer_bound() {
    let (_, xml) = create(&[("Count", "int32", ">=0")], &[]);
    assert!(xml.contains("type=\"whole\" operator=\"greaterThanOrEqual\""));
}

#[test]
fn decimal_bound() {
    let (_, xml) = create(&[("Weight", "float", "<1.5")], &[]);
    assert!(xml.contains("type=\"decimal\" operator=\"lessThan\""));
    assert!(xml.contains("<formula1>1.5</formula1>"));
}

#[test]
fn fixed_uses_entered_decimal() {
    let (_, xml) = create(&[("Rate", "fixed<1000>", "0..1")], &[]);
    assert!(xml.contains("type=\"decimal\""));
    assert!(xml.contains("<formula1>0</formula1><formula2>1</formula2>"));
}

#[test]
fn text_length() {
    let (_, xml) = create(&[("Name", "string", "len<=32")], &[]);
    assert!(xml.contains("type=\"textLength\" operator=\"lessThanOrEqual\""));
    assert!(xml.contains("<formula1>32</formula1>"));
}

#[test]
fn regex_and_required_stay_in_build() {
    let (_, xml) = create(&[("Name", "string", "required;regex:^[A-Z]+$")], &[]);
    assert!(!xml.contains("<dataValidations"));
}

#[test]
fn comma_array_has_no_element_validation() {
    let (_, xml) = create(&[("Values", "int32[]", "0..10")], &[]);
    assert!(!xml.contains("<dataValidations"));
}

#[test]
fn existing_file_is_untouched() {
    let (tmp, _) = create(&[("Kind", "string", "len<=4")], &[]);
    let out = tmp.join("Tables/Items.xlsx");
    let before = fs::read(&out).unwrap();
    let result = drtable(["new", "--table", "Items", "--out", &s(&out), "--schema", &s(&tmp.join("Tables/Schemas"))]);
    assert_eq!(result.code, 1);
    assert_eq!(fs::read(&out).unwrap(), before);
}
