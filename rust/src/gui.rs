//! drtable-gui: a window for DrTableSystem. Build and check, browse tables, see the reference
//! graph, create data workbooks. Uses the same library as `drtable`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use crate::commands::{self, BuildError, BuildOptions};
use crate::errors::{ErrorCollector, ValidationErrors};
use crate::excel::{load_model, DataModel};
use crate::i18n::{self, tr};
use crate::schema::{in_scopes, CLIENT_SCOPES, SERVER_SCOPES};
use crate::schemafile::{enum_folder, folder_of, load_schemas};
use crate::{check, headers, VERSION};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use serde::{Deserialize, Serialize};

/// Opens the window.
pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 760.0]).with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    let launch = Launch::from_args(std::env::args().skip(1).collect());
    eframe::run_native("DrTableSystem", options, Box::new(move |cc| Ok(Box::new(App::new(cc, launch)))))
}

/// Start-up options, so a shortcut can open a project:
/// `drtable-gui --input Design/Tables --schema Design/Tables/Schemas --check`.
#[derive(Default)]
pub struct Launch {
    values: HashMap<String, String>,
    check: bool,
    build: bool,
}

impl Launch {
    pub fn from_args(args: Vec<String>) -> Self {
        let mut launch = Launch::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--check" => launch.check = true,
                "--build" => launch.build = true,
                other => {
                    if let Some(key) = other.strip_prefix("--") {
                        if let Some((k, v)) = key.split_once('=') {
                            launch.values.insert(k.to_string(), v.to_string());
                        } else if let Some(value) = iter.next() {
                            launch.values.insert(key.to_string(), value);
                        }
                    }
                }
            }
        }
        launch
    }

    fn apply(&self, settings: &mut Settings) {
        for (key, value) in &self.values {
            let slot = match key.as_str() {
                "input" => &mut settings.input,
                "schema" => &mut settings.schema,
                "enums" => &mut settings.enums,
                "out-cpp" => &mut settings.out_cpp,
                "out-client" => &mut settings.out_client,
                "out-server" => &mut settings.out_server,
                "prefix" => &mut settings.prefix,
                "asset-name" => &mut settings.asset_name,
                "lang" => {
                    settings.korean = value == "ko";
                    continue;
                }
                _ => continue,
            };
            *slot = value.clone();
        }
    }
}

/// Everything the user sets; kept between runs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    korean: bool,
    input: String,
    schema: String,
    enums: String,
    out_cpp: String,
    out_client: String,
    out_server: String,
    prefix: String,
    ue_plugin: bool,
    asset_name: String,
    new_table: String,
    new_out: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            korean: true,
            input: String::new(),
            schema: String::new(),
            enums: String::new(),
            out_cpp: String::new(),
            out_client: String::new(),
            out_server: String::new(),
            prefix: "Dr".into(),
            ue_plugin: true,
            asset_name: "DA_{table}".into(),
            new_table: String::new(),
            new_out: String::new(),
        }
    }
}

impl Settings {
    fn schema_root(&self) -> PathBuf {
        if self.schema.trim().is_empty() { folder_of(Path::new(self.input.trim())) } else { PathBuf::from(self.schema.trim()) }
    }

    fn enum_root(&self) -> Option<PathBuf> {
        (!self.enums.trim().is_empty()).then(|| PathBuf::from(self.enums.trim()))
    }

    /// The enum folder in use: the one set, or "Enums" next to the schema folder
    /// (inside the data folder when no schema folder is set).
    fn enum_folder(&self) -> PathBuf {
        enum_folder(&self.schema_root(), self.enum_root().as_deref(), !self.schema.trim().is_empty())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Build,
    Tables,
    Graph,
    Files,
}

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Error,
    Warning,
    Info,
}

/// One line of output: a location such as "[Items.xlsx]Items!B4" and the message.
#[derive(Clone)]
struct Message {
    level: Level,
    location: String,
    text: String,
}

impl Message {
    fn parse(level: Level, line: &str) -> Self {
        // "[File.xlsx]Sheet!Cell: message" or "folder: warning: message".
        match line.split_once(": ") {
            Some((location, text)) if location.contains('!') || location.starts_with('[') => {
                Message { level, location: location.to_string(), text: text.to_string() }
            }
            _ => Message { level, location: String::new(), text: line.to_string() },
        }
    }

