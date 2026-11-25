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
    // The clipboard chords run in the text itself; these are their menu entries.
    ui("edit.cut", "Cut", "Edit", "select", Some("Cmd+X")),
    ui("edit.copy", "Copy", "Edit", "select", Some("Cmd+C")),
    ui("edit.paste", "Paste", "Edit", "select", Some("Cmd+V")),
    ui("edit.select_all", "Select All", "Edit", "select", Some("Cmd+A")),
    ui("edit.find", "Find", "Edit", "find", Some("Cmd+F")),
    ui("edit.replace", "Replace", "Edit", "find", Some("Cmd+Alt+F")),
    ui("edit.goto_line", "Go to Line…", "Edit", "find", Some("Cmd+Alt+G")),
    ui("view.go_to_file", "Go to File…", "Edit", "find", Some("Cmd+P")),
    ui("view.find_in_project", "Find in Project", "Edit", "find", Some("Cmd+Shift+F")),
    ui("view.show_in_pdf", "Show in PDF", "Edit", "find", Some("Cmd+Alt+J")),
    ui("edit.quick_fix", "Quick Fix", "Edit", "fix", Some("Cmd+.")),
    ui("edit.spelling.next", "Next Misspelling", "Edit", "fix", Some("F7")),
    ui("edit.fold", "Fold", "Edit", "fold", Some("Cmd+Shift+[")),
    ui("edit.unfold", "Unfold", "Edit", "fold", Some("Cmd+Shift+]")),
    ui("edit.fold_all", "Fold All", "Edit", "fold", Some("Cmd+Alt+Shift+[")),
    ui("edit.unfold_all", "Unfold All", "Edit", "fold", Some("Cmd+Alt+Shift+]")),
    ui("edit.comment", "Comment", "Edit", "review", Some("Cmd+Alt+M")),
    ui("edit.next_comment", "Next Comment", "Edit", "review", Some("F8")),
    ui("edit.previous_comment", "Previous Comment", "Edit", "review", Some("Shift+F8")),
    ui("edit.suggest_mode", "Suggest Changes", "Edit", "review", Some("Cmd+Shift+E")),
    ui("edit.accept_all", "Accept All Suggestions", "Edit", "review", None),
    // Sends the section at the caret to the connected model; its remarks arrive as comments.
    ui("edit.ai_review", "Review This Section with AI", "Edit", "review", None),
    ui("edit.reject_all", "Reject All Suggestions", "Edit", "review", None),
    ui("app.settings", "Settings…", "Edit", "settings", Some("Cmd+,")),
    // `{"how": "shorter"}`, or `{"how": "translate", "language": "French"}`: the Edit menu's and
    // the text's context menu's Rewrite with AI submenu, whose entries carry these parameters.
    ui("edit.rewrite", "Rewrite with AI", "Rewrite with AI", "ai", None),
    // Opens a field for how to change the selection, in the user's words ("flesh out these
    // bullet points"); the change arrives as a suggestion, as a rewrite does.
    ui("edit.instruct", "Ask AI to Change This…", "Rewrite with AI", "ai", None),
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
    ui("edit.symbols", "Symbols…", "Insert", "math", None),
    ui("edit.ref", "Cross-Reference", "Insert", "refs", None),
    ui("edit.cite", "Citation", "Insert", "refs", None),
    ui("edit.link", "Link", "Insert", "refs", None),
    ui("edit.itemize", "Bulleted List", "Insert", "lists", None),
    ui("edit.enumerate", "Numbered List", "Insert", "lists", None),
    // The connected model writes LaTeX (a table, an equation, a TikZ diagram, a figure) or a
    // BibTeX entry; it goes in as a suggested change.
    ui("edit.ai_make", "Make with AI…", "Insert", "ai", None),
    ui("edit.ai_reference", "Add a Reference with AI…", "Insert", "ai", None),
    // Not in the menu bar: each has its button in the window (the layout switch, the rail, the
    // PDF pane's toolbar, the app mark), and its shortcut and command for the keyboard and agents.
    ui("view.layout.split", "Editor and PDF", "View", "layout", None),
    ui("view.layout.editor", "Editor Only", "View", "layout", None),
    ui("view.layout.preview", "PDF Only", "View", "layout", None),
    // A narrow window's tabs (a phone); in a wide one, the layout shows that pane too.
    ui("view.pane.source", "Show the Source", "View", "layout", None),
    ui("view.pane.pdf", "Show the PDF", "View", "layout", None),
    ui("view.side.files", "Project Files", "View", "side", None),
    ui("view.side.outline", "Outline", "View", "side", None),
    ui("view.side.review", "Review", "View", "side", None),
    ui("view.side.search", "Search", "View", "side", None),
    ui("view.side.history", "History", "View", "side", None),
    ui("view.side.ask", "Ask AI", "View", "side", None),
    ui("view.side.hide", "Hide Side Panel", "View", "side", None),
    ui("view.comments", "Show or Hide the Comments Beside the Text", "View", "side", None),
    ui("view.logs", "Build Log", "View", "pdf", None),
    // The build log's output files: `{"output": "pdf"}` (`build.outputs` lists them). Desktop only,
    // once the file is saved: the build keeps its files in memory.
    ui("view.show_output", "Show Build Output in Folder", "View", "pdf", None),
    // Desktop only; closing the window, or choosing a layout, puts the PDF back.
    ui("view.pdf_window", "PDF in Its Own Window", "View", "pdf", None),
    ui("view.zoom_in", "Zoom In", "View", "pdf", Some("Cmd+=")),
    ui("view.zoom_out", "Zoom Out", "View", "pdf", Some("Cmd+-")),
    ui("view.fit_width", "Fit Width", "View", "pdf", None),
    ui("view.fit_page", "Fit Page", "View", "pdf", None),
    ui("view.actual_size", "Actual Size", "View", "pdf", Some("Cmd+0")),
    // `{"percent": 200}`: the zoom menu's fixed zooms, and any other from 25 to 400 %.
    ui("view.zoom", "Zoom To", "View", "pdf", None),
    ui("view.present", "Present the PDF", "View", "present", Some("F5")),
    // In presentation mode the arrows, Space, Page Up and Page Down, and Escape run these.
    ui("view.present.next", "Next Page", "View", "present", None),
    ui("view.present.previous", "Previous Page", "View", "present", None),
    ui("view.present.end", "Leave Presentation", "View", "present", None),
    // Alt pressed and released alone does it too (not on a Mac, whose menus are the system's).
    ui("view.menu_bar", "Go to the Menu Bar", "View", "regions", Some("F10")),
    ui("view.next_region", "Next Region", "View", "regions", Some("F6")),
    ui("view.previous_region", "Previous Region", "View", "regions", Some("Shift+F6")),
    // Hides the menu bar, the rail and the side panel; Escape, when no field has the keyboard,
    // leaves it too.
    ui("view.focus_mode", "Focus Mode", "View", "focus", Some("Cmd+Shift+Enter")),
    ui("view.theme", "Switch Light and Dark", "View", "theme", None),
    ui("app.start", "Start Page", "View", "settings", None),
    // The text's context menu on a misspelled word; `{"text": …}` chooses the spelling, `{"to":
    // "project"}` adds to the project's words.
    ui("edit.spelling.replace", "Change Spelling", "Spelling", "spelling", None),
    ui("edit.spelling.add", "Add to Dictionary", "Spelling", "spelling", None),
    ui("edit.spelling.ignore", "Ignore Word", "Spelling", "spelling", None),
    // The palette's symbols: `{"command": "\\alpha"}`.
    ui("edit.insert_symbol", "Insert Symbol", "Symbols", "symbols", None),
    // The user guide online; `{"page": "ai.html"}` opens one of its pages (docs/guide/).
    ui("help.guide", "User Guide", "Help", "help", Some("F1")),
    ui("help.shortcuts", "Keyboard Shortcuts", "Help", "help", None),
    ui("help.manual", "retex User Manual", "Help", "help", None),
    ui("help.about", "About TeXCraft", "Help", "about", None),
];

