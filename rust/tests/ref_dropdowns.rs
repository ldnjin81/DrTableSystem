//! `drtable new --input`: reference fields get a dropdown of the target table's keys, read from
//! every data file at creation time. A key outside the list is only warned about.

#[macro_use]
mod common;

use common::*;
use std::path::{Path, PathBuf};

/// Tables/Schemas (Items, Quests) and Tables/Data with Items split over two files.
fn project(item_ids: &[i32]) -> (Tmp, PathBuf, PathBuf) {
    let tmp = Tmp::new();
    let schemas = tmp.join("Tables/Schemas");
    let data = tmp.join("Tables/Data");
    write_schema(&schemas.join("Items.schema.xlsx"), "Items", &fields(&[("Id", "ID<int32>", "all"), ("Group", "SubKey<int32>", "all")]));
    write_schema(
        &schemas.join("Quests.schema.xlsx"),
        "Quests",
        &fields(&[("Id", "ID<int32>", "all"), ("Reward", "Ref<Items>", "all"), ("Bonus", "Ref<Items>[]", "all"), ("Note", "string", "all"), ("Group", "Ref<Items.Group>", "all")]),
    );
    let (first, rest) = item_ids.split_at(item_ids.len() / 2);
    for (file, ids) in [("a.xlsx", first), ("nested/b.xlsx", rest)] {
        let mut rows = vec![row!["Id", "Group"], vec![], vec![]];
        rows.extend(ids.iter().map(|id| row![*id, *id / 100]));
        Book::new().with("Items", rows).save_plain(&data.join(file));
    }
    (tmp, schemas, data)
}

fn new(schemas: &Path, out: &Path, input: Option<&Path>) -> Output {
    let mut args = vec!["new".to_string(), "--table".into(), "Quests".into(), "--out".into(), s(out), "--schema".into(), s(schemas)];
    if let Some(input) = input {
        args.extend(["--input".into(), s(input)]);
    }
    drtable(args)
}

#[test]
fn reference_lists_keys_from_every_data_file() {
    let (tmp, schemas, data) = project(&[1001, 1002, 2001]);
    let out = tmp.join("Tables/Data/Quests.xlsx");
    let result = new(&schemas, &out, Some(&data));
    assert_eq!(result.code, 0, "{}", result.stderr);
    let sheet = zip_parts(&out, |name| name == "xl/worksheets/sheet1.xml").pop().unwrap();
    // Reward is column B; the list lives on the hidden #Lists sheet with an "accept anyway" style.
    assert!(sheet.contains("sqref=\"B4:B1048576\""), "{sheet}");
    assert!(sheet.contains("errorStyle=\"warning\""), "{sheet}");
    assert!(sheet.contains("'#Lists'!$A$1:$A$3"), "{sheet}");
    // Arrays (Bonus, column C) get no element dropdown.
    assert!(!sheet.contains("sqref=\"C4:C1048576\""), "{sheet}");
    // A sub key reference (Group, column E) lists the distinct values: 10 and 20.
    assert!(sheet.contains("sqref=\"E4:E1048576\""), "{sheet}");
    assert!(sheet.contains("'#Lists'!$B$1:$B$2"), "{sheet}");
    let lists = zip_parts(&out, |name| name == "xl/worksheets/sheet2.xml").pop().unwrap();
    for key in ["1001", "1002", "2001"] {
        // Numbers stay numbers so that a typed key matches.
        assert!(lists.contains(&format!("<v>{key}</v>")), "{lists}");
    }
}

#[test]
fn without_input_there_is_no_reference_list() {
    let (tmp, schemas, _) = project(&[1001, 1002]);
    let out = tmp.join("Quests.xlsx");
    assert_eq!(new(&schemas, &out, None).code, 0);
    let sheet = zip_parts(&out, |name| name == "xl/worksheets/sheet1.xml").pop().unwrap();
    assert!(!sheet.contains("type=\"list\""), "{sheet}");
}

#[test]
fn large_targets_are_skipped_with_a_warning() {
    let ids: Vec<i32> = (1..=10_001).collect();
    let (tmp, schemas, data) = project(&ids);
    let out = tmp.join("Quests.xlsx");
    let result = new(&schemas, &out, Some(&data));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(result.stderr.contains("Items") && result.stderr.contains("10001"), "{}", result.stderr);
    let sheet = zip_parts(&out, |name| name == "xl/worksheets/sheet1.xml").pop().unwrap();
    assert!(!sheet.contains("sqref=\"B4:B1048576\""), "{sheet}");
    // The sub key target is small (101 groups), so Group keeps its list.
    assert!(sheet.contains("sqref=\"E4:E1048576\""), "{sheet}");
}

#[test]
fn data_errors_stop_new() {
    let (tmp, schemas, data) = project(&[1001]);
    let mut rows = vec![row!["Id", "Group"], vec![], vec![]];
    rows.push(row!["oops", 1]);
    Book::new().with("Items", rows).save_plain(&data.join("broken.xlsx"));
    let out = tmp.join("Quests.xlsx");
    assert_eq!(new(&schemas, &out, Some(&data)).code, 1);
    assert!(!out.exists());
}
