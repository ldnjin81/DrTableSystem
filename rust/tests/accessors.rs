//! Runtime outputs: row lookups and reference functions, per-table .cpp, registration header.

#[macro_use]
mod common;

use common::*;
use std::path::Path;

const RUNTIME: &str = "TableData/DrTableRuntime.h";

fn source(path: &Path) {
    Book::new()
        .with("<enum>Kind", vec![row!["Id", "Value"], row!["ID<name>", "int32"], row!["all", "all"], row!["A", 0], row!["B", 1]])
        .with("Items", vec![
            row!["Id", "Kind", "Quest", "ServerQuest"],
            row!["ID<int32>", "SubKey<EKind>", "Ref<Quests>", "Ref<Quests>"],
            row!["all", "all", "all", "server"],
            row![1001, "A", 1, 1],
        ])
        .with("Quests", vec![
            row!["Id", "Reward", "Next[0]", "Next[1]"],
            row!["ID<int32>", "SubKey<Ref<Items>>", "Ref<Quests>[]", "Ref<Quests>[]"],
            row!["all", "all", "all", "all"],
            row![1, 1001, 2, ()],
            row![2, (), (), ()],
        ])
        .with("DropTable", vec![
            row!["Id", "GroupId", "KindKey"],
            row!["ID<int32>", "SubKey<int32>", "SubKey<EKind>"],
            row!["all", "all", "all"],
            row![1, 10, "A"],
        ])
        .with("Monsters", vec![
            row!["Id", "DropGroup", "ByKind", "Name"],
            row!["ID<name>", "Ref<DropTable.GroupId>", "Ref<DropTable.KindKey>", "Ref<Monsters>"],
            row!["all", "all", "all", "all"],
            row!["Wolf", 10, "A", ()],
        ])
        .save(path);
}

/// Builds the shared source into tmp and returns it.
fn built(extra: &[&str]) -> Tmp {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path);
    let result = build(&path, &tmp, extra);
    assert_eq!(result.code, 0, "{}", result.stderr);
    tmp
}

fn cpp(root: &Path, name: &str) -> String {
    read(&root.join("cpp").join(name))
}

#[test]
fn without_runtime_header_no_accessors_are_generated() {
    let tmp = built(&[]);
    let names = names(&tmp.join("cpp"));
    assert!(!names.iter().any(|name| name.ends_with(".cpp")));
    assert!(!names.contains("DrTableRegistration.h"));
    let row = cpp(&tmp, "DrQuestsRow.h");
    assert!(!row.contains("static"));
    assert!(!row.contains("struct FDrItemsRow;"));
}

#[test]
fn row_header_declarations_and_forward_declarations() {
    let tmp = built(&["--runtime-header", RUNTIME]);
    let quests = cpp(&tmp, "DrQuestsRow.h");
    assert!(quests.contains("struct FDrItemsRow;"));
    assert!(!quests.contains("struct FDrQuestsRow;"), "no forward declaration of itself");
    assert!(!quests.contains("#include \"DrItemsRow.h\""), "no circular include");
    for line in [
        "static const FDrQuestsRow* Find(int32 Key);",
        "static TArray<const FDrQuestsRow*> FindByReward(int32 Key);",
        "static TConstArrayView<FDrQuestsRow> GetAll();",
        "const FDrItemsRow* GetReward() const;",
        "const FDrQuestsRow* GetNext(int32 Index) const;",
    ] {
        assert!(quests.contains(line), "{line}");
    }
    let items = cpp(&tmp, "DrItemsRow.h");
    assert!(items.contains("static TArray<const FDrItemsRow*> FindByKind(EDrKind Key);"));
    assert!(!items.contains("GetServerQuest"), "server-only fields are not in the client struct");
    let monsters = cpp(&tmp, "DrMonstersRow.h");
    for line in [
        "static const FDrMonstersRow* Find(FName Key);",
        "TArray<const FDrDropTableRow*> GetDropGroup() const;",
        "TArray<const FDrDropTableRow*> GetByKind() const;",
        "const FDrMonstersRow* GetName() const;",
    ] {
        assert!(monsters.contains(line), "{line}");
    }
}

#[test]
fn row_source_definitions() {
    let tmp = built(&["--runtime-header", RUNTIME]);
    let quests = cpp(&tmp, "DrQuestsRow.cpp");
    let includes: Vec<&str> = quests.lines().skip(1).take(3).collect();
    let runtime_include = format!("#include \"{RUNTIME}\"");
    assert_eq!(includes, ["#include \"DrQuestsRow.h\"", "#include \"DrItemsRow.h\"", runtime_include.as_str()]);
    for line in [
        "return DrTableRuntime::FindByKey<FDrQuestsRow>(Key);",
        "return DrTableRuntime::FindAllBySubKey<FDrQuestsRow>(FName(TEXT(\"Reward\")), Key);",
        "return DrTableRuntime::GetAll<FDrQuestsRow>();",
        "TConstArrayView<int32> FDrQuestsRow::GetNext() const\n{\n    return DrTableRuntime::GetArray<FDrQuestsRow, int32>(FName(TEXT(\"Next\")), Next_Start, Next_Num);\n}",
        "    const TConstArrayView<int32> Items = GetNext();\n    if (Index < 0 || Index >= Items.Num() || Items[Index] == 0)",
        "    return FDrQuestsRow::Find(Items[Index]);",
    ] {
        assert!(quests.contains(line), "{line}");
    }
    let monsters = cpp(&tmp, "DrMonstersRow.cpp");
    for part in [
        "    if (DropGroup == 0)\n    {\n        return {};\n    }",
        "    return FDrDropTableRow::FindByGroupId(DropGroup);",
        "    return FDrDropTableRow::FindByKindKey(ByKind);",
        "    if (Name.IsNone())\n    {\n        return nullptr;\n    }",
    ] {
        assert!(monsters.contains(part), "{part}");
    }
    // Enum sub key targets cannot be empty, so there is no empty-value check.
    let by_kind = monsters.split_once("GetByKind() const\n{\n").unwrap().1.split_once("}\n").unwrap().0;
    assert!(!by_kind.contains("if ("));
}

