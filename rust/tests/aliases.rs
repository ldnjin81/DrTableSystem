//! Type aliases: `*.using.xlsx` names a type once (`ItemID = Ref<Items>`) for every schema.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::{Path, PathBuf};

type AliasRow<'a> = (&'a str, &'a str);

/// Table/Schema (schemas and aliases) and Table (data).
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

    fn aliases(&self, file: &str, rows: &[(&str, &str)]) {
        let mut sheet = vec![row!["Name", "Type", "Comment"]];
        sheet.extend(rows.iter().map(|(name, kind)| row![*name, *kind]));
        Book::new().with("Types", sheet).save_plain(&self.table.join("Schema").join(file));
    }

    fn schema(&self, name: &str, items: &[(&str, &str, &str)]) {
        write_schema(&self.table.join("Schema").join(format!("{name}.schema.xlsx")), name, &fields(items));
    }

    fn data(&self, name: &str, header: Row, data: Vec<Row>) {
        let mut rows = vec![header, vec![], vec![]];
        rows.extend(data);
        Book::new().with(name, rows).save_plain(&self.table.join(format!("{name}.xlsx")));
    }

    fn build(&self, extra: &[&str]) -> Output {
        let schema = s(&self.table.join("Schema"));
        let args: Vec<&str> = ["--schema", schema.as_str()].into_iter().chain(extra.iter().copied()).collect();
        build(&self.table, &self.out, &args)
    }

    fn read(&self, path: &str) -> String {
        read(&self.out.join(path))
    }
}

fn items(project: &Project) {
    project.schema("Items", &[("Id", "ID<ItemID>", "all"), ("Chance", "Rate", "all"), ("Level", "Level", "all")]);
    project.data("Items", row!["Id", "Chance", "Level"], vec![row![1001, 0.25, ()], row![1002, "5%", 7]]);
}

#[test]
fn aliases_expand_everywhere() {
    let project = Project::new();
    project.aliases("Types.using.xlsx", &[("ItemKey", "int32"), ("ItemID", "ItemKey"), ("Rate", "fixed<10000>"), ("Level", "int32=1"), ("ItemRef", "Ref<Items>")]);
    items(&project);
    project.schema("Quests", &[("Id", "ID<int32>", "all"), ("Reward", "ItemRef", "all"), ("Group", "SubKey<ItemRef>", "all"), ("MinLevel", "Level=5", "all")]);
    project.data("Quests", row!["Id", "Reward", "Group", "MinLevel"], vec![row![1, 1001, 1002, ()]]);
    let result = project.build(&["--ue-plugin", "--prefix", "Gm"]);
    assert_eq!(result.code, 0, "{}", result.stderr);

    let rows = json(&project.out.join("client/Items.json"))["rows"].clone();
    assert_eq!(rows, json!([{"Id": 1001, "Chance": 2500, "Level": 1}, {"Id": 1002, "Chance": 500, "Level": 7}]));
    // A field's own default wins over the alias default.
    assert_eq!(json(&project.out.join("client/Quests.json"))["rows"][0]["MinLevel"], 5);
    let manifest = json(&project.out.join("client/manifest.json"));
    let reward = manifest["references"].as_array().unwrap().iter().find(|r| r["field"] == "Reward").unwrap().clone();
    assert_eq!(reward["target"], "Items");

    let items_row = project.read("cpp/GmItemsRow.h");
    assert!(items_row.contains("meta = (DrType = \"ItemID\"))\n    int32 Id = 0;"), "{items_row}");
    assert!(items_row.contains("meta = (DrFixedScale = \"10000\", DrType = \"Rate\"))\n    int32 Chance = 0;"));
    assert!(items_row.contains("meta = (DrType = \"Level\"))\n    int32 Level = 1;"));
    let quests_row = project.read("cpp/GmQuestsRow.h");
    assert!(quests_row.contains("meta = (TableRef = \"Items\", DrType = \"ItemRef\"))\n    int32 Reward = 0;"), "{quests_row}");
    assert!(quests_row.contains("const FGmItemsRow* GetReward() const;"));

    let types = project.read("cpp/GmTypes.h");
    for line in [
        "using GmItemID = int32;",
        "using GmItemKey = int32;",
        "using GmItemRef = int32;",
        "using GmLevel = int32;",
        "using GmRate = int32;",
        "inline constexpr int32 GmRateScale = 10000;",
    ] {
        assert!(types.contains(line), "{line}\n{types}");
    }
}

