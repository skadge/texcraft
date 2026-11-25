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

mod app_icon;
pub mod control;
mod dialogs;
mod editor;
mod figure_view;
mod find_bar;
pub mod fonts;
mod forge;
mod formatted;
mod gallery;
mod goto_file;
pub mod icons;
mod keys;
mod pdf_view;
pub mod prefs;
mod source;
pub use picks::{Pick, PickKind, PickRequest};
pub use start::{THUMBNAIL_WIDTH, Thumbnail};
mod history;
mod picks;
mod preview;
mod rail;
mod review;
mod search;
mod settings;
mod side;
mod start;
mod status;
mod suggest;
mod system_menu;
mod top_bar;
pub mod ui_cmd;
pub mod widgets;
mod window;

use serde_json::{Value, json};
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;

use theme::{Scheme, Tokens};

/// A region of the window, for F6 and Shift+F6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    TopBar,
    Rail,
    Side,
    Editor,
    Review,
    Preview,
}

impl Region {
    fn name(self) -> &'static str {
        match self {
            Region::TopBar => tl!("Menu bar and project"),
            Region::Rail => tl!("Sections"),
            Region::Side => tl!("Project files and outline"),
            Region::Editor => tl!("Editor"),
            Region::Review => tl!("Comments"),
            Region::Preview => tl!("PDF and build log"),
        }
    }
}

/// What the side panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Files,
    Outline,
    /// The review's threads (where the margin goes when the editor pane is narrow).
    Review,
    /// Finding and replacing across the project's files.
    Search,
    /// The project's saved versions (its git history).
    History,
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
    /// Each region's first focusable control this frame, in the order F6 visits them.
    pub regions: Vec<(Region, egui::Id)>,
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
    /// The running build's progress and how long it has run, this frame.
    pub progress: Option<(texcraft_engine::Progress, f64)>,
    /// The running build's draft PDF, this frame.
    pub draft: Option<texcraft_engine::Draft>,
    /// The name typed in Settings › Review before a key is made.
    pub identity_draft: String,
    /// The project's name being edited in the top bar.
    pub rename: Option<top_bar::Rename>,
    /// The menu bar, while the keyboard is in it (F10, or Alt alone).
    pub menu_nav: Option<top_bar::MenuNav>,
    /// The menu bar's buttons, last frame, in order.
    pub menu_ids: Vec<egui::Id>,
    /// Focus mode (`view.focus_mode`): the menu bar, the rail and the side panel are hidden, for
    /// this session only.
    pub focus_mode: bool,
    /// The PDF pane is in its own window (`view.pdf_window`, desktop only); the main window then
    /// shows the editor alone.
    pub pdf_window: bool,
    /// The control with the keyboard at the end of the last frame (Escape clears the focus before
    /// a frame sees it).
    pub focused_last: Option<egui::Id>,
}

pub struct App {
    session: Session,
    tokens: Tokens,
    view: View,
    editor: editor::Editor,
    review: review::ReviewView,
    search: search::Search,
    history: history::HistoryView,
    figure: figure_view::FigureView,
    pdf: pdf_view::PdfView,
    applied: Option<Scheme>,
    fonts_ready: bool,
    prefs: prefs::Prefs,
    keys: keys::Keys,
    gallery: gallery::Gallery,
    /// The start page is shown in place of the project.
    start: bool,
    thumbs: start::Thumbnails,
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
    /// The shell runs in a desktop window, whose frame the app can draw itself.
    desktop_window: bool,
    /// Whether the system's frame was last asked for (`ViewportCommand::Decorations`).
    decorations: Option<bool>,
    /// Files dropped in the browser, waiting for the shell to read them.
    dropped: Vec<egui::DroppedFileHandle>,
    /// The name given to an untitled project on the desktop, while the user chooses the folder
    /// to make its folder in.
    pending_name: Option<String>,
    /// The files opened in this session, the latest first, for Go to File.
    recent_files: Vec<ProjectPath>,
    /// The files changed since the GitLab branch's base, as the top bar last counted them.
    forge_count: forge::Count,
    /// The GitLab branch checked for new commits.
    forge_check: forge::Check,
    /// Alt pressed alone, to reach the menu bar when it is released.
    alt: top_bar::AltTap,
    /// The system's menu bar (macOS), when the shell installed it; the in-window one is hidden then.
    system_menu: Option<craft_menu::NativeMenu>,
}

impl App {
    pub fn new(session: Session) -> Self {
        Self::with_prefs(session, prefs::Prefs::default())
    }

