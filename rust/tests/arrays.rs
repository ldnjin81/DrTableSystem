//! Arrays: one schema field (`int32[]`, `int32[4]`) whose length each row sets, written in a data
//! sheet as element columns (`Reward[0]`, `Reward[1]`, ...) or as one comma-separated cell. The
//! client JSON (baked into assets) keeps elements in one pool per field; the server JSON keeps
//! each row's array.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn schema(folder: &Path, name: &str, items: &[(&str, &str, &str)]) {
    write_schema(&folder.join(format!("{name}.schema.xlsx")), name, &fields(items));
}

/// A data workbook: sheets of (name, header row, data rows); rows 2-3 are left empty.
fn data(path: &Path, sheets: Vec<(&str, Row, Vec<Row>)>) {
    let mut book = Book::new();
    for (name, header, rows) in sheets {
        let mut all = vec![header, vec![], vec![]];
        all.extend(rows);
        book.add(name, all);
    }
    book.save_plain(path);
}

fn server_rows(out: &Path, table: &str) -> serde_json::Value {
    json(&out.join(format!("server/{table}.json")))["rows"].clone()
}

const ITEMS: [(&str, &str, &str); 3] = [("Id", "ID<int32>", "all"), ("Reward", "int32[]", "all"), ("Tags", "name[]", "all")];

