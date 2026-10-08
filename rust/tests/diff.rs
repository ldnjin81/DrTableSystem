#[macro_use]
mod common;

use common::*;
use serde_json::Value;
use std::path::Path;

fn items(path: &Path, rows: Vec<Row>, kind: &str) {
    let mut sheet = vec![row!["Id", "Name", "Reward"], row!["ID<int32>", "string", kind], row!["all", "all", "all"]];
    sheet.extend(rows);
    Book::new().with("Items", sheet).save(path);
}

fn pair(old: Vec<Row>, new: Vec<Row>, old_kind: &str, new_kind: &str) -> Tmp {
    let tmp = Tmp::new();
    let old_file = tmp.join("old/input.xlsx");
    let new_file = tmp.join("new/input.xlsx");
    items(&old_file, old, old_kind);
    items(&new_file, new, new_kind);
    for side in ["old", "new"] {
        let result = build(&tmp.join(side).join("input.xlsx"), &tmp.join(side).join("out"), &[]);
        assert_eq!(result.code, 0, "{}", result.stderr);
    }
    tmp
}

fn diff(tmp: &Path, args: &[&str]) -> Output {
    let mut argv = vec!["diff".to_string(), s(&tmp.join("old/out/server")), s(&tmp.join("new/out/server"))];
    argv.extend(args.iter().map(|v| v.to_string()));
    drtable(argv)
}

fn changes(output: &Output) -> Vec<Value> {
    serde_json::from_str::<Value>(&output.stdout).unwrap()["changes"].as_array().unwrap().clone()
}

#[test]
fn identical_outputs_have_no_changes() {
    let tmp = pair(vec![row![1, "A", "1,2"]], vec![row![1, "A", "1,2"]], "int32[]", "int32[]");
    let output = diff(&tmp, &[]);
    assert_eq!(output.code, 0);
    assert!(output.stdout.ends_with("변경 없음\n"));
    assert!(output.stdout.starts_with("fixed 배율을 알 수 없습니다."));
}

#[test]
fn added_row_is_reported() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "A", "1"], row![2, "B", "2"]], "int32[]", "int32[]");
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["kind"], "row_added");
    assert_eq!(c[0]["key"], 2);
}

#[test]
fn removed_row_is_reported() {
    let tmp = pair(vec![row![1, "A", "1"], row![2, "B", "2"]], vec![row![1, "A", "1"]], "int32[]", "int32[]");
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["kind"], "row_removed");
    assert_eq!(c[0]["key"], 2);
}

#[test]
fn changed_field_has_both_values() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "B", "1"]], "int32[]", "int32[]");
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["field"], "Name");
    assert_eq!(c[0]["before"], "A");
    assert_eq!(c[0]["after"], "B");
}

#[test]
fn arrays_show_element_changes() {
    let tmp = pair(vec![row![1, "A", "1,2"]], vec![row![1, "A", "1,3,4"]], "int32[]", "int32[]");
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert_eq!(c.len(), 2);
    assert_eq!(c[0]["field"], "Reward");
    assert_eq!(c[0]["index"], 1);
    assert_eq!(c[0]["before"], 2);
    assert_eq!(c[0]["after"], 3);
    assert_eq!(c[1]["index"], 2);
    assert!(c[1].get("before").is_none());
}

#[test]
fn client_array_pool_is_decoded() {
    let tmp = pair(vec![row![1, "A", "1,2"]], vec![row![1, "A", "1,3"]], "int32[]", "int32[]");
    let output = drtable(["diff", &s(&tmp.join("old/out/client")), &s(&tmp.join("new/out/client")), "--format", "json"]);
    let c = changes(&output);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["index"], 1);
    assert_eq!(c[0]["before"], 2);
}

#[test]
fn schema_hash_change_is_reported() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "A", "1"]], "int32[]", "int64[]");
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert!(c.iter().any(|v| v["kind"] == "schema_changed"));
}

#[test]
fn markdown_escapes_cells() {
    let tmp = pair(vec![row![1, "A|B", "1"]], vec![row![1, "C|D", "1"]], "int32[]", "int32[]");
    let output = diff(&tmp, &["--format", "markdown"]);
    assert_eq!(output.code, 0);
    assert!(output.stdout.contains("| 필드 변경 | Items | 1 | Name | A\\|B | C\\|D |"));
}

#[test]
fn exit_code_is_optional() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "B", "1"]], "int32[]", "int32[]");
    assert_eq!(diff(&tmp, &[]).code, 0);
    assert_eq!(diff(&tmp, &["--exit-code"]).code, 1);
}

#[test]
fn empty_diff_with_exit_code_still_succeeds() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "A", "1"]], "int32[]", "int32[]");
    assert_eq!(diff(&tmp, &["--exit-code"]).code, 0);
}

#[test]
fn table_filter_excludes_other_tables() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "B", "1"]], "int32[]", "int32[]");
    assert!(diff(&tmp, &["--table", "Other"]).stdout.ends_with("변경 없음\n"));
}

#[test]
fn invalid_format_is_usage_error() {
    let output = drtable(["diff", "a", "b", "--format", "csv"]);
    assert_eq!(output.code, 2);
}

#[test]
fn missing_manifest_is_input_error() {
    let tmp = Tmp::new();
    let output = diff(&tmp, &[]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("manifest.json"));
}

