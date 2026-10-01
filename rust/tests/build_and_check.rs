//! Build and check: outputs, scopes, arrays, defaults, key types, validation and sorting.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::Path;

fn add_enum(book: &mut Book) {
    book.add("<enum>Element", vec![
        row!["Id", "Value", "Comment"],
        row!["ID<name>", "int32", "string"],
        row!["all", "all", "#"],
        row!["Fire", 0, "불"],
        row!["Water", (), "물"],
    ]);
}

fn add_table(book: &mut Book) -> &mut Sheet {
    book.add("Effects", vec![
        row!["Id", "Name", "Element", "ClientOnly", "ServerOnly", "Memo", "Reward[1]", "Reward[0]"],
        row!["ID<int32>", "SubKey<name>", "SubKey<EElement>", "float", "int64", "설명", "int32", "int32"],
        row!["all", "all", "all", "client", "server", "#", "all", "all"],
        row![1001, "Burn", "Fire", 12.5, 99, "무시", 20, 10],
        row![1002, "Freeze", "Water", (), 100, "무시", (), 30],
    ])
}

fn valid_book() -> Book {
    let mut book = Book::new();
    add_enum(&mut book);
    add_table(&mut book);
    book.add("#메모", vec![row!["이 시트는 형식과 무관하게 무시된다"]]);
    book
}

fn save_valid(path: &Path) {
    valid_book().save(path);
}

/// Primary keys in descending order, and an enum whose name order (Water < Zeta) differs
/// from its value order (Zeta = 0, Water = 1).
fn save_unsorted(path: &Path) {
    Book::new()
        .with("<enum>Element", vec![
            row!["Id", "Value", "Comment"],
            row!["ID<name>", "int32", "string"],
            row!["all", "all", "#"],
            row!["Zeta", 0, "이름은 뒤지만 값이 앞"],
            row!["Water", 1, "물"],
        ])
        .with("Effects", vec![
            row!["Id", "Name", "Element", "ServerOnly"],
            row!["ID<int32>", "SubKey<name>", "SubKey<EElement>", "int32"],
            row!["all", "all", "all", "server"],
            row![1003, "Curse", "Water", 3],
            row![1001, "Burn", "Zeta", 1],
            row![1002, "Freeze", "Water", 2],
        ])
        .save(path);
}

fn one_sheet(name: &str, rows: Vec<Row>) -> Book {
    Book::new().with(name, rows)
}

fn check(source: &Path) -> Output {
    drtable(["check", "--input", &s(source)])
}

fn assert_no_outputs(root: &Path) {
    for folder in ["cpp", "client", "server"] {
        assert!(!root.join(folder).exists(), "{folder} was written");
    }
}

#[test]
fn build_outputs_scope_array_and_determinism() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let first = files(&tmp);
    let cpp = tmp.join("cpp");
    assert_eq!(names(&cpp), ["EDrElement.h", "DrEffectsRow.h", "DrEffectsTable.h", "DrGeneratedTables.h"]
        .into_iter().map(String::from).collect());
    let header = read(&cpp.join("DrEffectsRow.h"));
    assert!(read(&cpp.join("EDrElement.h")).contains("Fire = 0, // 불"));
    for line in [
        "int32 Id = 0;",
        "FName Name;",
        "EDrElement Element = EDrElement::Fire;",
        "float ClientOnly = 0.0f;",
        "int32 Reward[2] = {};",
    ] {
        assert!(header.contains(line), "{line}");
    }
    assert!(!header.contains("ServerOnly"));
    let client = json(&tmp.join("client/Effects.json"));
    let server = json(&tmp.join("server/Effects.json"));
    assert_eq!(client["rows"][0]["Reward"], json!([10, 20]));
    assert_eq!(client["rows"][1]["Reward"], json!([30, 0]));
    let client_row = client["rows"][0].as_object().unwrap();
    let server_row = server["rows"][0].as_object().unwrap();
    assert!(client_row.contains_key("ClientOnly") && !client_row.contains_key("ServerOnly"));
    assert!(server_row.contains_key("ServerOnly") && !server_row.contains_key("ClientOnly"));
    assert!(!client_row.contains_key("Memo"));
    let fields: Vec<_> = client["sub_keys"].as_array().unwrap().iter().map(|item| item["field"].clone()).collect();
    assert_eq!(fields, [json!("Name"), json!("Element")]);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    assert_eq!(files(&tmp), first);
}

