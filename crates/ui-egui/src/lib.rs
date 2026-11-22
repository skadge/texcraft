//! The TeXCraft interface in egui, shared by the desktop and the web app.
//!
//! The window is laid out as a rail of section buttons at the far left, a side panel (project
//! files and outline), the source editor and the PDF preview, with the build log taking the
//! preview's place when opened. Everything that changes the project goes through
//! [`Session::execute`]; the interface keeps only view state (layout, zoom, which panel is open).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[macro_use]
pub mod i18n;
pub mod theme;

pub mod control;
mod dialogs;
mod editor;
pub mod fonts;
pub mod icons;
mod keys;
mod pdf_view;
pub mod prefs;
pub use picks::{Pick, PickKind, PickRequest};
mod picks;
mod preview;
mod rail;
mod settings;
mod side;
mod status;
mod top_bar;
pub mod ui_cmd;
pub mod widgets;

use serde_json::{Value, json};
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;

use theme::{Scheme, Tokens};

/// What the side panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Files,
    Outline,
}

/// How the editor and the preview share the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Split,
    EditorOnly,
    PreviewOnly,
}

/// The view state the interface keeps for itself.
pub(crate) struct View {
    pub side: Option<Side>,
    pub layout: Layout,
    pub logs: bool,
    pub dialog: Option<dialogs::Dialog>,
    /// The Settings dialog's open tab.
    pub settings_tab: usize,
    /// Which messages the build log lists.
    pub log_filter: preview::Filter,
    /// Folders folded in the file tree.
    pub collapsed: std::collections::HashSet<String>,
    /// The caret's line and column (from 1) in the editor, for the status bar.
    pub cursor: Option<(u32, u32)>,
    /// The latest message for the status line, also announced to screen readers.
    pub status: Option<(String, bool)>,
    /// Seconds (egui time) of the last edit, for the automatic build.
    pub last_edit: Option<f64>,
    /// Move the editor's cursor to this line (from 1) of this file next frame.
    pub goto: Option<(ProjectPath, u32)>,
}

pub struct App {
    session: Session,
    tokens: Tokens,
    view: View,
    editor: editor::Editor,
    pdf: pdf_view::PdfView,
    applied: Option<Scheme>,
    fonts_ready: bool,
    prefs: prefs::Prefs,
    keys: keys::Keys,
    /// Picks for the shell to show.
    picks: Vec<picks::PickRequest>,
    /// Whether the shell shows native pickers; without them paths are typed.
    native_pickers: bool,
    /// What to do once a Save As for unsaved changes has finished.
    after_save: Option<dialogs::Next>,
    /// The user chose to quit; the shell closes the window.
    quit: bool,
    /// Closing was confirmed: the next close request is let through.
    close_confirmed: bool,
    /// The context of the last frame, for commands run outside one.
    ctx: egui::Context,
    /// The preferences changed since the shell last took them.
    prefs_changed: bool,
}

impl App {
    pub fn new(session: Session) -> Self {
        Self::with_prefs(session, prefs::Prefs::default())
    }

    /// An interface over `session`, with the preferences the shell stored.
    pub fn with_prefs(mut session: Session, prefs: prefs::Prefs) -> Self {
        session.settings.auto_compile = prefs.auto_compile;
        session.settings.auto_compile_delay_ms = prefs.auto_compile_delay_ms.clamp(200, 10_000);
        let (layout, side) = (prefs.layout(), prefs.side());
        let keys = keys::Keys::new(prefs.shortcuts.clone());
        i18n::set_language(&prefs.language);
        let mut app = Self {
            session,
            tokens: Tokens::dark(),
            // A project is built once when it opens, through the automatic build's timer.
            view: View {
                side,
                layout,
                logs: false,
                dialog: None,
                cursor: None,
                log_filter: preview::Filter::All,
                settings_tab: 0,
                collapsed: std::collections::HashSet::new(),
                status: None,
                last_edit: Some(0.0),
                goto: None,
            },
            editor: editor::Editor::default(),
            pdf: pdf_view::PdfView::default(),
            applied: None,
            fonts_ready: false,
            prefs,
            keys,
            picks: Vec::new(),
            after_save: None,
            quit: false,
            close_confirmed: false,
            native_pickers: false,
            ctx: egui::Context::default(),
            prefs_changed: false,
        };
        app.remember_project();
        app
    }

    /// The preferences, when they changed since the last call; the shell stores them.
    pub fn take_changed_prefs(&mut self) -> Option<prefs::Prefs> {
        let mut current = self.prefs.clone();
        current.set_view(self.view.layout, self.view.side);
        current.auto_compile = self.session.settings.auto_compile;
        if current != self.prefs {
            self.prefs = current;
            self.prefs_changed = true;
        }
        if std::mem::take(&mut self.prefs_changed) { Some(self.prefs.clone()) } else { None }
    }

    /// The project's name: its folder's, or "Untitled project".
    pub(crate) fn project_name(&self) -> String {
        self.session.dir().and_then(|d| d.file_name()).map_or_else(|| tl!("Untitled project").to_owned(), |n| n.to_string_lossy().into_owned())
    }

    /// Does `next` now, or first asks what to do with unsaved changes.
    pub(crate) fn guard(&mut self, next: dialogs::Next) {
        if self.session.is_dirty() {
            self.view.dialog = Some(dialogs::Dialog::Unsaved(next));
        } else {
            self.proceed(next);
        }
    }

