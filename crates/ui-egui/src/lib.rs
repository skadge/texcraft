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

mod ai;
mod ai_ask;
mod ai_make;
mod app_icon;
pub mod control;
mod dialogs;
mod drop;
mod editor;
mod fields;
mod figure_dialog;
mod figure_view;
mod find_bar;
pub mod fonts;
mod forge;
mod forge_pick;
mod forge_save;
mod forge_server;
mod formatted;
mod formatted_bar;
mod formatted_edit;
mod gallery;
mod goto_file;
pub mod icons;
mod insert_pick;
mod keys;
mod pdf_view;
pub mod prefs;
mod source;
mod spell;
mod symbols;
pub use picks::{Pick, PickKind, PickRequest};
pub use start::{THUMBNAIL_WIDTH, Thumbnail};
mod history;
mod panes;
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
    /// The symbol palette (`edit.symbols`).
    Symbols,
    /// Questions to the connected model about the file or the project (`view.side.ask`).
    Ask,
}

/// How the editor and the preview share the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Split,
    EditorOnly,
    PreviewOnly,
}

/// Which of the editor and the PDF a narrow window shows (`view.pane.source`, `view.pane.pdf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Source,
    Pdf,
}

/// The view state the interface keeps for itself.
pub(crate) struct View {
    pub side: Option<Side>,
    pub layout: Layout,
    pub logs: bool,
    pub dialog: Option<dialogs::Dialog>,
    /// Each region's first focusable control this frame, in the order F6 visits them.
    pub regions: Vec<(Region, egui::Id)>,
    /// Where each region is this frame, for F6 from a control inside one.
    pub region_areas: Vec<(Region, egui::Rect)>,
    /// A dialog was open at the end of the last frame.
    pub modal_was_open: bool,
    /// The control that had the keyboard before the open dialog opened; it gets it back.
    pub focus_before_modal: Option<egui::Id>,
    /// Frames since the open dialog opened, to give it the keyboard once it is drawn.
    pub modal_frames: u8,
    /// The control to give the keyboard back to, the frame after a dialog closed.
    pub focus_restore: Option<egui::Id>,
    /// Give the open dialog's first control the keyboard at the start of the next frame.
    pub focus_into_modal: bool,
    /// Where the keyboard goes back to when the control that had it is gone: the button of the
    /// menu whose entry opened the dialog.
    pub focus_fallback: Option<egui::Id>,
    /// The Settings dialog's open tab.
    pub settings_tab: usize,
    /// Which messages the build log lists.
    pub log_filter: preview::Filter,
    /// The raw log's search.
    pub log_search: preview::LogSearch,
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
    /// The window is narrower than the theme's `narrow_width` this frame (a phone): the side
    /// panel is a drawer and the editor and the PDF are tabs.
    pub narrow: bool,
    /// Whether the window was narrow last frame (`None` before the first).
    pub narrow_last: Option<bool>,
    /// The side panel shown before the window became narrow, shown again when it widens.
    pub side_wide: Option<Side>,
    /// The tab a narrow window shows.
    pub pane: Pane,
    /// The side panel as a drawer, while it is open in a narrow window.
    pub drawer: Option<side::Drawer>,
    /// The rail's button of the open section, this frame.
    pub rail_active: Option<egui::Id>,
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
    /// Whether the shell can show a file in the system's file manager ([`App::take_reveals`]).
    file_manager: bool,
    /// Files for the shell to show in the file manager.
    reveals: Vec<std::path::PathBuf>,
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
    /// The files changed since the forge branch's base, as the top bar last counted them.
    forge_count: forge::Count,
    /// The forge branch checked for new commits.
    forge_check: forge::Check,
    /// Alt pressed alone, to reach the menu bar when it is released.
    alt: top_bar::AltTap,
    /// The system's menu bar (macOS), when the shell installed it; the in-window one is hidden then.
    system_menu: Option<craft_menu::NativeMenu>,
    /// The text field the Edit menu acts on, and what it is to do there.
    fields: fields::Fields,
    /// Settings › AI's form, and the AI settings last kept in the preferences.
    ai: ai::AiView,
    /// The symbol palette's search.
    symbols: symbols::Palette,
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
        session.restore_ai(prefs.ai.clone());
        session.restore_forge_servers(prefs.forge_servers.clone());
        session.settings.spelling = prefs.spelling.clone();
        session.set_user_words(&prefs.words);
        let (layout, side) = (prefs.layout(), prefs.side());
        let keys = keys::Keys::new(prefs.shortcuts.clone());
        i18n::set_language(&prefs.language);
        session.set_interface_language(i18n::current_code());
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
                log_search: preview::LogSearch::default(),
                settings_tab: 0,
                regions: Vec::new(),
                region_areas: Vec::new(),
                modal_was_open: false,
                focus_before_modal: None,
                modal_frames: 0,
                focus_restore: None,
                focus_into_modal: false,
                focus_fallback: None,
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
                narrow: false,
                narrow_last: None,
                side_wide: None,
                pane: Pane::Source,
                drawer: None,
                rail_active: None,
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
            file_manager: false,
            reveals: Vec::new(),
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
            fields: fields::Fields::default(),
            ai: ai::AiView::default(),
            symbols: symbols::Palette::default(),
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
        // A narrow window's drawer is not the side panel the wide window keeps.
        let side = if self.view.narrow { self.view.side_wide } else { self.view.side };
        current.set_view(self.view.layout, side);
        current.auto_compile = self.session.settings.auto_compile;
        current.build = self.session.settings.build;
        current.spelling = self.session.settings.spelling.clone();
        current.words = self.session.user_words();
        if current != self.prefs {
            self.prefs = current;
            self.prefs_changed = true;
        }
        if std::mem::take(&mut self.prefs_changed) { Some(self.prefs.clone()) } else { None }
    }

    /// The project's name: its folder's, the forge project's for one opened from a forge in no
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
            dialogs::Next::ImportProject(params, name) => self.import_project(params, name),
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

    /// Whether the shell shows files in the system's file manager for [`App::take_reveals`]
    /// (the desktop app); without it the build log offers no Show in Folder.
    pub fn set_file_manager(&mut self, on: bool) {
        self.file_manager = on;
    }

    /// Files to show in the system's file manager, oldest first.
    pub fn take_reveals(&mut self) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut self.reveals)
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

    /// Records where `region` is this frame.
    pub(crate) fn region_area(&mut self, region: Region, rect: egui::Rect) {
        if self.ctx.viewport_id() == egui::ViewportId::ROOT {
            self.view.region_areas.push((region, rect));
        }
    }

    /// The region the control `id` is in: the smallest that holds it.
    fn region_of(&self, id: egui::Id) -> Option<Region> {
        let rect = self.ctx.read_response(id)?.rect;
        self.view
            .region_areas
            .iter()
            .filter(|(_, area)| area.contains(rect.center()))
            .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
            .map(|(r, _)| *r)
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
        let len = regions.len();
        let at = focused.and_then(|f| regions.iter().position(|(_, id)| *id == f));
        // A control inside a region, not its first: F6 goes on from that region, Shift+F6 back
        // to its start.
        let inside = focused.filter(|_| at.is_none()).and_then(|f| self.region_of(f)).and_then(|r| regions.iter().position(|(x, _)| *x == r));
        let next = match (at, inside, forward) {
            (Some(i), _, true) | (None, Some(i), true) => (i + 1) % len,
            (Some(i), _, false) => (i + len - 1) % len,
            (None, Some(i), false) => i,
            (None, None, true) => 0,
            (None, None, false) => len - 1,
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

    /// The browser came back from signing in to GitLab, with the page's query (`code` and
    /// `state`, or `error`): it is finished in the background (`forge.sign_in_return`).
    pub fn forge_sign_in_returned(&mut self, params: Value) {
        forge::returned(self, params);
    }

    /// A link's query (`gitlab=…&client_id=…`, the browser's address or the desktop's argument):
    /// when it proposes a server, a dialog asks before adding it. Whether it proposed one.
    pub fn forge_propose_server(&mut self, query: &str) -> bool {
        forge_server::propose(self, query)
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
        // A dialog to give the keyboard to, or a menu opened from the keyboard last frame: Tab's
        // move, before anything is drawn, gives it to the dialog's first control, or from the
        // menu's button to its first entry, drawn right after it.
        let menu_entered = ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::unique(widgets::MENU_ENTER))).unwrap_or(false);
        if std::mem::take(&mut self.view.focus_into_modal) || menu_entered {
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Next));
        }
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
        ai::poll(self, ctx);
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
        fields::deliver(self, ctx);
        self.escape_focus_mode(ctx);
        // Presentation mode: the PDF's page takes the whole window.
        if self.pdf.presenting().is_some() {
            preview::present(self, ui);
            dialogs::show(self, ctx);
            self.modal_focus(ctx);
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
        self.view.region_areas.clear();
        self.view.rail_active = None;
        self.narrow_check(ctx);
        // In focus mode the menu bar shows only while the keyboard is in it (F10, Alt).
        let focus_mode = self.view.focus_mode && !self.start;
        if !focus_mode || self.view.menu_nav.is_some() {
            top_bar::show(self, ui);
        }
        let status = status::reserve(self, ui);
        if self.start {
            start::show(self, ui);
            status::show(self, ui, status);
            gallery::show(self, ctx);
            dialogs::show(self, ctx);
            self.modal_focus(ctx);
            window::edges(self, ctx);
            fields::track(self, ctx);
            return;
        }
        if !focus_mode {
            rail::show(self, ui);
            if self.view.side.is_some() && !self.view.narrow {
                side::show(self, ui);
            }
        }
        self.panes(ui);
        status::show(self, ui, status);
        if self.view.narrow && !focus_mode {
            side::drawer(self, ctx);
        }
        // A narrow window shows one pane: a line to go to is shown in the source.
        if self.view.narrow && self.view.goto.is_some() {
            self.view.pane = Pane::Source;
        }
        if self.view.pdf_window {
            preview::window(self, ctx);
        }
        gallery::show(self, ctx);
        dialogs::show(self, ctx);
        self.modal_focus(ctx);
        window::edges(self, ctx);
        if self.session.is_building() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.view.focused_last = ctx.memory(egui::Memory::focused);
        fields::track(self, ctx);
    }

    /// A dialog takes the keyboard once it is drawn, if it did not take it itself, and gives it
    /// back to the control that had it when it closes (standards/accessibility.md, rule 1).
    fn modal_focus(&mut self, ctx: &egui::Context) {
        let open = self.view.dialog.is_some() || self.gallery.open;
        match (self.view.modal_was_open, open) {
            (false, true) => {
                self.view.focus_fallback = ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(widgets::MENU_OWNER)));
                self.view.focus_before_modal = self.view.focused_last.or(self.view.focus_fallback);
                self.view.modal_frames = 0;
            }
            // The frame after: egui still keeps the keyboard in the dialog's layer this frame.
            (true, false) => {
                self.view.focus_restore = self.view.focus_before_modal.take();
                ctx.request_repaint();
            }
            (false, false) => {
                // A menu's entry that opened the dialog is gone: the menu's button, then.
                if let Some(id) = self.view.focus_restore.take() {
                    let id = if ctx.read_response(id).is_some() { Some(id) } else { self.view.focus_fallback.take() };
                    if let Some(id) = id {
                        ctx.memory_mut(|m| m.request_focus(id));
                    }
                }
            }
            _ => {}
        }
        self.view.modal_was_open = open;
        if !open || self.view.modal_frames > 2 {
            return;
        }
        self.view.modal_frames += 1;
        // The dialog has been drawn once: if nothing in it has the keyboard, its first control
        // takes it next frame (as Tab gives it to the first control of the top layer).
        let focused = ctx.memory(egui::Memory::focused);
        if self.view.modal_frames == 2 && (focused.is_none() || focused == self.view.focus_before_modal) {
            self.view.focus_into_modal = true;
        }
        ctx.request_repaint();
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

    /// Notes whether the window is narrow this frame. Becoming narrow hides the side panel (it
    /// opens as a drawer from the rail); widening again shows the one there was.
    fn narrow_check(&mut self, ctx: &egui::Context) {
        let narrow = ctx.content_rect().width() < self.tokens.narrow_width;
        if self.view.narrow_last != Some(narrow) {
            self.view.drawer = None;
            if narrow {
                self.view.side_wide = self.view.side.take();
            } else if self.view.side.is_none() {
                self.view.side = self.view.side_wide.take();
            }
            self.view.narrow_last = Some(narrow);
        }
        self.view.narrow = narrow;
    }

    fn panes(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let frame = egui::Frame::new().fill(t.frame);
        if self.view.narrow && !self.view.pdf_window {
            panes::tabs(self, ui);
            match self.view.pane {
                Pane::Source => {
                    let editor = egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| editor::show(self, ui));
                    self.region_area(Region::Editor, editor.response.rect);
                }
                Pane::Pdf => {
                    let preview = egui::CentralPanel::default().frame(frame).show(ui, |ui| preview::show(self, ui));
                    self.region_area(Region::Preview, preview.response.rect);
                }
            }
            return;
        }
        let layout = if self.view.pdf_window { Layout::EditorOnly } else { self.view.layout };
        match layout {
            // The editor's pane first, so the keyboard reaches it before the PDF's, as it reads.
            Layout::Split => {
                let width = ui.available_width();
                let editor = egui::Panel::left("editor-pane")
                    .resizable(true)
                    .default_size(width * 0.5)
                    .min_size(280.0)
                    .max_size((width - 280.0).max(280.0))
                    .frame(egui::Frame::new())
                    .show(ui, |ui| editor::show(self, ui));
                widgets::name_splitter(ui, "editor-pane", tl!("Resize the editor and the PDF"));
                self.region_area(Region::Editor, editor.response.rect);
                let preview = egui::CentralPanel::default().frame(frame).show(ui, |ui| preview::show(self, ui));
                self.region_area(Region::Preview, preview.response.rect);
            }
            Layout::EditorOnly => {
                let editor = egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| editor::show(self, ui));
                self.region_area(Region::Editor, editor.response.rect);
            }
            Layout::PreviewOnly => {
                let preview = egui::CentralPanel::default().frame(frame).show(ui, |ui| preview::show(self, ui));
                self.region_area(Region::Preview, preview.response.rect);
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
