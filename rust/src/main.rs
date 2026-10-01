//! DrTableSystem (DesignToRuntime Table System) command line:
//! `drtable build | graph | check | new`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use drtable::commands::{self, BuildError, BuildOptions};
use drtable::emit_cpp::{DEFAULT_ASSET_BASE, DEFAULT_ASSET_NAME};
use drtable::errors::{ErrorCollector, ValidationErrors};
use drtable::i18n::{self, tr};
use drtable::{check, excel, graph, headers, schemafile, VERSION};

const HELP: &str = "usage: drtable [-h] [--version] [--lang {en,ko}] {build,graph,check,new} ...

Generate C++, JSON and Unreal DataAssets from spreadsheet tables.

commands:
  build     generate C++, client JSON and server JSON
  graph     write the table reference graph as Mermaid Markdown
  check     check references in generated JSON (or validate a spreadsheet with --input)
  new       create a data workbook for a table, with reference formulas in rows 2-3

options:
  --version       show the version and exit
  --lang {en,ko}  language of messages (default: $DRTABLE_LANG or en)";

/// Options a command accepts: (name, takes a value).
fn command_options(command: &str) -> Option<&'static [(&'static str, bool)]> {
    Some(match command {
        "build" => &[
            ("input", true), ("schema", true), ("enums", true), ("out-cpp", true), ("out-client", true),
            ("out-server", true), ("prefix", true), ("stamp", true), ("asset-base", true),
            ("asset-base-header", true), ("ue-plugin", false), ("runtime-header", true), ("asset-name", true),
        ],
        "graph" => &[("input", true), ("schema", true), ("enums", true), ("out", true)],
        "check" => &[("client", true), ("server", true), ("input", true), ("schema", true), ("enums", true)],
        "new" => &[("table", true), ("out", true), ("schema", true), ("enums", true)],
        _ => return None,
    })
}

fn required(command: &str) -> &'static [&'static str] {
    match command {
        "build" => &["input", "out-cpp", "out-client", "out-server"],
        "graph" => &["input", "out"],
        "new" => &["table", "out", "schema"],
        _ => &[],
    }
}

struct UsageError(String, String);

struct Args {
    command: String,
    values: HashMap<String, String>,
    flags: HashSet<String>,
}

impl Args {
    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.get(name).map(PathBuf::from)
    }
}

enum Parsed {
    Run(Args),
    Exit(ExitCode),
}

fn parse(argv: &[String]) -> Result<Parsed, UsageError> {
    let mut index = 0;
    let mut command: Option<String> = None;
    let mut values = HashMap::new();
    let mut flags = HashSet::new();
    let usage = |command: &Option<String>| match command {
        Some(c) => format!("usage: drtable {c} [options]"),
        None => "usage: drtable [-h] [--version] [--lang {en,ko}] {build,graph,check,new} ...".to_string(),
    };
    while index < argv.len() {
        let arg = &argv[index];
        index += 1;
        if command.is_none() {
            match arg.as_str() {
                "-h" | "--help" => {
                    println!("{HELP}");
                    return Ok(Parsed::Exit(ExitCode::SUCCESS));
                }
                "--version" => {
                    println!("{VERSION}");
                    return Ok(Parsed::Exit(ExitCode::SUCCESS));
                }
                _ => {}
            }
            if let Some(rest) = arg.strip_prefix("--lang") {
                let language = if let Some(value) = rest.strip_prefix('=') {
                    value.to_string()
                } else if rest.is_empty() && index < argv.len() {
                    index += 1;
                    argv[index - 1].clone()
                } else {
                    return Err(UsageError(usage(&command), "argument --lang: expected one argument".into()));
                };
                if !i18n::SUPPORTED.contains(&language.as_str()) {
                    return Err(UsageError(usage(&command), format!("argument --lang: invalid choice: '{language}' (choose from 'en', 'ko')")));
                }
                i18n::set_language(&language);
                continue;
            }
            if command_options(arg).is_none() {
                return Err(UsageError(usage(&command), format!("argument command: invalid choice: '{arg}' (choose from 'build', 'graph', 'check', 'new')")));
            }
            command = Some(arg.clone());
            continue;
        }
        let name = command.as_deref().unwrap_or("");
        if arg == "-h" || arg == "--help" {
            println!("{}\n\nsee docs/en/manual.md for the options of '{name}'", usage(&command));
            return Ok(Parsed::Exit(ExitCode::SUCCESS));
        }
        let Some(option) = arg.strip_prefix("--") else {
            return Err(UsageError(usage(&command), format!("unrecognized arguments: {arg}")));
        };
        let (key, inline) = match option.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (option.to_string(), None),
        };
        let Some(&(_, takes_value)) = command_options(name).unwrap().iter().find(|(o, _)| *o == key) else {
            return Err(UsageError(usage(&command), format!("unrecognized arguments: {arg}")));
        };
        if takes_value {
            let value = match inline {
                Some(v) => v,
                None if index < argv.len() && !argv[index].starts_with("--") => {
                    index += 1;
                    argv[index - 1].clone()
                }
                None => return Err(UsageError(usage(&command), format!("argument --{key}: expected one argument"))),
            };
            values.insert(key, value);
        } else {
            flags.insert(key);
        }
    }
    let Some(command) = command else {
        return Err(UsageError(usage(&None), "the following arguments are required: command".into()));
    };
    let missing: Vec<String> = required(&command).iter().filter(|r| !values.contains_key(**r)).map(|r| format!("--{r}")).collect();
    if !missing.is_empty() {
        return Err(UsageError(usage(&Some(command)), format!("the following arguments are required: {}", missing.join(", "))));
    }
    if let Some(stamp) = values.get("stamp")
        && !is_iso8601(stamp) {
            return Err(UsageError(usage(&Some(command)), format!("argument --stamp: {}", tr("--stamp는 ISO 8601 형식이어야 합니다", "--stamp must be ISO 8601"))));
        }
    Ok(Parsed::Run(Args { command, values, flags }))
}

