//! Fixed-point types: `fixed<N>` (int32) and `fixed64<N>` (int64) hold decimals as integer
//! counts of 1/N, so clients and servers compute with exact integers.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::Path;

/// Bonus is an array of types[1], written as two element columns.
fn source(path: &Path, types: [&str; 3], data: Vec<Row>) {
    let bonus = format!("{}[]", types[1]);
    let mut rows = vec![
        row!["Id", "CritRate", "Bonus[0]", "Bonus[1]", "Gold"],
        row!["ID<int32>", types[0], bonus.as_str(), bonus.as_str(), types[2]],
        row!["all", "all", "all", "all", "all"],
    ];
    rows.extend(data);
    Book::new().with("Items", rows).save(path);
}

fn default_types() -> [&'static str; 3] {
    ["fixed<10000>=0.05", "fixed<100>", "fixed64<1000000>"]
}

#[test]
fn decimals_become_scaled_integers() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path, default_types(), vec![
        row![1, 0.1234, 1.25, (), 123456.789012],
        row![2, "12.34%", "-0.5", 2, 0],
        row![3, (), (), (), ()],
    ]);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let rows = json(&tmp.join("server/Items.json"))["rows"].clone();
    assert_eq!(rows[0], json!({"Id": 1, "CritRate": 1234, "Bonus": [125], "Gold": 123456789012i64}));
    assert_eq!(rows[1], json!({"Id": 2, "CritRate": 1234, "Bonus": [-50, 200], "Gold": 0}));
    // The declared default is scaled once (0.05 -> 500); an empty array has no elements.
    assert_eq!(rows[2], json!({"Id": 3, "CritRate": 500, "Bonus": [], "Gold": 0}));
    // The client gets the same exact integers, the array elements in its pool.
    let client = json(&tmp.join("client/Items.json"));
    assert_eq!(client["arrays"], json!([{"field": "Bonus", "pool": [125, -50, 200]}]));
    assert_eq!(client["rows"][2], json!({"Id": 3, "CritRate": 500, "Bonus_Start": 3, "Bonus_Num": 0, "Gold": 0}));

    let header = read(&tmp.join("cpp/DrItemsRow.h"));
    for part in [
        "    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = \"Dr|Items\", meta = (DrFixedScale = \"10000\"))\n    int32 CritRate = 500;\n    static constexpr int32 CritRateScale = 10000;",
        "    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = \"Dr|Items\", meta = (DrFixedScale = \"100\", DrArray = \"Bonus\"))\n    int32 Bonus_Start = 0;\n\n    UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = \"Dr|Items\", meta = (DrArray = \"Bonus\"))\n    int32 Bonus_Num = 0;\n    static constexpr int32 BonusScale = 100;",
        "    int64 Gold = 0;\n    static constexpr int64 GoldScale = 1000000;",
    ] {
        assert!(header.contains(part), "{part}\n---\n{header}");
    }
}

#[test]
fn values_finer_than_the_scale_are_errors() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path, default_types(), vec![row![1, 0.12345, (), (), ()]]);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[in.xlsx]Items!B4: '0.12345' 값은 fixed<10000>로 정확히 나타낼 수 없습니다(소수 4자리까지)"), "{}", result.stderr);
}

#[test]
fn values_out_of_range_are_errors() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path, default_types(), vec![row![1, 300000, (), (), ()]]);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[in.xlsx]Items!B4: '300000' 값이 fixed<10000> 범위(±214748)를 넘습니다"), "{}", result.stderr);
}

#[test]
fn non_numbers_are_errors() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path, default_types(), vec![row![1, "abc", (), (), ()]]);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[in.xlsx]Items!B4: 'abc' 값을 fixed<10000> 자료형으로 변환할 수 없습니다"), "{}", result.stderr);
}

#[test]
fn scales_must_be_powers_of_ten() {
    for bad in ["fixed<1234>", "fixed<1>", "fixed<10000000000>"] {
        let tmp = Tmp::new();
        let path = tmp.join("in.xlsx");
        source(&path, [bad, "fixed<100>", "fixed64<100>"], vec![row![1, 0, 0, 0, 0]]);
        let result = build(&path, &tmp, &[]);
        assert_eq!(result.code, 1, "{bad}");
        assert!(result.stderr.contains("[Items.schema.xlsx]Items!B3"), "{bad}: {}", result.stderr);
        assert!(result.stderr.contains("10의 거듭제곱"), "{bad}: {}", result.stderr);
    }
}

#[test]
fn fixed_point_cannot_be_a_key() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    Book::new()
        .with("Items", vec![row!["Id", "Rate"], row!["ID<int32>", "SubKey<fixed<100>>"], row!["all", "all"], row![1, 0.5]])
        .save(&path);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[Items.schema.xlsx]Items!B3"), "{}", result.stderr);
}

#[test]
fn scale_constant_clashing_with_a_field_is_an_error() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    Book::new()
        .with("Items", vec![
            row!["Id", "CritRate", "CritRateScale"],
            row!["ID<int32>", "fixed<10000>", "int32"],
            row!["all", "all", "all"],
            row![1, 0.5, 3],
        ])
        .save(&path);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("배율 상수 'CritRateScale'이 같은 이름의 필드와 겹칩니다"), "{}", result.stderr);
    assert!(!tmp.join("cpp").exists());
}

#[test]
fn changing_the_scale_changes_the_schema_hash() {
    let tmp = Tmp::new();
    let mut hashes = Vec::new();
    for (run, scale) in [("a", "fixed<100>"), ("b", "fixed<1000>")] {
        let path = tmp.join(format!("{run}/in.xlsx"));
        source(&path, [scale, "fixed<100>", "fixed64<100>"], vec![row![1, 0.5, 0, 0, 0]]);
        let out = tmp.join(format!("{run}/out"));
        assert_eq!(build(&path, &out, &[]).code, 0);
        hashes.push(json(&out.join("client/Items.json"))["schema_hash"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
}
