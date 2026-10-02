//! Time types: `datetime` (`datetime<+09:00>` reads Excel values in that offset) and `duration`.
//! JSON holds milliseconds (Unix time for datetime); C++ FDateTime / FTimespan.

#[macro_use]
mod common;

use common::*;
use serde_json::json;
use std::path::Path;

/// Milliseconds since 1970-01-01 UTC of a wall-clock time at a UTC offset in hours.
fn unix_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32, offset_hours: i64) -> i64 {
    let naive = chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap().and_hms_opt(hour, minute, 0).unwrap();
    naive.and_utc().timestamp_millis() - offset_hours * 3_600_000
}

fn source(path: &Path, types: [&str; 3], data: Vec<Row>) {
    let mut rows = vec![
        row!["Id", "Start", "Local", "Cooldown"],
        row!["ID<int32>", types[0], types[1], types[2]],
        row!["all", "all", "all", "all"],
    ];
    rows.extend(data);
    Book::new().with("Events", rows).save(path);
}

const TYPES: [&str; 3] = ["datetime", "datetime<+09:00>", "duration=30s"];

fn values(tmp: &Path) -> serde_json::Value {
    json(&tmp.join("client/Events.json"))["rows"].clone()
}

#[test]
fn excel_cells_and_text_become_milliseconds() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path, TYPES, vec![
        vec![V::from(1), V::Date(2026, 10, 1, 10, 0, 0.0), V::Date(2026, 10, 1, 10, 0, 0.0), V::Time(1.5 / 24.0)],
        row![2, "2026-10-01 10:00", "2026-10-01 10:00", "1h30m"],
        row![3, "2026/10/01", "2026-10-01T10:00:00Z", "90s"],
        row![4, (), "2026-10-01 10:00+00:00", ()],
    ]);
    let result = build(&path, &tmp, &[]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let rows = values(&tmp);
    let utc = unix_ms(2026, 10, 1, 10, 0, 0);
    let kst = unix_ms(2026, 10, 1, 10, 0, 9);
    assert_eq!(rows[0], json!({"Id": 1, "Start": utc, "Local": kst, "Cooldown": 5_400_000}));
    assert_eq!(rows[1], json!({"Id": 2, "Start": utc, "Local": kst, "Cooldown": 5_400_000}));
    // A written offset (Z, +00:00) wins over the field's offset.
    assert_eq!(rows[2], json!({"Id": 3, "Start": unix_ms(2026, 10, 1, 0, 0, 0), "Local": utc, "Cooldown": 90_000}));
    assert_eq!(rows[3], json!({"Id": 4, "Start": 0, "Local": utc, "Cooldown": 30_000}));
    assert_eq!(json(&tmp.join("server/Events.json"))["rows"], rows);
}

#[test]
fn duration_forms() {
    let cases: [(V, i64); 9] = [
        (V::from("1:30:00"), 5_400_000),
        (V::from("1:30"), 5_400_000),
        (V::from("0:00:01.5"), 1_500),
        (V::from("2d"), 172_800_000),
        (V::from("1h 30m 15s"), 5_415_000),
        (V::from("500ms"), 500),
        (V::from(30), 30_000),
        (V::from(1.5), 1_500),
        (V::Time(36.0 / 24.0), 129_600_000),
    ];
    for (value, expected) in cases {
        let tmp = Tmp::new();
        let path = tmp.join("in.xlsx");
        source(&path, TYPES, vec![vec![V::from(1), V::E, V::E, value.clone()]]);
        let result = build(&path, &tmp, &[]);
        assert_eq!(result.code, 0, "{value:?}: {}", result.stderr);
        assert_eq!(values(&tmp)[0]["Cooldown"], expected, "{value:?}");
    }
}

#[test]
fn unreadable_values_are_errors() {
    let cases = [
        (row![1, "next week", (), ()], "B4", "날짜·시각으로 읽을 수 없습니다"),
        (row![1, 45000, (), ()], "B4", "날짜·시각으로 읽을 수 없습니다"),
        (row![1, (), "2026-13-01", ()], "C4", "날짜·시각으로 읽을 수 없습니다"),
        (row![1, (), (), "soon"], "D4", "시간 길이로 읽을 수 없습니다"),
        (row![1, (), (), "-5s"], "D4", "시간 길이로 읽을 수 없습니다"),
        (row![1, (), (), "5x"], "D4", "시간 길이로 읽을 수 없습니다"),
    ];
    for (data, cell, message) in cases {
        let tmp = Tmp::new();
        let path = tmp.join("in.xlsx");
        source(&path, TYPES, vec![data]);
        let result = build(&path, &tmp, &[]);
        assert_eq!(result.code, 1, "{message}");
        assert!(result.stderr.contains(&format!("[in.xlsx]Events!{cell}: ")), "{cell}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{message}: {}", result.stderr);
    }
}

#[test]
fn schema_errors() {
    let cases = [
        (["datetime<+15:00>", "datetime", "duration"], "B3", "시간대는 -14:00 ~ +14:00"),
        (["ID<datetime>", "datetime", "duration"], "B2", "datetime"),
    ];
    for (types, cell, message) in cases {
        let tmp = Tmp::new();
        let path = tmp.join("in.xlsx");
        let mut rows = vec![row!["Start", "Local", "Cooldown"], types.iter().map(|t| V::from(*t)).collect(), row!["all", "all", "all"]];
        rows.push(row!["2026-01-01", (), ()]);
        if types[0].starts_with("ID<") {
            Book::new().with("Events", rows).save(&path);
        } else {
            source(&path, types, vec![row![1, (), (), ()]]);
        }
        let result = build(&path, &tmp, &[]);
        assert_eq!(result.code, 1, "{message}");
        assert!(result.stderr.contains(&format!("[Events.schema.xlsx]Events!{cell}")), "{}", result.stderr);
        assert!(result.stderr.contains(message), "{message}: {}", result.stderr);
    }
}

