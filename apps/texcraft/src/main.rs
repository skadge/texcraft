//! TeXCraft for the desktop: `texcraft [FOLDER]`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use serde_json::json;

struct Shell(texcraft_ui_egui::App);

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let shell = &mut self.0;
        if let Err(e) = texcraft_engine::guard::catch_panic(|| shell.frame(ui)) {
            log::error!("frame: {e}");
        }
    }
}

fn main() -> eframe::Result {
    texcraft_ui_egui::i18n::use_system_language();
    let mut session = texcraft_engine::Session::new();
    if let Some(folder) = std::env::args().nth(1)
        && let Err(e) = session.execute("project.open", &json!({"path": folder}))
    {
        eprintln!("{e}");
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("TeXCraft").with_app_id("ai.storyteller.texcraft").with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("TeXCraft", options, Box::new(|_cc| Ok(Box::new(Shell(texcraft_ui_egui::App::new(session))))))
}