    pub(crate) fn proceed(&mut self, next: dialogs::Next) {
        match next {
            dialogs::Next::Quit => {
                self.close_confirmed = true;
                self.quit = true;
            }
            dialogs::Next::NewProject => {
                if self.run("project.new", json!({})).is_some() {
                    self.pdf = pdf_view::PdfView::default();
                }
            }
            dialogs::Next::OpenProject(folder) => self.open_project(&folder),
            dialogs::Next::PickProject => ui_cmd::pick_project(self),
        }
    }

    /// Saves, then does `next`. A project without a folder is saved through Save As first.
    pub(crate) fn save_then(&mut self, next: dialogs::Next) {
        if self.session.dir().is_some() {
            if self.run("project.save", json!({})).is_some() {
                self.proceed(next);
            }
        } else {
            self.after_save = Some(next);
            ui_cmd::run(self, "project.save_as");
        }
    }

    /// The window was asked to close. Returns whether the close must be cancelled (the user is
    /// asked about unsaved changes first).
    pub fn close_requested(&mut self) -> bool {
        if self.close_confirmed || !self.session.is_dirty() {
            return false;
        }
        if !matches!(self.view.dialog, Some(dialogs::Dialog::Unsaved(_))) {
            self.view.dialog = Some(dialogs::Dialog::Unsaved(dialogs::Next::Quit));
        }
        true
    }

    /// Whether the user chose to quit since the last call; the shell then closes the window.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// Opens the project in `folder` and remembers it among the recent projects.
    pub fn open_project(&mut self, folder: &str) {
        if self.run("project.open", json!({"path": folder})).is_some() {
            self.remember_project();
            self.pdf = pdf_view::PdfView::default();
            self.view.last_edit = Some(0.0);
        }
    }

    /// Puts the open project's folder first in the recent projects.
    pub(crate) fn remember_project(&mut self) {
        if let Some(dir) = self.session.dir() {
            let path = dir.display().to_string();
            self.prefs.recent.retain(|p| *p != path);
            self.prefs.recent.insert(0, path);
            self.prefs.recent.truncate(prefs::RECENT_MAX);
            self.prefs_changed = true;
        }
    }

    /// Applies a theme preference.
    pub(crate) fn set_theme(&mut self, ctx: &egui::Context, theme: prefs::ThemePref) {
        self.prefs.theme = theme;
        self.prefs_changed = true;
        self.tokens = match theme {
            prefs::ThemePref::System => Tokens::system(ctx),
            prefs::ThemePref::Light => Tokens::light(),
            prefs::ThemePref::Dark => Tokens::dark(),
        };
        self.apply_editor_font();
    }

    /// The source text's size from the preferences.
    pub(crate) fn apply_editor_font(&mut self) {
        let size = self.prefs.editor_font_size.clamp(10.0, 22.0);
        self.tokens.code_size = size;
        self.tokens.code_line_height = (size * 1.55).round();
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Runs an engine command and puts its error, if any, on the status line.
    pub(crate) fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.view.status = Some((e, true));
                None
            }
        }
    }

    /// Draws one frame into the window's root `ui`. Called by the desktop and web shells from
    /// `eframe::App::ui`.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        self.ctx = ctx.clone();
        if !self.fonts_ready {
            fonts::install(ctx);
            icons::install(ctx);
            self.fonts_ready = true;
        }
        if self.applied.is_none() {
            let theme = self.prefs.theme;
            self.set_theme(ctx, theme);
            self.prefs_changed = false;
        }
        if self.applied != Some(self.tokens.scheme) {
            self.tokens.apply(ctx);
            self.applied = Some(self.tokens.scheme);
        }
        if let Some(outcome) = self.session.poll() {
            let message = if let texcraft_engine::Status::Unavailable(reason) = &outcome.status {
                (reason.clone(), true)
            } else {
                let (e, w) = (outcome.errors(), outcome.warnings());
                let text = tl!("Compiled in {seconds} s: {errors} errors, {warnings} warnings")
                    .replace("{seconds}", &format!("{:.1}", outcome.seconds))
                    .replace("{errors}", &e.to_string())
                    .replace("{warnings}", &w.to_string());
                (text, e > 0)
            };
            self.view.status = Some(message);
        }
        self.auto_compile(ctx);
        for id in self.keys.dispatch(ctx) {
            ui_cmd::run(self, &id);
        }
        top_bar::show(self, ui);
        status::show(self, ui);
        rail::show(self, ui);
        if self.view.side.is_some() {
            side::show(self, ui);
        }
        self.panes(ui);
        dialogs::show(self, ctx);
        if self.session.is_building() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn panes(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let frame = egui::Frame::new().fill(t.frame);
        match self.view.layout {
            Layout::Split => {
                let width = ui.available_width() * 0.5;
                egui::Panel::right("preview").resizable(true).default_size(width).min_size(280.0).frame(frame).show(ui, |ui| preview::show(self, ui));
                editor::show(self, ui);
            }
            Layout::EditorOnly => editor::show(self, ui),
            Layout::PreviewOnly => {
                egui::CentralPanel::default().frame(frame).show(ui, |ui| preview::show(self, ui));
            }
        }
    }

    fn auto_compile(&mut self, ctx: &egui::Context) {
        let Some(edited) = self.view.last_edit else { return };
        if !self.session.settings.auto_compile || self.session.is_building() || !self.session.is_stale() {
            return;
        }
        let delay = self.session.settings.auto_compile_delay_ms as f64 / 1000.0;
        let now = ctx.input(|i| i.time);
        if now - edited >= delay {
            self.view.last_edit = None;
            self.run("build.compile", json!({}));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - (now - edited)));
        }
    }
}