#[test]
fn cpp_array_property_is_not_exposed_to_blueprint() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    assert_eq!(build(&source, &tmp.join("output"), &[]).code, 0);
    let header = read(&tmp.join("output/cpp/DrEffectsRow.h"));
    assert!(header.contains("    UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = \"Dr|Effects\")\n    int32 Id = 0;"));
    assert!(header.contains("    UPROPERTY(EditAnywhere, Category = \"Dr|Effects\")\n    int32 Reward[2] = {};"));
}

#[test]
fn enum_info_table_and_stamp() {
    // The compact enum sheet's extra columns become an ItemTypeInfo table (schema and data file).
    let tmp = Tmp::new();
    one_sheet("<enum>ItemType", vec![
        row!["Id", "Value", "Comment", "DisplayName", "MaxStack"],
        row!["ID<name>", "int32", "string", "string", "int32"],
        row!["all", "all", "#", "client", "server"],
        row!["Weapon", 0, "무기류", "무기", 1],
    ])
    .save(&tmp.join("enum-info.xlsx"));
    let input = tmp.to_path_buf();
    assert_eq!(build(&input, &tmp, &["--stamp", "2026-09-17T00:00:00Z"]).code, 0);
    let row_header = read(&tmp.join("cpp/DrItemTypeInfoRow.h"));
    assert!(row_header.contains("EDrItemType Id = EDrItemType::Weapon;"));
    assert!(!row_header.contains("Value") && !row_header.contains("Comment"));
    assert_eq!(json(&tmp.join("client/ItemTypeInfo.json"))["rows"], json!([{"Id": "Weapon", "DisplayName": "무기"}]));
    assert_eq!(json(&tmp.join("server/ItemTypeInfo.json"))["rows"], json!([{"Id": "Weapon", "MaxStack": 1}]));
    assert_eq!(json(&tmp.join("client/manifest.json"))["generated_at"], "2026-09-17T00:00:00Z");
}

#[test]
fn cpp_scalar_initializers() {
    let tmp = Tmp::new();
    let source = tmp.join("initializers.xlsx");
    one_sheet("Numbers", vec![
        row!["Id", "Big", "Ratio", "Precise", "Enabled", "Label"],
        row!["ID<int32>", "int64", "float", "double", "bool", "string"],
        row!["all", "all", "all", "all", "all", "all"],
        row![1, 2, 3.5, 4.5, true, "값"],
    ])
    .save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let header = read(&tmp.join("cpp/DrNumbersRow.h"));
    for line in ["int32 Id = 0;", "int64 Big = 0;", "float Ratio = 0.0f;", "double Precise = 0.0;",
                 "bool Enabled = false;", "FString Label;"] {
        assert!(header.contains(line), "{line}");
    }
}

#[test]
fn column_defaults_apply_to_cpp_and_empty_cells_deterministically() {
    let tmp = Tmp::new();
    let mut book = Book::new();
    add_enum(&mut book);
    book.add("Defaults", vec![
        row!["Id", "Multiplier", "Count", "Enabled", "Label", "Name", "DisplayName", "StateTag", "Icon",
             "Element", "Reward[0]", "Reward[1]"],
        row!["ID<int32>", "float=1.0", "int32=7", "bool=true", "string=기본값", "name=Fallback", "text=표시값",
             "tag=State.Default", "path=/Game/UI/T_Default.T_Default", "EElement=Water", "int32=10", "int32=20"],
        vec![V::from("all"); 12],
        row![1, (), (), (), (), (), (), (), (), (), (), ()],
        row![2, 2.5, 3, false, "직접값", "Direct", "직접 표시", "State.Direct", "/Game/UI/T_Direct.T_Direct",
             "Fire", 30, 40],
    ]);
    let source = tmp.join("defaults.xlsx");
    book.save(&source);
    let output = tmp.join("output");
    assert_eq!(build(&source, &output, &[]).code, 0);
    let first = files(&output);
    let header = read(&output.join("cpp/DrDefaultsRow.h"));
    for line in [
        "float Multiplier = 1.0f;",
        "int32 Count = 7;",
        "bool Enabled = true;",
        "FString Label = FString(TEXT(\"기본값\"));",
        "FName Name = FName(TEXT(\"Fallback\"));",
        "FText DisplayName = FText::FromString(TEXT(\"표시값\"));",
        "FGameplayTag StateTag = FGameplayTag::RequestGameplayTag(FName(TEXT(\"State.Default\")), false);",
        "FSoftObjectPath Icon = FSoftObjectPath(TEXT(\"/Game/UI/T_Default.T_Default\"));",
        "EDrElement Element = EDrElement::Water;",
        "int32 Reward[2] = {10, 20};",
    ] {
        assert!(header.contains(line), "{line}");
    }
    let expected = json!({
        "Id": 1, "Multiplier": 1.0, "Count": 7, "Enabled": true, "Label": "기본값", "Name": "Fallback",
        "DisplayName": "표시값", "StateTag": "State.Default", "Icon": "/Game/UI/T_Default.T_Default",
        "Element": "Water", "Reward": [10, 20],
    });
    assert_eq!(json(&output.join("client/Defaults.json"))["rows"][0], expected);
    assert_eq!(json(&output.join("server/Defaults.json"))["rows"][0], expected);
    assert_eq!(build(&source, &output, &[]).code, 0);
    assert_eq!(files(&output), first);
}