    /// An interface over `session`, with the preferences the shell stored.
    pub fn with_prefs(mut session: Session, prefs: prefs::Prefs) -> Self {
        session.settings.auto_compile = prefs.auto_compile;
        session.settings.auto_compile_delay_ms = prefs.auto_compile_delay_ms.clamp(200, 10_000);
        session.settings.build = prefs.build;
        session.set_author(prefs.identity.as_ref().map(prefs::Identity::author));
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
                regions: Vec::new(),
                collapsed: std::collections::HashSet::new(),
                status: None,
                last_edit: Some(0.0),
                goto: None,
                progress: None,
                draft: None,
                identity_draft: prefs::system_user_name(),
                rename: None,
                menu_nav: None,
                menu_ids: Vec::new(),
                focus_mode: false,
                pdf_window: false,
                focused_last: None,
            },
            editor: editor::Editor::default(),
            review: review::ReviewView::default(),
            search: search::Search::default(),
            history: history::HistoryView::default(),
            figure: figure_view::FigureView::default(),
            pdf: pdf_view::PdfView::default(),
            applied: None,
            fonts_ready: false,
            prefs,
            keys,
            gallery: gallery::Gallery::default(),
            start: false,
            thumbs: start::Thumbnails::default(),
            picks: Vec::new(),
            after_save: None,
            quit: false,
            close_confirmed: false,
            native_pickers: false,
            ctx: egui::Context::default(),
            prefs_changed: false,
            desktop_window: false,
            decorations: None,
            dropped: Vec::new(),
            pending_name: None,
            recent_files: Vec::new(),
            forge_count: forge::Count::default(),
            forge_check: forge::Check::default(),
            alt: top_bar::AltTap::default(),
            system_menu: None,
        };
        app.remember_project();
        app
    }

    /// The shell runs in a desktop window: unless the preferences ask for the system's title bar,
    /// the window has no system frame and the top bar holds its controls (`window.rs`).
    pub fn set_desktop_window(&mut self, on: bool) {
        self.desktop_window = on;
    }

    /// Whether the app draws the window's frame: its controls, moving and resizing.
    pub(crate) fn own_frame(&self) -> bool {
        self.desktop_window && !self.prefs.system_title_bar
    }

    /// The preferences, when they changed since the last call; the shell stores them.
    pub fn take_changed_prefs(&mut self) -> Option<prefs::Prefs> {
        let mut current = self.prefs.clone();
        current.set_view(self.view.layout, self.view.side);
        current.auto_compile = self.session.settings.auto_compile;
        current.build = self.session.settings.build;
        if current != self.prefs {
            self.prefs = current;
            self.prefs_changed = true;
        }
        if std::mem::take(&mut self.prefs_changed) { Some(self.prefs.clone()) } else { None }
    }

    /// The project's name: its folder's, the GitLab project's for one opened from GitLab in no
    /// folder, or "Untitled project".
    pub(crate) fn project_name(&self) -> String {
        if let Some(name) = self.session.dir().and_then(|d| d.file_name()) {
            return name.to_string_lossy().into_owned();
        }
        self.session.forge().map_or_else(|| tl!("Untitled project").to_owned(), |l| l.address.name().to_owned())
    }

    /// Gives the project the name `name` typed in the top bar. An untitled project is saved under
    /// it: in the browser at once, on the desktop in a new folder of that name, once the user has
    /// chosen where. A saved project is renamed: in the browser, its stored copy; on the desktop,
    /// its folder.
    pub(crate) fn rename_project(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || (self.session.dir().is_some() && name == self.project_name()) {
            return;
        }
        let Some(old) = self.session.dir().map(|d| d.display().to_string()) else {
            if self.session.library().is_some() {
                if self.run("project.save_as", json!({"path": name})).is_some() {
                    self.remember_project();
                    self.view.status = Some((tl!("The project is saved as {name}.").replace("{name}", name), false));
                }
            } else if let Err(e) = texcraft_engine::library::check_name(name) {
                self.view.status = Some((e, true));
            } else if self.native_pickers {
                self.pending_name = Some(name.to_owned());
                self.request_pick(picks::Pick::SaveProjectIn);
            } else {
                ui_cmd::ask_save_as(self, name.to_owned());
            }
            return;
        };
        let Some(result) = self.run("project.rename", json!({"name": name})) else { return };
        let new = result["path"].as_str().unwrap_or(name).to_owned();
        self.project_renamed(&old, &new);
        let said =
            if self.session.library().is_some() { tl!("The project is renamed {name}.") } else { tl!("The project's folder is renamed {name}.") };
        self.view.status = Some((said.replace("{name}", name), false));
    }

    /// The open project moved from `old` to `new`: its place among the recent projects and its
    /// thumbnail follow it.
    fn project_renamed(&mut self, old: &str, new: &str) {
        for path in &mut self.prefs.recent {
            if path == old {
                new.clone_into(path);
            }
        }
        self.prefs_changed = true;
        self.move_thumbnail(old, new);
        self.remember_project();
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
                self.start = false;
                if self.run("project.new", json!({})).is_some() {
                    self.pdf = pdf_view::PdfView::default();
                }
            }
            dialogs::Next::OpenProject(folder) => self.open_project(&folder),
            dialogs::Next::PickProject => ui_cmd::pick_project(self),
            dialogs::Next::OpenFromForge => forge::ask_open(self),
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

    /// Shows the start page (the desktop app does when it starts without a project).
    pub fn show_start_page(&mut self) {
        self.start = true;
    }

    /// Opens what the command line or the desktop names: a project folder, or a file of one (its
    /// folder is opened and the file shown).
    pub fn open_path(&mut self, path: &str) {
        let p = std::path::Path::new(path);
        if p.is_file()
            && let (Some(dir), Some(name)) = (p.parent(), p.file_name().and_then(|n| n.to_str()))
        {
            let dir = if dir.as_os_str().is_empty() { std::path::Path::new(".") } else { dir };
            self.open_project(&dir.display().to_string());
            if self.session.dir().is_some() {
                self.run("file.open", json!({"path": name}));
            }
            return;
        }
        self.open_project(path);
    }

    /// Opens the project in `folder` and remembers it among the recent projects.
    pub fn open_project(&mut self, folder: &str) {
        if self.run("project.open", json!({"path": folder})).is_some() {
            self.start = false;
            self.remember_project();
            self.pdf = pdf_view::PdfView::default();
            self.view.last_edit = Some(0.0);
        }
    }

    /// Whether the project at `path` is there to open: its folder, or its name in the browser's
    /// library.
    pub(crate) fn project_exists(&self, path: &str) -> bool {
        match self.session.library() {
            Some(library) => library.contains(path),
            None => std::path::Path::new(path).is_dir(),
        }
    }

    /// The files an export made in the browser, for the shell to hand to the browser as downloads.
    pub fn take_exported_files(&mut self) -> Vec<(String, Vec<u8>)> {
        self.session.take_exported_files()
    }

    /// The library's changes since the last call (the browser's projects), for the shell to
    /// store.
    pub fn take_library_changes(&mut self) -> Vec<texcraft_engine::library::Change> {
        self.session.take_library_changes()
    }

    /// Shows `message` on the status line, as an error or not: for the shell's own work, such
    /// as storing the projects.
    pub fn report(&mut self, message: String, error: bool) {
        self.view.status = Some((message, error));
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

    /// Records `id` as `region`'s first focusable control, once a frame. Only the main window's
    /// regions count: F6 does not cross into the PDF's own window.
    pub(crate) fn region(&mut self, region: Region, id: egui::Id) {
        if self.ctx.viewport_id() != egui::ViewportId::ROOT {
            return;
        }
        if !self.view.regions.iter().any(|(r, _)| *r == region) {
            self.view.regions.push((region, id));
        }
    }

    /// Moves the keyboard focus to the next region (or the previous one), and says which.
    pub(crate) fn cycle_region(&mut self, forward: bool) {
        let mut regions = self.view.regions.clone();
        let order = [Region::TopBar, Region::Rail, Region::Side, Region::Editor, Region::Review, Region::Preview];
        regions.sort_by_key(|(r, _)| order.iter().position(|o| o == r));
        if regions.is_empty() {
            return;
        }
        let focused = self.ctx.memory(|m| m.focused());
        let at = focused.and_then(|f| regions.iter().position(|(_, id)| *id == f));
        let len = regions.len();
        let next = match (at, forward) {
            (Some(i), true) => (i + 1) % len,
            (Some(i), false) => (i + len - 1) % len,
            (None, true) => 0,
            (None, false) => len - 1,
        };
        if let Some((region, id)) = regions.get(next).copied() {
            self.ctx.memory_mut(|m| m.request_focus(id));
            self.view.status = Some((region.name().to_owned(), false));
        }
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
            Ok(v) => {
                if id == "file.open"
                    && let Some(path) = params["path"].as_str().and_then(|p| ProjectPath::new(p).ok())
                {
                    goto_file::opened(&mut self.recent_files, path);
                }
                if matches!(id, "project.save" | "project.save_as") {
                    self.saved(&v);
                }
                Some(v)
            }
            Err(e) => {
                self.view.status = Some((e, true));
                None
            }
        }
    }

    /// After a save: says which removed files went to the trash, or could not leave the folder.
    fn saved(&mut self, result: &Value) {
        let names = |key: &str| -> Vec<String> {
            result["removed"][key].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default()
        };
        let trashed = names("trashed");
        if let Some(error) = result["removed"]["left"].get(0).and_then(|l| l["error"].as_str()) {
            self.view.status = Some((error.to_owned(), true));
        } else if !trashed.is_empty() {
            self.view.status = Some((tl!("Moved to the trash: {files}").replace("{files}", &trashed.join(", ")), false));
        }
    }

    /// Draws one frame into the window's root `ui`. Called by the desktop and web shells from
    /// `eframe::App::ui`.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.draw(ui);
        // After the frame, so the system's menus show what it changed.
        self.sync_system_menu();
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
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
        // Following the system: its theme may be known only after the first frames (the
        // browser), or change while the app runs.
        if self.prefs.theme == prefs::ThemePref::System && Tokens::system(ctx).scheme != self.tokens.scheme {
            let changed = self.prefs_changed;
            self.set_theme(ctx, prefs::ThemePref::System);
            self.prefs_changed = changed;
        }
        if self.applied != Some(self.tokens.scheme) {
            self.tokens.apply(ctx);
            self.applied = Some(self.tokens.scheme);
        }
        self.poll_gallery();
        match self.session.poll_export() {
            Some(Ok(done)) => {
                let path = done["path"].as_str().unwrap_or_default().to_owned();
                self.view.status = Some((tl!("Exported to {path}").replace("{path}", &path), false));
            }
            Some(Err(e)) => self.view.status = Some((e, true)),
            None => {}
        }
        forge::poll(self, ctx);
        if self.session.exporting() {
            // Look again until it ends.
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let mut thumbnail = false;
        if let Some(outcome) = self.session.poll() {
            thumbnail = outcome.pdf.is_some();
            // The build's result replaces any earlier message in the status bar.
            self.view.status = None;
        }
        self.view.progress = self.session.progress();
        self.view.draft = self.session.draft_pdf().cloned();
        self.auto_compile(ctx);
        // Vim's own Ctrl keys go to the editor before the window's shortcuts see them.
        editor::claim_keys(self, ctx);
        if self.alt.tapped(ctx) {
            ui_cmd::run(self, "view.menu_bar");
        }
        self.system_menu_clicks(ctx);
        for id in self.keys.dispatch(ctx) {
            ui_cmd::run(self, &id);
        }
        self.escape_focus_mode(ctx);
        // Presentation mode: the PDF's page takes the whole window.
        if self.pdf.presenting().is_some() {
            preview::present(self, ui);
            dialogs::show(self, ctx);
            return;
        }
        if thumbnail {
            self.update_thumbnail();
        }
        if self.desktop_window && self.decorations != Some(self.prefs.system_title_bar) {
            self.decorations = Some(self.prefs.system_title_bar);
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(self.prefs.system_title_bar));
        }
        self.dropped_files(ctx);
        self.view.regions.clear();
        // In focus mode the menu bar shows only while the keyboard is in it (F10, Alt).
        let focus_mode = self.view.focus_mode && !self.start;
        if !focus_mode || self.view.menu_nav.is_some() {
            top_bar::show(self, ui);
        }
        status::show(self, ui);
        if self.start {
            start::show(self, ui);
            gallery::show(self, ctx);
            dialogs::show(self, ctx);
            window::edges(self, ctx);
            return;
        }
        if !focus_mode {
            rail::show(self, ui);
            if self.view.side.is_some() {
                side::show(self, ui);
            }
        }
        self.panes(ui);
        if self.view.pdf_window {
            preview::window(self, ctx);
        }
        gallery::show(self, ctx);
        dialogs::show(self, ctx);
        window::edges(self, ctx);
        if self.session.is_building() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.view.focused_last = ctx.memory(egui::Memory::focused);
    }

    /// Focus mode: Escape leaves it when nothing had the keyboard (in the standard bindings, the
    /// first Escape leaves the editor, the second focus mode) and nothing else is open to close.
    fn escape_focus_mode(&mut self, ctx: &egui::Context) {
        if !self.view.focus_mode
            || self.start
            || self.view.dialog.is_some()
            || self.view.menu_nav.is_some()
            || self.view.focused_last.is_some()
            || self.pdf.presenting().is_some()
        {
            return;
        }
        if egui::Popup::is_any_open(ctx) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            ui_cmd::run(self, "view.focus_mode");
        }
    }

    fn panes(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let frame = egui::Frame::new().fill(t.frame);
        let layout = if self.view.pdf_window { Layout::EditorOnly } else { self.view.layout };
        match layout {
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
        if self.start {
            return;
        }
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