    /// The workbook named in the location, e.g. "Items.xlsx" from "[Items.xlsx]Items!B4".
    fn file(&self) -> Option<&str> {
        let rest = self.location.strip_prefix('[')?;
        rest.split_once(']').map(|(file, _)| file).filter(|f| !f.is_empty())
    }
}

/// The result of work done on a background thread.
enum Outcome {
    Checked { model: Option<Arc<DataModel>>, messages: Vec<Message>, summary: String },
    Files { messages: Vec<Message>, summary: String },
}

pub struct App {
    settings: Settings,
    tab: Tab,
    running: Option<Receiver<Outcome>>,
    messages: Vec<Message>,
    summary: String,
    show_warnings: bool,
    model: Option<Arc<DataModel>>,
    broken: BTreeSet<(String, String)>,
    selected: Option<String>,
    positions: HashMap<String, Pos2>,
    dragging: Option<String>,
    tables_for_new: Vec<String>,
    schema_list_tried: bool,
    /// `--screenshot <file.png>`: save the window once the work is done, then quit (for docs).
    screenshot: Option<(PathBuf, u32)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        install_fonts(&cc.egui_ctx);
        let mut settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        launch.apply(&mut settings);
        i18n::set_language(if settings.korean { "ko" } else { "en" });
        let tab = match launch.values.get("tab").map(String::as_str) {
            Some("tables") => Tab::Tables,
            Some("graph") => Tab::Graph,
            Some("files") => Tab::Files,
            _ => Tab::Build,
        };
        let mut app = Self {
            settings,
            tab: Tab::Build,
            running: None,
            messages: Vec::new(),
            summary: String::new(),
            show_warnings: true,
            model: None,
            broken: BTreeSet::new(),
            selected: None,
            positions: HashMap::new(),
            dragging: None,
            tables_for_new: Vec::new(),
            schema_list_tried: false,
            screenshot: launch.values.get("screenshot").map(|p| (PathBuf::from(p), 0)),
        };
        app.tab = tab;
        app.selected = launch.values.get("table").cloned();
        if launch.check || launch.build {
            app.run_check(&cc.egui_ctx, launch.build);
        }
        app
    }

    fn start(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Outcome + Send + 'static) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let korean = self.settings.korean;
        std::thread::spawn(move || {
            i18n::set_language(if korean { "ko" } else { "en" });
            let _ = sender.send(work());
            ctx.request_repaint();
        });
        self.running = Some(receiver);
        self.summary = tr("작업 중…", "Working…");
    }

    /// True while background work (check, build, new) is running.
    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    fn poll(&mut self) {
        let Some(receiver) = &self.running else { return };
        let Ok(outcome) = receiver.try_recv() else { return };
        self.running = None;
        match outcome {
            Outcome::Checked { model, messages, summary } => {
                self.broken = broken_references(&messages);
                if model.is_some() {
                    self.model = model;
                }
                self.messages = messages;
                self.summary = summary;
            }
            Outcome::Files { messages, summary } => {
                self.messages = messages;
                self.summary = summary;
            }
        }
    }

    fn run_check(&mut self, ctx: &egui::Context, build: bool) {
        let settings = self.settings.clone();
        self.start(ctx, move || check_or_build(&settings, build));
    }
}

