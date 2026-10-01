//! String tables: language columns, the strings folder, per-language output and text references.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::{Path, PathBuf};

type Field<'a> = (&'a str, &'a str, &'a str);

const UI_FIELDS: [Field; 4] =
    [("Id", "ID<name>", "all"), ("ko", "Base<lang>", "client"), ("en", "lang", "client"), ("zh_Hans", "lang", "client")];

fn view() -> [Row; 2] {
    [vec![], vec![]]
}

/// A data sheet: header, the two view rows, then data.
fn sheet(header: Row, data: Vec<Row>) -> Vec<Row> {
    let mut rows = vec![header];
    rows.extend(view());
    rows.extend(data);
    rows
}

/// Table/Schema (schemas), Table/Strings (string data), Table (other data).
struct Project {
    _tmp: Tmp,
    table: PathBuf,
    out: PathBuf,
}

impl Project {
    fn new() -> Self {
        let tmp = Tmp::new();
        let table = tmp.join("Table");
        let out = tmp.join("out");
        Project { _tmp: tmp, table, out }
    }

    fn schema(&self, name: &str, items: &[(&str, &str, &str)]) {
        write_schema(&self.table.join("Schema").join(format!("{name}.schema.xlsx")), name, &fields(items));
    }

    fn data(&self, relative: &str, sheets: Vec<(&str, Vec<Row>)>) {
        let mut book = Book::new();
        for (name, rows) in sheets {
            book.add(name, rows);
        }
        book.save_plain(&self.table.join(relative));
    }

    fn ui_strings(&self, data: Vec<Row>) {
        self.schema("UIStrings", &UI_FIELDS);
        self.data("Strings/UI.xlsx", vec![("UIStrings", sheet(row!["Id", "ko", "en", "zh_Hans"], data))]);
    }

    fn build(&self, extra: &[&str]) -> Output {
        let schema = s(&self.table.join("Schema"));
        let args: Vec<&str> = ["--schema", schema.as_str()].into_iter().chain(extra.iter().copied()).collect();
        build(&self.table, &self.out, &args)
    }

    fn json(&self, path: &str) -> serde_json::Value {
        json(&self.out.join(path))
    }

    fn read(&self, path: &str) -> String {
        read(&self.out.join(path))
    }
}

fn sample_rows() -> Vec<Row> {
    vec![
        row!["Title_Main", "메인", "Main", "主界面"],
        row!["Btn_OK", "확인", "OK", ()],
        row!["Msg_Gold", "{0} 골드", "{0} gold", "{0} 金币"],
    ]
}