#[test]
fn key_defaults_are_rejected_without_outputs() {
    for key_type in ["ID<int32>=1", "SubKey<name>=Fallback"] {
        let tmp = Tmp::new();
        let is_id = key_type.starts_with("ID");
        let types = if is_id { row![key_type, "string"] } else { row!["ID<int32>", key_type] };
        let source = tmp.join("key-default.xlsx");
        one_sheet("KeyDefault", vec![row!["Id", "Lookup"], types, row!["all", "all"], row![1, "값"]]).save(&source);
        let output = tmp.join("output");
        let result = build(&source, &output, &[]);
        assert_eq!(result.code, 1, "{key_type}");
        let cell = if is_id { "B2" } else { "B3" };
        assert!(result.stderr.contains(&format!("[KeyDefault.schema.xlsx]KeyDefault!{cell}")), "{}", result.stderr);
        assert!(result.stderr.contains("기본키와 서브키에는 기본값을 지정할 수 없습니다"));
        assert_no_outputs(&output);
    }
}

#[test]
fn invalid_column_default_is_rejected_without_outputs() {
    let tmp = Tmp::new();
    let source = tmp.join("invalid-default.xlsx");
    one_sheet("InvalidDefault", vec![
        row!["Id", "Multiplier"], row!["ID<int32>", "float=not-a-number"], row!["all", "all"], row![1, ()],
    ])
    .save(&source);
    let output = tmp.join("output");
    let result = build(&source, &output, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[InvalidDefault.schema.xlsx]InvalidDefault!B3"));
    assert!(result.stderr.contains("float 자료형으로 변환할 수 없습니다"));
    assert_no_outputs(&output);
}

#[test]
fn semantic_string_types_generate_cpp_json_and_conditional_includes() {
    let tmp = Tmp::new();
    let source = tmp.join("types.xlsx");
    one_sheet("Types", vec![
        row!["Id", "Label", "DisplayName", "StateTag", "Icon", "Count", "Enabled"],
        row!["ID<name>", "string", "text", "tag", "path", "int32", "bool"],
        row!["all", "all", "all", "all", "all", "all", "all"],
        row!["Effect.Burn", "Burn", "화상", "State.Debuff.Burn", "/Game/UI/T_Burn.T_Burn", 3, true],
    ])
    .save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let first = files(&tmp);
    let header = read(&tmp.join("cpp/DrTypesRow.h"));
    for line in [
        "FName Id;",
        "FString Label;",
        "FText DisplayName;",
        "FGameplayTag StateTag;",
        "FSoftObjectPath Icon;",
        "#include \"Internationalization/Text.h\"",
        "#include \"GameplayTagContainer.h\"",
        "#include \"UObject/SoftObjectPath.h\"",
    ] {
        assert!(header.contains(line), "{line}");
    }
    let expected = json!([{
        "Id": "Effect.Burn", "Label": "Burn", "DisplayName": "화상", "StateTag": "State.Debuff.Burn",
        "Icon": "/Game/UI/T_Burn.T_Burn", "Count": 3, "Enabled": true,
    }]);
    assert_eq!(json(&tmp.join("client/Types.json"))["rows"], expected);
    assert_eq!(json(&tmp.join("server/Types.json"))["rows"], expected);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    assert_eq!(files(&tmp), first);
}

#[test]
fn type_specific_includes_are_omitted_when_unused() {
    let tmp = Tmp::new();
    let source = tmp.join("plain.xlsx");
    one_sheet("Plain", vec![row!["Id", "Label"], row!["ID<int32>", "string"], row!["all", "all"], row![1, "값"]])
        .save(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let header = read(&tmp.join("cpp/DrPlainRow.h"));
    for include in ["Internationalization/Text.h", "GameplayTagContainer.h", "UObject/SoftObjectPath.h"] {
        assert!(!header.contains(include), "{include}");
    }
}

#[test]
fn unsupported_key_types_are_rejected_without_outputs() {
    for role in ["ID", "SubKey"] {
        for value_type in ["string", "tag", "path", "float", "double", "bool"] {
            let tmp = Tmp::new();
            let is_id = role == "ID";
            let types = if is_id {
                row![format!("ID<{value_type}>"), "string"]
            } else {
                row!["ID<int32>", format!("SubKey<{value_type}>")]
            };
            let key = if is_id { V::from("Key") } else { V::from(1) };
            let source = tmp.join(format!("invalid-{}-{value_type}.xlsx", role.to_lowercase()));
            one_sheet("InvalidKey", vec![row!["Id", "Value"], types, row!["all", "all"], vec![key, V::from("Value")]])
                .save(&source);
            let output = tmp.join("output");
            let result = build(&source, &output, &[]);
            assert_eq!(result.code, 1, "{role}<{value_type}>");
            let cell = if is_id { "B2" } else { "B3" };
            assert!(result.stderr.contains(&format!("[InvalidKey.schema.xlsx]InvalidKey!{cell}")), "{}", result.stderr);
            assert!(result.stderr.contains(&format!("{value_type} 자료형은 기본키나 서브키")));
            assert!(result.stderr.contains("int32, int64, name, 열거형(E*)"));
            assert_no_outputs(&output);
        }
    }
}

#[test]
fn supported_key_types_remain_available() {
    let tmp = Tmp::new();
    let mut book = Book::new();
    add_enum(&mut book);
    book.add("SupportedKeys", vec![
        row!["Id", "Numeric", "Name", "Element"],
        row!["ID<int64>", "SubKey<int32>", "SubKey<name>", "SubKey<EElement>"],
        row!["all", "all", "all", "all"],
        row![9_000_000_001i64, 7, "Burn", "Fire"],
    ]);
    let source = tmp.join("supported-keys.xlsx");
    book.save(&source);
    assert_eq!(build(&source, &tmp.join("output"), &[]).code, 0);
}

#[test]
fn text_cannot_be_used_as_key() {
    for key_type in ["ID<text>", "SubKey<text>"] {
        let tmp = Tmp::new();
        let source = tmp.join("text-key.xlsx");
        one_sheet("TextKey", vec![
            row!["Id", "Localized"], row!["ID<int32>", key_type], row!["all", "all"], row![1, "지역화 값"],
        ])
        .save(&source);
        let result = check(&source);
        assert_eq!(result.code, 1);
        assert!(result.stderr.contains("[TextKey.schema.xlsx]TextKey!B3"), "{}", result.stderr);
        assert!(result.stderr.contains("text 자료형은 기본키나 서브키"));
        assert!(result.stderr.contains("int32, int64, name, 열거형(E*)"));
    }
}

#[test]
fn typed_path_syntax_is_rejected() {
    let tmp = Tmp::new();
    let source = tmp.join("typed-path.xlsx");
    one_sheet("TypedPath", vec![
        row!["Id", "Icon"], row!["ID<int32>", "path<UTexture2D>"], row!["all", "all"], row![1, "/Game/UI/T_Icon.T_Icon"],
    ])
    .save(&source);
    let result = check(&source);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[TypedPath.schema.xlsx]TypedPath!B3"));
    assert!(result.stderr.contains("알 수 없는 자료형"));
}

#[test]
fn manifest_has_no_stamp_by_default() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let manifest = json(&tmp.join("client/manifest.json"));
    assert!(manifest.get("generated_at").is_none());
    assert!(manifest.get("generated_at_utc").is_none());
    assert_eq!(manifest["source_files"], json!(["Tables.xlsx"]));
}

fn items_without_sub_keys(path: &Path) {
    one_sheet("Items", vec![row!["Id", "Name"], row!["ID<name>", "string"], row!["all", "all"], row!["Sword", "검"]])
        .save(path);
}

#[test]
fn sub_keys_can_be_empty() {
    let tmp = Tmp::new();
    let source = tmp.join("input.xlsx");
    items_without_sub_keys(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    assert_eq!(json(&tmp.join("client/Items.json"))["sub_keys"], json!([]));
}

#[test]
fn primary_key_scope_must_be_all() {
    let tmp = Tmp::new();
    let mut book = Book::new();
    add_enum(&mut book);
    add_table(&mut book).set("A3", "client");
    let source = tmp.join("bad-primary-scope.xlsx");
    book.save(&source);
    let result = check(&source);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Effects.schema.xlsx]Effects!C2"));
    assert!(result.stderr.contains("기본키 범위는 all"));
}

#[test]
fn check_does_not_write_files() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    let before = files(&tmp);
    assert_eq!(check(&source).code, 0);
    assert_eq!(files(&tmp), before);
}

