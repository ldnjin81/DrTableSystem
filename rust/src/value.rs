//! Cell values read from spreadsheets, converted table values, and their text forms.
//!
//! The text forms are fixed: numbers print like Python's `repr(float)` and JSON follows
//! `json.dumps` exactly, so generated files and content hashes stay byte-for-byte stable.

use std::cmp::Ordering;
use std::fmt::Write;

/// A value as read from a spreadsheet cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Empty,
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    /// A date or time cell, already in its text form ("2026-01-02 00:00:00").
    Date(String),
}

impl Cell {
    /// True for cells the tool treats as blank (no value or an empty string).
    pub fn is_blank(&self) -> bool {
        matches!(self, Cell::Empty) || matches!(self, Cell::Str(s) if s.is_empty())
    }

    /// The text Python's `str()` gives for this value; `None` prints as "None".
    pub fn py_str(&self) -> String {
        match self {
            Cell::Empty => "None".to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => py_float_repr(*f),
            Cell::Str(s) => s.clone(),
            Cell::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Cell::Date(s) => s.clone(),
        }
    }

    /// `"" if value is None else str(value).strip()`
    pub fn text_or_empty(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            other => other.py_str().trim().to_string(),
        }
    }
}

/// A converted table value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
}

impl Value {
    /// The text Python's `str()` gives for this value.
    pub fn py_str(&self) -> String {
        match self {
            Value::Null => "None".to_string(),
            Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => py_float_repr(*f),
            Value::Str(s) => s.clone(),
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(Value::py_repr).collect();
                format!("[{}]", inner.join(", "))
            }
        }
    }

    /// The text Python's `repr()` gives for this value.
    pub fn py_repr(&self) -> String {
        match self {
            Value::Str(s) => py_str_repr(s),
            other => other.py_str(),
        }
    }

    /// A hashable identity matching Python's dict key equality for keys (ints and strings).
    pub fn key(&self) -> String {
        match self {
            Value::Int(i) => format!("i{i}"),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => format!("i{}", *f as i64),
            Value::Float(f) => format!("f{f:?}"),
            Value::Bool(b) => format!("i{}", *b as i64),
            Value::Str(s) => format!("s{s}"),
            Value::Null => "n".to_string(),
            Value::List(_) => format!("l{}", self.py_repr()),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Python's ordering for sort keys: numbers numerically, strings by code point.
pub fn py_cmp(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        _ => {
            let x = number(a);
            let y = number(b);
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
    }
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        Value::Bool(b) => *b as i64 as f64,
        _ => 0.0,
    }
}

/// Python's `repr(float)`: the shortest round-trip digits, fixed notation when the decimal
/// exponent is in [-4, 16), otherwise scientific with a signed two-digit exponent.
pub fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = if digits.chars().all(|c| c == '0') { "0".to_string() } else { digits };
    let decpt = exponent + 1;
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let body = if value != 0.0 && (decpt <= -4 || decpt > 16) {
        let (first, rest) = digits.split_at(1);
        let mantissa = if rest.is_empty() { first.to_string() } else { format!("{first}.{rest}") };
        let exp = decpt - 1;
        let exp_sign = if exp < 0 { '-' } else { '+' };
        format!("{mantissa}e{exp_sign}{:02}", exp.abs())
    } else if decpt <= 0 {
        format!("0.{}{}", "0".repeat((-decpt) as usize), digits)
    } else if decpt as usize >= digits.len() {
        format!("{}{}.0", digits, "0".repeat(decpt as usize - digits.len()))
    } else {
        let (head, tail) = digits.split_at(decpt as usize);
        format!("{head}.{tail}")
    };
    format!("{sign}{body}")
}

/// Python's `repr(str)` for plain text: single quotes unless the text contains one.
pub fn py_str_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A JSON string literal as Python's `json.dumps(..., ensure_ascii=False)` writes it.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON document node that keeps key order (Python dicts are insertion-ordered).
#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl From<&Value> for Json {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::Int(i) => Json::Int(*i),
            Value::Float(f) => Json::Float(*f),
            Value::Str(s) => Json::Str(s.clone()),
            Value::List(items) => Json::List(items.iter().map(Json::from).collect()),
        }
    }
}

impl From<&str> for Json {
    fn from(value: &str) -> Self {
        Json::Str(value.to_string())
    }
}

impl From<String> for Json {
    fn from(value: String) -> Self {
        Json::Str(value)
    }
}

impl Json {
    /// `json.dumps(value, ensure_ascii=False, indent=2)`
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_pretty(&self, out: &mut String, level: usize) {
        match self {
            Json::List(items) if !items.is_empty() => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    out.push_str(if index == 0 { "\n" } else { ",\n" });
                    out.push_str(&"  ".repeat(level + 1));
                    item.write_pretty(out, level + 1);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(level));
                out.push(']');
            }
            Json::Object(entries) if !entries.is_empty() => {
                out.push('{');
                for (index, (key, item)) in entries.iter().enumerate() {
                    out.push_str(if index == 0 { "\n" } else { ",\n" });
                    out.push_str(&"  ".repeat(level + 1));
                    out.push_str(&json_string(key));
                    out.push_str(": ");
                    item.write_pretty(out, level + 1);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(level));
                out.push('}');
            }
            other => other.write_compact(out, false),
        }
    }

    /// `json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))`
    pub fn compact_sorted(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out, true);
        out
    }

    fn write_compact(&self, out: &mut String, sort: bool) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Json::Float(f) => {
                if f.is_nan() {
                    out.push_str("NaN");
                } else if f.is_infinite() {
                    out.push_str(if *f > 0.0 { "Infinity" } else { "-Infinity" });
                } else {
                    out.push_str(&py_float_repr(*f));
                }
            }
            Json::Str(s) => out.push_str(&json_string(s)),
            Json::List(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write_compact(out, sort);
                }
                out.push(']');
            }
            Json::Object(entries) => {
                let mut ordered: Vec<&(String, Json)> = entries.iter().collect();
                if sort {
                    ordered.sort_by(|a, b| a.0.cmp(&b.0));
                }
                out.push('{');
                for (index, (key, item)) in ordered.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_string(key));
                    out.push(':');
                    item.write_compact(out, sort);
                }
                out.push('}');
            }
        }
    }
}

/// Builds a JSON object from (key, value) pairs.
pub fn object<I, K>(entries: I) -> Json
where
    I: IntoIterator<Item = (K, Json)>,
    K: Into<String>,
{
    Json::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (12.5, "12.5"),
            (0.1, "0.1"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (-3.25e-7, "-3.25e-07"),
            (2.5e22, "2.5e+22"),
            (100.0, "100.0"),
        ];
        for (value, expected) in cases {
            assert_eq!(py_float_repr(value), expected, "{value}");
        }
    }

    #[test]
    fn json_matches_python() {
        let doc = object([
            ("b", Json::List(vec![Json::Int(1), Json::Float(2.0)])),
            ("a", Json::Object(vec![])),
            ("c", Json::Str("줄\n\"따옴\"\u{1}".into())),
        ]);
        assert_eq!(
            doc.pretty(),
            "{\n  \"b\": [\n    1,\n    2.0\n  ],\n  \"a\": {},\n  \"c\": \"줄\\n\\\"따옴\\\"\\u0001\"\n}"
        );
        assert_eq!(doc.compact_sorted(), "{\"a\":{},\"b\":[1,2.0],\"c\":\"줄\\n\\\"따옴\\\"\\u0001\"}");
    }
}
