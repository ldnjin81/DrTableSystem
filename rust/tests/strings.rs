//! String tables: `<Name>String.string.xlsx` language lists, the strings folder, per-language
//! output and text references. Data sheets are named after the table (`UIString`).

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::{Path, PathBuf};

/// (language code, base language, scope; empty = default client)
type Language<'a> = (&'a str, bool, &'a str);

const UI_LANGUAGES: [Language; 3] = [("ko", true, ""), ("en", false, ""), ("zh-Hans", false, "")];

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

    /// Schema/<name>.string.xlsx (name ends with String): one language per row (Language, Base, Scope).
    fn string_schema(&self, name: &str, languages: &[Language]) {
        let mut rows = vec![row!["Language", "Base", "Scope", "Comment"]];
        for (code, base, scope) in languages {
            rows.push(vec![V::from(*code), if *base { V::from("✓") } else { V::E }, if scope.is_empty() { V::E } else { V::from(*scope) }]);
        }
        Book::new().with(name, rows).save_plain(&self.table.join("Schema").join(format!("{name}.string.xlsx")));
    }

    fn data(&self, relative: &str, sheets: Vec<(&str, Vec<Row>)>) {
        let mut book = Book::new();
        for (name, rows) in sheets {
            book.add(name, rows);
        }
        book.save_plain(&self.table.join(relative));
    }

    fn ui_strings(&self, data: Vec<Row>) {
        self.string_schema("UIString", &UI_LANGUAGES);
        self.data("Strings/UIString.xlsx", vec![("UIString", sheet(row!["Id", "ko", "en", "zh-Hans"], data))]);
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
    let ko = project.json("client/Strings/ko/UIString.json");
    assert_eq!(ko["table"], "UIString");
    assert_eq!(ko["language"], "ko");
    assert_eq!(ko["base_language"], "ko");
    assert_eq!(ko["keys"], json!(["Btn_OK", "Msg_Gold", "Title_Main"]), "keys sorted like primary keys");
    assert_eq!(ko["values"], json!(["확인", "{0} 골드", "메인"]));
    assert_eq!(project.json("client/Strings/en/UIString.json")["values"], json!(["OK", "{0} gold", "Main"]));
    // The empty zh-Hans cell takes the base text.
    let zh = project.json("client/Strings/zh-Hans/UIString.json");
    assert_eq!(zh["values"], json!(["확인", "{0} 金币", "主界面"]));
    assert!(result.stderr.contains("[UIString.string.xlsx]UIString: zh-Hans 번역 1칸이 비어 기준 언어(ko) 값으로 채웠습니다"), "{}", result.stderr);
    assert!(result.stderr.contains("[Strings/UIString.xlsx]UIString!D5"), "{}", result.stderr);
    // The structure hash is shared; each language has its own content hash.
    assert_eq!(ko["schema_hash"], zh["schema_hash"]);
    assert_ne!(ko["content_hash"], zh["content_hash"]);

    let manifest = project.json("client/manifest.json");
    assert!(manifest["tables"].as_array().unwrap().iter().all(|t| t["name"] != "UIString"));
    let entry = &manifest["string_tables"][0];
    assert_eq!(entry["name"], "UIString");
    assert_eq!(entry["schema"], "UIString.string.xlsx");
    assert_eq!(entry["base_language"], "ko");
    assert_eq!(entry["languages"], json!(["ko", "en", "zh-Hans"]));
    assert_eq!(entry["content_hashes"]["en"], project.json("client/Strings/en/UIString.json")["content_hash"]);
    assert_eq!(entry["sources"], json!([{"file": "Strings/UIString.xlsx", "sheet": "UIString", "rows": 3}]));
    // Languages default to client scope: nothing for the server.
    assert!(!project.out.join("server/Strings").exists());
    assert!(project.json("server/manifest.json").get("string_tables").is_none());
    // No row struct or asset class for string tables.
    let cpp = names(&project.out.join("cpp"));
    assert!(!cpp.iter().any(|name| name.contains("UIString")), "{cpp:?}");
}

