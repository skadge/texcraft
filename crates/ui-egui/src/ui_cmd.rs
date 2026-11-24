//! Commands that change only the view (layout, panels, theme), and the menu entries that run
//! engine commands needing input first. Menus are built from [`MENUS`] and the engine registry,
//! so every entry has a command id the control channel can run too (docs/ROADMAP.md).

use serde_json::json;

use crate::dialogs::{Dialog, Next, Prompt};
use crate::picks::Pick;
use crate::prefs::ThemePref;
use crate::theme::Scheme;
use crate::{App, Layout, Side};

pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: &'static str,
    /// Entries of one group are separated from the next by a line.
    pub group: &'static str,
    pub shortcut: Option<&'static str>,
}

const fn ui(id: &'static str, label: &'static str, menu: &'static str, group: &'static str, shortcut: Option<&'static str>) -> UiCommand {
    UiCommand { id, label, menu, group, shortcut }
}

/// View commands, in menu order.
pub const UI_COMMANDS: &[UiCommand] = &[
    ui("edit.undo", "Undo", "Edit", "history", Some("Cmd+Z")),
    ui("edit.redo", "Redo", "Edit", "history", Some("Cmd+Shift+Z")),
    ui("edit.select_all", "Select All", "Edit", "select", Some("Cmd+A")),
    ui("edit.find", "Find", "Edit", "find", Some("Cmd+F")),
    ui("edit.replace", "Replace", "Edit", "find", Some("Cmd+Alt+F")),
    ui("edit.goto_line", "Go to Line…", "Edit", "find", Some("Cmd+Alt+G")),
    ui("view.go_to_file", "Go to File…", "Edit", "find", Some("Cmd+P")),
    ui("view.show_in_pdf", "Show in PDF", "Edit", "find", Some("Cmd+Alt+J")),
    ui("edit.quick_fix", "Quick Fix", "Edit", "fix", Some("Cmd+.")),
    ui("edit.comment", "Comment", "Edit", "review", Some("Cmd+Alt+M")),
    ui("edit.next_comment", "Next Comment", "Edit", "review", Some("F8")),
    ui("edit.previous_comment", "Previous Comment", "Edit", "review", Some("Shift+F8")),
    ui("app.settings", "Settings…", "Edit", "settings", Some("Cmd+,")),
    // Insert holds what the editor's toolbar does, for the keyboard and when the toolbar is narrow.
    ui("edit.section", "Section", "Insert", "headings", None),
    ui("edit.subsection", "Subsection", "Insert", "headings", None),
    ui("edit.subsubsection", "Subsubsection", "Insert", "headings", None),
    ui("edit.paragraph", "Paragraph Heading", "Insert", "headings", None),
    ui("edit.bold", "Bold", "Insert", "style", Some("Cmd+B")),
    ui("edit.italic", "Italic", "Insert", "style", Some("Cmd+I")),
    ui("edit.figure", "Figure", "Insert", "float", None),
    ui("edit.table", "Table", "Insert", "float", None),
    ui("edit.math_inline", "Inline Formula", "Insert", "math", Some("Cmd+Shift+M")),
    ui("edit.math_display", "Displayed Formula", "Insert", "math", None),
    ui("edit.equation", "Numbered Equation", "Insert", "math", None),
    ui("edit.ref", "Cross-Reference", "Insert", "refs", None),
    ui("edit.cite", "Citation", "Insert", "refs", None),
    ui("edit.link", "Link", "Insert", "refs", None),
    ui("edit.itemize", "Bulleted List", "Insert", "lists", None),
    ui("edit.enumerate", "Numbered List", "Insert", "lists", None),
    // Not in the menu bar: each has its button in the window (the layout switch, the rail, the
    // PDF pane's toolbar, the app mark), and its shortcut and command for the keyboard and agents.
    ui("view.layout.split", "Editor and PDF", "View", "layout", None),
    ui("view.layout.editor", "Editor Only", "View", "layout", None),
    ui("view.layout.preview", "PDF Only", "View", "layout", None),
    ui("view.side.files", "Project Files", "View", "side", None),
    ui("view.side.outline", "Outline", "View", "side", None),
    ui("view.side.review", "Review", "View", "side", None),
    ui("view.side.hide", "Hide Side Panel", "View", "side", None),
    ui("view.comments", "Show or Hide the Comments Beside the Text", "View", "side", None),
    ui("view.logs", "Build Log", "View", "pdf", None),
    ui("view.zoom_in", "Zoom In", "View", "pdf", Some("Cmd+=")),
    ui("view.zoom_out", "Zoom Out", "View", "pdf", Some("Cmd+-")),
    ui("view.fit_width", "Fit Width", "View", "pdf", None),
    ui("view.next_region", "Next Region", "View", "regions", Some("F6")),
    ui("view.previous_region", "Previous Region", "View", "regions", Some("Shift+F6")),
    ui("view.theme", "Switch Light and Dark", "View", "theme", None),
    ui("app.start", "Start Page", "View", "settings", None),
    ui("help.shortcuts", "Keyboard Shortcuts", "Help", "help", None),
    ui("help.manual", "retex User Manual", "Help", "help", None),
    ui("help.about", "About TeXCraft", "Help", "about", None),
];

/// retex's user manual: the engine's options, its packages and fonts, and its messages.
pub const RETEX_MANUAL: &str = "https://severin-lemaignan.gitlab.io/retex/retex-manual.pdf";

/// The menus of the menu bar, in order.
/// The menus of the menu bar, in order. Compiling and its options are on the Compile button, the
/// view's switches on their buttons: the bar keeps what has no button of its own, or is text work.
pub const MENUS: &[&str] = &["File", "Edit", "Insert", "Help"];