/// Loads the model; with `build`, writes the outputs and checks the references in them.
fn check_or_build(settings: &Settings, build: bool) -> Outcome {
    let input = PathBuf::from(settings.input.trim());
    if settings.input.trim().is_empty() {
        let text = tr("데이터 폴더를 지정하세요", "choose the data folder");
        return Outcome::Checked { model: None, messages: vec![Message::parse(Level::Error, &text)], summary: text };
    }
    let schema = (!settings.schema.trim().is_empty()).then(|| PathBuf::from(settings.schema.trim()));
    let enums = settings.enum_root();
    let model = match load_model(&input, schema.as_deref(), enums.as_deref()) {
        Ok(model) => model,
        Err(ValidationErrors(errors)) => {
            let count = errors.len();
            return Outcome::Checked {
                model: None,
                messages: errors.iter().map(|e| Message::parse(Level::Error, e)).collect(),
                summary: tr(format!("오류 {count}개"), format!("{count} errors")),
            };
        }
    };
    let mut messages: Vec<Message> = model.warnings.iter().map(|w| Message::parse(Level::Warning, w)).collect();
    let tables = model.tables.len();
    let rows: usize = model.tables.iter().map(|t| t.rows.len()).sum();
    if !build {
        let summary = tr(format!("검사 통과: 테이블 {tables}개, 행 {rows}개"), format!("Check passed: {tables} tables, {rows} rows"));
        return Outcome::Checked { model: Some(Arc::new(model)), messages, summary };
    }
    let options = BuildOptions {
        out_cpp: PathBuf::from(settings.out_cpp.trim()),
        out_client: PathBuf::from(settings.out_client.trim()),
        out_server: PathBuf::from(settings.out_server.trim()),
        prefix: settings.prefix.trim().to_string(),
        asset_name: settings.asset_name.trim().to_string(),
        ue_plugin: settings.ue_plugin,
        ..BuildOptions::default()
    };
    if [&settings.out_cpp, &settings.out_client, &settings.out_server].iter().any(|p| p.trim().is_empty()) {
        let text = tr("출력 폴더 세 개를 모두 지정하세요", "choose all three output folders");
        messages.push(Message::parse(Level::Error, &text));
        return Outcome::Checked { model: Some(Arc::new(model)), messages, summary: text };
    }
    let failed = match commands::build(&model, &options) {
        Ok(()) => None,
        Err(BuildError::Usage(message)) | Err(BuildError::Io(message)) => Some(vec![message]),
        Err(BuildError::Invalid(errors)) => Some(errors),
    };
    if let Some(errors) = failed {
        let count = errors.len();
        messages.extend(errors.iter().map(|e| Message::parse(Level::Error, e)));
        return Outcome::Checked { model: Some(Arc::new(model)), messages, summary: tr(format!("빌드 실패: 오류 {count}개"), format!("Build failed: {count} errors")) };
    }
    let mut unique: BTreeSet<String> = BTreeSet::new();
    for (folder, side) in [(&options.out_client, tr("클라 JSON", "client JSON")), (&options.out_server, tr("서버 JSON", "server JSON"))] {
        match check::check_directory(folder) {
            Ok((failures, warnings)) => {
                for failure in failures {
                    unique.insert(failure.clone());
                    messages.push(Message { level: Level::Error, location: side.clone(), text: failure });
                }
                for warning in warnings {
                    messages.push(Message { level: Level::Warning, location: side.clone(), text: warning });
                }
            }
            Err(error) => messages.push(Message::parse(Level::Error, &error.0)),
        }
    }
    let broken = unique.len();
    let summary = if broken == 0 {
        tr(format!("빌드 완료: 테이블 {tables}개, 행 {rows}개, 참조 문제 없음"), format!("Built {tables} tables, {rows} rows; references OK"))
    } else {
        tr(format!("빌드 완료, 끊긴 참조 {broken}개"), format!("Built; broken references: {broken}"))
    };
    Outcome::Checked { model: Some(Arc::new(model)), messages, summary }
}

/// (table, field) pairs named by broken-reference reports such as "Quests.Next[2](0) = 9 → …".
fn broken_references(messages: &[Message]) -> BTreeSet<(String, String)> {
    messages
        .iter()
        .filter(|m| m.text.contains(" → ") && m.level == Level::Error)
        .filter_map(|m| {
            let head = m.text.split('[').next()?;
            let (table, field) = head.split_once('.')?;
            Some((table.to_string(), field.to_string()))
        })
        .collect()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        self.take_screenshot(&ctx);
        i18n::set_language(if self.settings.korean { "ko" } else { "en" });
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("DrTableSystem");
                ui.label(format!("v{VERSION}"));
                ui.separator();
                for (tab, label) in [
                    (Tab::Build, tr("빌드·검사", "Build & check")),
                    (Tab::Tables, tr("테이블", "Tables")),
                    (Tab::Graph, tr("참조 그래프", "References")),
                    (Tab::Files, tr("새 파일", "New file")),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(tr("설정 초기화", "Reset settings"))
                        .on_hover_text(tr("저장된 폴더·접두사 설정을 지우고 기본값으로 돌립니다", "Forget the saved folders and prefix and use the defaults"))
                        .clicked()
                    {
                        let korean = self.settings.korean;
                        self.settings = Settings { korean, ..Settings::default() };
                        self.tables_for_new.clear();
                        self.schema_list_tried = false;
                    }
                    ui.separator();
                    if ui.selectable_label(!self.settings.korean, "English").clicked() {
                        self.settings.korean = false;
                    }
                    if ui.selectable_label(self.settings.korean, "한국어").clicked() {
                        self.settings.korean = true;
                    }
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.running.is_some() {
                    ui.spinner();
                }
                ui.label(&self.summary);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Build => self.build_tab(ui, &ctx),
            Tab::Tables => self.tables_tab(ui),
            Tab::Graph => self.graph_tab(ui),
            Tab::Files => self.files_tab(ui, &ctx),
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}

/// A labelled path field with a folder (or file) picker.
fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, pick: Pick) {
    ui.label(label);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(tr("찾기…", "Browse…")).clicked() {
            let dialog = rfd::FileDialog::new();
            let dialog = match Path::new(value.trim()) {
                p if p.is_dir() => dialog.set_directory(p),
                p => match p.parent() {
                    Some(parent) if parent.is_dir() => dialog.set_directory(parent),
                    _ => dialog,
                },
            };
            let picked = match pick {
                Pick::Folder => dialog.pick_folder(),
                Pick::SaveXlsx => dialog.add_filter("Excel", &["xlsx"]).save_file(),
            };
            if let Some(path) = picked {
                *value = path.display().to_string();
            }
        }
        ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    });
    ui.end_row();
}