fn is_iso8601(value: &str) -> bool {
    use chrono::{DateTime, NaiveDate, NaiveDateTime};
    let normalized = value.replace('Z', "+00:00");
    DateTime::parse_from_rfc3339(&normalized).is_ok()
        || ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
            .iter()
            .any(|f| NaiveDateTime::parse_from_str(value, f).is_ok())
        || ["%Y-%m-%dT%H:%M:%S%.f%:z", "%Y-%m-%dT%H:%M:%S%:z", "%Y-%m-%d %H:%M:%S%:z"]
            .iter()
            .any(|f| DateTime::parse_from_str(&normalized, f).is_ok())
        || NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

fn usage_exit(error: UsageError) -> ExitCode {
    eprintln!("{}\ndrtable: error: {}", error.0, error.1);
    ExitCode::from(2)
}

fn main() -> ExitCode {
    i18n::init_from_env();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&argv) {
        Ok(Parsed::Run(args)) => args,
        Ok(Parsed::Exit(code)) => return code,
        Err(error) => return usage_exit(error),
    };
    run(args)
}

fn print_errors(messages: &[String]) {
    for message in messages {
        eprintln!("{message}");
    }
}

fn run(args: Args) -> ExitCode {
    let usage = |message: String| usage_exit(UsageError(format!("usage: drtable {} [options]", args.command), message));
    if args.command == "check" && args.get("client").is_some() {
        let mut failures = Vec::new();
        for directory in [args.get("client"), args.get("server")].into_iter().flatten() {
            match check::check_directory(Path::new(directory)) {
                Ok((found, warnings)) => {
                    for warning in warnings {
                        eprintln!("{directory}: {}: {warning}", tr("경고", "warning"));
                    }
                    failures.extend(found.into_iter().map(|f| format!("{directory}: {f}")));
                }
                Err(error) => {
                    eprintln!("{}", error.0);
                    return ExitCode::from(2);
                }
            }
        }
        print_errors(&failures);
        return if failures.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) };
    }
    if args.command == "new" {
        let schema_dir = args.path("schema").unwrap();
        let out = args.path("out").unwrap();
        let mut errors = ErrorCollector::default();
        let schemas = match schemafile::load_schemas(&schema_dir, &schemafile::enum_folder(&schema_dir, args.path("enums").as_deref(), true), &mut errors) {
            Ok(schemas) => schemas,
            Err(ValidationErrors(messages)) => {
                print_errors(&messages);
                return ExitCode::from(1);
            }
        };
        if errors.messages.is_empty() && headers::new_workbook(&out, args.get("table").unwrap(), &schemas, &mut errors) {
            println!("{}", out.display());
        }
        print_errors(&errors.messages);
        return if errors.messages.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) };
    }
    if args.command == "check" && (args.get("server").is_some() || args.get("input").is_none()) {
        return usage(tr("check에는 --client 또는 --input이 필요합니다", "check needs --client or --input"));
    }
    let input = args.path("input").unwrap();
    let model = match excel::load_model(&input, args.path("schema").as_deref(), args.path("enums").as_deref()) {
        Ok(model) => model,
        Err(ValidationErrors(messages)) => {
            print_errors(&messages);
            return ExitCode::from(1);
        }
    };
    for warning in &model.warnings {
        eprintln!("{}: {warning}", tr("경고", "warning"));
    }
    if args.command == "check" {
        return ExitCode::SUCCESS;
    }
    if args.command == "graph" {
        return match graph::emit_graph(&model, &args.path("out").unwrap()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        };
    }
    build(&args, &model, usage)
}

fn build(args: &Args, model: &excel::DataModel, usage: impl Fn(String) -> ExitCode) -> ExitCode {
    let options = BuildOptions {
        out_cpp: args.path("out-cpp").unwrap(),
        out_client: args.path("out-client").unwrap(),
        out_server: args.path("out-server").unwrap(),
        prefix: args.get("prefix").unwrap_or("Dr").to_string(),
        stamp: args.get("stamp").map(str::to_string),
        asset_base: args.get("asset-base").unwrap_or(DEFAULT_ASSET_BASE).to_string(),
        asset_base_header: args.get("asset-base-header").map(str::to_string),
        runtime_header: args.get("runtime-header").map(str::to_string),
        asset_name: args.get("asset-name").unwrap_or(DEFAULT_ASSET_NAME).to_string(),
        ue_plugin: args.flags.contains("ue-plugin"),
    };
    match commands::build(model, &options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(BuildError::Usage(message)) => usage(message),
        Err(BuildError::Invalid(messages)) => {
            print_errors(&messages);
            ExitCode::from(1)
        }
        Err(BuildError::Io(message)) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}