/// Runs `id`: a view command, an engine command that first asks for input, or a plain engine
/// command.
pub fn run(app: &mut App, id: &str) {
    match id {
        "view.layout.split" => app.view.layout = Layout::Split,
        "view.layout.editor" => app.view.layout = Layout::EditorOnly,
        "view.layout.preview" => app.view.layout = Layout::PreviewOnly,
        "view.side.files" => app.view.side = Some(Side::Files),
        "view.side.outline" => app.view.side = Some(Side::Outline),
        "view.side.review" => app.view.side = Some(Side::Review),
        "view.side.hide" => app.view.side = None,
        "view.comments" => crate::review::toggle_margin(app),
        "view.logs" => app.view.logs = !app.view.logs,
        "view.theme" => {
            let next = if app.tokens.scheme == Scheme::Dark { ThemePref::Light } else { ThemePref::Dark };
            let ctx = app.ctx.clone();
            app.set_theme(&ctx, next);
        }
        "project.clear_recent" => {
            app.prefs.recent.clear();
            app.prefs_changed = true;
        }
        "view.show_in_pdf" => match (app.session.current().cloned(), app.view.cursor) {
            (Some(path), Some((line, _))) => crate::preview::show_in_pdf(app, &path, line),
            _ => app.view.status = Some((tl!("Open a source file to show its place in the PDF.").to_owned(), false)),
        },
        "view.zoom_in" => app.pdf.zoom_by(1.25),
        "view.zoom_out" => app.pdf.zoom_by(0.8),
        "view.fit_width" => app.pdf.fit_width(),
        id if id.starts_with("window.") => {
            if !crate::window::run(app, id) {
                log::warn!("unknown window command {id}");
            }
        }
        id if id.starts_with("edit.") => {
            if !crate::editor::edit(app, id) {
                app.view.status = Some((tl!("Open a text file to edit it.").to_owned(), true));
            }
        }
        "file.close" => {
            if let Some(path) = app.session.current().cloned() {
                app.run("file.close", json!({"path": path.as_str()}));
            }
        }
        "view.go_to_file" => crate::goto_file::open(app),
        "help.shortcuts" => app.view.dialog = Some(Dialog::Shortcuts),
        "help.manual" => {
            app.ctx.open_url(egui::OpenUrl::new_tab(RETEX_MANUAL));
            app.view.status = Some((tl!("Opening the retex user manual (PDF) in your browser.").to_owned(), false));
        }
        "help.about" => app.view.dialog = Some(Dialog::About),
        "app.settings" => app.view.dialog = Some(Dialog::Settings),
        "app.start" => app.start = true,
        "view.next_region" => app.cycle_region(true),
        "view.previous_region" => app.cycle_region(false),
        "project.open" => app.guard(Next::PickProject),
        "project.new" => app.guard(Next::NewProject),
        "project.from_template" => app.open_gallery(),
        "project.save_as" if app.session.library().is_some() => {
            ask(app, tl!("Save Project As"), tl!("A name for the project, kept in this browser"), tl!("Save"), "project.save_as")
        }
        "project.save_as" if app.native_pickers => app.request_pick(Pick::SaveProjectAs),
        "build.save_pdf" if app.native_pickers => app.request_pick(Pick::SavePdf),
        "file.import" if app.native_pickers => app.request_pick(Pick::AddFiles),
        // Without native pickers (the web app, for now) the path is typed.
        "file.import" => {
            let prompt = Prompt {
                title: tl!("Add a File"),
                label: tl!("The file on disk to copy into the project"),
                value: String::new(),
                action: tl!("Add"),
                command: "file.import",
                params: Box::new(|v| json!({"source": v})),
                focus: true,
            };
            app.view.dialog = Some(Dialog::Prompt(prompt));
        }
        "project.save_as" => ask(app, tl!("Save Project As"), tl!("The folder to write the project's files to"), tl!("Save"), "project.save_as"),
        "build.save_pdf" => ask(app, tl!("Save PDF As"), tl!("The file to write the PDF to"), tl!("Save"), "build.save_pdf"),
        "project.save" if app.session.dir().is_none() => run(app, "project.save_as"),
        "file.new" => ask(app, tl!("New File"), tl!("Its path in the project, such as chapters/intro.tex"), tl!("Create"), "file.new"),
        "file.new_folder" => ask(app, tl!("New Folder"), tl!("Its path in the project, such as chapters/figures"), tl!("Create"), "file.new_folder"),
        "build.auto" => {
            let on = !app.session.settings.auto_compile;
            app.run("build.auto", json!({"on": on}));
        }
        _ => {
            app.run(id, json!({}));
        }
    }
}

fn ask(app: &mut App, title: &'static str, label: &'static str, action: &'static str, command: &'static str) {
    let prompt = Prompt { title, label, value: String::new(), action, command, params: Box::new(|v| json!({"path": v})), focus: true };
    app.view.dialog = Some(Dialog::Prompt(prompt));
}

/// The typed Save As (no native picker on the desktop), its path started with `value`.
pub(crate) fn ask_save_as(app: &mut App, value: String) {
    ask(app, tl!("Save Project As"), tl!("The folder to write the project's files to"), tl!("Save"), "project.save_as");
    if let Some(Dialog::Prompt(p)) = &mut app.view.dialog {
        p.value = value;
    }
}

/// Asks for a project folder: with a native picker, or a typed path.
pub(crate) fn pick_project(app: &mut App) {
    if app.session.library().is_some() {
        // The start page lists the browser's projects.
        app.start = true;
    } else if app.native_pickers {
        app.request_pick(Pick::OpenProject);
    } else {
        ask(app, tl!("Open Project Folder"), tl!("The folder that holds the project's files"), tl!("Open"), "project.open");
    }
}
