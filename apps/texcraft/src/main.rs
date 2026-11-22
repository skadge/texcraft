//! TeXCraft for the desktop: `texcraft [FOLDER]`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Instant;

use craft_dialogs::{Answer, Dialogs, FileDialog};
use craft_settings::{Location, SettingsStore};
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{Pick, PickKind, PickRequest};

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
    /// Native pickers, shown without blocking the event loop.
    dialogs: Dialogs<Pick>,
}

fn file_dialog(req: PickRequest) -> FileDialog {
    let mut dialog = match req.kind {
        PickKind::Folder => FileDialog::folder(),
        PickKind::OpenFiles => FileDialog::open_many(),
        PickKind::SaveFile => FileDialog::save(),
    }
    .title(req.title);
    if let Some(dir) = req.directory {
        dialog = dialog.directory(dir);
    }
    if let Some(name) = req.file_name {
        dialog = dialog.file_name(name);
    }
    if let Some((name, extensions)) = req.filter {
        dialog = dialog.filter(name, &extensions);
    }
    dialog
}

impl Shell {
    fn pickers(&mut self, frame: &eframe::Frame) {
        for req in self.app.take_picks() {
            let pick = req.pick;
            self.dialogs.ask(file_dialog(req), pick);
        }
        self.dialogs.show_pending(Some(frame));
        for (pick, answer) in self.dialogs.take_answers() {
            match answer {
                Answer::Paths(paths) if !paths.is_empty() => self.app.picked(pick, paths),
                Answer::Unavailable(reason) => self.app.pick_failed(reason),
                _ => {}
            }
        }
    }
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let app = &mut self.app;
        if let Err(e) = texcraft_engine::guard::catch_panic(|| app.frame(ui)) {
            log::error!("frame: {e}");
        }
        self.pickers(frame);
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
    app.set_native_pickers(true);
    if let Some(folder) = std::env::args().nth(1) {
        app.open_project(&folder);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("TeXCraft").with_app_id("ai.storyteller.texcraft").with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "TeXCraft",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let dialogs = Dialogs::new(Arc::new(move || ctx.request_repaint()));
            Ok(Box::new(Shell { app, settings, dialogs }))
        }),
    )
}
