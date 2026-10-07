#[macro_use]
mod common;

use common::*;
use std::path::Path;

fn fixture(root: &Path, field_type: &str, rule: &str, value: impl Into<V>) {
    Book::new().with("Items", vec![row!["Id", "Amount"], row!["ID<int32>", field_type], row!["all", "all"], vec![V::from(1), value.into()]]).save_plain(&root.join("Items.xlsx"));
    Book::new().with("Items", vec![row!["Field", "Type", "Scope", "Comment", "Rule"], row!["Id", "ID<int32>", "all"], row!["Amount", field_type, "all", "", rule]]).save_plain(&root.join("Items.schema.xlsx"));
}

fn run(field_type: &str, rule: &str, value: impl Into<V>) -> Output {
    let tmp = Tmp::new();
    fixture(&tmp, field_type, rule, value);
    build(&tmp.join("Items.xlsx"), &tmp, &[])
}

#[test] fn range_lower_boundary_passes() { let out = run("int32", "0..100", 0); assert_eq!(out.code, 0, "{}", out.stderr); }
#[test] fn range_upper_boundary_passes() { let out = run("int32", "0..100", 100); assert_eq!(out.code, 0, "{}", out.stderr); }
#[test] fn range_below_fails() { let out = run("int32", "0..100", -1); assert_eq!(out.code, 1); assert!(out.stderr.contains("[Items.xlsx]Items!B4"), "{}", out.stderr); }
#[test] fn range_above_fails() { let out = run("int32", "0..100", 101); assert_eq!(out.code, 1); assert!(out.stderr.contains("[Items.xlsx]Items!B4"), "{}", out.stderr); }
#[test] fn greater_equal_passes() { let out = run("float", ">=0", 0); assert_eq!(out.code, 0, "{}", out.stderr); }
#[test] fn less_than_fails_on_boundary() { let out = run("int32", "<1000", 1000); assert_eq!(out.code, 1); assert!(out.stderr.contains("B4"), "{}", out.stderr); }
#[test] fn required_rejects_blank() { let out = run("string", "필수", ()); assert_eq!(out.code, 1); assert!(out.stderr.contains("B4"), "{}", out.stderr); }
#[test] fn length_accepts_unicode_count() { let out = run("string", "len<=2", "가나"); assert_eq!(out.code, 0, "{}", out.stderr); }
#[test] fn length_rejects_long_text() { let out = run("string", "len<=2", "가나다"); assert_eq!(out.code, 1); assert!(out.stderr.contains("B4"), "{}", out.stderr); }
#[test] fn regex_accepts_match() { let out = run("name", "regex:^[A-Z_]+$", "ABC_D"); assert_eq!(out.code, 0, "{}", out.stderr); }
#[test] fn regex_rejects_mismatch() { let out = run("name", "regex:^[A-Z_]+$", "abc"); assert_eq!(out.code, 1); assert!(out.stderr.contains("B4"), "{}", out.stderr); }
#[test] fn fixed_compares_written_decimal() { let out = run("fixed<100>", "0..1", 0.75); assert_eq!(out.code, 0, "{}", out.stderr); }
#[test] fn fixed_rejects_outside_decimal_range() { let out = run("fixed<100>", "0..1", 1.01); assert_eq!(out.code, 1); assert!(out.stderr.contains("B4"), "{}", out.stderr); }
#[test] fn invalid_rule_points_to_schema() { let out = run("int32", "regex:[", 1); assert_eq!(out.code, 1); assert!(out.stderr.contains("[Items.schema.xlsx]Items!E3"), "{}", out.stderr); }
#[test] fn default_rule_points_to_schema() { let out = run("int32=101", "0..100", ()); assert_eq!(out.code, 1); assert!(out.stderr.contains("[Items.schema.xlsx]Items!E3"), "{}", out.stderr); }
#[test] fn english_message() {
    let tmp = Tmp::new(); fixture(&tmp, "int32", "0..100", 101);
    let out = drtable_lang("en", ["check", "--input", &s(&tmp.join("Items.xlsx"))]);
    assert_eq!(out.code, 1); assert!(out.stderr.contains("does not satisfy rule"), "{}", out.stderr);
}

#[test] fn array_checks_every_element() {
    let out = run("int32[]", "0..10", "1,11");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("[Items.xlsx]Items!B4"), "{}", out.stderr);
}

#[test] fn required_array_rejects_empty() {
    let out = run("int32[]", "required", ());
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("[Items.xlsx]Items!B4"), "{}", out.stderr);
}

#[test] fn rule_free_schema_keeps_output_bytes() {
    let tmp = Tmp::new();
    let data = tmp.join("Items.xlsx");
    fixture(&tmp, "int32", "", 5);
    let first = build(&data, &tmp.join("first"), &[]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    Book::new().with("Items", vec![row!["Field", "Type", "Scope", "Comment"], row!["Id", "ID<int32>", "all"], row!["Amount", "int32", "all"]]).save_plain(&tmp.join("Items.schema.xlsx"));
    let second = build(&data, &tmp.join("second"), &[]);
    assert_eq!(second.code, 0, "{}", second.stderr);
    assert_eq!(files(&tmp.join("first")), files(&tmp.join("second")));
}
