//! Message language for user-facing output (errors, warnings, reports).
//!
//! Generated artifacts are always in English. Only messages printed to the user are
//! translated. The language comes from `--lang` or `DRTABLE_LANG` (`en` or `ko`, default en).

use std::sync::atomic::{AtomicBool, Ordering};

static KOREAN: AtomicBool = AtomicBool::new(false);

pub const SUPPORTED: [&str; 2] = ["en", "ko"];

/// Reads DRTABLE_LANG; an unknown value falls back to English.
pub fn init_from_env() {
    let language = std::env::var("DRTABLE_LANG").unwrap_or_default().to_lowercase();
    KOREAN.store(language == "ko", Ordering::Relaxed);
}

pub fn set_language(language: &str) {
    KOREAN.store(language == "ko", Ordering::Relaxed);
}

pub fn is_korean() -> bool {
    KOREAN.load(Ordering::Relaxed)
}

/// The message in the active language. Both texts are already formatted.
pub fn tr(ko: impl Into<String>, en: impl Into<String>) -> String {
    if is_korean() { ko.into() } else { en.into() }
}