#[test]
fn registration_header() {
    let tmp = built(&["--runtime-header", RUNTIME, "--asset-name", "BT_{table}"]);
    let text = cpp(&tmp, "DrTableRegistration.h");
    for part in [
        "#include \"DrGeneratedTables.h\"",
        "#include \"DrQuestsTable.h\"",
        "namespace DrGeneratedTables",
        "    template <typename TRegistry>\n    void RegisterAll(TRegistry& Registry)",
        "Registry.template Register<FDrQuestsRow, UDrQuestsTable>(TEXT(\"BT_Quests\"), &UDrQuestsTable::Rows, &UDrQuestsTable::PrimaryKeys)",
        "            .WithSchemaHash(QuestsSchemaHash)\n            .WithSubKey(TEXT(\"Reward\"), &UDrQuestsTable::Reward_Keys, &UDrQuestsTable::Reward_Offsets, &UDrQuestsTable::Reward_Indices)\n            .WithArray(TEXT(\"Next\"), &UDrQuestsTable::Next_Pool);",
        "            .WithSchemaHash(MonstersSchemaHash);",
    ] {
        assert!(text.contains(part), "{part}");
    }
    assert!(!text.contains("ContentHash"));
    // Tables are registered in name order (deterministic).
    let order: Vec<usize> = ["DropTable", "Items", "KindInfo", "Monsters", "Quests"]
        .iter()
        .filter_map(|name| text.find(&format!("Register<FDr{name}Row")))
        .collect();
    assert_eq!(order.len(), 4);
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn asset_name_requires_table_placeholder() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path);
    let result = build(&path, &tmp, &["--runtime-header", RUNTIME, "--asset-name", "DA_Fixed"]);
    assert!(result.is_usage_error(), "{}", result.stderr);
    assert!(result.stderr.contains("{table}"));
}

#[test]
fn runtime_outputs_are_deterministic() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path);
    let mut snapshots = Vec::new();
    for run in ["a", "b"] {
        let root = tmp.join(run);
        assert_eq!(build(&path, &root, &["--runtime-header", RUNTIME]).code, 0);
        snapshots.push(files(&root.join("cpp")));
    }
    assert_eq!(snapshots[0], snapshots[1]);
    assert!(snapshots[0].values().all(|content| !content.windows(2).any(|pair| pair == b"\r\n")));
}

#[test]
fn member_name_collisions_are_errors() {
    let cases: [(&[&str], &[&str], &str); 4] = [
        (&["Id", "Find"], &["ID<int32>", "int32"], "'Find'이 같은 이름의 필드"),
        (&["Id", "GetAll"], &["ID<int32>", "int32"], "'GetAll'이 같은 이름의 필드"),
        (&["Id", "Other", "GetOther"], &["ID<int32>", "Ref<T>", "int32"], "'GetOther'이 같은 이름의 필드"),
        (&["Id", "Group", "FindByGroup"], &["ID<int32>", "SubKey<int32>", "int32"], "'FindByGroup'이 같은 이름의 필드"),
    ];
    for (headers, types, message) in cases {
        let tmp = Tmp::new();
        let mut data = vec![V::from(1)];
        data.resize(headers.len(), V::E);
        let path = tmp.join("in.xlsx");
        Book::new()
            .with("T", vec![
                headers.iter().map(|h| V::from(*h)).collect(),
                types.iter().map(|t| V::from(*t)).collect(),
                vec![V::from("all"); headers.len()],
                data,
            ])
            .save(&path);
        let result = build(&path, &tmp, &["--runtime-header", RUNTIME]);
        assert_eq!(result.code, 1, "{message}");
        assert!(result.stderr.contains(message), "{}", result.stderr);
        assert!(result.stderr.starts_with("[T.schema.xlsx]T!"), "{}", result.stderr);
        assert!(!tmp.join("cpp").exists(), "nothing is written on errors");
    }
}

#[test]
fn member_name_collision_is_ignored_without_runtime_header() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    Book::new().with("T", vec![row!["Id", "Find"], row!["ID<int32>", "int32"], row!["all", "all"], row![1, 2]]).save(&path);
    assert_eq!(build(&path, &tmp, &[]).code, 0);
}
