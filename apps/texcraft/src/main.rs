//! TeXCraft for the desktop: `texcraft [--control [PORT]] [FOLDER | FILE]`.
//!
//! `--control` (or `TEXCRAFT_CONTROL_PORT`) starts the control channel for agents and scripts,
//! docs/control-protocol.md.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod control;
#[cfg(target_os = "macos")]
mod mac_menu;
mod thumbnails;

use std::sync::Arc;
use std::time::Instant;

use craft_dialogs::{Answer, Dialogs, FileDialog};
use craft_settings::{Location, SaveStatus, SettingsStore};
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
    /// The last save of the preferences failed (and was said).
    save_failed: bool,
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

impl Shell {
    /// Saves changed preferences once they settle, and comes back when they are due even if the
    /// window is idle (what the user agreed to send to an AI service is in them, and a run ended
    /// from a terminal never reaches `on_exit`). A save that fails is said once on the status line.
    fn save_prefs(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let tick = self.settings.tick(now);
        if let Some(due) = tick.next_deadline {
            ctx.request_repaint_after(due.saturating_duration_since(now));
        }
        match tick.status {
            SaveStatus::Failed(e) if !self.save_failed => {
                self.save_failed = true;
                log::warn!("saving the preferences: {e}");
                let what = texcraft_ui_egui::i18n::t("The preferences could not be saved, so changes are kept for this session only: {error}");
                self.app.report(what.replace("{error}", &e.to_string()), true);
            }
            SaveStatus::Saved => self.save_failed = false,
            _ => {}
        }
    }
}

impl eframe::App for Shell {
    /// The system menu's key equivalents go back into egui's input as the key presses they were.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.app.system_menu_input(raw_input);
    }

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
            control.answer(&mut self.app, ui.ctx());
        }
        let app = &mut self.app;
        if let Err(e) = texcraft_engine::guard::catch_panic(|| app.frame(ui)) {
            log::error!("frame: {e}");
        }
        self.pickers(frame);
        for path in self.app.take_reveals() {
            if let Err(e) = craft_update::process::reveal(&path) {
                let what = texcraft_ui_egui::i18n::t("The file manager did not open: {error}");
                self.app.report(what.replace("{error}", &e), true);
            }
        }
        for thumbnail in self.app.take_thumbnails() {
            thumbnails::store(&thumbnail);
        }
        if let Some(prefs) = self.app.take_changed_prefs() {
            self.settings.set(Stored(prefs));
        }
        self.save_prefs(ui.ctx());
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
const WINDOW_ICON: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/io.gitlab.severin_lemaignan.TeXCraft.png");