/// TeXCraft's user guide, published with the browser version (docs/guide/). The packages do not
/// carry a copy yet, so the desktop opens it online too.
pub const USER_GUIDE: &str = "https://texcraft-893d51.gitlab.io/guide/";

/// The address of the guide's `page` (`ai.html`, `forges.html#servers`): the guide's start when
/// `page` is not the name of one of its pages, so a parameter cannot open another site.
pub fn guide_page(page: &str) -> String {
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    let (file, anchor) = page.split_once('#').map_or((page, None), |(f, a)| (f, Some(a)));
    let ok = file.strip_suffix(".html").is_some_and(plain) && anchor.is_none_or(plain);
    if ok { format!("{USER_GUIDE}{page}") } else { USER_GUIDE.to_owned() }
}

/// retex's user manual: the engine's options, its packages and fonts, and its messages.
pub const RETEX_MANUAL: &str = "https://severin-lemaignan.gitlab.io/retex/retex-manual.pdf";

/// The menus of the menu bar, in order.
/// The menus of the menu bar, in order. Compiling and its options are on the Compile button, the
/// view's switches on their buttons: the bar keeps what has no button of its own, or is text work.
pub const MENUS: &[&str] = &["File", "Edit", "Insert", "Help"];

/// Whether view command `id` can run now, or why not (its menu entry is disabled with the reason).
pub(crate) fn enabled(app: &App, id: &str) -> Result<(), String> {
    match id {
        "edit.cut" | "edit.copy" if app.editor.source.selected().is_none() => Err(tl!("Select some text first.").to_owned()),
        "edit.rewrite" => crate::ai::can_rewrite(app, crate::ai::Kind::Rewrite),
        "edit.instruct" => crate::ai::can_rewrite(app, crate::ai::Kind::Instruct),
        "edit.ai_make" => crate::ai_make::can_make(app, crate::ai::Kind::Make),
        "edit.ai_reference" => crate::ai_make::can_make(app, crate::ai::Kind::Reference),
        "edit.ai_review" => crate::ai_ask::can_review(app),
        "edit.accept_all" | "edit.reject_all" if crate::review::open_suggestions_in_current(app) == 0 => {
            Err(tl!("There are no open suggestions in this file.").to_owned())
        }
        "view.pdf_window" if cfg!(target_arch = "wasm32") => {
            Err(tl!("In the browser the PDF stays in the page: a web page cannot open a second window of the app.").to_owned())
        }
        "view.show_output" if !app.file_manager || app.session.library().is_some() => {
            Err(tl!("This window cannot open the system's file manager.").to_owned())
        }
        "view.menu_bar" if app.has_system_menu() => {
            Err(tl!("The menus are in the menu bar at the top of the screen; Control+F2 reaches it from the keyboard.").to_owned())
        }
        // The browser gives a page the clipboard's text only with the paste keys.
        "edit.paste" if cfg!(target_arch = "wasm32") => {
            Err(tl!("In the browser, paste with Ctrl+V (Cmd+V on a Mac): a menu cannot read the clipboard there.").to_owned())
        }
        _ => Ok(()),
    }
}

