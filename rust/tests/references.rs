//! Table references (Ref, SubKey refs) and the reference check command, end to end.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

/// One sheet from its header rows (names, types, scopes) and data rows.
fn table(names: &[&str], types: &[&str], scopes: &[&str], data: Vec<Row>) -> Vec<Row> {
    let text = |items: &[&str]| items.iter().map(|item| V::from(*item)).collect::<Row>();
    let mut rows = vec![text(names), text(types), text(scopes)];
    rows.extend(data);
    rows
}

fn source_book() -> Book {
    Book::new()
        .with("Quests", table(
            &["Id", "Item", "Next[0]", "Next[1]", "ServerItem"],
            &["ID<int32>", "SubKey<Ref<Items>>", "Ref<Quests>", "Ref<Quests>", "Ref<Items>"],
            &["all", "all", "client", "client", "server"],
            vec![row![1, 1001, 2, (), 1001], row![2, (), (), (), ()]],
        ))
        .with("Items", table(&["Id", "Quest"], &["ID<int32>", "Ref<Quests>"], &["all", "all"], vec![row![1001, 1]]))
        .with("Names", table(&["Id"], &["ID<name>"], &["all"], vec![row!["Sword"]]))
        .with("Links", table(&["Id", "NameRef"], &["ID<int64>", "ref<Names>"], &["all", "all"], vec![row![3, ()]]))
}

fn check_output(client: &Path, server: Option<&Path>) -> Output {
    let mut args = vec!["check".to_string(), "--client".into(), s(client)];
    if let Some(server) = server {
        args.extend(["--server".to_string(), s(server)]);
    }
    drtable(args)
}

fn graph(source: &Path, out: &Path) -> Output {
    drtable(["graph", "--input", &s(source), "--out", &s(out)])
}

/// Every output file (not the workbooks).
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    files(root).into_iter().filter(|(name, _)| !name.ends_with(".xlsx")).collect()
}

fn refs(manifest: &serde_json::Value) -> &Vec<serde_json::Value> {
    manifest["references"].as_array().unwrap()
}

#[test]
fn ref_build_graph_check_and_determinism() {
    let tmp = Tmp::new();
    let source = tmp.join("ref.xlsx");
    source_book().save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let client = json(&tmp.join("client/manifest.json"));
    let server = json(&tmp.join("server/manifest.json"));
    let summary: Vec<_> = refs(&client)
        .iter()
        .map(|r| json!([r["table"], r["field"], r["target"], r["key_type"], r["array_length"], r["subkey"]]))
        .collect();
    assert_eq!(summary, [
        json!(["Items", "Quest", "Quests", "int32", 1, false]),
        json!(["Links", "NameRef", "Names", "name", 1, false]),
        json!(["Quests", "Item", "Items", "int32", 1, true]),
        json!(["Quests", "Next", "Quests", "int32", 2, false]),
    ]);
    let server_fields: Vec<_> = refs(&server).iter().filter(|r| r["table"] == "Quests").map(|r| r["field"].clone()).collect();
    assert_eq!(server_fields, [json!("Item"), json!("ServerItem")]);
    let quest = json(&tmp.join("client/Quests.json"));
    assert_eq!(quest["rows"][1]["Next"], json!([0, 0]));
    assert_eq!(quest["rows"][1]["Item"], json!(0));
    assert_eq!(json(&tmp.join("client/Links.json"))["rows"][0]["NameRef"], json!(""));
    assert!(read(&tmp.join("cpp/DrLinksRow.h")).contains("FName NameRef = NAME_None;"));
    let header = read(&tmp.join("cpp/DrQuestsRow.h"));
    for part in ["meta = (TableRef = \"Items\")", "meta = (TableRef = \"Quests\")", "int32 Item", "int32 Next[2]"] {
        assert!(header.contains(part), "{part}");
    }
    assert_eq!(check_output(&tmp.join("client"), Some(&tmp.join("server"))).code, 0);
    let diagram_path = tmp.join("graph.md");
    assert_eq!(graph(&source, &diagram_path).code, 0);
    let diagram = read(&diagram_path);
    for part in ["flowchart LR", "Names[\"Names (name)\"]", "Quests -->|\"Next[2]\"| Quests", "Quests -->|\"Item (SubKey)\"| Items"] {
        assert!(diagram.contains(part), "{part}");
    }
    let first = snapshot(&tmp);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    assert_eq!(graph(&source, &diagram_path).code, 0);
    assert_eq!(snapshot(&tmp), first);
}

#[test]
fn check_reports_all_broken_refs_and_input_errors() {
    let tmp = Tmp::new();
    let source = tmp.join("ref.xlsx");
    source_book().save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let path = tmp.join("client/Quests.json");
    let mut data = json(&path);
    data["rows"][0]["Item"] = json!(999);
    data["rows"][0]["Next"] = json!([77, 88]);
    write_json(&path, &data);
    let result = check_output(&tmp.join("client"), None);
    assert_eq!(result.code, 1);
    for part in [
        "Quests.Item[1] = 999 → Items 테이블에 없음",
        "Quests.Next[1](0) = 77 → Quests 테이블에 없음",
        "Quests.Next[1](1) = 88 → Quests 테이블에 없음",
    ] {
        assert!(result.stderr.contains(part), "{part}: {}", result.stderr);
    }
    assert_eq!(check_output(&tmp.join("missing"), None).code, 2);
}