const USAGE: &str = "usage: texcraft [--control [PORT]] [FOLDER | FILE]

  FOLDER            open the project in this folder
  FILE              open the project in the file's folder, showing the file
  LINK              a link that proposes a GitLab or GitHub Enterprise server to sign in to
                    (https://…?gitlab=…&client_id=…): TeXCraft asks before adding it
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

/// Whether `arg` is a link that proposes a server (docs/forges.md): its query, if so.
fn server_link(arg: &str) -> Option<&str> {
    let (_, query) = arg.strip_prefix("https://").or_else(|| arg.strip_prefix("http://"))?.split_once('?')?;
    let pairs = texcraft_engine::forge::oauth::parse_query(query);
    pairs.iter().any(|(k, _)| k == "gitlab" || k == "github").then_some(query)
}

/// The servers an organisation provides (docs/forges.md, "Self-managed servers"): the
/// `texcraft-forges.toml` of the system-wide folder (`/etc/texcraft`, `/Library/Application
/// Support/TeXCraft`, `%ProgramData%\TeXCraft`), then of the preferences' folder; the first to
/// list a server wins. What cannot be read is said in Settings › Accounts.
fn provided_servers(config: Option<&std::path::Path>) -> (Vec<texcraft_engine::forge::servers::Server>, Vec<String>) {
    use texcraft_engine::forge::servers;
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    dirs.push("/Library/Application Support/TeXCraft".into());
    #[cfg(all(unix, not(target_os = "macos")))]
    dirs.push("/etc/texcraft".into());
    #[cfg(windows)]
    if let Some(data) = std::env::var_os("ProgramData") {
        dirs.push(std::path::PathBuf::from(data).join("TeXCraft"));
    }
    dirs.extend(config.map(std::path::Path::to_path_buf));
    let (mut found, mut problems) = (Vec::<servers::Server>::new(), Vec::new());
    for dir in dirs {
        let file = dir.join(servers::CONFIG_FILE);
        match std::fs::read_to_string(&file) {
            Ok(text) => match servers::parse_config(&text) {
                Ok((listed, left_out)) => {
                    for server in listed {
                        if !found.iter().any(|s| s.instance == server.instance) {
                            found.push(server);
                        }
                    }
                    problems.extend(left_out.into_iter().map(|p| format!("{}: {p}", file.display())));
                }
                Err(e) => problems.push(format!("{}: {e}", file.display())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => problems.push(format!("{}: {e}", file.display())),
        }
    }
    for problem in &problems {
        log::warn!("the organisation's servers: {problem}");
    }
    (found, problems)
}

fn main() -> std::process::ExitCode {
    // Started again as the build worker (retex's `worker`): run that, and nothing of the window.
    if let Some(code) = texcraft_compile::worker_entry() {
        return code;
    }
    // Builds go to a worker running this program, which keeps the preamble snapshot.
    texcraft_compile::use_this_program_as_worker();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("texcraft: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> eframe::Result {
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
    let system_title_bar = prefs.system_title_bar;
    let mut session = texcraft_engine::Session::new();
    // The AI token goes to the system's keychain (docs/ROADMAP.md, AI1).
    session.set_ai_secrets(Arc::new(texcraft_engine::ai::Keychain));
    // Sign-ins to GitLab and GitHub too, so a restart stays signed in (docs/forges.md, step 4).
    session.set_forge_secrets(Arc::new(texcraft_engine::ai::Keychain));
    let (provided, problems) = provided_servers(settings.dir());
    session.set_forge_provided_servers(provided, problems);
    if let Some(data) = dirs::data_dir() {
        session.set_template_store(data.join("TeXCraft").join("templates"));
    }
    let mut app = texcraft_ui_egui::App::with_prefs(session, prefs);
    if let Some(why) = &report.read_only {
        let what = texcraft_ui_egui::i18n::t("The preferences cannot be saved, so changes are kept for this session only: {error}");
        app.report(what.replace("{error}", &why.to_string()), true);
    }
    app.set_native_pickers(true);
    app.set_file_manager(true);
    // Import a Project makes its folders in the documents folder until another is chosen.
    app.set_documents_dir(dirs::document_dir());
    for project in &settings.get().0.recent {
        if let Some(thumbnail) = thumbnails::load(project) {
            app.set_thumbnail(thumbnail);
        }
    }
    match (&args.folder, args.folder.as_deref().and_then(server_link)) {
        (_, Some(query)) => {
            app.show_start_page();
            app.forge_propose_server(query);
        }
        (Some(path), None) => app.open_path(path),
        (None, None) => app.show_start_page(),
    }
    // Without the system's title bar, the top bar holds the window's controls.
    app.set_desktop_window(true);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("TeXCraft")
        .with_app_id("io.gitlab.severin_lemaignan.TeXCraft")
        .with_inner_size([1400.0, 900.0])
        .with_decorations(system_title_bar);
    match eframe::icon_data::from_png_bytes(WINDOW_ICON) {
        Ok(icon) => viewport = viewport.with_icon(Arc::new(icon)),
        Err(e) => log::warn!("the window icon: {e}"),
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "TeXCraft",
        options,
        Box::new(|cc| {
            // In the creator, so that winit's default menu doesn't stay up.
            #[cfg(target_os = "macos")]
            let app = mac_menu::install(app, &cc.egui_ctx);
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
            Ok(Box::new(Shell { app, settings, dialogs, control, save_failed: false }))
        }),
    )
}
