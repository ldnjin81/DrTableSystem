//! 번역 작업 명령의 입출력과 원본 보존 검사.

#[macro_use]
mod common;

use common::*;
use std::fs;
use drtable::reader::read_workbook;

struct Project {
    _tmp: Tmp,
    input: std::path::PathBuf,
}

impl Project {
    fn new() -> Self {
        let tmp = Tmp::new();
        let input = tmp.join("Table");
        let schema = input.join("Schema/UIString.string.xlsx");
        Book::new().with("UIString", vec![
            row!["Language", "Base", "Scope", "Comment"],
            row!["ko", "✓", (), ()], row!["en", (), (), ()],
        ]).save_plain(&schema);
        Book::new().with("UIString", vec![
            row!["Id", "ko", "en", "#memo"], vec![], vec![],
            row!["Hello", "안녕", "Hello", "인사"],
            row!["Bye", "안녕히", (), ()],
            row!["Same", "같음", "같음", ()],
        ]).save_plain(&input.join("Strings/UIString.xlsx"));
        Project { _tmp: tmp, input }
    }

    fn run(&self, operation: &str, extra: &[&str]) -> Output {
        let mut args = vec!["l10n".to_string(), operation.to_string(), s(&self.input)];
        args.extend(extra.iter().map(|x| x.to_string()));
        drtable(args)
    }

    fn received(&self, translation: &str, base: &str, key: &str) -> std::path::PathBuf {
        let path = self._tmp.join("received.xlsx");
        Book::new().with("Translations", vec![
            row!["Key", "Base", "Translation", "Memo", "Source", "Sheet", "Table", "Language"],
            row![key, base, translation, (), "Strings/UIString.xlsx", "UIString", "UIString", "en"],
        ]).save_plain(&path);
        path
    }

    fn import(&self, received: &std::path::Path, out: &std::path::Path) -> Output {
        drtable(["l10n", "import", &s(received), "--input", &s(&self.input), "--out", &s(out)])
    }
}