#[test]
fn a_string_table_and_a_table_may_share_a_name() {
    // UIString.string.xlsx defines UIString, so a regular table UI is unaffected.
    let project = Project::new();
    project.ui_strings(sample_rows());
    project.schema("UI", &[("Id", "ID<int32>", "all")]);
    project.data("UI.xlsx", vec![("UI", sheet(row!["Id"], vec![row![1]]))]);
    let result = project.build(&[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(project.out.join("client/UI.json").exists());
    assert!(project.out.join("client/Strings/ko/UIString.json").exists());
}

#[test]
fn server_scope_languages_go_to_the_server_output() {
    let project = Project::new();
    project.string_schema("MailString", &[("ko", true, "all"), ("en", false, "")]);
    project.data("Strings/MailString.xlsx", vec![("MailString", sheet(row!["Id", "ko", "en"], vec![row!["Welcome", "환영", "Welcome"]]))]);
    assert_eq!(project.build(&[]).code, 0);
    assert_eq!(project.json("server/manifest.json")["string_tables"][0]["languages"], json!(["ko"]));
    assert!(project.out.join("server/Strings/ko/MailString.json").exists());
    assert!(!project.out.join("server/Strings/en").exists());
}

#[test]
fn references_to_string_tables_return_text() {
    let project = Project::new();
    project.ui_strings(sample_rows());
    project.schema("Items", &[("Id", "ID<int32>", "all"), ("Name", "Ref<UIString>", "all"), ("Tips[0]", "Ref<UIString>", "client")]);
    project.data("Items.xlsx", vec![("Items", sheet(row!["Id", "Name", "Tips[0]"], vec![row![1, "Btn_OK", "Title_Main"]]))]);
    let result = project.build(&["--ue-plugin"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let header = project.read("cpp/DrItemsRow.h");
    assert!(header.contains("FText GetName() const;"), "{header}");
    assert!(header.contains("FText GetTips(int32 Index) const;"));
    assert!(!header.contains("struct FDrUIStringRow;"));
    let source = project.read("cpp/DrItemsRow.cpp");
    assert!(!source.contains("DrUIStringRow.h"));
    assert!(source.contains("    if (Name.IsNone())\n    {\n        return FText::GetEmpty();\n    }"), "{source}");
    assert!(source.contains("    return DrTableRuntime::GetText(FName(TEXT(\"UIString\")), Name);"));
    assert!(source.contains("    return DrTableRuntime::GetText(FName(TEXT(\"UIString\")), Tips[Index]);"));
    assert!(!project.read("cpp/DrTableRegistration.h").contains("UIString"));
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
    project.schema("Items", &[("Id", "ID<int32>", "all"), ("Name", "Ref<UIString>", "all")]);
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
    assert!(!project.out.join("cpp/DrUIStringKeys.h").exists());
    assert_eq!(project.build(&["--string-keys"]).code, 0);
    let header = project.read("cpp/DrUIStringKeys.h");
    assert!(header.contains("namespace DrUIStringKeys"));
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
    project.string_schema("UIString", &UI_LANGUAGES);
    project.schema("Items", &[("Id", "ID<int32>", "all")]);
    project.data("UIString.xlsx", vec![("UIString", sheet(row!["Id", "ko", "en", "zh-Hans"], vec![row!["A", "가", (), ()]]))]);
    project.data("Strings/Items.xlsx", vec![("Items", sheet(row!["Id"], vec![row![1]]))]);
    project.data("Strings/Other.xlsx", vec![("MenuString", sheet(row!["Id", "ko"], vec![row!["A", "가"]]))]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[UIString.xlsx]UIString!A1: 스트링테이블 'UIString'의 데이터는 스트링 폴더("), "{}", result.stderr);
    assert!(result.stderr.contains("[Strings/Items.xlsx]Items!A1: 일반 테이블 'Items'의 데이터는 스트링 폴더("), "{}", result.stderr);
    assert!(result.stderr.contains("[Strings/Other.xlsx]MenuString!A1: 스트링테이블 스키마가 없습니다. 언어 목록을 'MenuString.string.xlsx'에"), "{}", result.stderr);
}

#[test]
fn strings_folder_option_and_default_without_a_schema_folder() {
    // Elsewhere with --strings.
    let project = Project::new();
    project.string_schema("UIString", &UI_LANGUAGES);
    let elsewhere = project.table.parent().unwrap().join("Localization");
    Book::new()
        .with("UIString", sheet(row!["Id", "ko", "en", "zh-Hans"], vec![row!["A", "가", "A", "甲"]]))
        .save_plain(&elsewhere.join("UIString.xlsx"));
    let result = project.build(&["--strings", &s(&elsewhere)]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(project.json("client/manifest.json")["string_tables"][0]["sources"][0]["file"], "Localization/UIString.xlsx");

    // Without --schema the schemas sit in the input folder and the strings in <input>/Strings.
    let tmp = Tmp::new();
    Book::new()
        .with("UIString", vec![row!["Language", "Base", "Scope"], row!["ko", true], row!["en"]])
        .save_plain(&tmp.join("UIString.string.xlsx"));
    Book::new().with("UIString", sheet(row!["Id", "ko", "en"], vec![row!["A", "가", ()]])).save_plain(&tmp.join("Strings/UIString.xlsx"));
    let result = build(&tmp, &tmp.join("out"), &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(tmp.join("out/client/Strings/en/UIString.json").exists());
}

#[test]
fn language_list_rules() {
    let cases: [(&[Language], &str, &str); 4] = [
        (&[("ko", false, ""), ("en", false, "")], "B2", "기준 언어를 하나 표시하세요"),
        (&[("ko", true, ""), ("en", true, "")], "B3", "기준 언어는 하나만"),
        (&[("ko", true, ""), ("en", false, "web")], "C3", "범위는 all, client, server"),
        (&[("ko", true, ""), ("ko", false, "")], "A3", "중복"),
    ];
    for (languages, cell, message) in cases {
        let project = Project::new();
        project.string_schema("UIString", languages);
        project.data("Strings/UIString.xlsx", vec![("UIString", sheet(row!["Id", "ko", "en"], vec![]))]);
        let result = project.build(&[]);
        assert_eq!(result.code, 1, "{message}");
        assert!(result.stderr.contains(&format!("[UIString.string.xlsx]UIString!{cell}")), "{message}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{message}: {}", result.stderr);
    }
}

#[test]
fn language_columns_belong_in_string_schemas() {
    let project = Project::new();
    project.schema("Items", &[("Id", "ID<int32>", "all"), ("ko", "lang", "client")]);
    project.data("Items.xlsx", vec![("Items", sheet(row!["Id", "ko"], vec![row![1, "가"]]))]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Items.schema.xlsx]Items!B3: 언어 열은 스트링테이블 스키마(<이름>.string.xlsx)에"), "{}", result.stderr);
}

#[test]
fn base_language_cells_are_required() {
    let project = Project::new();
    project.ui_strings(vec![row!["Btn_OK", (), "OK", ()]]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Strings/UIString.xlsx]UIString!B4: 기준 언어(ko) 칸이 비어 있습니다"), "{}", result.stderr);
}

#[test]
fn format_arguments_must_match_the_base_text() {
    let project = Project::new();
    project.ui_strings(vec![row!["Msg", "{Name}님 {0} 골드", "{0} gold", "{Name} {0} 金币"]]);
    let result = project.build(&[]);
    assert_eq!(result.code, 0);
    assert!(result.stderr.contains("[Strings/UIString.xlsx]UIString!C4: en 번역의 서식 인자가 기준 언어(ko)와 다릅니다: {0} / {0} {Name}"), "{}", result.stderr);
    assert!(!result.stderr.contains("UIString!D4"));
}

#[test]
fn numbers_in_language_cells_are_text() {
    let project = Project::new();
    project.ui_strings(vec![row!["Count", 100, 2.5, true]]);
    assert_eq!(project.build(&[]).code, 0);
    assert_eq!(project.json("client/Strings/ko/UIString.json")["values"], json!(["100"]));
    assert_eq!(project.json("client/Strings/en/UIString.json")["values"], json!(["2.5"]));
    assert_eq!(project.json("client/Strings/zh-Hans/UIString.json")["values"], json!(["TRUE"]));
}

#[test]
fn string_keys_differing_only_by_case_are_rejected() {
    let project = Project::new();
    project.ui_strings(vec![row!["Btn_OK", "확인", (), ()], row!["btn_ok", "확인2", (), ()]]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("대소문자만 다릅니다"), "{}", result.stderr);
}

#[test]
fn new_creates_a_string_data_workbook() {
    let project = Project::new();
    project.string_schema("UIString", &UI_LANGUAGES);
    let out = project.table.join("Strings/UIString.xlsx");
    let result = drtable(["new", "--table", "UIString", "--out", &s(&out), "--schema", &s(&project.table.join("Schema"))]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    use calamine::{Reader, Xlsx, open_workbook};
    let mut workbook: Xlsx<_> = open_workbook(&out).unwrap();
    assert_eq!(workbook.sheet_names(), ["UIString"]);
    let values = workbook.worksheet_range("UIString").unwrap();
    let names: Vec<String> = (0..4).map(|c| values.get_value((0, c)).unwrap().to_string()).collect();
    assert_eq!(names, ["Id", "ko", "en", "zh-Hans"]);
    let formulas = workbook.worksheet_formula("UIString").unwrap();
    let base_view = formulas.get_value((1, 1)).cloned().unwrap_or_default();
    assert!(base_view.contains("SUBSTITUTE(B$1,\"-\",\"_\")"), "{base_view}");
    // The new workbook builds (an empty string table).
    assert_eq!(project.build(&[]).code, 0);
}

#[test]
fn string_table_names_end_with_string() {
    let project = Project::new();
    project.string_schema("Mail", &[("ko", true, "")]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Mail.string.xlsx]Mail!A1: 스트링테이블 이름은 String으로 끝나야 합니다 (예: MailString.string.xlsx"), "{}", result.stderr);
}

#[test]
fn data_sheets_are_named_after_the_table() {
    let project = Project::new();
    project.string_schema("UIString", &UI_LANGUAGES);
    project.data("Strings/UI.xlsx", vec![("UI", sheet(row!["Id", "ko", "en", "zh-Hans"], vec![row!["A", "가", (), ()]]))]);
    let result = project.build(&[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Strings/UI.xlsx]UI!A1: 시트 이름은 테이블 이름 그대로 'UIString'으로 쓰세요"), "{}", result.stderr);
}