fn assert_effects_error(cell: &str, value: V, location: &str, message: &str) {
    let tmp = Tmp::new();
    let mut book = Book::new();
    add_enum(&mut book);
    add_table(&mut book).set(cell, value);
    let source = tmp.join("bad.xlsx");
    book.save(&source);
    let result = check(&source);
    assert_eq!(result.code, 1, "{cell}");
    assert!(result.stderr.contains(location), "{cell}: {}", result.stderr);
    assert!(result.stderr.contains(message), "{cell}: {}", result.stderr);
}

#[test]
fn validation_errors() {
    let cases: [(&str, V, &str, &str); 7] = [
        ("A2", V::from("int32"), "[Effects.schema.xlsx]Effects!B2", "기본키"),
        ("B2", V::from("ID<name>"), "[Effects.schema.xlsx]Effects!B2", "기본키"),
        ("A5", V::from(1001), "Effects!A5", "중복"),
        ("A5", V::E, "Effects!A5", "비어"),
        ("C2", V::from("EMissing"), "[Effects.schema.xlsx]Effects!B4", "정의되지 않은"),
        ("D4", V::from("숫자 아님"), "Effects!D4", "변환"),
        ("B1", V::from("Id"), "[Effects.schema.xlsx]Effects!A3", "중복"),
    ];
    for (cell, value, location, message) in cases {
        assert_effects_error(cell, value, location, message);
    }
}