#[test]
fn ref_schema_errors() {
    let cases = [
        ("Ref<Missing>", "B3"),
        ("Ref<<enum>Kind>", "B3"),
        ("Ref<#Notes>", "B3"),
        ("ID<Ref<Items>>", "B2"),
        ("Ref<Items>=1001", "B3"),
    ];
    for (decl, cell) in cases {
        let tmp = Tmp::new();
        let types = if decl.starts_with("ID<") { row![decl, "Ref<Items>"] } else { row!["ID<int32>", decl] };
        let source = tmp.join("bad.xlsx");
        Book::new()
            .with("Quests", vec![row!["Id", "Item"], types, row!["all", "all"], row![1, 1001]])
            .with("Items", table(&["Id"], &["ID<int32>"], &["all"], vec![row![1001]]))
            .with("#Notes", vec![])
            .save(&source);
        let result = build(&source, &tmp, &[]);
        assert_eq!(result.code, 1, "{decl}");
        assert!(result.stderr.contains(&format!("[Quests.schema.xlsx]Quests!{cell}")), "{decl}: {}", result.stderr);
    }
}

#[test]
fn enum_ref_empty_is_error_and_info_is_target() {
    let tmp = Tmp::new();
    let mut book = Book::new()
        .with("<enum>Kind", vec![
            row!["Id", "Value", "Label"], row!["ID<name>", "int32", "string"], row!["all", "all", "all"], row!["Sword", 0, "검"],
        ])
        .with("Uses", table(
            &["Id", "Kind", "Info"], &["ID<int32>", "Ref<KindInfo>", "Ref<KindInfo>"], &["all", "all", "all"],
            vec![row![1, "Sword", ()]],
        ));
    let source = tmp.join("enum.xlsx");
    book.save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("Uses!C4"), "{}", result.stderr);
    book.sheet("Uses").set("C4", "Sword");
    book.save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let header = read(&tmp.join("cpp/DrUsesRow.h"));
    assert!(header.contains("EDrKind Kind"));
    assert!(header.contains("TableRef = \"KindInfo\""));
}

#[test]
fn zero_key_warning() {
    let tmp = Tmp::new();
    let source = tmp.join("ref.xlsx");
    source_book().save(&source);
    // Add 0 to the built key set to see the checker's collision warning.
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let path = tmp.join("client/Items.json");
    let mut data = json(&path);
    data["rows"].as_array_mut().unwrap().push(json!({"Id": 0, "Quest": 0}));
    write_json(&path, &data);
    let result = check_output(&tmp.join("client"), None);
    assert_eq!(result.code, 0);
    assert!(result.stderr.contains("없음 값과 충돌"), "{}", result.stderr);
}

fn subkey_book() -> Book {
    Book::new()
        .with("<enum>Kind", table(&["Id", "Value"], &["ID<name>", "int32"], &["all", "all"], vec![row!["A", 0], row!["B", 1]]))
        .with("Monsters", table(
            &["Id", "DropGroup", "Groups[0]", "Groups[1]", "ByGroup", "Kind", "NameGroup"],
            &["ID<name>", "Ref<DropTable.GroupId>", "Ref<DropTable.GroupId>", "Ref<DropTable.GroupId>",
              "SubKey<Ref<DropTable.GroupId>>", "Ref<DropTable.KindKey>", "Ref<DropTable.NameKey>"],
            &["all", "all", "all", "all", "all", "all", "client"],
            vec![row!["Wolf", 10, 10, (), 10, "A", "Common"]],
        ))
        .with("DropTable", table(
            &["Id", "GroupId", "NameKey", "KindKey", "Description"],
            &["ID<int32>", "SubKey<int32>", "SubKey<name>", "SubKey<EKind>", "string"],
            &["all", "all", "client", "all", "all"],
            vec![row![1, 10, "Common", "A", "첫째"], row![2, 10, "Common", "A", "둘째"]],
        ))
}