#[test]
fn rows_set_their_own_length_in_either_form() {
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    schema(&folder, "Items", &ITEMS);
    // Element columns; another sheet of the same table has more of them; a third writes one cell.
    data(&folder.join("Items.xlsx"), vec![
        ("Items#a", row!["Id", "Reward[0]", "Reward[1]", "Tags"], vec![
            row![1, 10, 20, "Fire, Big"],
            row![2, 30, (), "Ice"],
            row![3, (), (), ()],
        ]),
        ("Items#b", row!["Id", "Reward[0]", "Reward[1]", "Reward[2]", "Tags[0]"], vec![row![4, 1, 2, 3, "Wind"]]),
    ]);
    data(&folder.join("More.xlsx"), vec![("Items#c", row!["Id", "Reward", "Tags"], vec![
        row![5, "7,8 , 9", ()],
        row![6, 42, "One"],
    ])]);
    let out = tmp.join("out");
    let result = build(&folder, &out, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(server_rows(&out, "Items"), json!([
        {"Id": 1, "Reward": [10, 20], "Tags": ["Fire", "Big"]},
        {"Id": 2, "Reward": [30], "Tags": ["Ice"]},
        {"Id": 3, "Reward": [], "Tags": []},
        {"Id": 4, "Reward": [1, 2, 3], "Tags": ["Wind"]},
        {"Id": 5, "Reward": [7, 8, 9], "Tags": []},
        {"Id": 6, "Reward": [42], "Tags": ["One"]},
    ]));
    // The client: one pool per field in row (primary key) order, each row's run in it.
    let client = json(&out.join("client/Items.json"));
    assert_eq!(client["arrays"], json!([
        {"field": "Reward", "pool": [10, 20, 30, 1, 2, 3, 7, 8, 9, 42]},
        {"field": "Tags", "pool": ["Fire", "Big", "Ice", "Wind", "One"]},
    ]));
    let runs: Vec<(i64, i64)> = client["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["Reward_Start"].as_i64().unwrap(), row["Reward_Num"].as_i64().unwrap()))
        .collect();
    assert_eq!(runs, [(0, 2), (2, 1), (3, 0), (3, 3), (6, 3), (9, 1)]);
    assert_eq!(drtable(["check", "--client", &s(&out.join("client")), "--server", &s(&out.join("server"))]).code, 0);
}

#[test]
fn data_errors_point_at_their_cells() {
    let cases: Vec<(Row, Row, &str, &str)> = vec![
        // A gap before a filled element column.
        (row!["Id", "Reward[0]", "Reward[1]", "Tags"], row![1, (), 20, ()], "Items!B4", "중간의 칸이 비어"),
        // An empty element in one cell.
        (row!["Id", "Reward", "Tags"], row![1, "10,,20", ()], "Items!B4", "2번째 원소가 비어"),
        // An element that is not a number, in either form.
        (row!["Id", "Reward", "Tags"], row![1, "10, abc", ()], "Items!B4", "'abc' 값을 int32"),
        (row!["Id", "Reward[0]", "Reward[1]", "Tags"], row![1, 10, "abc", ()], "Items!C4", "'abc' 값을 int32"),
        // Element columns of a field that is not an array.
        (row!["Id", "Reward", "Tags", "Id[0]"], row![1, (), (), 1], "Items!D1", "배열이 아니라"),
        // Element columns must start at 0 without gaps.
        (row!["Id", "Reward[1]", "Tags"], row![1, 5, ()], "Items!B1", "빈틈없이"),
    ];
    for (header, values, location, message) in cases {
        let tmp = Tmp::new();
        let folder = tmp.join("data");
        schema(&folder, "Items", &ITEMS);
        data(&folder.join("Items.xlsx"), vec![("Items", header.clone(), vec![values])]);
        let result = build(&folder, &tmp.join("out"), &[]);
        assert_eq!(result.code, 1, "{header:?}");
        assert!(result.stderr.contains(location), "{location}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{message}: {}", result.stderr);
        assert!(!tmp.join("out/client").exists());
    }
}

#[test]
fn a_declared_maximum_is_checked() {
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    schema(&folder, "Items", &[("Id", "ID<int32>", "all"), ("Slots", "int32[2]", "all")]);
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Slots"], vec![row![1, "1,2"], row![2, "1,2,3"]])]);
    let result = build(&folder, &tmp.join("out"), &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("Items!B5: 배열 'Slots'의 원소가 3개로 최대 2개를 넘습니다"), "{}", result.stderr);
    // The maximum shows in the generated comment.
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Slots"], vec![row![1, "1,2"]])]);
    assert_eq!(build(&folder, &tmp.join("out"), &[]).code, 0);
    assert!(read(&tmp.join("out/cpp/DrItemsRow.h")).contains("// Slots: int32[2]. Elements: UDrItemsTable::Slots_Pool[Slots_Start .. Slots_Start + Slots_Num)"));
}

#[test]
fn schema_errors() {
    let cases = [
        // The old per-element declaration, with the new form in the message.
        (("Reward[0]", "int32", "all"), "A3", "자료형 'int32[]'"),
        (("Reward", "int32[]=5", "all"), "B3", "기본값"),
        (("Reward", "int32[0]", "all"), "B3", "최대 길이"),
        (("Reward", "SubKey<int32>[]", "all"), "B3", "키로"),
    ];
    for ((name, kind, scope), cell, message) in cases {
        let tmp = Tmp::new();
        let folder = tmp.join("data");
        schema(&folder, "Items", &[("Id", "ID<int32>", "all"), (name, kind, scope)]);
        data(&folder.join("Items.xlsx"), vec![("Items", row!["Id"], vec![row![1]])]);
        let result = build(&folder, &tmp.join("out"), &[]);
        assert_eq!(result.code, 1, "{kind}");
        assert!(result.stderr.contains(&format!("[Items.schema.xlsx]Items!{cell}")), "{kind}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{kind}: {}", result.stderr);
    }
}

#[test]
fn enum_date_and_fixed_elements() {
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    write_enum(&folder.join("Enums/Element.enum.xlsx"), "Element", &[
        (V::from("Fire"), V::from(0), V::E),
        (V::from("Water"), V::from(1), V::E),
    ]);
    schema(&folder, "Items", &[
        ("Id", "ID<int32>", "all"),
        ("Elements", "EElement[]", "all"),
        ("Opens", "datetime<UTC>[]", "all"),
        ("Rates", "fixed<100>[]", "all"),
    ]);
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Elements", "Opens", "Rates[0]", "Rates[1]"], vec![
        row![1, "Water, Fire", "2026-01-01, 2026-01-02 12:00", 0.5, "1.25"],
    ])]);
    let out = tmp.join("out");
    let result = build(&folder, &out, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(server_rows(&out, "Items")[0], json!({
        "Id": 1,
        "Elements": ["Water", "Fire"],
        "Opens": [1767225600000i64, 1767355200000i64],
        "Rates": [50, 125],
    }));
    let asset = read(&out.join("cpp/DrItemsTable.h"));
    for line in ["TArray<EDrElement> Elements_Pool;", "TArray<FDateTime> Opens_Pool;", "TArray<int32> Rates_Pool;"] {
        assert!(asset.contains(line), "{line}\n{asset}");
    }
    assert!(read(&out.join("cpp/DrItemsRow.h")).contains("static constexpr int32 RatesScale = 100;"));
}