#[derive(Clone, Copy)]
enum Pick {
    Folder,
    SaveXlsx,
}

impl App {
    fn build_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Grid::new("settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            let s = &mut self.settings;
            path_row(ui, &tr("데이터 폴더", "Data folder"), &mut s.input, Pick::Folder);
            path_row(ui, &tr("스키마 폴더 (비우면 데이터 폴더)", "Schema folder (empty: data folder)"), &mut s.schema, Pick::Folder);
            path_row(ui, &tr("열거형 폴더 (비우면 스키마 폴더 옆 Enums)", "Enum folder (empty: Enums next to the schema folder)"), &mut s.enums, Pick::Folder);
            path_row(ui, &tr("C++ 출력", "C++ output"), &mut s.out_cpp, Pick::Folder);
            path_row(ui, &tr("클라 JSON 출력", "Client JSON output"), &mut s.out_client, Pick::Folder);
            path_row(ui, &tr("서버 JSON 출력", "Server JSON output"), &mut s.out_server, Pick::Folder);
            ui.label(tr("C++ 접두사", "C++ prefix"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.prefix).desired_width(80.0));
                ui.checkbox(&mut s.ue_plugin, tr("언리얼 플러그인용(--ue-plugin)", "Unreal plugin (--ue-plugin)"));
                ui.label(tr("에셋 이름", "Asset name"));
                ui.add(egui::TextEdit::singleline(&mut s.asset_name).desired_width(140.0));
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let idle = self.running.is_none();
            if ui.add_enabled(idle, egui::Button::new(tr("검사", "Check"))).clicked() {
                self.run_check(ctx, false);
            }
            if ui.add_enabled(idle, egui::Button::new(tr("빌드", "Build"))).clicked() {
                self.run_check(ctx, true);
            }
            ui.checkbox(&mut self.show_warnings, tr("경고 보기", "Show warnings"));
            ui.label(tr("오류를 더블클릭하면 파일을 엽니다", "Double-click a message to open its file"));
        });
        ui.separator();
        let roots = [PathBuf::from(self.settings.input.trim()), self.settings.schema_root()];
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("messages").num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.strong("");
                ui.strong(tr("위치", "Location"));
                ui.strong(tr("내용", "Message"));
                ui.end_row();
                for message in &self.messages {
                    if message.level == Level::Warning && !self.show_warnings {
                        continue;
                    }
                    let (icon, color) = match message.level {
                        Level::Error => ("●", Color32::from_rgb(220, 70, 60)),
                        Level::Warning => ("▲", Color32::from_rgb(220, 160, 40)),
                        Level::Info => ("•", ui.visuals().text_color()),
                    };
                    ui.colored_label(color, icon);
                    let location = ui.add(egui::Label::new(egui::RichText::new(&message.location).monospace()).sense(Sense::click()));
                    let text = ui.add(egui::Label::new(&message.text).sense(Sense::click()));
                    if (location.double_clicked() || text.double_clicked())
                        && let Some(path) = message.file().and_then(|f| roots.iter().map(|r| r.join(f)).find(|p| p.exists())) {
                            open_path(&path);
                        }
                    ui.end_row();
                }
            });
        });
    }

    fn tables_tab(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.model.clone() else {
            ui.label(tr("먼저 [빌드·검사]에서 검사나 빌드를 실행하세요", "Run Check or Build first (Build & check tab)"));
            return;
        };
        let schema_root = self.settings.schema_root();
        let input_root = PathBuf::from(self.settings.input.trim());
        egui::Panel::left("table-list").resizable(true).default_size(220.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.strong(tr("테이블", "Tables"));
                for table in &model.tables {
                    let label = format!("{}  ({})", table.name, table.rows.len());
                    if ui.selectable_label(self.selected.as_deref() == Some(table.name.as_str()), label).clicked() {
                        self.selected = Some(table.name.clone());
                    }
                }
                ui.add_space(8.0);
                ui.strong(tr("열거형", "Enums"));
                for e in &model.enums {
                    let key = format!("enum:{}", e.name);
                    if ui.selectable_label(self.selected.as_deref() == Some(key.as_str()), format!("E{}", e.name)).clicked() {
                        self.selected = Some(key);
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                let selected = self.selected.clone().unwrap_or_default();
                if let Some(name) = selected.strip_prefix("enum:") {
                    if let Some(e) = model.enums.iter().find(|e| e.name == name) {
                        ui.heading(format!("E{}", e.name));
                        // Enum files are named relative to the folder that holds the enum folder.
                        let enum_base = self.settings.enum_folder().parent().map(Path::to_path_buf).unwrap_or_default();
                        file_line(ui, &tr("스키마", "Schema"), &e.source_name, &enum_base);
                        egui::Grid::new("enum-values").striped(true).num_columns(3).show(ui, |ui| {
                            ui.strong(tr("이름", "Name"));
                            ui.strong(tr("값", "Value"));
                            ui.strong(tr("설명", "Comment"));
                            ui.end_row();
                            for value in &e.values {
                                ui.monospace(&value.name);
                                ui.monospace(value.value.to_string());
                                ui.label(&value.comment);
                                ui.end_row();
                            }
                        });
                    }
                    return;
                }
                let Some(table) = model.tables.iter().find(|t| t.name == selected) else {
                    ui.label(tr("왼쪽에서 테이블을 고르세요", "Pick a table on the left"));
                    return;
                };
                ui.heading(&table.name);
                file_line(ui, &tr("스키마", "Schema"), &table.schema_file, &schema_root);
                ui.label(format!("{}: {}   {}: {}", tr("행", "Rows"), table.rows.len(), tr("스키마 해시", "Schema hash"), table.schema_hash));
                ui.add_space(6.0);
                ui.strong(tr("필드", "Fields"));
                egui::Grid::new("fields").striped(true).num_columns(6).spacing([14.0, 4.0]).show(ui, |ui| {
                    for header in [tr("이름", "Name"), tr("자료형", "Type"), tr("역할", "Role"), tr("범위", "Scope"), tr("배열", "Array"), tr("참조", "Reference")] {
                        ui.strong(header);
                    }
                    ui.end_row();
                    for column in &table.columns {
                        ui.monospace(&column.name);
                        ui.monospace(&column.type_name);
                        ui.label(match column.role.as_deref() {
                            Some("id") => tr("기본키", "primary key"),
                            Some("subkey") => tr("서브키", "sub key"),
                            _ => String::new(),
                        });
                        let scope = if in_scopes(&column.scope, &CLIENT_SCOPES) && in_scopes(&column.scope, &SERVER_SCOPES) {
                            "all".to_string()
                        } else {
                            column.scope.clone()
                        };
                        ui.label(scope);
                        ui.label(column.array_size.map(|n| format!("[{n}]")).unwrap_or_default());
                        let reference = match (&column.ref_target, &column.ref_key) {
                            (Some(t), Some(k)) => format!("{t}.{k} (1:N)"),
                            (Some(t), None) => t.clone(),
                            _ => String::new(),
                        };
                        if !reference.is_empty() && self.broken.contains(&(table.name.clone(), column.name.clone())) {
                            ui.colored_label(Color32::from_rgb(220, 70, 60), format!("{reference}  ⚠"));
                        } else {
                            ui.label(reference);
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.strong(tr("데이터 위치", "Data"));
                if table.sources.is_empty() {
                    ui.label(tr("데이터 시트가 없습니다(행 0개)", "No data sheet (0 rows)"));
                }
                for part in &table.sources {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{} / {}", part.file, part.sheet));
                        ui.label(format!("{} {}", part.rows, tr("행", "rows")));
                        if ui.small_button(tr("열기", "Open")).clicked() {
                            open_path(&input_root.join(&part.file));
                        }
                    });
                }
            });
        });
    }

    fn graph_tab(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.model.clone() else {
            ui.label(tr("먼저 [빌드·검사]에서 검사나 빌드를 실행하세요", "Run Check or Build first (Build & check tab)"));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(tr("노드를 끌어 옮길 수 있습니다. 빨간 화살표는 끊긴 참조입니다.", "Drag nodes to move them. Red arrows are broken references."));
            if ui.button(tr("배치 초기화", "Reset layout")).clicked() {
                self.positions.clear();
            }
        });
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let area = response.rect;
        if model.tables.iter().any(|t| !self.positions.contains_key(&t.name)) {
            self.positions = layout(&model, area.shrink(70.0));
        }
        let font = FontId::proportional(14.0);
        let small = FontId::proportional(11.5);
        let text_color = ui.visuals().text_color();
        let node_size = |name: &str, key: &str| Vec2::new((name.len().max(key.len() + 2) as f32 * 8.0).max(90.0) + 20.0, 44.0);
        let boxes: HashMap<String, Rect> = model
            .tables
            .iter()
            .map(|t| {
                let size = node_size(&t.name, &t.primary_key().type_name);
                (t.name.clone(), Rect::from_center_size(self.positions[&t.name], size))
            })
            .collect();
        // Dragging a node moves it.
        if response.drag_started()
            && let Some(pointer) = response.interact_pointer_pos() {
                self.dragging = boxes.iter().find(|(_, r)| r.contains(pointer)).map(|(n, _)| n.clone());
            }
        if response.dragged()
            && let Some(name) = &self.dragging
                && let Some(position) = self.positions.get_mut(name) {
                    *position += response.drag_delta();
                }
        if response.drag_stopped() {
            self.dragging = None;
        }
        // Edges: references between the same two tables are spread apart so none overlap.
        struct Edge {
            from: String,
            to: String,
            label: String,
            broken: bool,
        }
        let mut edges: Vec<Edge> = Vec::new();
        let mut self_labels: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        for table in &model.tables {
            for column in &table.columns {
                let Some(target) = &column.ref_target else { continue };
                let broken = self.broken.contains(&(table.name.clone(), column.name.clone()));
                let mut label = column.name.clone();
                if let Some(size) = column.array_size {
                    label += &format!("[{size}]");
                }
                if let Some(key) = &column.ref_key {
                    label += &format!(" → {key} 1:N");
                }
                if target == &table.name {
                    self_labels.entry(table.name.clone()).or_default().push((label, broken));
                } else {
                    edges.push(Edge { from: table.name.clone(), to: target.clone(), label, broken });
                }
            }
        }
        let pair = |e: &Edge| if e.from < e.to { (e.from.clone(), e.to.clone()) } else { (e.to.clone(), e.from.clone()) };
        let mut per_pair: HashMap<(String, String), usize> = HashMap::new();
        for edge in &edges {
            *per_pair.entry(pair(edge)).or_default() += 1;
        }
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        let weak = ui.visuals().weak_text_color();
        let red = Color32::from_rgb(220, 70, 60);
        let label_bg = ui.visuals().panel_fill;
        for edge in &edges {
            let (Some(from), Some(to)) = (boxes.get(&edge.from), boxes.get(&edge.to)) else { continue };
            let key = pair(edge);
            let total = per_pair[&key] as f32;
            let index = seen.entry(key.clone()).or_default();
            // A consistent side for the pair, whichever way this edge points.
            let (a, b) = (boxes[&key.0].center(), boxes[&key.1].center());
            let normal = (b - a).normalized().rot90();
            let offset = normal * ((*index as f32 - (total - 1.0) / 2.0) * 16.0);
            *index += 1;
            let start = edge_point(*from, to.center()) + offset;
            let end = edge_point(*to, from.center()) + offset;
            let color = if edge.broken { red } else { weak };
            draw_arrow(&painter, start, end, Stroke::new(if edge.broken { 2.0 } else { 1.2 }, color));
            let at = start + (end - start) * 0.38;
            let galley = painter.layout_no_wrap(edge.label.clone(), small.clone(), color);
            let rect = Rect::from_center_size(at, galley.size() + Vec2::new(6.0, 2.0));
            painter.rect_filled(rect, 3.0, label_bg);
            painter.galley(rect.min + Vec2::new(3.0, 1.0), galley, color);
        }
        for (name, labels) in &self_labels {
            if let Some(rect) = boxes.get(name) {
                for (index, (label, broken)) in labels.iter().enumerate() {
                    let color = if *broken { red } else { weak };
                    painter.text(rect.center_bottom() + Vec2::new(0.0, 4.0 + index as f32 * 13.0), Align2::CENTER_TOP, format!("↻ {label}"), small.clone(), color);
                }
            }
        }
        // Nodes.
        for table in &model.tables {
            let rect = boxes[&table.name];
            let hovered = response.hover_pos().is_some_and(|p| rect.contains(p));
            painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
            painter.rect_stroke(rect, 6.0, Stroke::new(if hovered { 2.0 } else { 1.0 }, ui.visuals().widgets.active.bg_stroke.color), StrokeKind::Inside);
            painter.text(rect.center() - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, &table.name, font.clone(), text_color);
            painter.text(rect.center() + Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, format!("({})", table.primary_key().type_name), small.clone(), ui.visuals().weak_text_color());
        }
        if response.double_clicked()
            && let Some(pointer) = response.interact_pointer_pos()
                && let Some((name, _)) = boxes.iter().find(|(_, r)| r.contains(pointer)) {
                    self.selected = Some(name.clone());
                    self.tab = Tab::Tables;
                }
    }

    fn load_schema_list(&mut self) {
        let mut errors = ErrorCollector::default();
        let root = self.settings.schema_root();
        match load_schemas(&root, &self.settings.enum_folder(), &mut errors) {
            Ok(schemas) => self.tables_for_new = schemas.tables.keys().cloned().collect(),
            Err(ValidationErrors(messages)) => errors.messages.extend(messages),
        }
        if !self.tables_for_new.contains(&self.settings.new_table) {
            self.settings.new_table = self.tables_for_new.first().cloned().unwrap_or_default();
        }
        if !errors.messages.is_empty() {
            self.messages = errors.messages.iter().map(|m| Message::parse(Level::Error, m)).collect();
        }
    }

    fn files_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.tables_for_new.is_empty() && !self.settings.input.trim().is_empty() && !self.schema_list_tried {
            self.schema_list_tried = true;
            self.load_schema_list();
        }
        ui.heading(tr("새 데이터 파일", "New data workbook"));
        ui.label(tr(
            "스키마의 필드명과 참고 수식(2·3행)이 든 새 파일을 만듭니다. 이미 있는 파일은 고치지 않습니다.",
            "Creates a workbook with the schema's field names and the reference formulas (rows 2-3). Existing files are never changed.",
        ));
        egui::Grid::new("new").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(tr("테이블", "Table"));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("new-table").selected_text(&self.settings.new_table).show_ui(ui, |ui| {
                    for name in &self.tables_for_new {
                        ui.selectable_value(&mut self.settings.new_table, name.clone(), name);
                    }
                });
                if ui.button(tr("스키마 목록 다시 읽기", "Reload schema list")).clicked() {
                    self.load_schema_list();
                    self.summary = tr(format!("스키마 {}개", self.tables_for_new.len()), format!("{} schemas", self.tables_for_new.len()));
                }
            });
            ui.end_row();
            path_row(ui, &tr("만들 파일", "File to create"), &mut self.settings.new_out, Pick::SaveXlsx);
        });
        let idle = self.running.is_none();
        if ui.add_enabled(idle && !self.settings.new_table.is_empty() && !self.settings.new_out.trim().is_empty(), egui::Button::new(tr("만들기", "Create"))).clicked() {
            let settings = self.settings.clone();
            self.start(ctx, move || {
                let root = settings.schema_root();
                let out = PathBuf::from(settings.new_out.trim());
                let mut errors = ErrorCollector::default();
                let created = match load_schemas(&root, &settings.enum_folder(), &mut errors) {
                    Ok(schemas) => errors.messages.is_empty() && headers::new_workbook(&out, &settings.new_table, &schemas, &mut errors),
                    Err(ValidationErrors(messages)) => {
                        errors.messages.extend(messages);
                        false
                    }
                };
                let mut messages: Vec<Message> = errors.messages.iter().map(|m| Message::parse(Level::Error, m)).collect();
                if created {
                    messages.push(Message { level: Level::Info, location: String::new(), text: out.display().to_string() });
                    open_path(&out);
                }
                let summary = if created { tr("새 파일을 만들었습니다", "Created the workbook") } else { tr("만들지 못했습니다", "Nothing was created") };
                Outcome::Files { messages, summary }
            });
            self.tab = Tab::Build;
        }
    }
}

