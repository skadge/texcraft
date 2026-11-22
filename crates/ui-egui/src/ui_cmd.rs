//! Commands that change only the view (layout, panels, theme), and the menu entries that run
//! engine commands needing input first. Menus are built from [`MENUS`] and the engine registry,
//! so every entry has a command id the control channel can run too (docs/plan.md, phase 1).

use serde_json::json;

use crate::dialogs::{Dialog, Prompt};
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
    ui("view.layout.split", "Editor and PDF", "View", "layout", None),
    ui("view.layout.editor", "Editor Only", "View", "layout", None),
    ui("view.layout.preview", "PDF Only", "View", "layout", None),
    ui("view.side.files", "Project Files", "View", "side", None),
    ui("view.side.outline", "Outline", "View", "side", None),
    ui("view.side.hide", "Hide Side Panel", "View", "side", None),
    ui("view.logs", "Build Log", "View", "pdf", None),
    ui("view.zoom_in", "Zoom In", "View", "pdf", Some("Cmd+=")),
    ui("view.zoom_out", "Zoom Out", "View", "pdf", Some("Cmd+-")),
    ui("view.fit_width", "Fit Width", "View", "pdf", None),
    ui("view.theme", "Switch Light and Dark", "View", "theme", None),
    ui("help.shortcuts", "Keyboard Shortcuts", "Help", "help", None),
    ui("help.about", "About TeXCraft", "Help", "about", None),
];

/// The menus of the menu bar, in order.
pub const MENUS: &[&str] = &["File", "Build", "View", "Help"];

/// Runs `id`: a view command, an engine command that first asks for input, or a plain engine
/// command.
pub fn run(app: &mut App, id: &str) {
    match id {
        "view.layout.split" => app.view.layout = Layout::Split,
        "view.layout.editor" => app.view.layout = Layout::EditorOnly,
        "view.layout.preview" => app.view.layout = Layout::PreviewOnly,
        "view.side.files" => app.view.side = Some(Side::Files),
        "view.side.outline" => app.view.side = Some(Side::Outline),
        "view.side.hide" => app.view.side = None,
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
        "view.zoom_in" => app.pdf.zoom_by(1.25),
        "view.zoom_out" => app.pdf.zoom_by(0.8),
        "view.fit_width" => app.pdf.fit_width(),
        "help.shortcuts" => app.view.dialog = Some(Dialog::Shortcuts),
        "help.about" => app.view.dialog = Some(Dialog::About),
        // Native file and folder pickers come with craft-dialogs (docs/plan.md, phase 1); until then the path is typed.
        "project.open" => ask(app, tl!("Open Project Folder"), tl!("The folder that holds the project's files"), tl!("Open"), "project.open"),
        "project.save_as" => ask(app, tl!("Save Project As"), tl!("The folder to write the project's files to"), tl!("Save"), "project.save_as"),
        "build.save_pdf" => ask(app, tl!("Save PDF As"), tl!("The file to write the PDF to"), tl!("Save"), "build.save_pdf"),
        "project.save" if app.session.dir().is_none() => run(app, "project.save_as"),
        "file.new" => ask(app, tl!("New File"), tl!("Its path in the project, such as chapters/intro.tex"), tl!("Create"), "file.new"),
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