#[test]
fn status_counts_raw_empty_and_same_cells() {
    let p = Project::new();
    let result = p.run("status", &["--format", "json"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let rows: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
    let en = rows.as_array().unwrap().iter().find(|r| r["language"] == "en").unwrap();
    assert_eq!(en["total"], 3);
    assert_eq!(en["filled"], 2);
    assert_eq!(en["empty"], 1);
    assert_eq!(en["same_as_base"], 1);
}

#[test]
fn status_base_language_is_filled_and_same() {
    let p = Project::new();
    let result = p.run("status", &["--format", "json"]);
    let rows: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
    let ko = rows.as_array().unwrap().iter().find(|r| r["language"] == "ko").unwrap();
    assert_eq!(ko["filled"], 3);
    assert_eq!(ko["same_as_base"], 3);
}

#[test]
fn status_markdown_has_counts() {
    let p = Project::new();
    let result = p.run("status", &["--format", "markdown"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stdout.contains("| UIString | en | 3 | 2 | 1 | 1 |"));
}

#[test]
fn status_does_not_change_input() {
    let p = Project::new();
    let path = p.input.join("Strings/UIString.xlsx");
    let before = fs::read(&path).unwrap();
    assert_eq!(p.run("status", &[]).code, 0);
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn unused_lists_unreferenced_keys_in_order() {
    let p = Project::new();
    let result = p.run("unused", &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "UIString.Bye\nUIString.Hello\nUIString.Same\n");
}

#[test]
fn unused_respects_string_references() {
    let p = Project::new();
    write_schema(&p.input.join("Schema/Items.schema.xlsx"), "Items", &fields(&[("Id", "ID<int32>", "all"), ("Title", "Ref<UIString>", "client")]));
    Book::new().with("Items", vec![row!["Id", "Title"], vec![], vec![], row![1, "Hello"]]).save_plain(&p.input.join("Items.xlsx"));
    let result = p.run("unused", &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(!result.stdout.contains("UIString.Hello"));
    assert!(result.stdout.contains("UIString.Bye"));
}

#[test]
fn unused_respects_text_fields() {
    let p = Project::new();
    write_schema(&p.input.join("Schema/Items.schema.xlsx"), "Items", &fields(&[("Id", "ID<int32>", "all"), ("Title", "text", "client")]));
    Book::new().with("Items", vec![row!["Id", "Title"], vec![], vec![], row![1, "Bye"]]).save_plain(&p.input.join("Items.xlsx"));
    let result = p.run("unused", &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(!result.stdout.contains("UIString.Bye"));
}

#[test]
fn unused_does_not_change_input() {
    let p = Project::new();
    let path = p.input.join("Strings/UIString.xlsx");
    let before = fs::read(&path).unwrap();
    assert_eq!(p.run("unused", &[]).code, 0);
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn status_text_uses_message_language() {
    let p = Project::new();
    let args = ["l10n", "status", &s(&p.input)];
    let ko = drtable_lang("ko", args);
    let en = drtable_lang("en", args);
    assert_eq!(ko.code, 0, "{}", ko.stderr);
    assert_eq!(en.code, 0, "{}", en.stderr);
    assert!(ko.stdout.contains("비어 있음=1"));
    assert!(en.stdout.contains("empty=1"));
}

#[test]
fn status_rejects_unknown_format() {
    let p = Project::new();
    assert!(p.run("status", &["--format", "csv"]).is_usage_error());
}

#[test]
fn export_selects_empty_and_base_equal_rows() {
    let p = Project::new();
    let path = p._tmp.join("export.xlsx");
    let result = p.run("export", &["--lang", "en", "--out", &s(&path)]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let sheets = read_workbook(&path, |_| true).unwrap();
    let grid = sheets[0].grid.as_ref().unwrap();
    assert_eq!(grid.max_row(), 3);
    assert_eq!(grid.value(2, 1).text_or_empty(), "Bye");
    assert_eq!(grid.value(3, 1).text_or_empty(), "Same");
    assert_eq!(grid.value(2, 5).text_or_empty(), "Strings/UIString.xlsx");
}

#[test]
fn export_refuses_existing_file() {
    let p = Project::new();
    let path = p._tmp.join("export.xlsx");
    fs::write(&path, b"sentinel").unwrap();
    assert_eq!(p.run("export", &["--lang", "en", "--out", &s(&path)]).code, 1);
    assert_eq!(fs::read(&path).unwrap(), b"sentinel");
}

#[test]
fn import_writes_new_string_file_and_keeps_source() {
    let p = Project::new();
    let received = p.received("Goodbye", "안녕히", "Bye");
    let output = p._tmp.join("new");
    let source = p.input.join("Strings/UIString.xlsx");
    let before = fs::read(&source).unwrap();
    let result = p.import(&received, &output);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let sheets = read_workbook(&output.join("Strings/UIString.xlsx"), |_| true).unwrap();
    let grid = sheets[0].grid.as_ref().unwrap();
    assert_eq!(grid.value(5, 3).text_or_empty(), "Goodbye");
    assert_eq!(grid.value(4, 3).text_or_empty(), "Hello");
    assert_eq!(grid.value(4, 4).text_or_empty(), "인사");
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn import_refuses_existing_output() {
    let p = Project::new();
    let received = p.received("Goodbye", "안녕히", "Bye");
    let output = p._tmp.join("new");
    let target = output.join("Strings/UIString.xlsx");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, b"sentinel").unwrap();
    assert_eq!(p.import(&received, &output).code, 1);
    assert_eq!(fs::read(target).unwrap(), b"sentinel");
}

#[test]
fn import_warns_for_missing_key() {
    let p = Project::new();
    let received = p.received("Missing", "없음", "Gone");
    let result = p.import(&received, &p._tmp.join("new"));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stderr.contains("원본에 없는 키"), "{}", result.stderr);
}

#[test]
fn import_warns_when_base_text_changed_and_applies_translation() {
    let p = Project::new();
    let received = p.received("Goodbye", "옛 문구", "Bye");
    let output = p._tmp.join("new");
    let result = p.import(&received, &output);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stderr.contains("기준 문구가 바뀜"), "{}", result.stderr);
    let sheets = read_workbook(&output.join("Strings/UIString.xlsx"), |_| true).unwrap();
    assert_eq!(sheets[0].grid.as_ref().unwrap().value(5, 3).text_or_empty(), "Goodbye");
}

#[test]
fn export_does_not_change_input() {
    let p = Project::new();
    let source = p.input.join("Strings/UIString.xlsx");
    let before = fs::read(&source).unwrap();
    let path = p._tmp.join("export.xlsx");
    assert_eq!(p.run("export", &["--lang", "en", "--out", &s(&path)]).code, 0);
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn import_skips_empty_translation() {
    let p = Project::new();
    let received = p.received("", "안녕히", "Bye");
    let output = p._tmp.join("new");
    let result = p.import(&received, &output);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stdout.contains("0"));
    assert!(!output.join("Strings/UIString.xlsx").exists());
}

#[test]
fn import_rejects_output_inside_input() {
    let p = Project::new();
    let received = p.received("Goodbye", "안녕히", "Bye");
    let output = p.input.join("translated");
    let result = p.import(&received, &output);
    assert_eq!(result.code, 1);
    assert!(!output.exists());
}

#[test]
fn export_is_deterministic() {
    let p = Project::new();
    let first = p._tmp.join("first.xlsx");
    let second = p._tmp.join("second.xlsx");
    assert_eq!(p.run("export", &["--lang", "en", "--out", &s(&first)]).code, 0);
    assert_eq!(p.run("export", &["--lang", "en", "--out", &s(&second)]).code, 0);
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}