#[test]
fn string_table_outputs_one_file_per_language() {
    let project = Project::new();
    project.ui_strings(sample_rows());
    let result = project.build(&[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let ko = project.json("client/Strings/ko/UIStrings.json");
    assert_eq!(ko["table"], "UIStrings");
    assert_eq!(ko["language"], "ko");
    assert_eq!(ko["base_language"], "ko");
    assert_eq!(ko["keys"], json!(["Btn_OK", "Msg_Gold", "Title_Main"]), "keys sorted like primary keys");
    assert_eq!(ko["values"], json!(["확인", "{0} 골드", "메인"]));
    assert_eq!(project.json("client/Strings/en/UIStrings.json")["values"], json!(["OK", "{0} gold", "Main"]));
    // zh_Hans is the culture zh-Hans; its empty cell takes the base text.
    let zh = project.json("client/Strings/zh-Hans/UIStrings.json");
    assert_eq!(zh["values"], json!(["확인", "{0} 金币", "主界面"]));
    assert!(result.stderr.contains("zh_Hans 번역 1칸이 비어 기준 언어(ko) 값으로 채웠습니다"), "{}", result.stderr);
    assert!(result.stderr.contains("[Strings/UI.xlsx]UIStrings!D5"), "{}", result.stderr);
    // The structure hash is shared; each language has its own content hash.
    assert_eq!(ko["schema_hash"], zh["schema_hash"]);
    assert_ne!(ko["content_hash"], zh["content_hash"]);

    let manifest = project.json("client/manifest.json");
    assert!(manifest["tables"].as_array().unwrap().iter().all(|t| t["name"] != "UIStrings"));
    let entry = &manifest["string_tables"][0];
    assert_eq!(entry["name"], "UIStrings");
    assert_eq!(entry["base_language"], "ko");
    assert_eq!(entry["languages"], json!(["ko", "en", "zh-Hans"]));
    assert_eq!(entry["content_hashes"]["en"], project.json("client/Strings/en/UIStrings.json")["content_hash"]);
    assert_eq!(entry["sources"], json!([{"file": "Strings/UI.xlsx", "sheet": "UIStrings", "rows": 3}]));
    // Client-only languages: nothing for the server.
    assert!(!project.out.join("server/Strings").exists());
    assert!(project.json("server/manifest.json").get("string_tables").is_none());
    // No row struct or asset class for string tables.
    let cpp = names(&project.out.join("cpp"));
    assert!(!cpp.iter().any(|name| name.contains("UIStrings")), "{cpp:?}");
}

#[test]
fn server_scope_languages_go_to_the_server_output() {
    let project = Project::new();
    project.schema("Mail", &[("Id", "ID<name>", "all"), ("ko", "Base<lang>", "all"), ("en", "lang", "client")]);
    project.data("Strings/Mail.xlsx", vec![("Mail", sheet(row!["Id", "ko", "en"], vec![row!["Welcome", "환영", "Welcome"]]))]);
    assert_eq!(project.build(&[]).code, 0);
    assert_eq!(project.json("server/manifest.json")["string_tables"][0]["languages"], json!(["ko"]));
    assert!(project.out.join("server/Strings/ko/Mail.json").exists());
    assert!(!project.out.join("server/Strings/en").exists());
}

#[test]
fn references_to_string_tables_return_text() {
    let project = Project::new();
    project.ui_strings(sample_rows());
    project.schema("Items", &[("Id", "ID<int32>", "all"), ("Name", "Ref<UIStrings>", "all"), ("Tips[0]", "Ref<UIStrings>", "client")]);
    project.data("Items.xlsx", vec![("Items", sheet(row!["Id", "Name", "Tips[0]"], vec![row![1, "Btn_OK", "Title_Main"]]))]);
    let result = project.build(&["--ue-plugin"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let header = project.read("cpp/DrItemsRow.h");
    assert!(header.contains("FText GetName() const;"), "{header}");
    assert!(header.contains("FText GetTips(int32 Index) const;"));
    assert!(!header.contains("struct FDrUIStringsRow;"));
    let source = project.read("cpp/DrItemsRow.cpp");
    assert!(!source.contains("DrUIStringsRow.h"));
    assert!(source.contains("    if (Name.IsNone())\n    {\n        return FText::GetEmpty();\n    }"), "{source}");
    assert!(source.contains("    return DrTableRuntime::GetText(FName(TEXT(\"UIStrings\")), Name);"));
    assert!(source.contains("    return DrTableRuntime::GetText(FName(TEXT(\"UIStrings\")), Tips[Index]);"));
    let registration = project.read("cpp/DrTableRegistration.h");
    assert!(!registration.contains("UIStrings"));
    // The reference checker knows the string keys.
    let check = |dir: &Path| drtable(["check", "--client", &s(dir)]);
    assert_eq!(check(&project.out.join("client")).code, 0);
    let path = project.out.join("client/Items.json");
    let mut items = json(&path);
    items["rows"][0]["Name"] = json!("Missing_Key");
    write_json(&path, &items);
    let result = check(&project.out.join("client"));
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("Missing_Key"), "{}", result.stderr);
}

#[test]
fn data_edits_never_change_generated_code() {
    let project = Project::new();
    project.schema("Items", &[("Id", "ID<int32>", "all"), ("Name", "Ref<UIStrings>", "all")]);
    project.data("Items.xlsx", vec![("Items", sheet(row!["Id", "Name"], vec![row![1, "Btn_OK"]]))]);
    let mut outputs = Vec::new();
    for data in [sample_rows(), vec![row!["Btn_OK", "좋아", "Fine", ()], row!["New_Key", "새 키", (), ()]]] {
        project.ui_strings(data);
        assert_eq!(project.build(&["--ue-plugin"]).code, 0);
        outputs.push(files(&project.out.join("cpp")));
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn string_keys_header_is_opt_in() {
    let project = Project::new();
    project.ui_strings(vec![row!["Btn_OK", "확인", (), ()], row!["Menu.Open", "열기", (), ()], row!["1st", "첫째", (), ()]]);
    assert_eq!(project.build(&[]).code, 0);
    assert!(!project.out.join("cpp/DrUIStringsKeys.h").exists());
    assert_eq!(project.build(&["--string-keys"]).code, 0);
    let header = project.read("cpp/DrUIStringsKeys.h");
    assert!(header.contains("namespace DrUIStringsKeys"));
    assert!(header.contains("    inline constexpr TCHAR Btn_OK[] = TEXT(\"Btn_OK\");"));
    assert!(header.contains("    inline constexpr TCHAR Menu_Open[] = TEXT(\"Menu.Open\");"));
    assert!(header.contains("    inline constexpr TCHAR _1st[] = TEXT(\"1st\");"));
    // Keys that would share a constant name are errors.
    project.ui_strings(vec![row!["Menu.Open", "열기", (), ()], row!["Menu_Open", "열기2", (), ()]]);
    let result = project.build(&["--string-keys"]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("같은 상수 이름 Menu_Open"), "{}", result.stderr);
}

#[test]
fn string_data_belongs_in_the_strings_folder() {
    let project = Project::new();
    project.schema("UIStrings", &UI_FIELDS);
    project.schema("Items", &[("Id", "ID<int32>", "all")]);
    project.data("UI.xlsx", vec![("UIStrings", sheet(row!["Id", "ko", "en", "zh_Hans"], vec![row!["A", "가", (), ()]]))]);
    project.data("Strings/Items.xlsx", vec![("Items", sheet(row!["Id"], vec![row![1]]))]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[UI.xlsx]UIStrings!A1: 스트링테이블 'UIStrings'의 데이터는 스트링 폴더("), "{}", result.stderr);
    assert!(result.stderr.contains("[Strings/Items.xlsx]Items!A1: 일반 테이블 'Items'의 데이터는 스트링 폴더("), "{}", result.stderr);
}

#[test]
fn strings_folder_option_and_default_without_a_schema_folder() {
    // Elsewhere with --strings.
    let project = Project::new();
    project.schema("UIStrings", &UI_FIELDS);
    let elsewhere = project.table.parent().unwrap().join("Localization");
    Book::new()
        .with("UIStrings", sheet(row!["Id", "ko", "en", "zh_Hans"], vec![row!["A", "가", "A", "甲"]]))
        .save_plain(&elsewhere.join("UI.xlsx"));
    let result = project.build(&["--strings", &s(&elsewhere)]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(project.json("client/manifest.json")["string_tables"][0]["sources"][0]["file"], "Localization/UI.xlsx");

    // Without --schema the schemas sit in the input folder and the strings in <input>/Strings.
    let tmp = Tmp::new();
    write_schema(&tmp.join("UIStrings.schema.xlsx"), "UIStrings", &fields(&UI_FIELDS));
    Book::new()
        .with("UIStrings", sheet(row!["Id", "ko", "en", "zh_Hans"], vec![row!["A", "가", (), ()]]))
        .save_plain(&tmp.join("Strings/UI.xlsx"));
    let result = build(&tmp, &tmp.join("out"), &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(tmp.join("out/client/Strings/en/UIStrings.json").exists());
}

#[test]
fn string_schema_rules() {
    let cases: [(&[Field], &str, &str); 6] = [
        (&[("Id", "ID<name>", "all"), ("ko", "lang", "client"), ("en", "lang", "client")], "B3", "기준 언어 열(Base<lang>)이 하나"),
        (&[("Id", "ID<name>", "all"), ("ko", "Base<lang>", "client"), ("en", "Base<lang>", "client")], "B4", "기준 언어(Base<lang>)는 하나만"),
        (&[("Id", "ID<name>", "all"), ("ko", "Base<lang>", "client"), ("Count", "int32", "client")], "B4", "기본키와 언어(lang) 열만"),
        (&[("Id", "ID<int32>", "all"), ("ko", "Base<lang>", "client")], "B2", "기본키는 ID<name>"),
        (&[("Id", "ID<name>", "all"), ("ko", "Base<lang>", "client"), ("en", "lang=x", "client")], "B4", "배열이나 기본값"),
        (&[("Id", "ID<name>", "all"), ("ko", "Base<lang>", "client"), ("en", "SubKey<lang>", "client")], "B4", "lang 자료형은 기본키나 서브키"),
    ];
    for (items, cell, message) in cases {
        let project = Project::new();
        project.schema("UIStrings", items);
        let header: Row = items.iter().map(|(name, _, _)| V::from(*name)).collect();
        project.data("Strings/UI.xlsx", vec![("UIStrings", sheet(header, vec![]))]);
        let result = project.build(&[]);
        assert_eq!(result.code, 1, "{message}");
        assert!(result.stderr.contains(&format!("[UIStrings.schema.xlsx]UIStrings!{cell}")), "{message}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{message}: {}", result.stderr);
    }
}

#[test]
fn base_language_cells_are_required() {
    let project = Project::new();
    project.ui_strings(vec![row!["Btn_OK", (), "OK", ()]]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Strings/UI.xlsx]UIStrings!B4: 기준 언어(ko) 칸이 비어 있습니다"), "{}", result.stderr);
}

#[test]
fn format_arguments_must_match_the_base_text() {
    let project = Project::new();
    project.ui_strings(vec![row!["Msg", "{Name}님 {0} 골드", "{0} gold", "{Name} {0} 金币"]]);
    let result = project.build(&[]);
    assert_eq!(result.code, 0);
    assert!(result.stderr.contains("[Strings/UI.xlsx]UIStrings!C4: en 번역의 서식 인자가 기준 언어(ko)와 다릅니다: {0} / {0} {Name}"), "{}", result.stderr);
    assert!(!result.stderr.contains("UIStrings!D4"));
}

#[test]
fn numbers_in_language_cells_are_text() {
    let project = Project::new();
    project.ui_strings(vec![row!["Count", 100, 2.5, true]]);
    assert_eq!(project.build(&[]).code, 0);
    assert_eq!(project.json("client/Strings/ko/UIStrings.json")["values"], json!(["100"]));
    assert_eq!(project.json("client/Strings/en/UIStrings.json")["values"], json!(["2.5"]));
    assert_eq!(project.json("client/Strings/zh-Hans/UIStrings.json")["values"], json!(["TRUE"]));
}

#[test]
fn string_keys_differing_only_by_case_are_rejected() {
    let project = Project::new();
    project.ui_strings(vec![row!["Btn_OK", "확인", (), ()], row!["btn_ok", "확인2", (), ()]]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("대소문자만 다릅니다"), "{}", result.stderr);
}
