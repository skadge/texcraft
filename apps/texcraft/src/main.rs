//! TeXCraft for the desktop: `texcraft [FOLDER]`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::Instant;

use craft_settings::{Location, SettingsStore};
use texcraft_ui_egui::prefs::Prefs;

/// The preferences as stored in craft-settings' `config.toml`.
#[derive(Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Stored(Prefs);

impl craft_settings::Settings for Stored {
    const APP_ID: &'static str = "texcraft";
    const SCHEMA_VERSION: u32 = 1;
}

struct Shell {
    app: texcraft_ui_egui::App,
    settings: SettingsStore<Stored>,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let app = &mut self.app;
        if let Err(e) = texcraft_engine::guard::catch_panic(|| app.frame(ui)) {
            log::error!("frame: {e}");
        }
        if let Some(prefs) = self.app.take_changed_prefs() {
            self.settings.set(Stored(prefs));
        }
        self.settings.tick(Instant::now());
    }

    fn on_exit(&mut self) {
        if let Err(e) = self.settings.flush() {
            log::warn!("saving the preferences: {e}");
        }
    }
}

fn main() -> eframe::Result {
    texcraft_ui_egui::i18n::use_system_language();
    let (settings, report) = SettingsStore::<Stored>::open(Location::Auto);
    log::info!("preferences: {report:?}");
    let prefs = settings.get().0.clone();
    let mut app = texcraft_ui_egui::App::with_prefs(texcraft_engine::Session::new(), prefs);
    if let Some(folder) = std::env::args().nth(1) {
        app.open_project(&folder);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("TeXCraft").with_app_id("ai.storyteller.texcraft").with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("TeXCraft", options, Box::new(|_cc| Ok(Box::new(Shell { app, settings }))))
}
