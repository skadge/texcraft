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

mod chrome;
mod editor;
pub mod fonts;
pub mod icons;
mod pdf_view;
pub mod ui_cmd;

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

/// A dialog that asks for one line of text, then runs a command with it.
pub(crate) struct Prompt {
    pub title: &'static str,
    pub label: &'static str,
    pub value: String,
    pub command: &'static str,
    /// Builds the command's parameters from the text typed.
    pub params: fn(&str) -> Value,
    pub focus: bool,
}

/// The view state the interface keeps for itself.
pub(crate) struct View {
    pub side: Option<Side>,
    pub layout: Layout,
    pub logs: bool,
    pub prompt: Option<Prompt>,
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
}

impl App {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            tokens: Tokens::dark(),
            // A project is built once when it opens, through the automatic build's timer.
            view: View { side: Some(Side::Files), layout: Layout::Split, logs: false, prompt: None, status: None, last_edit: Some(0.0), goto: None },
            editor: editor::Editor::default(),
            pdf: pdf_view::PdfView::default(),
            applied: None,
            fonts_ready: false,
        }
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
        if !self.fonts_ready {
            fonts::install(ctx);
            icons::install(ctx);
            self.fonts_ready = true;
        }
        if self.applied.is_none() {
            // The first frame follows the system's theme; View › Switch Light and Dark changes it.
            self.tokens = Tokens::system(ctx);
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
        self.shortcuts(ctx);
        chrome::top_bar(self, ui);
        chrome::rail(self, ui);
        if self.view.side.is_some() {
            chrome::side_panel(self, ui);
        }
        self.panes(ui);
        chrome::prompt(self, ctx);
        if self.session.is_building() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn panes(&mut self, ui: &mut egui::Ui) {
        let show_preview = self.view.layout != Layout::EditorOnly;
        let show_editor = self.view.layout != Layout::PreviewOnly;
        if show_preview && show_editor {
            let width = ui.available_width() * 0.5;
            egui::Panel::right("preview").resizable(true).default_size(width).min_size(200.0).show(ui, |ui| self.preview(ui));
            egui::CentralPanel::default().show(ui, |ui| editor::show(self, ui));
        } else if show_editor {
            egui::CentralPanel::default().show(ui, |ui| editor::show(self, ui));
        } else {
            egui::CentralPanel::default().show(ui, |ui| self.preview(ui));
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        chrome::preview_toolbar(self, ui);
        if self.view.logs {
            chrome::logs(self, ui);
        } else {
            self.pdf.sync(self.session.pdf());
            self.pdf.show(ui, &self.tokens);
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

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let compile = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        if ctx.input_mut(|i| i.consume_shortcut(&compile)) {
            self.run("build.compile", json!({}));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&save)) {
            ui_cmd::run(self, "project.save");
        }
    }
}