#[test]
fn cpp_uses_unreal_time_types() {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    source(&path, ["datetime=2026-01-01", "datetime<-05:30>", "duration=30s"], vec![row![1, (), (), ()]]);
    assert_eq!(build(&path, &tmp, &[]).code, 0);
    let header = read(&tmp.join("cpp/DrEventsRow.h"));
    let new_year = 621_355_968_000_000_000 + unix_ms(2026, 1, 1, 0, 0, 0) * 10_000;
    for part in [
        "#include \"Misc/DateTime.h\"".to_string(),
        "#include \"Misc/Timespan.h\"".to_string(),
        format!("    FDateTime Start = FDateTime({new_year});"),
        "meta = (DrTimeZone = \"-05:30\"))\n    FDateTime Local = FDateTime(621355968000000000);".to_string(),
        "    FTimespan Cooldown = FTimespan(300000000);".to_string(),
    ] {
        assert!(header.contains(&part), "{part}\n---\n{header}");
    }
    assert_eq!(values(&tmp)[0]["Start"], unix_ms(2026, 1, 1, 0, 0, 0));
}

/// One datetime column in the given type, one row per value.
fn zone_values(kind: &str, values: Vec<V>) -> (Tmp, Output) {
    let tmp = Tmp::new();
    let path = tmp.join("in.xlsx");
    let mut rows = vec![row!["Id", "At"], row!["ID<int32>", kind], row!["all", "all"]];
    for (index, value) in values.into_iter().enumerate() {
        rows.push(vec![V::from(index as i32 + 1), value]);
    }
    Book::new().with("Events", rows).save(&path);
    let result = build(&path, &tmp, &[]);
    (tmp, result)
}

fn at(tmp: &Path) -> Vec<i64> {
    values(tmp).as_array().unwrap().iter().map(|row| row["At"].as_i64().unwrap()).collect()
}

#[test]
fn named_zones_and_abbreviations() {
    for (kind, offset) in [("datetime<utc>", 0), ("datetime<GMT>", 0), ("datetime<kst>", 9), ("datetime<JST>", 9), ("datetime<Asia/Seoul>", 9), ("datetime<SGT>", 8)] {
        let (tmp, result) = zone_values(kind, vec![V::from("2026-10-01 10:00")]);
        assert_eq!(result.code, 0, "{kind}: {}", result.stderr);
        assert_eq!(at(&tmp), [unix_ms(2026, 10, 1, 10, 0, offset)], "{kind}");
    }
    // KST is the same type as +09:00 (same schema hash).
    let (a, _) = zone_values("datetime<KST>", vec![V::from("2026-10-01 10:00")]);
    let (b, _) = zone_values("datetime<+09:00>", vec![V::from("2026-10-01 10:00")]);
    assert_eq!(json(&a.join("client/Events.json"))["schema_hash"], json(&b.join("client/Events.json"))["schema_hash"]);
}

#[test]
fn iana_zones_follow_daylight_saving_time() {
    let (tmp, result) = zone_values("datetime<America/New_York>", vec![
        V::from("2026-01-15 12:00"),
        V::Date(2026, 7, 15, 12, 0, 0.0),
        // 01:30 happens twice when clocks go back on 2026-11-01: the earlier (EDT) instant.
        V::from("2026-11-01 01:30"),
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(at(&tmp), [unix_ms(2026, 1, 15, 12, 0, -5), unix_ms(2026, 7, 15, 12, 0, -4), unix_ms(2026, 11, 1, 1, 30, -4)]);
    let header = read(&tmp.join("cpp/DrEventsRow.h"));
    assert!(header.contains("meta = (DrTimeZone = \"America/New_York\"))"), "{header}");
}

#[test]
fn skipped_local_times_are_errors() {
    // Clocks jump from 02:00 to 03:00 on 2026-03-08 in New York.
    let (_, result) = zone_values("datetime<America/New_York>", vec![V::from("2026-03-08 02:30")]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("[in.xlsx]Events!B4: '2026-03-08 02:30' 시각은 America/New_York 서머타임 전환으로 건너뛰어 존재하지 않습니다"), "{}", result.stderr);
}

#[test]
fn ambiguous_or_unknown_zones_are_errors() {
    for (kind, message) in [
        ("datetime<CST>", "America/Chicago, Asia/Shanghai"),
        ("datetime<IST>", "Asia/Kolkata"),
        ("datetime<EST>", "America/New_York"),
        ("datetime<kr>", "모호하거나 계절에 따라 바뀝니다"),
        ("datetime<Mars/Base>", "알 수 없는 시간대 이름 'Mars/Base'"),
    ] {
        let (_, result) = zone_values(kind, vec![V::from("2026-10-01 10:00")]);
        assert_eq!(result.code, 1, "{kind}");
        assert!(result.stderr.contains("[Events.schema.xlsx]Events!B3"), "{kind}: {}", result.stderr);
        assert!(result.stderr.contains(message), "{kind}: {}", result.stderr);
    }
}