/// Runs `id`: a view command, an engine command that first asks for input, or a plain engine
/// command.
pub fn run(app: &mut App, id: &str) {
    run_with(app, id, &json!({}));
}

/// Runs `id` with `params`, for the view commands that take some (`view.zoom`); the others
/// ignore them.
pub fn run_with(app: &mut App, id: &str, params: &serde_json::Value) {
    match id {
        "view.zoom" => match params.get("percent").and_then(serde_json::Value::as_f64) {
            Some(p) if (25.0..=400.0).contains(&p) => app.pdf.set_fit(crate::pdf_view::Fit::Zoom(p as f32 / 100.0)),
            _ => app.view.status = Some((tl!("Give the zoom as a percentage from 25 to 400.").to_owned(), true)),
        },
        "view.layout.split" | "view.layout.editor" | "view.layout.preview" => {
            app.view.layout = match id {
                "view.layout.split" => Layout::Split,
                "view.layout.editor" => Layout::EditorOnly,
                _ => Layout::PreviewOnly,
            };
            // A layout is the main window's: the PDF comes back into it.
            app.view.pdf_window = false;
        }
        "view.pane.source" => {
            app.view.pane = crate::Pane::Source;
            if !app.view.narrow && app.view.layout == Layout::PreviewOnly {
                app.view.layout = Layout::Split;
            }
        }
        "view.pane.pdf" => {
            app.view.pane = crate::Pane::Pdf;
            if !app.view.narrow && app.view.layout == Layout::EditorOnly && !app.view.pdf_window {
                app.view.layout = Layout::Split;
            }
        }
        "view.pdf_window" => match enabled(app, id) {
            Err(reason) => app.view.status = Some((reason, true)),
            Ok(()) => {
                app.view.pdf_window = !app.view.pdf_window;
                let said = if app.view.pdf_window {
                    tl!("The PDF is in its own window; close it to put the PDF back.")
                } else {
                    tl!("The PDF is back in the main window.")
                };
                app.view.status = Some((said.to_owned(), false));
            }
        },
        "view.side.files" => app.view.side = Some(Side::Files),
        "view.side.outline" => app.view.side = Some(Side::Outline),
        "view.side.review" => app.view.side = Some(Side::Review),
        "view.side.search" => app.view.side = Some(Side::Search),
        "view.side.history" => app.view.side = Some(Side::History),
        "view.side.ask" => app.view.side = Some(Side::Ask),
        "view.find_in_project" => crate::search::open(app),
        "view.side.hide" => app.view.side = None,
        "view.comments" => crate::review::toggle_margin(app),
        "view.logs" => app.view.logs = !app.view.logs,
        "view.show_output" => show_output(app, params),
        "build.save_output" => save_output(app, params),
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
        "view.present" => {
            crate::preview::sync(app);
            app.pdf.present();
            if app.pdf.presenting().is_none() {
                app.view.status = Some((tl!("There is no PDF to present yet: compile the project first.").to_owned(), true));
            }
        }
        "view.present.next" => app.pdf.present_step(1),
        "view.present.previous" => app.pdf.present_step(-1),
        "view.present.end" => app.pdf.present_end(),
        "view.fit_width" => app.pdf.set_fit(crate::pdf_view::Fit::Width),
        "view.fit_page" => app.pdf.set_fit(crate::pdf_view::Fit::Page),
        "view.actual_size" => app.pdf.set_fit(crate::pdf_view::Fit::Zoom(1.0)),
        "edit.spelling.replace" | "edit.spelling.add" | "edit.spelling.ignore" | "edit.spelling.next" => crate::spell::run(app, id, params),
        "edit.symbols" => crate::symbols::toggle(app),
        "edit.insert_symbol" => crate::symbols::run(app, params),
        id if id.starts_with("window.") => {
            if !crate::window::run(app, id) {
                log::warn!("unknown window command {id}");
            }
        }
        "edit.rewrite" => crate::ai::rewrite(app, params),
        "edit.instruct" => crate::ai::open_instruct(app),
        "edit.ai_make" => crate::ai_make::open_make(app),
        "edit.ai_reference" => crate::ai_make::open_reference(app),
        "edit.ai_review" => crate::ai_ask::review(app),
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
        "help.guide" => {
            let page = params.get("page").and_then(serde_json::Value::as_str).unwrap_or_default();
            app.ctx.open_url(egui::OpenUrl::new_tab(guide_page(page)));
            app.view.status = Some((tl!("Opening the user guide in your browser.").to_owned(), false));
        }
        "help.shortcuts" => app.view.dialog = Some(Dialog::Shortcuts),
        "help.manual" => {
            app.ctx.open_url(egui::OpenUrl::new_tab(RETEX_MANUAL));
            app.view.status = Some((tl!("Opening the retex user manual (PDF) in your browser.").to_owned(), false));
        }
        "help.about" => app.view.dialog = Some(Dialog::About),
        "app.settings" => app.view.dialog = Some(Dialog::Settings),
        "app.start" => app.start = true,
        "view.menu_bar" => crate::top_bar::toggle_menu_bar(app),
        "view.focus_mode" => {
            app.view.focus_mode = !app.view.focus_mode;
            let said = if app.view.focus_mode {
                tl!("Focus mode: only the text and its PDF. Escape, when no field has the keyboard, leaves it.")
            } else {
                tl!("Focus mode is off.")
            };
            app.view.status = Some((said.to_owned(), false));
        }
        "view.next_region" => app.cycle_region(true),
        "view.previous_region" => app.cycle_region(false),
        // With a path (the system menu's Open Recent), that project; without, a folder to choose.
        "project.open" => match params.get("path").and_then(serde_json::Value::as_str) {
            Some(path) => app.guard(Next::OpenProject(path.to_owned())),
            None => app.guard(Next::PickProject),
        },
        // Opening from a forge replaces the project: unsaved changes are asked about first.
        "forge.open" => app.guard(Next::OpenFromForge),
        "forge.commit" => crate::forge::ask_commit(app),
        "forge.pull" => crate::forge::pull(app),
        "forge.save_to_github" => crate::forge_save::ask(app, texcraft_engine::forge::Service::GitHub),
        "forge.save_to_gitlab" => crate::forge_save::ask(app, texcraft_engine::forge::Service::GitLab),
        "git.push" => crate::history::push(app),
        "git.pull" => crate::history::pull(app),
        "project.new" => app.guard(Next::NewProject),
        "project.from_template" => app.open_gallery(),
        "project.save_as" if app.session.library().is_some() => {
            ask(app, tl!("Save Project As"), tl!("A name for the project, kept in this browser"), tl!("Save"), "project.save_as")
        }
        "project.save_as" if app.native_pickers => app.request_pick(Pick::SaveProjectAs),
        "build.save_pdf" if app.native_pickers => app.request_pick(Pick::SavePdf),
        "file.import" if app.native_pickers => app.request_pick(Pick::AddFiles),
        "file.unzip" if app.native_pickers => app.request_pick(Pick::ImportZip),
        "file.unzip" => {
            let prompt = Prompt {
                title: tl!("Import a Zip File"),
                label: tl!("The zip file on disk to unzip into the project"),
                value: String::new(),
                action: tl!("Unzip"),
                command: "file.unzip",
                params: Box::new(|v| json!({"source": v})),
                focus: true,
            };
            app.view.dialog = Some(Dialog::Prompt(prompt));
        }
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

/// Exports to `format`: asks where (a native picker, or a typed path), then runs `build.export`.
/// The PDF is the last build's (`build.save_pdf`).
pub(crate) fn export(app: &mut App, format: texcraft_engine::Export) {
    // In the browser there is no file to choose: the shell downloads what the export makes.
    if app.session.library().is_some() {
        let name = format!("{}.{}", app.main_stem(), format.extension());
        if let Some(done) = app.run("build.export", json!({"format": format, "path": name, "background": true})) {
            export_started(app, &done);
        }
        return;
    }
    if format == texcraft_engine::Export::Pdf {
        return run(app, "build.save_pdf");
    }
    if app.native_pickers {
        return app.request_pick(Pick::Export(format));
    }
    let prompt = Prompt {
        title: tl!("Export As"),
        label: tl!("The file to write the export to"),
        value: String::new(),
        action: tl!("Export"),
        command: "build.export",
        // On its thread, as from the system's file dialog: the window shows the export running.
        params: Box::new(move |v| json!({"format": format, "path": v, "background": true})),
        focus: true,
    };
    app.view.dialog = Some(Dialog::Prompt(prompt));
}

/// After `build.export`: an export started on its thread is said on the status line, which says
/// again when it ends (`App::draw`).
pub(crate) fn export_started(app: &mut App, done: &serde_json::Value) {
    if done["started"] != true {
        return;
    }
    let Ok(format) = serde_json::from_value::<texcraft_engine::Export>(done["format"].clone()) else { return };
    let label = crate::preview::export_label(format);
    app.view.status = Some((tl!("Exporting as {format}…").replace("{format}", label), false));
}

/// The kind of build output `params` names (`{"output": "pdf"}`), as listed by `build.outputs`.
fn output_kind(app: &mut App, params: &serde_json::Value) -> Option<&'static str> {
    let asked = params.get("output").and_then(serde_json::Value::as_str).unwrap_or_default();
    let kind = texcraft_engine::OUTPUT_KINDS.into_iter().find(|k| *k == asked);
    if kind.is_none() {
        app.view.status = Some((tl!("Name the build output: pdf, pdfx, synctex or log.").to_owned(), true));
    }
    kind
}

/// Saves one of the last build's files: downloaded in the browser, else where the user chooses.
fn save_output(app: &mut App, params: &serde_json::Value) {
    let Some(kind) = output_kind(app, params) else { return };
    if let Some(path) = params.get("path").and_then(serde_json::Value::as_str) {
        return app.save_output(kind, path);
    }
    let Some(name) = app.session.outputs().into_iter().find(|o| o.kind == kind).map(|o| o.name) else {
        app.view.status = Some((tl!("The last build made no file of this kind: compile the project first.").to_owned(), true));
        return;
    };
    if app.session.library().is_some() {
        app.save_output(kind, &name);
    } else if app.native_pickers {
        app.request_pick(Pick::SaveOutput(kind));
    } else {
        let prompt = Prompt {
            title: tl!("Save Build Output As"),
            label: tl!("The file to write it to"),
            value: name,
            action: tl!("Save"),
            command: "build.save_output",
            params: Box::new(move |v| json!({"output": kind, "path": v})),
            focus: true,
        };
        app.view.dialog = Some(Dialog::Prompt(prompt));
    }
}

/// Shows the saved copy of one of the build's files in the system's file manager.
fn show_output(app: &mut App, params: &serde_json::Value) {
    if let Err(reason) = enabled(app, "view.show_output") {
        app.view.status = Some((reason, true));
        return;
    }
    let Some(kind) = output_kind(app, params) else { return };
    match app.session.saved_output(kind) {
        Some(path) => {
            let path = path.to_path_buf();
            app.view.status = Some((tl!("Showing {path} in its folder.").replace("{path}", &path.display().to_string()), false));
            app.reveals.push(path);
        }
        None => app.view.status = Some((tl!("Save it first: the build keeps its files in memory.").to_owned(), true)),
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
