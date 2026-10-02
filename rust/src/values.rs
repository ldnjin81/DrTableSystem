//! Converts cell values to schema types.

use crate::errors::ErrorCollector;
use crate::i18n::tr;
use crate::schema::{datetime_zone, enum_of, fixed_of, Enums, TimeZone};
use crate::value::{Cell, Value};

const STRING_TYPES: [&str; 6] = ["name", "string", "text", "tag", "path", "lang"];

/// Converts a cell to `type_name`. On failure reports an error and returns the type default.
/// With `use_default_for_empty`, blank cells take the type default.
pub fn convert_value(
    value: &Cell,
    type_name: &str,
    enums: &Enums,
    sheet: &str,
    cell: &str,
    errors: &mut ErrorCollector,
    use_default_for_empty: bool,
) -> Option<Value> {
    if use_default_for_empty && value.is_blank() {
        return Some(default_value(type_name, enums));
    }
    let failed = |errors: &mut ErrorCollector| {
        errors.add(sheet, cell, tr(
            format!("'{}' 값을 {type_name} 자료형으로 변환할 수 없습니다", value.py_str()),
            format!("cannot convert '{}' to {type_name}", value.py_str()),
        ));
        Some(default_value(type_name, enums))
    };
    if let Some(zone) = datetime_zone(type_name) {
        return match parse_datetime_ms(value, zone) {
            Ok(ms) => Some(Value::Int(ms)),
            Err(DateFailure::Skipped) => {
                errors.add(sheet, cell, tr(
                    format!("'{}' 시각은 {} 서머타임 전환으로 건너뛰어 존재하지 않습니다", value.py_str(), zone.label()),
                    format!("'{}' does not exist in {}: the clocks skip it for daylight saving time", value.py_str(), zone.label()),
                ));
                Some(Value::Int(0))
            }
            Err(DateFailure::Unreadable) => {
                errors.add(sheet, cell, tr(
                    format!("'{}' 값을 날짜·시각으로 읽을 수 없습니다. 날짜 서식 칸이나 2026-10-01 10:00(+09:00) 형식으로 쓰세요", value.py_str()),
                    format!("cannot read '{}' as a date and time. Use a date-formatted cell or 2026-10-01 10:00 (+09:00)", value.py_str()),
                ));
                Some(Value::Int(0))
            }
        };
    }
    if type_name == "duration" {
        return match parse_duration_ms(value) {
            Some(ms) => Some(Value::Int(ms)),
            None => {
                errors.add(sheet, cell, tr(
                    format!("'{}' 값을 시간 길이로 읽을 수 없습니다. 1:30:00, 90s, 1h30m, 2d, 500ms처럼 쓰세요(숫자만 쓰면 초)", value.py_str()),
                    format!("cannot read '{}' as a duration. Use 1:30:00, 90s, 1h30m, 2d or 500ms (a plain number is seconds)", value.py_str()),
                ));
                Some(Value::Int(0))
            }
        };
    }
    if let Some((scale, wide)) = fixed_of(type_name) {
        // 0.1234, "0.1234" or "12.34%" -> 1234 for fixed<10000>; values finer than the scale are errors.
        let number = match value {
            Cell::Int(i) => Some(*i as f64),
            Cell::Float(f) => Some(*f),
            Cell::Str(s) => {
                let text = s.trim();
                match text.strip_suffix('%') {
                    Some(percent) => parse_python_float(percent.trim()).map(|n| n / 100.0),
                    None => parse_python_float(text),
                }
            }
            _ => None,
        };
        let Some(number) = number.filter(|n| n.is_finite()) else { return failed(errors) };
        let scaled = number * scale as f64;
        let raw = scaled.round();
        if (scaled - raw).abs() > 1e-6 * raw.abs().max(1.0) {
            let digits = scale.to_string().len() - 1;
            errors.add(sheet, cell, tr(
                format!("'{}' 값은 {type_name}로 정확히 나타낼 수 없습니다(소수 {digits}자리까지)", value.py_str()),
                format!("'{}' cannot be represented exactly as {type_name} (up to {digits} decimal places)", value.py_str()),
            ));
            return Some(Value::Int(0));
        }
        let limit = if wide { i64::MAX as f64 } else { i32::MAX as f64 };
        if raw.abs() > limit {
            let maximum = limit / scale as f64;
            errors.add(sheet, cell, tr(
                format!("'{}' 값이 {type_name} 범위(±{maximum:.0})를 넘습니다", value.py_str()),
                format!("'{}' is out of the {type_name} range (±{maximum:.0})", value.py_str()),
            ));
            return Some(Value::Int(0));
        }
        return Some(Value::Int(raw as i64));
    }
    match type_name {
        "int32" | "int64" => {
            let number: Option<i128> = match value {
                Cell::Int(i) => Some(*i as i128),
                Cell::Float(f) if f.fract() == 0.0 && f.is_finite() => Some(*f as i128),
                Cell::Str(s) => {
                    let text = s.trim();
                    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
                    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                        text.parse::<i128>().ok()
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let (minimum, maximum) = if type_name == "int32" {
                (i32::MIN as i128, i32::MAX as i128)
            } else {
                (i64::MIN as i128, i64::MAX as i128)
            };
            match number {
                Some(n) if n >= minimum && n <= maximum => Some(Value::Int(n as i64)),
                _ => failed(errors),
            }
        }
        "float" | "double" => {
            let number = match value {
                Cell::Int(i) => Some(*i as f64),
                Cell::Float(f) => Some(*f),
                Cell::Str(s) => parse_python_float(s),
                _ => None,
            };
            match number {
                Some(n) if n.is_finite() => Some(Value::Float(n)),
                _ => failed(errors),
            }
        }
        "bool" => match value {
            Cell::Bool(b) => Some(Value::Bool(*b)),
            Cell::Int(0) => Some(Value::Bool(false)),
            Cell::Int(1) => Some(Value::Bool(true)),
            Cell::Float(f) if *f == 0.0 => Some(Value::Bool(false)),
            Cell::Float(f) if *f == 1.0 => Some(Value::Bool(true)),
            Cell::Str(s) => match s.trim().to_lowercase().as_str() {
                "true" | "1" => Some(Value::Bool(true)),
                "false" | "0" => Some(Value::Bool(false)),
                _ => failed(errors),
            },
            _ => failed(errors),
        },
        t if STRING_TYPES.contains(&t) => match value {
            Cell::Str(s) => Some(Value::Str(s.clone())),
            // A translation that is just a number ("100") is still text.
            Cell::Int(_) | Cell::Float(_) if t == "lang" => Some(Value::Str(value.py_str())),
            Cell::Bool(b) if t == "lang" => Some(Value::Str(if *b { "TRUE" } else { "FALSE" }.into())),
            _ => failed(errors),
        },
        t => match enum_of(t) {
            Some(name) => {
                let Some(enum_schema) = enums.get(name) else {
                    return Some(Value::Str(String::new()));
                };
                let text = value.py_str();
                if enum_schema.values.iter().any(|v| v.name == text) {
                    Some(Value::Str(text))
                } else {
                    failed(errors)
                }
            }
            None => {
                errors.add(sheet, cell, tr(
                    format!("지원하지 않는 자료형 '{type_name}'"),
                    format!("unsupported type '{type_name}'"),
                ));
                None
            }
        },
    }
}

/// Python's float(str): surrounding spaces, an optional sign, decimal or exponent forms and
/// inf/nan words.
fn parse_python_float(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() || text.contains('_') {
        return None;
    }
    text.parse::<f64>().ok()
}

/// The value an empty cell takes: 0, false, "" or the first enumerator.
pub fn default_value(type_name: &str, enums: &Enums) -> Value {
    if fixed_of(type_name).is_some() || datetime_zone(type_name).is_some() || type_name == "duration" {
        return Value::Int(0);
    }
    match type_name {
        "int32" | "int64" | "float" | "double" => Value::Int(0),
        "bool" => Value::Bool(false),
        t if STRING_TYPES.contains(&t) => Value::Str(String::new()),
        t => match enum_of(t).and_then(|name| enums.get(name)).and_then(|e| e.values.first()) {
            Some(first) => Value::Str(first.name.clone()),
            None => Value::Str(String::new()),
        },
    }
}

/// A converted value turned back into a cell (declared defaults fill empty cells).
pub fn value_to_cell(value: &Value) -> Cell {
    match value {
        Value::Null => Cell::Empty,
        Value::Bool(b) => Cell::Bool(*b),
        Value::Int(i) => Cell::Int(*i),
        Value::Float(f) => Cell::Float(*f),
        Value::Str(s) => Cell::Str(s.clone()),
        Value::List(_) => Cell::Str(value.py_str()),
    }
}

static DATETIME_TEXT_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})(?:[ T]+(\d{1,2}):(\d{2})(?::(\d{2})(?:\.(\d{1,9}))?)?)?\s*(Z|[+-]\d{2}:?\d{2})?$",
    )
    .unwrap()
});
static CLOCK_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^(\d+):(\d{2})(?::(\d{2})(?:\.(\d{1,3}))?)?$").unwrap());
static UNIT_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(\d+(?:\.\d+)?)\s*(ms|d|h|m|s)").unwrap());

