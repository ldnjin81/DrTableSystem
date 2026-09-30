//! Finding input files and reading sheet names.

use std::path::{Path, PathBuf};

use crate::errors::ValidationErrors;
use crate::i18n::tr;

/// `<enum>Name` sheet titles (old layout).
pub fn enum_sheet_name(title: &str) -> Option<&str> {
    let name = title.strip_prefix("<enum>")?;
    is_identifier(name).then_some(name)
}

/// `^[A-Za-z][A-Za-z0-9_]*$`
pub fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The sheet name without its #comment and surrounding spaces.
pub fn strip_sheet_comment(title: &str) -> &str {
    title.split('#').next().unwrap_or("").trim()
}

/// Table name for a table sheet, or None for notes and enums. Sheets with the same table
/// name, in one file or in several, are parts of one table.
pub fn table_name_of(title: &str) -> Option<&str> {
    if title.starts_with('#') || title.starts_with("<enum>") {
        return None;
    }
    Some(strip_sheet_comment(title))
}

fn ends_with_any(name: &str, suffixes: &[&str]) -> bool {
    let lower = name.to_lowercase();
    suffixes.iter().any(|suffix| lower.ends_with(suffix))
}

/// Files with one of the suffixes and their paths relative to the input folder, sorted by
/// that path. Excel lock files (~$Book.xlsx) and hidden folders such as .git are skipped.
pub fn find_files(
    input: &Path,
    suffixes: &[&str],
    allow_empty: bool,
) -> Result<Vec<(PathBuf, String)>, ValidationErrors> {
    let file_name = input.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if input.is_file() && ends_with_any(&file_name, suffixes) {
        return Ok(vec![(input.to_path_buf(), file_name)]);
    }
    if input.is_dir() {
        let mut found = Vec::new();
        walk(input, input, suffixes, &mut found);
        if !found.is_empty() || allow_empty {
            found.sort_by(|a, b| a.1.cmp(&b.1));
            return Ok(found);
        }
    }
    Err(ValidationErrors(vec![tr(
        format!("입력!A1: xlsx 파일을 찾을 수 없습니다: {}", input.display()),
        format!("input!A1: no xlsx file found: {}", input.display()),
    )]))
}

fn walk(root: &Path, folder: &Path, suffixes: &[&str], found: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !name.starts_with('.') {
                walk(root, &path, suffixes, found);
            }
        } else if path.is_file()
            && ends_with_any(&name, suffixes)
            && !name.starts_with("~$")
            && !name.starts_with('.')
        {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let relative: Vec<String> =
                relative.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
            found.push((path.clone(), relative.join("/")));
        }
    }
}
