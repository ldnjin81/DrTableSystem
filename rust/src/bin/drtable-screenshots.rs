//! Renders every tab of the GUI without a window and saves PNG files:
//! `drtable-screenshots <out folder> <drtable-gui start-up options...>`.

use std::path::PathBuf;

use drtable::gui::{App, Launch};
use egui_kittest::Harness;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: drtable-screenshots <out folder> [--input ... --schema ... --build ...]");
        std::process::exit(2);
    }
    let out = PathBuf::from(args.remove(0));
    std::fs::create_dir_all(&out).expect("output folder");
    for tab in ["build", "tables", "graph", "files"] {
        let mut launch_args = args.clone();
        launch_args.extend(["--tab".to_string(), tab.to_string()]);
        let mut harness = Harness::builder()
            .with_size(eframe::egui::Vec2::new(1180.0, 760.0))
            .wgpu()
            .build_eframe(|cc| App::new(cc, Launch::from_args(launch_args)));
        for _ in 0..600 {
            harness.step();
            if !harness.state().is_busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        harness.run_steps(8);
        let image = harness.render().expect("render");
        let path = out.join(format!("{tab}.png"));
        image.save(&path).expect("save png");
        println!("{}", path.display());
    }
}