fn table_pair(extra_old: bool, extra_new: bool) -> Tmp {
    let tmp = Tmp::new();
    for (side, extra) in [("old", extra_old), ("new", extra_new)] {
        let mut book = Book::new().with("Items", vec![row!["Id"], row!["ID<int32>"], row!["all"], row![1]]);
        if extra { book.add("Extras", vec![row!["Id"], row!["ID<int32>"], row!["all"], row![2]]); }
        book.save(&tmp.join(side).join("input.xlsx"));
        let result = build(&tmp.join(side).join("input.xlsx"), &tmp.join(side).join("out"), &[]);
        assert_eq!(result.code, 0, "{}", result.stderr);
    }
    tmp
}

#[test]
fn added_table_is_reported() {
    let tmp = table_pair(false, true);
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["kind"], "table_added");
    assert_eq!(c[0]["table"], "Extras");
}

#[test]
fn removed_table_is_reported() {
    let tmp = table_pair(true, false);
    let c = changes(&diff(&tmp, &["--format", "json"]));
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["kind"], "table_removed");
}

fn string_pair(old_text: &str, new_text: &str) -> Tmp {
    let tmp = Tmp::new();
    for (side, value) in [("old", old_text), ("new", new_text)] {
        let root = tmp.join(side).join("input");
        let schema = vec![row!["Language", "Base", "Scope"], row!["ko", "✓", "client"], row!["en", (), "client"]];
        Book::new().with("UIString", schema).save_plain(&root.join("Schema/UIString.string.xlsx"));
        let data = vec![row!["Id", "ko", "en"], row![(), (), ()], row![(), (), ()], row!["Greeting", "안녕", value]];
        Book::new().with("UIString", data).save_plain(&root.join("Strings/UIString.xlsx"));
        let result = build(&root, &tmp.join(side).join("out"), &["--schema", &s(&root.join("Schema"))]);
        assert_eq!(result.code, 0, "{}", result.stderr);
    }
    tmp
}

#[test]
fn string_translation_is_reported_by_language() {
    let tmp = string_pair("Hello", "Welcome");
    let output = drtable(["diff", &s(&tmp.join("old/out/client")), &s(&tmp.join("new/out/client")), "--format", "json"]);
    let c = changes(&output);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["kind"], "string_changed");
    assert_eq!(c[0]["field"], "en");
    assert_eq!(c[0]["key"], "Greeting");
    assert_eq!(c[0]["before"], "Hello");
    assert_eq!(c[0]["after"], "Welcome");
}

#[test]
fn english_report_is_translated() {
    let tmp = pair(vec![row![1, "A", "1"]], vec![row![1, "B", "1"]], "int32[]", "int32[]");
    let output = drtable_lang("en", ["diff", &s(&tmp.join("old/out/server")), &s(&tmp.join("new/out/server"))]);
    assert!(output.stdout.contains("Field changed"));
}

#[test]
fn fixed_values_are_decimals_with_schema() {
    let tmp = pair(vec![row![1, "A", 0.125]], vec![row![1, "A", 0.375]], "fixed<1000>", "fixed<1000>");
    let schema = s(&tmp.join("new"));
    let output = diff(&tmp, &["--format", "json", "--schema", &schema]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let c = changes(&output);
    assert_eq!(c[0]["before"], "0.125");
    assert_eq!(c[0]["after"], "0.375");
    assert!(output.stdout.find("\"note\"").is_none());
}

#[test]
fn fixed_array_elements_are_decimals_with_schema() {
    let tmp = pair(vec![row![1, "A", "0.25,0.5"]], vec![row![1, "A", "0.25,0.75"]], "fixed<100>[]", "fixed<100>[]");
    let schema = s(&tmp.join("new"));
    let output = diff(&tmp, &["--format", "json", "--schema", &schema]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let c = changes(&output);
    assert_eq!(c[0]["index"], 1);
    assert_eq!(c[0]["before"], "0.5");
    assert_eq!(c[0]["after"], "0.75");
}

#[test]
fn fixed_without_schema_keeps_stored_integer_and_has_note() {
    let tmp = pair(vec![row![1, "A", 0.125]], vec![row![1, "A", 0.375]], "fixed<1000>", "fixed<1000>");
    let output = diff(&tmp, &["--format", "json"]);
    let report: Value = serde_json::from_str(&output.stdout).unwrap();
    assert!(report["note"].as_str().unwrap().contains("--schema"));
    assert_eq!(report["changes"][0]["before"], 125);
    assert_eq!(report["changes"][0]["after"], 375);
}

#[test]
fn fixed_markdown_report_uses_decimals() {
    let tmp = pair(vec![row![1, "A", 0.125]], vec![row![1, "A", 0.375]], "fixed<1000>", "fixed<1000>");
    let schema = s(&tmp.join("new"));
    let output = diff(&tmp, &["--format", "markdown", "--schema", &schema]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("| 필드 변경 | Items | 1 | Reward | 0.125 | 0.375 |"));
}

#[test]
fn fixed64_decimals_keep_full_precision_and_sign() {
    let tmp = pair(vec![row![1, "A", "-0.000000000000000001"]], vec![row![1, "A", "0.000000000000000002"]], "fixed64<1000000000000000000>", "fixed64<1000000000000000000>");
    let schema = s(&tmp.join("new"));
    let output = diff(&tmp, &["--format", "json", "--schema", &schema]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let c = changes(&output);
    assert_eq!(c[0]["before"], "-0.000000000000000001");
    assert_eq!(c[0]["after"], "0.000000000000000002");
}