#[test]
fn subkey_ref_outputs_and_check() {
    let tmp = Tmp::new();
    let source = tmp.join("subkey.xlsx");
    subkey_book().save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let client = json(&tmp.join("client/manifest.json"));
    let found: BTreeMap<String, serde_json::Value> = refs(&client)
        .iter()
        .map(|r| (r["field"].as_str().unwrap().to_string(), json!([r["target_key"], r["cardinality"], r["key_type"]])))
        .collect();
    let expected: BTreeMap<String, serde_json::Value> = [
        ("ByGroup", json!(["GroupId", "many", "int32"])),
        ("DropGroup", json!(["GroupId", "many", "int32"])),
        ("Groups", json!(["GroupId", "many", "int32"])),
        ("Kind", json!(["KindKey", "many", "EKind"])),
        ("NameGroup", json!(["NameKey", "many", "name"])),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_string(), value))
    .collect();
    assert_eq!(found, expected);
    let server = json(&tmp.join("server/manifest.json"));
    assert!(!refs(&server).iter().any(|r| r["field"] == "NameGroup"));
    let header = read(&tmp.join("cpp/DrMonstersRow.h"));
    assert!(header.contains("meta = (TableRef = \"DropTable\", TableRefKey = \"GroupId\")"));
    assert!(header.contains("meta = (TableRef = \"DropTable\", TableRefKey = \"KindKey\")"));
    assert!(header.contains("EDrKind Kind"));
    assert_eq!(check_output(&tmp.join("client"), Some(&tmp.join("server"))).code, 0);
    let diagram = tmp.join("graph.md");
    assert_eq!(graph(&source, &diagram).code, 0);
    assert!(read(&diagram).contains("GroupId 1:N"));
    let path = tmp.join("client/Monsters.json");
    let mut payload = json(&path);
    payload["rows"][0]["Groups"][0] = json!(404);
    write_json(&path, &payload);
    let result = check_output(&tmp.join("client"), None);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("DropTable.GroupId에 해당 값 없음"), "{}", result.stderr);
}

#[test]
fn subkey_ref_schema_errors() {
    let cases = [
        ("Ref<DropTable.Id>", "Ref<DropTable>를 쓰세요"),
        ("Ref<DropTable.Description>", "SubKey로 선언하세요"),
        ("Ref<DropTable.Missing>", "없습니다"),
        ("Ref<DropTable.NameKey>", "범위"),
        ("ID<Ref<DropTable.GroupId>>", "기본키"),
        ("Ref<DropTable.GroupId>=10", "기본값"),
    ];
    for (decl, message) in cases {
        let tmp = Tmp::new();
        let source = tmp.join("subkey.xlsx");
        let mut book = subkey_book();
        let monsters = book.sheet("Monsters");
        monsters.set("B2", decl);
        monsters.set("B3", "all");
        book.save(&source);
        let result = build(&source, &tmp, &[]);
        assert_eq!(result.code, 1, "{decl}");
        assert!(result.stderr.contains("[Monsters.schema.xlsx]Monsters!B3"), "{decl}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{decl}: {}", result.stderr);
    }
}

#[test]
fn subkey_enum_empty_and_warning() {
    let tmp = Tmp::new();
    let source = tmp.join("subkey.xlsx");
    let mut book = subkey_book();
    book.sheet("Monsters").set("F4", ());
    book.save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("Monsters!F4"), "{}", result.stderr);
    book.sheet("Monsters").set("F4", "A");
    book.sheet("DropTable").append(row![3, 0, "", "B", "없음 키"]);
    book.save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let result = check_output(&tmp.join("client"), None);
    assert_eq!(result.code, 0);
    for part in ["DropTable.GroupId", "DropTable.NameKey", "없음 값과 충돌"] {
        assert!(result.stderr.contains(part), "{part}: {}", result.stderr);
    }
}

#[test]
fn chained_subkey_ref_resolves_final_type() {
    let tmp = Tmp::new();
    let source = tmp.join("chain.xlsx");
    Book::new()
        .with("Uses", table(&["Id", "Value"], &["ID<int32>", "Ref<Top.Key>"], &["all", "all"], vec![row![1, 7]]))
        .with("Top", table(&["Id", "Key"], &["ID<int32>", "SubKey<Ref<Middle.Key>>"], &["all", "all"], vec![row![1, 7]]))
        .with("Middle", table(&["Id", "Key"], &["ID<int32>", "SubKey<Ref<Base>>"], &["all", "all"], vec![row![1, 7]]))
        .with("Base", table(&["Id"], &["ID<int64>"], &["all"], vec![row![7]]))
        .save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    assert!(read(&tmp.join("cpp/DrUsesRow.h")).contains("int64 Value"));
    let manifest = json(&tmp.join("client/manifest.json"));
    let reference = refs(&manifest).iter().find(|r| r["table"] == "Uses").unwrap();
    assert_eq!(json!([reference["target"], reference["target_key"], reference["key_type"]]), json!(["Top", "Key", "int64"]));
    assert_eq!(check_output(&tmp.join("client"), None).code, 0);
}

#[test]
fn chained_subkey_type_cycle_reports_path() {
    let tmp = Tmp::new();
    let source = tmp.join("cycle.xlsx");
    Book::new()
        .with("First", table(&["Id", "Key"], &["ID<int32>", "SubKey<Ref<Second.Key>>"], &["all", "all"], vec![row![1, 1]]))
        .with("Second", table(&["Id", "Key"], &["ID<int32>", "SubKey<Ref<First.Key>>"], &["all", "all"], vec![row![1, 1]]))
        .save(&source);
    let result = build(&source, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("자료형 순환: Second.Key → First.Key → Second.Key"), "{}", result.stderr);
    assert!(result.stderr.contains("[First.schema.xlsx]First!B3"));
}