#[test]
fn array_validation_errors() {
    let cases: [(&str, &str, &str, &str); 4] = [
        ("G1", "Reward[2]", "[Effects.schema.xlsx]Effects!A8", "연속"),
        ("G1", "Reward[0]", "[Effects.schema.xlsx]Effects!A9", "중복"),
        ("G2", "float", "[Effects.schema.xlsx]Effects!B8", "자료형"),
        ("G3", "client", "[Effects.schema.xlsx]Effects!C8", "범위"),
    ];
    for (cell, value, location, message) in cases {
        assert_effects_error(cell, V::from(value), location, message);
    }
}

#[test]
fn asset_class_header_is_generated() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let header = read(&tmp.join("cpp/DrEffectsTable.h"));
    for line in [
        "class UDrEffectsTable : public UPrimaryDataAsset",
        "#include \"Engine/DataAsset.h\"",
        "#include \"DrEffectsRow.h\"",
        "#include \"EDrElement.h\"",
        "TArray<FDrEffectsRow> Rows;",
        "TArray<int32> PrimaryKeys;",
        "TArray<FName> Name_Keys;",
        "TArray<int32> Name_Offsets;",
        "TArray<int32> Name_Indices;",
        "TArray<EDrElement> Element_Keys;",
    ] {
        assert!(header.contains(line), "{line}");
    }
}

