//! drtable-gui: the DrTableSystem window (build and check, tables, reference graph, new files).

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> eframe::Result {
    drtable::gui::run()
}