#[test]
fn changing_an_alias_changes_every_user() {
    let project = Project::new();
    let mut hashes = Vec::new();
    for kind in ["int32", "int64"] {
        project.aliases("Types.using.xlsx", &[("ItemID", kind), ("Rate", "fixed<10000>"), ("Level", "int32")]);
        items(&project);
        assert_eq!(project.build(&[]).code, 0);
        assert!(project.read("cpp/DrItemsRow.h").contains(&format!("{kind} Id = 0;")));
        hashes.push(json(&project.out.join("client/Items.json"))["schema_hash"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
}

#[test]
fn renaming_an_alias_keeps_the_schema_hash() {
    let project = Project::new();
    let mut hashes = Vec::new();
    for name in ["Points", "Score"] {
        project.aliases("Types.using.xlsx", &[(name, "int32")]);
        project.schema("Items", &[("Id", "ID<int32>", "all"), ("Value", name, "all")]);
        project.data("Items", row!["Id", "Value"], vec![row![1, 2]]);
        assert_eq!(project.build(&[]).code, 0);
        hashes.push(json(&project.out.join("client/Items.json"))["schema_hash"].clone());
    }
    assert_eq!(hashes[0], hashes[1]);
}

#[test]
fn aliases_may_be_split_over_files() {
    let project = Project::new();
    project.aliases("Items.using.xlsx", &[("ItemID", "int32")]);
    project.aliases("Combat.using.xlsx", &[("Rate", "fixed<10000>"), ("Level", "int32")]);
    items(&project);
    assert_eq!(project.build(&[]).code, 0);
}

fn assert_error(project: &Project, location: &str, message: &str) {
    let result = project.build(&[]);
    assert_eq!(result.code, 1, "{message}");
    assert!(result.stderr.contains(location), "{location}: {}", result.stderr);
    assert!(result.stderr.contains(message), "{message}: {}", result.stderr);
}

#[test]
fn alias_errors() {
    let cases: [(&[AliasRow], &str, &str); 6] = [
        (&[("ItemID", "int32"), ("ItemID", "int64")], "[Types.using.xlsx]Types!A3", "별칭 'ItemID'이 [Types.using.xlsx]Types!A2에도 있습니다"),
        (&[("int32", "int64")], "[Types.using.xlsx]Types!A2", "자료형 이름이라 별칭으로 쓸 수 없습니다"),
        (&[("Items", "int32")], "[Types.using.xlsx]Types!A2", "같은 이름의 테이블과 겹칩니다"),
        (&[("A", "B"), ("B", "A")], "[Types.using.xlsx]Types!B2", "자기 자신을 거쳐 돌아옵니다"),
        (&[("Bad", "float<3>")], "[Types.using.xlsx]Types!B2", "알 수 없는 자료형"),
        (&[("Key", "ID<int32>")], "[Types.using.xlsx]Types!B2", "별칭에는 키 역할"),
    ];
    for (aliases, location, message) in cases {
        let project = Project::new();
        project.aliases("Types.using.xlsx", aliases);
        project.schema("Items", &[("Id", "ID<int32>", "all")]);
        project.data("Items", row!["Id"], vec![row![1]]);
        assert_error(&project, location, message);
    }
}

#[test]
fn key_rules_apply_through_aliases() {
    let project = Project::new();
    project.aliases("Types.using.xlsx", &[("Ratio", "float"), ("Level", "int32=1")]);
    project.schema("Items", &[("Id", "ID<Ratio>", "all")]);
    project.data("Items", row!["Id"], vec![row![1]]);
    assert_error(&project, "[Items.schema.xlsx]Items!B2", "float 자료형은 기본키나 서브키로 사용할 수 없습니다");
    project.schema("Items", &[("Id", "ID<Level>", "all")]);
    assert_error(&project, "[Items.schema.xlsx]Items!B2", "기본키와 서브키에는 기본값을 지정할 수 없습니다");
}

#[test]
fn no_types_header_without_aliases() {
    let project = Project::new();
    project.schema("Items", &[("Id", "ID<int32>", "all")]);
    project.data("Items", row!["Id"], vec![row![1]]);
    assert_eq!(project.build(&[]).code, 0);
    assert!(!Path::new(&project.out.join("cpp/DrTypes.h")).exists());
}
