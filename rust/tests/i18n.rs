//! Message language selection and English-only generated output.

#[macro_use]
mod common;

use common::*;
use std::path::Path;

fn has_hangul(text: &str) -> bool {
    text.chars().any(|c| ('가'..='힣').contains(&c))
}

fn save(path: &Path, rows: Vec<Row>) {
    Book::new().with("Items", rows).save(path);
}

fn duplicate_key_build(language: &str, flag: &str, tmp: &Path) -> Output {
    let source = tmp.join("in.xlsx");
    save(&source, vec![row!["Id"], row!["ID<int32>"], row!["all"], row![1], row![1]]);
    drtable_lang(language, [
        "--lang", flag, "build", "--input", &s(&source), "--out-cpp", &s(&tmp.join("c")),
        "--out-client", &s(&tmp.join("cl")), "--out-server", &s(&tmp.join("s")),
    ])
}

#[test]
fn lang_en_prints_english_errors() {
    let tmp = Tmp::new();
    let result = duplicate_key_build("ko", "en", &tmp);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("Items!A5: duplicate primary key '1'"), "{}", result.stderr);
    assert!(!has_hangul(&result.stderr));
}

#[test]
fn lang_ko_prints_korean_errors() {
    let tmp = Tmp::new();
    let result = duplicate_key_build("en", "ko", &tmp);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("기본키 값 '1'이 중복되었습니다"), "{}", result.stderr);
}

#[test]
fn generated_output_is_english() {
    let tmp = Tmp::new();
    let source = tmp.join("in.xlsx");
    save(&source, vec![
        row!["Id", "Name", "Next"], row!["ID<int32>", "string", "Ref<Items>"], row!["all", "all", "all"], row![1, "검", 0],
    ]);
    assert_eq!(build(&source, &tmp, &["--ue-plugin"]).code, 0);
    let graph = tmp.join("graph.md");
    assert_eq!(drtable(["graph", "--input", &s(&source), "--out", &s(&graph)]).code, 0);
    let mut outputs: Vec<_> = names(&tmp.join("cpp")).into_iter().map(|name| tmp.join("cpp").join(name)).collect();
    outputs.push(graph);
    for path in outputs {
        assert!(!has_hangul(&read(&path)), "{}", path.display());
    }
}

#[test]
fn folder_input_skips_excel_lock_files() {
    let tmp = Tmp::new();
    let folder = tmp.join("in");
    save(&folder.join("Data.xlsx"), vec![row!["Id"], row!["ID<int32>"], row!["all"], row![1]]);
    std::fs::write(folder.join("~$Data.xlsx"), b"lock").unwrap(); // what Excel leaves while the book is open
    assert_eq!(build(&folder, &tmp, &[]).code, 0);
}

#[test]
fn scope_words_are_case_insensitive() {
    let tmp = Tmp::new();
    let source = tmp.join("in.xlsx");
    save(&source, vec![
        row!["Id", "A", "B", "Note"], row!["ID<int32>", "int32", "int32", "string"],
        row!["All", "Client", "SERVER", "#"], row![1, 2, 3, "memo"],
    ]);
    assert_eq!(drtable(["check", "--input", &s(&source)]).code, 0);
}