#[test]
fn generated_cpp_reads_elements_through_the_pool() {
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    schema(&folder, "Items", &ITEMS);
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Reward", "Tags"], vec![row![1, "1,2", "A"]])]);
    let out = tmp.join("out");
    assert_eq!(build(&folder, &out, &["--ue-plugin"]).code, 0);
    let header = read(&out.join("cpp/DrItemsRow.h"));
    for line in [
        "    // Array elements (generated): a view into the table's pool, Num() long.",
        "    TConstArrayView<int32> GetReward() const;",
        "    TConstArrayView<FName> GetTags() const;",
    ] {
        assert!(header.contains(line), "{line}\n{header}");
    }
    assert!(!header.contains("EditAnywhere"));
    let source = read(&out.join("cpp/DrItemsRow.cpp"));
    assert!(source.contains(
        "TConstArrayView<int32> FDrItemsRow::GetReward() const\n{\n    return DrTableRuntime::GetArray<FDrItemsRow, int32>(FName(TEXT(\"Reward\")), Reward_Start, Reward_Num);\n}"
    ));
    let registration = read(&out.join("cpp/DrTableRegistration.h"));
    assert!(registration.contains(
        "            .WithArray(TEXT(\"Reward\"), &UDrItemsTable::Reward_Pool)\n            .WithArray(TEXT(\"Tags\"), &UDrItemsTable::Tags_Pool);"
    ), "{registration}");
}

#[test]
fn array_members_must_not_clash_with_fields() {
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    schema(&folder, "Items", &[("Id", "ID<int32>", "all"), ("Reward", "int32[]", "all"), ("Reward_Num", "int32", "all")]);
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Reward", "Reward_Num"], vec![row![1, (), 0]])]);
    let result = build(&folder, &tmp.join("out"), &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("배열 필드 'Reward'의 멤버 'Reward_Num'이 같은 이름의 필드와 겹칩니다"), "{}", result.stderr);
}

#[test]
fn aliases_take_an_array_suffix_but_are_not_arrays() {
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    Book::new()
        .with("Types", vec![row!["Name", "Type", "Comment"], row!["ItemID", "int32"]])
        .save_plain(&folder.join("Common.using.xlsx"));
    schema(&folder, "Items", &[("Id", "ID<int32>", "all"), ("Links", "ItemID[]", "all")]);
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Links"], vec![row![1, "3,4"]])]);
    let out = tmp.join("out");
    let result = build(&folder, &out, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(server_rows(&out, "Items")[0]["Links"], json!([3, 4]));
    assert!(read(&out.join("cpp/DrItemsRow.h")).contains("meta = (DrType = \"ItemID\", DrArray = \"Links\")"));
    // An alias itself cannot be an array.
    Book::new()
        .with("Types", vec![row!["Name", "Type", "Comment"], row!["ItemID", "int32"], row!["Bag", "int32[]"]])
        .save_plain(&folder.join("Common.using.xlsx"));
    let result = build(&folder, &out, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("별칭 'Bag'은 배열이 될 수 없습니다"), "{}", result.stderr);
}

#[test]
fn the_maximum_is_structure_the_data_form_is_not() {
    let hash = |kind: &str, header: Row, values: Row| {
        let tmp = Tmp::new();
        let folder = tmp.join("data");
        schema(&folder, "Items", &[("Id", "ID<int32>", "all"), ("Reward", kind, "all")]);
        data(&folder.join("Items.xlsx"), vec![("Items", header, vec![values])]);
        let out = tmp.join("out");
        assert_eq!(build(&folder, &out, &[]).code, 0);
        json(&out.join("client/Items.json"))["schema_hash"].clone()
    };
    let cells = hash("int32[]", row!["Id", "Reward"], row![1, "1,2"]);
    let columns = hash("int32[]", row!["Id", "Reward[0]", "Reward[1]"], row![1, 1, 2]);
    let bounded = hash("int32[4]", row!["Id", "Reward"], row![1, "1,2"]);
    assert_eq!(cells, columns);
    assert_ne!(cells, bounded);
}

#[test]
fn an_old_style_schema_gives_one_error_per_field() {
    // Before 0.7.0 a table declared Reward[0], Reward[1] in the schema and the data had the same
    // columns: only the schema line is reported, with the new form.
    let tmp = Tmp::new();
    let folder = tmp.join("data");
    schema(&folder, "Items", &[("Id", "ID<int32>", "all"), ("Reward[0]", "int32", "all"), ("Reward[1]", "int32", "all")]);
    data(&folder.join("Items.xlsx"), vec![("Items", row!["Id", "Reward[0]", "Reward[1]"], vec![row![1, 10, 20]])]);
    let result = build(&folder, &tmp.join("out"), &[]);
    assert_eq!(result.code, 1);
    let lines: Vec<&str> = result.stderr.lines().filter(|l| l.contains('!')).collect();
    assert_eq!(lines.len(), 1, "{}", result.stderr);
    assert!(lines[0].contains("[Items.schema.xlsx]Items!A3") && lines[0].contains("자료형 'int32[]'"), "{}", result.stderr);
}