enum DateFailure {
    Unreadable,
    /// A local time that daylight saving time skips (02:30 on a spring-forward night).
    Skipped,
}

/// Milliseconds since 1970-01-01 UTC. Excel date cells and text without an offset are read in
/// `zone`; text may carry its own (`Z`, `+09:00`). In a repeated hour (daylight saving time
/// ends) the earlier instant is taken.
fn parse_datetime_ms(value: &Cell, zone: TimeZone) -> Result<i64, DateFailure> {
    use chrono::TimeZone as _;
    let text = match value {
        Cell::Date(s) | Cell::Str(s) => s.trim().to_string(),
        _ => return Err(DateFailure::Unreadable),
    };
    let unreadable = || DateFailure::Unreadable;
    let c = DATETIME_TEXT_RE.captures(&text).ok_or_else(unreadable)?;
    let number = |i: usize| c.get(i).map(|m| m.as_str().parse::<u32>()).transpose().ok().flatten();
    let date = chrono::NaiveDate::from_ymd_opt(c[1].parse().map_err(|_| unreadable())?, number(2).ok_or_else(unreadable)?, number(3).ok_or_else(unreadable)?)
        .ok_or_else(unreadable)?;
    let millis = c.get(7).map(|m| {
        let digits = format!("{:0<3}", m.as_str());
        digits[..3].parse::<u32>().unwrap_or(0)
    });
    let time = chrono::NaiveTime::from_hms_milli_opt(number(4).unwrap_or(0), number(5).unwrap_or(0), number(6).unwrap_or(0), millis.unwrap_or(0))
        .ok_or_else(unreadable)?;
    let local = date.and_time(time);
    let written = match c.get(8).map(|m| m.as_str()) {
        None => None,
        Some("Z") => Some(0),
        Some(offset) => {
            let digits: String = offset[1..].chars().filter(|ch| ch.is_ascii_digit()).collect();
            let minutes = digits[..2].parse::<i32>().map_err(|_| unreadable())? * 60 + digits[2..].parse::<i32>().map_err(|_| unreadable())?;
            Some(if offset.starts_with('-') { -minutes } else { minutes })
        }
    };
    let fixed = |minutes: i32| local.and_utc().timestamp_millis() - minutes as i64 * 60_000;
    match (written, zone) {
        (Some(minutes), _) | (None, TimeZone::Fixed(minutes)) => Ok(fixed(minutes)),
        (None, TimeZone::Named(tz)) => match tz.from_local_datetime(&local) {
            chrono::LocalResult::Single(moment) => Ok(moment.timestamp_millis()),
            chrono::LocalResult::Ambiguous(earlier, _) => Ok(earlier.timestamp_millis()),
            chrono::LocalResult::None => Err(DateFailure::Skipped),
        },
    }
}