fn file_line(ui: &mut egui::Ui, label: &str, file: &str, root: &Path) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        ui.monospace(file);
        if ui.small_button(tr("열기", "Open")).clicked() {
            open_path(&root.join(file));
        }
    });
}

impl App {
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.screenshot else { return };
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let _ = save_png(path, &image);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        *frames += 1;
        if self.running.is_none() && *frames == 30 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint();
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}

/// A line with a small arrowhead at `end`.
fn draw_arrow(painter: &egui::Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let direction = end - start;
    if direction.length() < 1.0 {
        return;
    }
    let unit = direction.normalized();
    let head = 9.0;
    let base = end - unit * head;
    let side = unit.rot90() * (head * 0.45);
    painter.line_segment([start, base], stroke);
    painter.add(egui::Shape::convex_polygon(vec![end, base + side, base - side], stroke.color, Stroke::NONE));
}

/// Places the tables with a small force-directed layout: nodes push each other apart and
/// references pull them together. Deterministic: it starts from a circle.
fn layout(model: &DataModel, area: Rect) -> HashMap<String, Pos2> {
    let names: Vec<&str> = model.tables.iter().map(|t| t.name.as_str()).collect();
    let n = names.len().max(1);
    let index: HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    let mut links: Vec<(usize, usize)> = Vec::new();
    for table in &model.tables {
        for column in &table.columns {
            if let (Some(target), Some(&a)) = (&column.ref_target, index.get(table.name.as_str()))
                && let Some(&b) = index.get(target.as_str()) {
                    // One pull per pair of tables, however many references join them.
                    if a != b && !links.contains(&(a.min(b), a.max(b))) {
                        links.push((a.min(b), a.max(b)));
                    }
                }
        }
    }
    let center = area.center();
    let radius = area.width().min(area.height()) * 0.4;
    let mut points: Vec<Vec2> = (0..names.len())
        .map(|i| {
            let angle = i as f32 / n as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect();
    let k = (area.width() * area.height() / n as f32).sqrt() * 0.75;
    let mut temperature = area.width().min(area.height()) * 0.1;
    for _ in 0..300 {
        let mut moves = vec![Vec2::ZERO; points.len()];
        for i in 0..points.len() {
            for j in 0..points.len() {
                if i != j {
                    let delta = points[i] - points[j];
                    let distance = delta.length().max(1.0);
                    moves[i] += delta / distance * (k * k / distance);
                }
            }
        }
        for &(a, b) in &links {
            let delta = points[a] - points[b];
            let distance = delta.length().max(1.0);
            let pull = delta / distance * (distance * distance / k);
            moves[a] -= pull;
            moves[b] += pull;
        }
        for (point, movement) in points.iter_mut().zip(moves) {
            let length = movement.length();
            if length > 0.0 {
                *point += movement / length * length.min(temperature);
            }
            point.x = point.x.clamp(-area.width() / 2.0, area.width() / 2.0);
            point.y = point.y.clamp(-area.height() / 2.0, area.height() / 2.0);
        }
        temperature *= 0.985;
    }
    names.iter().zip(points).map(|(name, point)| (name.to_string(), center + point)).collect()
}

/// Where the line from a box's center toward `toward` leaves the box.
fn edge_point(rect: Rect, toward: Pos2) -> Pos2 {
    let center = rect.center();
    let direction = toward - center;
    if direction.length() < 1.0 {
        return center;
    }
    let scale_x = if direction.x.abs() > 0.0 { rect.width() / 2.0 / direction.x.abs() } else { f32::INFINITY };
    let scale_y = if direction.y.abs() > 0.0 { rect.height() / 2.0 / direction.y.abs() } else { f32::INFINITY };
    center + direction * scale_x.min(scale_y)
}

/// Opens a file or folder with the system's default application (Excel for .xlsx).
fn open_path(path: &Path) {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    let _ = result;
}

/// egui's built-in fonts have no Hangul, so a system font is added as a fallback.
fn install_fonts(ctx: &egui::Context) {
    let candidates: &[(&str, u32)] = &[
        ("C:\\Windows\\Fonts\\malgun.ttf", 0),
        ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
        ("/System/Library/Fonts/Supplemental/AppleGothic.ttf", 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 1),
        ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 1),
        ("/usr/share/fonts/truetype/nanum/NanumGothic.ttf", 0),
    ];
    let Some((bytes, index)) = candidates.iter().find_map(|(path, index)| std::fs::read(path).ok().map(|b| (b, *index))) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    fonts.font_data.insert("hangul".into(), Arc::new(data));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("hangul".into());
    }
    ctx.set_fonts(fonts);
}
