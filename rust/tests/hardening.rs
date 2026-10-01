//! Regression tests for the defects found in the 2026-09-30 review.

#[macro_use]
mod common;

use common::*;
use std::path::Path;

fn enum_sheet(values: &[(&str, i32)]) -> Vec<Row> {
    let mut rows = vec![row!["Id", "Value"], row!["ID<name>", "int32"], row!["all", "all"]];
    rows.extend(values.iter().map(|(name, value)| row![*name, *value]));
    rows
}

fn schema_hash(root: &Path, table: &str) -> String {
    json(&root.join("client").join(format!("{table}.json")))["schema_hash"].as_str().unwrap().to_string()
}

#[test]
fn enum_definition_changes_schema_hash() {
    let tmp = Tmp::new();
    for (run, values) in [("a", [("A", 0), ("B", 1)]), ("b", [("A", 1), ("B", 0)])] {
        let source = tmp.join(format!("{run}/in.xlsx"));
        Book::new()
            .with("<enum>Kind", enum_sheet(&values))
            .with("T", vec![row!["Id", "Kind"], row!["ID<int32>", "SubKey<EKind>"], row!["all", "all"], row![1, "A"], row![2, "B"]])
            .save(&source);
        assert_eq!(build(&source, &tmp.join(format!("{run}/out")), &[]).code, 0);
    }
    // Changing only enum values changes the baked index order, so the schema hash changes too.
    assert_ne!(schema_hash(&tmp.join("a/out"), "T"), schema_hash(&tmp.join("b/out"), "T"));
}

#[test]
fn unused_enum_does_not_change_schema_hash() {
    let tmp = Tmp::new();
    let mut hashes = Vec::new();
    for (run, values) in [("a", vec![("A", 0)]), ("b", vec![("A", 0), ("Z", 9)])] {
        let source = tmp.join(format!("{run}/in.xlsx"));
        Book::new()
            .with("<enum>Other", enum_sheet(&values))
            .with("T", vec![row!["Id"], row!["ID<int32>"], row!["all"], row![1]])
            .save(&source);
        let out = tmp.join(format!("{run}/out"));
        assert_eq!(build(&source, &out, &[]).code, 0);
        hashes.push(schema_hash(&out, "T"));
    }
    assert_eq!(hashes[0], hashes[1]);
}

#[test]
fn name_keys_differing_only_by_case_are_rejected() {
    let cases: [(Vec<&str>, Vec<Row>); 2] = [
        (vec!["ID<name>"], vec![row!["Sword"], row!["sword"]]),
        (vec!["ID<int32>", "SubKey<name>"], vec![row![1, "Fire"], row![2, "FIRE"]]),
    ];
    for (types, data) in cases {
        let tmp = Tmp::new();
        let headers = &["Id", "Group"][..types.len()];
        let mut rows = vec![
            headers.iter().map(|h| V::from(*h)).collect(),
            types.iter().map(|t| V::from(*t)).collect(),
            vec![V::from("all"); types.len()],
        ];
        rows.extend(data);
        let source = tmp.join("in.xlsx");
        Book::new().with("T", rows).save(&source);
        let result = build(&source, &tmp, &[]);
        assert_eq!(result.code, 1);
        assert!(result.stderr.contains("대소문자만 다릅니다"), "{}", result.stderr);
        assert!(result.stderr.starts_with("[in.xlsx]T!"), "{}", result.stderr);
    }
}

#[test]
fn same_subkey_value_repeated_is_fine() {
    let tmp = Tmp::new();
    let source = tmp.join("in.xlsx");
    Book::new()
        .with("T", vec![row!["Id", "Group"], row!["ID<int32>", "SubKey<name>"], row!["all", "all"], row![1, "Fire"], row![2, "Fire"]])
        .save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
}

#[test]
fn content_hash_tracks_values_and_manifest_records_naming() {
    let tmp = Tmp::new();
    let mut hashes = Vec::new();
    for (run, value) in [("a", 10), ("b", 11)] {
        let source = tmp.join(format!("{run}/in.xlsx"));
        Book::new()
            .with("T", vec![row!["Id", "Power"], row!["ID<int32>", "int32"], row!["all", "all"], row![1, value]])
            .save(&source);
        let out = tmp.join(format!("{run}/out"));
        assert_eq!(build(&source, &out, &["--prefix", "Gm", "--asset-name", "BT_{table}"]).code, 0);
        let payload = json(&out.join("client/T.json"));
        let manifest = json(&out.join("client/manifest.json"));
        assert_eq!(manifest["cpp_prefix"], "Gm");
        assert_eq!(manifest["asset_name"], "BT_{table}");
        assert_eq!(manifest["tables"][0]["content_hash"], payload["content_hash"]);
        // The content hash stays out of the code (data edits do not change the code).
        assert!(!read(&out.join("cpp/GmGeneratedTables.h")).contains("ContentHash"));
        hashes.push((payload["schema_hash"].clone(), payload["content_hash"].clone()));
    }
    assert_eq!(hashes[0].0, hashes[1].0, "same structure");
    assert_ne!(hashes[0].1, hashes[1].1, "values changed");
}

#[test]
fn ue_plugin_preset() {
    let tmp = Tmp::new();
    let source = tmp.join("in.xlsx");
    Book::new().with("T", vec![row!["Id"], row!["ID<int32>"], row!["all"], row![1]]).save(&source);
    assert_eq!(build(&source, &tmp, &["--ue-plugin", "--prefix", "Gm"]).code, 0);
    let table = read(&tmp.join("cpp/GmTTable.h"));
    assert!(table.contains("#include \"DrTableAssetBase.h\""));
    assert!(table.contains("class UGmTTable : public UDrTableAssetBase"));
    assert!(read(&tmp.join("cpp/GmTRow.cpp")).contains("#include \"DrTableRuntime.h\""));
    assert!(tmp.join("cpp/GmTableRegistration.h").exists());
}
