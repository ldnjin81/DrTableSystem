//! Validation error collection.

/// Every validation error found in one run.
#[derive(Debug)]
pub struct ValidationErrors(pub Vec<String>);

/// Collects as many errors as possible before failing. Each message starts with its
/// location: `[File.xlsx]Sheet!Cell`.
#[derive(Debug, Default)]
pub struct ErrorCollector {
    pub messages: Vec<String>,
}

impl ErrorCollector {
    pub fn add(&mut self, sheet: &str, cell: &str, message: impl AsRef<str>) {
        self.messages.push(format!("{sheet}!{cell}: {}", message.as_ref()));
    }

    pub fn raise_if_any(&mut self) -> Result<(), ValidationErrors> {
        if self.messages.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors(std::mem::take(&mut self.messages)))
        }
    }
}
