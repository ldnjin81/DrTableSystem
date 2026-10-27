//! Table scope: the primary key's scope (all, client, server, #) decides where a table is emitted.

#[macro_use]
mod common;

use common::*;
use serde_json::json;

fn table(names: &[&str], types: &[&str], scopes: &[&str], data: Vec<Row>) -> Vec<Row> {
    let text = |items: &[&str]| items.iter().map(|item| V::from(*item)).collect::<Row>();
    let mut rows = vec![text(names), text(types), text(scopes)];
    rows.extend(data);
    rows
}

fn manifest_tables(path: &std::path::Path) -> Vec<String> {
    json(path)["tables"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn client_table_is_emitted_only_to_the_client() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    Book::new()
        .with("Hints", table(&["Id", "Text", "Note"], &["ID<int32>", "string", "string"], &["client", "all", "client"],
            vec![row![1, "점프는 Space", "메모"]]))
        .with("Items", table(&["Id"], &["ID<int32>"], &["all"], vec![row![10]]))
        .save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    // An `all` field in a client table narrows to the client.
    assert_eq!(json(&tmp.join("client/Hints.json"))["rows"], json!([{"Id": 1, "Text": "점프는 Space", "Note": "메모"}]));
    assert!(!tmp.join("server/Hints.json").exists());
    assert_eq!(manifest_tables(&tmp.join("client/manifest.json")), vec!["Hints", "Items"]);
    assert_eq!(manifest_tables(&tmp.join("server/manifest.json")), vec!["Items"]);
    assert!(tmp.join("cpp/DrHintsRow.h").exists());
    assert!(read(&tmp.join("cpp/DrGeneratedTables.h")).contains("HintsName"));
}

#[test]
fn server_table_has_no_client_output_or_cpp() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    Book::new()
        .with("DropRates", table(&["Id", "Rate"], &["ID<int32>", "float"], &["server", "all"], vec![row![1, 0.5]]))
        .with("Items", table(&["Id"], &["ID<int32>"], &["all"], vec![row![10]]))
        .save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(json(&tmp.join("server/DropRates.json"))["rows"], json!([{"Id": 1, "Rate": 0.5}]));
    assert!(!tmp.join("client/DropRates.json").exists());
    assert_eq!(manifest_tables(&tmp.join("client/manifest.json")), vec!["Items"]);
    assert_eq!(manifest_tables(&tmp.join("server/manifest.json")), vec!["DropRates", "Items"]);
    assert!(!tmp.join("cpp/DrDropRatesRow.h").exists());
    assert!(!tmp.join("cpp/DrDropRatesTable.h").exists());
    assert!(!read(&tmp.join("cpp/DrGeneratedTables.h")).contains("DropRates"));
    // Both sides stay self-consistent for the checker.
    let check = drtable(["check", "--client", &s(&tmp.join("client")), "--server", &s(&tmp.join("server"))]);
    assert_eq!(check.code, 0, "{}", check.stderr);
}

#[test]
fn field_for_the_other_side_is_warned_and_left_out() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    Book::new()
        .with("DropRates", table(&["Id", "Rate", "Icon"], &["ID<int32>", "float", "string"], &["server", "all", "client"],
            vec![row![1, 0.5, "icon.png"]]))
        .save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stderr.contains("필드 'Icon'의 범위 client가 테이블 범위 server(기본키 범위)와 맞지 않아"), "{}", result.stderr);
    assert!(result.stderr.contains("DropRates.schema.xlsx]DropRates!C4"), "{}", result.stderr);
    assert_eq!(json(&tmp.join("server/DropRates.json"))["rows"], json!([{"Id": 1, "Rate": 0.5}]));
}

#[test]
fn design_only_table_is_not_built() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    Book::new()
        .with("Scratch", table(&["Id", "Idea"], &["ID<int32>", "string"], &["#", "all"], vec![row![1, "보스 2페이즈 아이디어"]]))
        .with("Items", table(&["Id"], &["ID<int32>"], &["all"], vec![row![10]]))
        .save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    for side in ["client", "server"] {
        assert!(!tmp.join(format!("{side}/Scratch.json")).exists());
        assert_eq!(manifest_tables(&tmp.join(format!("{side}/manifest.json"))), vec!["Items"]);
    }
    assert!(!tmp.join("cpp/DrScratchRow.h").exists());
}

#[test]
fn reference_must_not_reach_a_table_missing_on_its_side() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    Book::new()
        .with("DropRates", table(&["Id"], &["ID<int32>"], &["server"], vec![row![1]]))
        .with("Items", table(&["Id", "Drop", "ServerDrop"], &["ID<int32>", "Ref<DropRates>", "Ref<DropRates>"], &["all", "all", "server"],
            vec![row![10, 1, 1]]))
        .save(&source);
    let result = drtable(["check", "--input", &s(&source)]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("참조 범위 all가 대상 테이블 범위 server보다 넓습니다"), "{}", result.stderr);
    // The server-only reference (ServerDrop) is fine: it is reported once, for Drop only.
    assert_eq!(result.stderr.matches("대상 테이블 범위").count(), 1, "{}", result.stderr);
}

#[test]
fn client_table_may_reference_an_all_table() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    Book::new()
        .with("Hints", table(&["Id", "Item"], &["ID<int32>", "Ref<Items>"], &["client", "all"], vec![row![1, 10]]))
        .with("Items", table(&["Id"], &["ID<int32>"], &["all"], vec![row![10]]))
        .save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(json(&tmp.join("client/Hints.json"))["rows"], json!([{"Id": 1, "Item": 10}]));
}
