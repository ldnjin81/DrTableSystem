//! 스키마 E열의 값 검증 규칙.

use regex::Regex;

use crate::errors::ErrorCollector;
use crate::i18n::tr;
use crate::schema::fixed_of;
use crate::value::Cell;

#[derive(Clone, Debug)]
pub struct Rules {
    pub text: String,
    required: bool,
    checks: Vec<Check>,
}

#[derive(Clone, Debug)]
enum Check {
    Min(f64, bool),
    Max(f64, bool),
    Len(usize),
    Pattern(Regex),
}

pub fn parse(cell: &Cell, type_name: &str, sheet: &str, place: &str, errors: &mut ErrorCollector) -> Option<Rules> {
    let text = cell.text_or_empty();
    if text.is_empty() { return None; }
    let numeric = matches!(type_name, "int32" | "int64" | "float" | "double") || fixed_of(type_name).is_some();
    let string = matches!(type_name, "name" | "string" | "text" | "tag" | "path" | "lang");
    let mut rules = Rules { text, required: false, checks: Vec::new() };
    for fragment in rules.text.split(';') {
        let part = fragment.trim();
        let parsed = if matches!(part, "required" | "필수") {
            rules.required = true;
            true
        } else if let Some((low, high)) = part.split_once("..") {
            if numeric {
                if let (Ok(low), Ok(high)) = (low.trim().parse::<f64>(), high.trim().parse::<f64>()) {
                    if low.is_finite() && high.is_finite() && low <= high {
                        rules.checks.push(Check::Min(low, true));
                        rules.checks.push(Check::Max(high, true));
                        true
                    } else { false }
                } else { false }
            } else { false }
        } else if let Some(bound) = part.strip_prefix("len<=") {
            if string { match bound.trim().parse::<usize>() {
                Ok(n) => { rules.checks.push(Check::Len(n)); true },
                Err(_) => false,
            }} else { false }
        } else if let Some(pattern) = part.strip_prefix("regex:") {
            if string { match Regex::new(pattern) {
                Ok(regex) if !pattern.is_empty() => { rules.checks.push(Check::Pattern(regex)); true },
                _ => false,
            }} else { false }
        } else if let Some(bound) = part.strip_prefix(">=") {
            add_bound(&mut rules.checks, bound, numeric, true, true)
        } else if let Some(bound) = part.strip_prefix('>') {
            add_bound(&mut rules.checks, bound, numeric, true, false)
        } else if let Some(bound) = part.strip_prefix("<=") {
            add_bound(&mut rules.checks, bound, numeric, false, true)
        } else if let Some(bound) = part.strip_prefix('<') {
            add_bound(&mut rules.checks, bound, numeric, false, false)
        } else { false };
        if !parsed {
            errors.add(sheet, place, tr(format!("올바르지 않거나 자료형에 맞지 않는 규칙 '{part}'"), format!("invalid rule or rule incompatible with type: '{part}'")));
        }
    }
    Some(rules)
}

fn add_bound(checks: &mut Vec<Check>, text: &str, numeric: bool, min: bool, inclusive: bool) -> bool {
    if !numeric { return false; }
    match text.trim().parse::<f64>() {
        Ok(number) if number.is_finite() => {
            checks.push(if min { Check::Min(number, inclusive) } else { Check::Max(number, inclusive) });
            true
        }
        _ => false,
    }
}

pub fn check_required(rules: &Rules, empty: bool, sheet: &str, place: &str, errors: &mut ErrorCollector) {
    if rules.required && empty {
        errors.add(sheet, place, tr("필수 값이 비어 있습니다", "a required value is empty"));
    }
}

pub fn check_value(rules: &Rules, value: &Cell, sheet: &str, place: &str, errors: &mut ErrorCollector) {
    if value.is_blank() { return; }
    for check in &rules.checks {
        let valid = match check {
            Check::Min(bound, inclusive) => number(value).is_some_and(|n| if *inclusive { n >= *bound } else { n > *bound }),
            Check::Max(bound, inclusive) => number(value).is_some_and(|n| if *inclusive { n <= *bound } else { n < *bound }),
            Check::Len(max) => value.text_or_empty().chars().count() <= *max,
            Check::Pattern(pattern) => pattern.is_match(&value.text_or_empty()),
        };
        if !valid {
            errors.add(sheet, place, tr(format!("값 '{}'이 규칙 '{}'을(를) 만족하지 않습니다", value.py_str(), rules.text), format!("value '{}' does not satisfy rule '{}'", value.py_str(), rules.text)));
        }
    }
}

fn number(cell: &Cell) -> Option<f64> {
    let value = match cell {
        Cell::Int(n) => *n as f64,
        Cell::Float(n) => *n,
        Cell::Str(s) => match s.trim().strip_suffix('%') {
            Some(percent) => percent.trim().parse::<f64>().ok()? / 100.0,
            None => s.trim().parse::<f64>().ok()?,
        },
        _ => return None,
    };
    value.is_finite().then_some(value)
}