/// Milliseconds. An Excel time cell (1:30:00, also [h]:mm over a day), "h:mm[:ss[.fff]]",
/// units ("1h 30m", "90s", "2d", "500ms") or a plain number of seconds. Not negative.
fn parse_duration_ms(value: &Cell) -> Option<i64> {
    let ms = match value {
        Cell::Date(s) => {
            // Excel serial 0 is 1899-12-31 up to serial 60 (Excel's 1900 leap-year bug), 1899-12-30 after.
            let moment = chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S%.f").ok()?;
            let march = chrono::NaiveDate::from_ymd_opt(1900, 3, 1)?.and_hms_opt(0, 0, 0)?;
            let base_day = if moment < march { 31 } else { 30 };
            let base = chrono::NaiveDate::from_ymd_opt(1899, 12, base_day)?.and_hms_opt(0, 0, 0)?;
            (moment - base).num_milliseconds()
        }
        Cell::Int(i) => i.checked_mul(1000)?,
        Cell::Float(f) if f.is_finite() => (f * 1000.0).round() as i64,
        Cell::Str(s) => {
            let text = s.trim().to_lowercase();
            if let Some(c) = CLOCK_RE.captures(&text) {
                let part = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<i64>().ok());
                let millis = c.get(4).map_or(0, |m| format!("{:0<3}", m.as_str()).parse::<i64>().unwrap_or(0));
                ((part(1)? * 60 + part(2)?) * 60 + part(3)?) * 1000 + millis
            } else if let Ok(seconds) = text.parse::<f64>() {
                (seconds * 1000.0).round() as i64
            } else {
                // Every character must belong to a unit term.
                if UNIT_RE.replace_all(&text, "").trim().chars().any(|ch| !ch.is_whitespace()) {
                    return None;
                }
                let mut total = 0f64;
                let mut any = false;
                for c in UNIT_RE.captures_iter(&text) {
                    any = true;
                    let amount: f64 = c[1].parse().ok()?;
                    total += amount * match &c[2] {
                        "d" => 86_400_000.0,
                        "h" => 3_600_000.0,
                        "m" => 60_000.0,
                        "s" => 1_000.0,
                        _ => 1.0,
                    };
                }
                if !any {
                    return None;
                }
                total.round() as i64
            }
        }
        _ => return None,
    };
    (ms >= 0).then_some(ms)
}