#[test]
fn asset_class_without_sub_keys_has_no_index_arrays() {
    let tmp = Tmp::new();
    let source = tmp.join("items.xlsx");
    items_without_sub_keys(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let header = read(&tmp.join("cpp/DrItemsTable.h"));
    assert!(header.contains("TArray<FName> PrimaryKeys;"));
    for part in ["_Keys;", "_Offsets;", "_Indices;"] {
        assert!(!header.contains(part), "{part}");
    }
}

#[test]
fn asset_base_option_uses_given_class_and_header() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    let extra = ["--asset-base", "UDrTableAsset", "--asset-base-header", "TableData/DrTableAsset.h"];
    assert_eq!(build(&source, &tmp, &extra).code, 0);
    let header = read(&tmp.join("cpp/DrEffectsTable.h"));
    assert!(header.contains("class UDrEffectsTable : public UDrTableAsset"));
    assert!(header.contains("#include \"TableData/DrTableAsset.h\""));
    assert!(!header.contains("#include \"Engine/DataAsset.h\""));
}

#[test]
fn asset_base_without_header_is_usage_error() {
    let tmp = Tmp::new();
    let source = tmp.join("Tables.xlsx");
    save_valid(&source);
    let result = build(&source, &tmp, &["--asset-base", "UDrTableAsset"]);
    assert!(result.is_usage_error(), "{}", result.stderr);
    assert!(result.stderr.contains("--asset-base-header"));
}

#[test]
fn client_rows_are_sorted_by_primary_key() {
    let tmp = Tmp::new();
    let source = tmp.join("unsorted.xlsx");
    save_unsorted(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let ids = |payload: &serde_json::Value| -> Vec<i64> {
        payload["rows"].as_array().unwrap().iter().map(|row| row["Id"].as_i64().unwrap()).collect()
    };
    let client = json(&tmp.join("client/Effects.json"));
    assert_eq!(ids(&client), [1001, 1002, 1003]);
    assert_eq!(client["primary_keys"], json!([1001, 1002, 1003]));
    // The server JSON uses the same row order (it only has no index).
    assert_eq!(ids(&json(&tmp.join("server/Effects.json"))), [1001, 1002, 1003]);
}

#[test]
fn client_csr_index_is_valid_and_enum_keys_sort_by_value() {
    let tmp = Tmp::new();
    let source = tmp.join("unsorted.xlsx");
    save_unsorted(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let client = json(&tmp.join("client/Effects.json"));
    let rows = client["rows"].as_array().unwrap();
    for entry in client["sub_keys"].as_array().unwrap() {
        let keys = entry["keys"].as_array().unwrap();
        let offsets: Vec<usize> = entry["offsets"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
        let indices = entry["indices"].as_array().unwrap();
        assert_eq!(offsets.len(), keys.len() + 1);
        assert_eq!(*offsets.last().unwrap(), indices.len());
        assert!(offsets.windows(2).all(|pair| pair[0] <= pair[1]));
        for (position, key) in keys.iter().enumerate() {
            let bucket = &indices[offsets[position]..offsets[position + 1]];
            assert!(!bucket.is_empty(), "empty bucket: {key}");
            for index in bucket {
                let field = entry["field"].as_str().unwrap();
                assert_eq!(&rows[index.as_u64().unwrap() as usize][field], key);
            }
        }
    }
    let by_name = |name: &str| {
        client["sub_keys"].as_array().unwrap().iter().find(|item| item["name"] == name).unwrap()["keys"].clone()
    };
    // Enum sub keys follow the values (Zeta = 0, Water = 1), not the names.
    assert_eq!(by_name("Element"), json!(["Zeta", "Water"]));
    assert_eq!(by_name("Name"), json!(["Burn", "Curse", "Freeze"]));
}

#[test]
fn server_json_has_no_index() {
    let tmp = Tmp::new();
    let source = tmp.join("unsorted.xlsx");
    save_unsorted(&source);
    assert_eq!(build(&source, &tmp, &[]).code, 0);
    let server = json(&tmp.join("server/Effects.json"));
    assert!(server.get("primary_keys").is_none());
    for entry in server["sub_keys"].as_array().unwrap() {
        let keys: Vec<&String> = entry.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 2);
        assert!(entry.get("name").is_some() && entry.get("field").is_some());
    }
}
