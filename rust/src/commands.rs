//! Command logic shared by the command line and the GUI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::emit_cpp::{self, DEFAULT_ASSET_BASE, DEFAULT_ASSET_NAME};
use crate::emit_json;
use crate::errors::{ErrorCollector, ValidationErrors};
use crate::excel::DataModel;
use crate::i18n::tr;
use crate::schema::{in_scopes, CLIENT_SCOPES};
use crate::schemafile;
use crate::sources::is_identifier;

pub const PLUGIN_ASSET_BASE: &str = "UDrTableAssetBase";
pub const PLUGIN_ASSET_BASE_HEADER: &str = "DrTableAssetBase.h";
pub const PLUGIN_RUNTIME_HEADER: &str = "DrTableRuntime.h";

/// Options of `drtable build` (after the model is loaded).
#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub out_cpp: PathBuf,
    pub out_client: PathBuf,
    pub out_server: PathBuf,
    pub prefix: String,
    pub stamp: Option<String>,
    pub asset_base: String,
    pub asset_base_header: Option<String>,
    pub runtime_header: Option<String>,
    pub asset_name: String,
    pub ue_plugin: bool,
    /// Also write a header of key constants per string table (it changes whenever a key is added).
    pub string_keys: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            out_cpp: PathBuf::new(),
            out_client: PathBuf::new(),
            out_server: PathBuf::new(),
            prefix: "Dr".into(),
            stamp: None,
            asset_base: DEFAULT_ASSET_BASE.into(),
            asset_base_header: None,
            runtime_header: None,
            asset_name: DEFAULT_ASSET_NAME.into(),
            ue_plugin: false,
            string_keys: false,
        }
    }
}

pub enum BuildError {
    /// A wrong option (the command line reports it as a usage error).
    Usage(String),
    /// Validation errors, one message each.
    Invalid(Vec<String>),
    Io(String),
}

/// Writes the C++ headers and the client and server JSON for a loaded model.
pub fn build(model: &DataModel, options: &BuildOptions) -> Result<(), BuildError> {
    if !is_identifier(&options.prefix) {
        return Err(BuildError::Usage(tr(
            "--prefix는 영문자로 시작하는 C++ 식별자여야 합니다",
            "--prefix must be a C++ identifier starting with a letter",
        )));
    }
    let outputs = [&options.out_cpp, &options.out_client, &options.out_server];
    let resolved: HashSet<PathBuf> = outputs.iter().map(|p| normalize(&schemafile::absolute(p))).collect();
    if resolved.len() != outputs.len() {
        return Err(BuildError::Usage(tr("출력 디렉터리는 서로 달라야 합니다", "the output folders must be different")));
    }
    let mut asset_base = options.asset_base.clone();
    let mut asset_base_header = options.asset_base_header.clone();
    let mut runtime_header = options.runtime_header.clone();
    if options.ue_plugin {
        // Explicit options win; only unset ones take the plugin defaults.
        if asset_base == DEFAULT_ASSET_BASE {
            asset_base = PLUGIN_ASSET_BASE.to_string();
        }
        asset_base_header.get_or_insert_with(|| PLUGIN_ASSET_BASE_HEADER.to_string());
        runtime_header.get_or_insert_with(|| PLUGIN_RUNTIME_HEADER.to_string());
    }
    if asset_base != DEFAULT_ASSET_BASE && asset_base_header.is_none() {
        // Changing the base class without its header would emit code that does not compile.
        return Err(BuildError::Usage(tr(
            "--asset-base를 바꾸면 --asset-base-header도 필요합니다",
            "--asset-base requires --asset-base-header",
        )));
    }
    if !options.asset_name.contains("{table}") {
        return Err(BuildError::Usage(tr(
            "--asset-name에는 {table}이 들어가야 합니다",
            "--asset-name must contain {table}",
        )));
    }
    if runtime_header.is_some() {
        check_member_names(model).map_err(|e| BuildError::Invalid(e.0))?;
    }
    let mut key_headers = Vec::new();
    if options.string_keys {
        let mut failures = Vec::new();
        for table in model.string_tables() {
            match emit_cpp::string_keys_header(table, &options.prefix) {
                Ok(text) => key_headers.push((format!("{}{}Keys.h", options.prefix, table.name), text)),
                Err(mut errors) => failures.append(&mut errors),
            }
        }
        if !failures.is_empty() {
            return Err(BuildError::Invalid(failures));
        }
    }
    for output in outputs {
        clear_output(output).map_err(|e| BuildError::Invalid(e.0))?;
    }
    emit_cpp::emit_cpp(
        model,
        &options.out_cpp,
        &options.prefix,
        &asset_base,
        asset_base_header.as_deref(),
        runtime_header.as_deref(),
        &options.asset_name,
    )
    .and_then(|_| {
        emit_json::emit_json(
            model,
            &options.out_client,
            &options.out_server,
            options.stamp.as_deref(),
            &options.prefix,
            &options.asset_name,
        )
    })
    .and_then(|_| key_headers.iter().try_for_each(|(name, text)| std::fs::write(options.out_cpp.join(name), text)))
    .map_err(|e| BuildError::Io(e.to_string()))
}

/// Generated member functions must not clash with fields or with each other.
pub fn check_member_names(model: &DataModel) -> Result<(), ValidationErrors> {
    let mut errors = ErrorCollector::default();
    for table in model.data_tables() {
        let fields: HashSet<&str> =
            table.columns.iter().filter(|c| in_scopes(&c.scope, &CLIENT_SCOPES)).map(|c| c.name.as_str()).collect();
        let mut seen: HashMap<String, String> = HashMap::new();
        for (name, cell) in emit_cpp::generated_member_names(table) {
            if fields.contains(name.as_str()) {
                errors.add(&table.header_location(), &cell, tr(
                    format!("생성할 함수 '{name}'이 같은 이름의 필드와 겹칩니다"),
                    format!("generated function '{name}' clashes with a field of the same name"),
                ));
            } else if let Some(first) = seen.get(&name) {
                errors.add(&table.header_location(), &cell, tr(
                    format!("생성할 함수 '{name}'이 {first}에서 만든 함수와 겹칩니다"),
                    format!("generated function '{name}' clashes with the one generated for {first}"),
                ));
            } else {
                seen.insert(name, cell);
            }
        }
    }
    errors.raise_if_any()
}

fn clear_output(path: &Path) -> Result<(), ValidationErrors> {
    if path.exists() {
        if !path.is_dir() {
            return Err(ValidationErrors(vec![tr(
                format!("출력!A1: 출력 경로가 디렉터리가 아닙니다: {}", path.display()),
                format!("output!A1: the output path is not a folder: {}", path.display()),
            )]));
        }
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let child = entry.path();
                let _ = if child.is_dir() { std::fs::remove_dir_all(&child) } else { std::fs::remove_file(&child) };
            }
        }
    }
    let _ = std::fs::create_dir_all(path);
    Ok(())
}

/// Removes `.` and `..` components without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
