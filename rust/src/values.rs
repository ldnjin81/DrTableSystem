//! Converts cell values to schema types.

use crate::errors::ErrorCollector;
use crate::i18n::tr;
use crate::schema::{enum_of, fixed_of, Enums};
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
    if fixed_of(type_name).is_some() {
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
