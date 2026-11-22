//! TeXCraft for the desktop: `texcraft [--control [PORT]] [FOLDER]`.
//!
//! `--control` (or `TEXCRAFT_CONTROL_PORT`) starts the control channel for agents and scripts,
//! docs/control-protocol.md.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod control;
mod thumbnails;

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
    control: Option<control::Control>,
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
    /// Runs before `ui`, also while the window is hidden: closing asks about unsaved changes first
    /// (craft-quit's order: the app's decision, then `on_close_request`).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && self.app.close_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if self.app.take_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        craft_quit::on_close_request(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(control) = &self.control {
            control.answer(&mut self.app);
        }
        let app = &mut self.app;
        if let Err(e) = texcraft_engine::guard::catch_panic(|| app.frame(ui)) {
            log::error!("frame: {e}");
        }
        self.pickers(frame);
        for thumbnail in self.app.take_thumbnails() {
            thumbnails::store(&thumbnail);
        }
        if let Some(prefs) = self.app.take_changed_prefs() {
            self.settings.set(Stored(prefs));
        }
        self.settings.tick(Instant::now());
    }

    fn on_exit(&mut self) {
        if let Some(control) = &mut self.control {
            control.shut_down();
        }
        if let Err(e) = self.settings.flush() {
            log::warn!("saving the preferences: {e}");
        }
    }
}

/// The window and taskbar icon: on Apple's grid on macOS, the full tile elsewhere (assets/app-icon).
#[cfg(target_os = "macos")]
const WINDOW_ICON: &[u8] = include_bytes!("../../../assets/app-icon/texcraft-macos-512.png");
#[cfg(not(target_os = "macos"))]
const WINDOW_ICON: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.texcraft.png");

const USAGE: &str = "usage: texcraft [--control [PORT]] [FOLDER]

  FOLDER            open the project in this folder
  --control [PORT]  start the control channel for agents and scripts on PORT (default: the first
                    free port from 7841); see docs/control-protocol.md. Also TEXCRAFT_CONTROL_PORT.
  --version         print the version";

/// What the command line asks for.
struct Args {
    folder: Option<String>,
    /// `Some(None)`: the control channel on the default port.
    control: Option<Option<u16>>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { folder: None, control: None };
    if let Some(port) = std::env::var("TEXCRAFT_CONTROL_PORT").ok().filter(|p| !p.is_empty()) {
        args.control = Some(Some(port.parse().map_err(|_| format!("TEXCRAFT_CONTROL_PORT is not a port: {port}"))?));
    }
    let mut list = std::env::args().skip(1).peekable();
    while let Some(a) = list.next() {
        match a.as_str() {
            "--control" => {
                let port = match list.peek() {
                    Some(next) if !next.starts_with('-') && next.parse::<u16>().is_ok() => list.next().and_then(|p| p.parse().ok()),
                    _ => None,
                };
                args.control = Some(port);
            }
            "--version" => {
                println!("texcraft {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            folder => args.folder = Some(folder.to_owned()),
        }
    }
    Ok(args)
}

fn main() -> eframe::Result {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    // Warnings and errors by default; RUST_LOG=info (or debug) for more.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    texcraft_ui_egui::i18n::use_system_language();
    let (settings, report) = SettingsStore::<Stored>::open(Location::Auto);
    log::info!("preferences: {report:?}");
    let prefs = settings.get().0.clone();
    let mut app = texcraft_ui_egui::App::with_prefs(texcraft_engine::Session::new(), prefs);
    app.set_native_pickers(true);
    for project in &settings.get().0.recent {
        if let Some(thumbnail) = thumbnails::load(project) {
            app.set_thumbnail(thumbnail);
        }
    }
    match &args.folder {
        Some(folder) => app.open_project(folder),
        None => app.show_start_page(),
    }
    let mut viewport =
        egui::ViewportBuilder::default().with_title("TeXCraft").with_app_id("ai.storyteller.texcraft").with_inner_size([1400.0, 900.0]);
    match eframe::icon_data::from_png_bytes(WINDOW_ICON) {
        Ok(icon) => viewport = viewport.with_icon(Arc::new(icon)),
        Err(e) => log::warn!("the window icon: {e}"),
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "TeXCraft",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let wake = ctx.clone();
            let dialogs = Dialogs::new(Arc::new(move || wake.request_repaint()));
            let control = match args.control {
                Some(port) => control::Control::start(port, ctx).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    None
                }),
                None => None,
            };
            Ok(Box::new(Shell { app, settings, dialogs, control }))
        }),
    )
}
