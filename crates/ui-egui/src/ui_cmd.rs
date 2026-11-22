//! Commands that change only the view (layout, panels, theme), and the menu entries that run
//! engine commands needing input first. Menus are built from [`MENUS`] and the engine registry,
//! so every entry has a command id the control channel can run too (docs/plan.md, phase 1).

use serde_json::json;

use crate::theme::{Scheme, Tokens};
use crate::{App, Layout, Prompt, Side};

pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: &'static str,
}

/// View commands, in menu order.
pub const UI_COMMANDS: &[UiCommand] = &[
    UiCommand { id: "view.layout.split", label: "Editor and Preview", menu: "View" },
    UiCommand { id: "view.layout.editor", label: "Editor Only", menu: "View" },
    UiCommand { id: "view.layout.preview", label: "Preview Only", menu: "View" },
    UiCommand { id: "view.side.files", label: "Project Files", menu: "View" },
    UiCommand { id: "view.side.outline", label: "Outline", menu: "View" },
    UiCommand { id: "view.side.hide", label: "Hide Side Panel", menu: "View" },
    UiCommand { id: "view.logs", label: "Build Log", menu: "View" },
    UiCommand { id: "view.theme", label: "Switch Light and Dark", menu: "View" },
    UiCommand { id: "view.zoom_in", label: "Zoom In", menu: "View" },
    UiCommand { id: "view.zoom_out", label: "Zoom Out", menu: "View" },
    UiCommand { id: "view.fit_width", label: "Fit Width", menu: "View" },
];

/// The menus of the menu bar, in order.
pub const MENUS: &[&str] = &["File", "Build", "View"];

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
            let next = if app.tokens.scheme == Scheme::Dark { Scheme::Light } else { Scheme::Dark };
            app.tokens = Tokens::for_scheme(next);
        }
        "view.zoom_in" => app.pdf.zoom_by(1.25),
        "view.zoom_out" => app.pdf.zoom_by(0.8),
        "view.fit_width" => app.pdf.fit_width(),
        "project.open" => ask(app, tl!("Open Project Folder"), tl!("Folder"), "project.open", |v| json!({"path": v})),
        "project.save_as" => ask(app, tl!("Save Project As"), tl!("Folder"), "project.save_as", |v| json!({"path": v})),
        "project.save" if app.session.dir().is_none() => run(app, "project.save_as"),
        "file.new" => ask(app, tl!("New File"), tl!("File name"), "file.new", |v| json!({"path": v})),
        "build.auto" => {
            let on = !app.session.settings.auto_compile;
            app.run("build.auto", json!({"on": on}));
        }
        _ => {
            app.run(id, json!({}));
        }
    }
}

fn ask(app: &mut App, title: &'static str, label: &'static str, command: &'static str, params: fn(&str) -> serde_json::Value) {
    // Native folder pickers come with craft-dialogs (docs/plan.md, phase 1); until then the path is typed.
    app.view.prompt = Some(Prompt { title, label, value: String::new(), command, params, focus: true });
}
