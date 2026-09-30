//! DrTableSystem (DesignToRuntime Table System): spreadsheet tables to Unreal C++, baked
//! DataAssets and server/client JSON. Used by the `drtable` command line and `drtable-gui`.

pub mod check;
pub mod commands;
pub mod emit_cpp;
pub mod emit_json;
pub mod errors;
pub mod excel;
pub mod graph;
pub mod headers;
pub mod i18n;
pub mod migrate;
pub mod reader;
pub mod schema;
pub mod schemafile;
pub mod sources;
pub mod value;
pub mod values;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
