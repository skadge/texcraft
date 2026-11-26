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
    /// Where the command belongs, as the Keyboard Shortcuts list groups it; where the menus put
    /// it is [`MENU_BAR`]'s.
    pub menu: &'static str,
    pub shortcut: Option<&'static str>,
}

const fn ui(id: &'static str, label: &'static str, menu: &'static str, shortcut: Option<&'static str>) -> UiCommand {
    UiCommand { id, label, menu, shortcut }
}

/// View commands.
pub const UI_COMMANDS: &[UiCommand] = &[
    ui("edit.undo", "Undo", "Edit", Some("Cmd+Z")),
    ui("edit.redo", "Redo", "Edit", Some("Cmd+Shift+Z")),
    // The clipboard chords run in the text itself; these are their menu entries.
    ui("edit.cut", "Cut", "Edit", Some("Cmd+X")),
    ui("edit.copy", "Copy", "Edit", Some("Cmd+C")),
    ui("edit.paste", "Paste", "Edit", Some("Cmd+V")),
    ui("edit.select_all", "Select All", "Edit", Some("Cmd+A")),
    ui("edit.find", "Find", "Edit", Some("Cmd+F")),
    ui("edit.replace", "Replace", "Edit", Some("Cmd+Alt+F")),
    ui("edit.goto_line", "Go to Line…", "Edit", Some("Cmd+Alt+G")),
    ui("view.go_to_file", "Go to File…", "Edit", Some("Cmd+P")),
    ui("view.find_in_project", "Find in Project", "Edit", Some("Cmd+Shift+F")),
    ui("view.show_in_pdf", "Show in PDF", "Edit", Some("Cmd+Alt+J")),
    // From the formatted view: the source, with the caret and selection where they were.
    ui("view.show_in_source", "Show in Source", "Edit", None),
    ui("edit.quick_fix", "Quick Fix", "Edit", Some("Cmd+.")),
    ui("edit.spelling.next", "Next Spelling or Grammar Problem", "Edit", Some("F7")),
    // Sends the selection, or the paragraph at the caret, to the connected model; the problems it
    // finds are underlined as grammar problems.
    ui("edit.ai_grammar", "Check Grammar with AI", "Edit", None),
    ui("edit.fold", "Fold", "Edit", Some("Cmd+Shift+[")),
    ui("edit.unfold", "Unfold", "Edit", Some("Cmd+Shift+]")),
    ui("edit.fold_all", "Fold All", "Edit", Some("Cmd+Alt+Shift+[")),
    ui("edit.unfold_all", "Unfold All", "Edit", Some("Cmd+Alt+Shift+]")),
    ui("edit.comment", "Comment", "Edit", Some("Cmd+Alt+M")),
    ui("edit.next_comment", "Next Comment", "Edit", Some("F8")),
    ui("edit.previous_comment", "Previous Comment", "Edit", Some("Shift+F8")),
    ui("edit.suggest_mode", "Suggest Changes", "Edit", Some("Cmd+Shift+E")),
    ui("edit.accept_all", "Accept All Suggestions", "Edit", None),
    // Sends the section at the caret to the connected model; its remarks arrive as comments.
    ui("edit.ai_review", "Review This Section with AI", "Edit", None),
    ui("edit.reject_all", "Reject All Suggestions", "Edit", None),
    ui("app.settings", "Settings…", "Edit", Some("Cmd+,")),
    // Every command in a searchable list (`commands.rs`): what the menus leave out is there.
    ui("view.commands", "All Commands…", "Edit", Some("Cmd+Shift+P")),
    // `{"how": "shorter"}`, or `{"how": "translate", "language": "French"}`: the Edit menu's and
    // the text's context menu's Rewrite with AI submenu, whose entries carry these parameters.
    ui("edit.rewrite", "Rewrite with AI", "Rewrite with AI", None),
    // Opens a field for how to change the selection, in the user's words ("flesh out these
    // bullet points"); the change arrives as a suggestion, as a rewrite does.
    ui("edit.instruct", "Ask AI to Change This…", "Rewrite with AI", None),
    // Insert holds what the editor's toolbar does, for the keyboard and when the toolbar is narrow.
    // The paragraph styles: body text, and Heading 1 to 3 as the document's sectioning levels
    // (`\chapter`, `\section`, `\subsection` in a class with chapters).
    ui("edit.body", "Body Text", "Insert", Some("Cmd+Alt+0")),
    ui("edit.heading_1", "Heading 1", "Insert", Some("Cmd+Alt+1")),
    ui("edit.heading_2", "Heading 2", "Insert", Some("Cmd+Alt+2")),
    ui("edit.heading_3", "Heading 3", "Insert", Some("Cmd+Alt+3")),
    ui("edit.chapter", "Chapter", "Insert", None),
    ui("edit.section", "Section", "Insert", None),
    ui("edit.subsection", "Subsection", "Insert", None),
    ui("edit.subsubsection", "Subsubsection", "Insert", None),
    ui("edit.paragraph", "Paragraph Heading", "Insert", None),
    ui("edit.bold", "Bold", "Insert", Some("Cmd+B")),
    ui("edit.italic", "Italic", "Insert", Some("Cmd+I")),
    ui("edit.code", "Code", "Insert", Some("Cmd+Alt+C")),
    ui("edit.figure", "Figure", "Insert", None),
    ui("edit.table", "Table", "Insert", None),
    ui("edit.math_inline", "Inline Formula", "Insert", Some("Cmd+Shift+M")),
    ui("edit.math_display", "Displayed Formula", "Insert", None),
    ui("edit.equation", "Numbered Equation", "Insert", None),
    ui("edit.symbols", "Symbols…", "Insert", None),
    // In the formatted view these ask for what they insert (a picker of the project's labels and
    // bibliography entries, a field); `{key}`, `{url, text?}`, `{text}` give it.
    ui("edit.ref", "Cross-Reference", "Insert", Some("Cmd+Alt+R")),
    ui("edit.cite", "Citation", "Insert", Some("Cmd+Alt+B")),
    ui("edit.link", "Link", "Insert", Some("Cmd+K")),
    ui("edit.footnote", "Footnote", "Insert", Some("Cmd+Alt+N")),
    ui("edit.itemize", "Bulleted List", "Insert", Some("Cmd+Alt+8")),
    ui("edit.enumerate", "Numbered List", "Insert", Some("Cmd+Alt+7")),
    ui("edit.indent", "Indent Item", "Insert", Some("Cmd+]")),
    ui("edit.outdent", "Outdent Item", "Insert", Some("Cmd+[")),
    // The connected model writes LaTeX (a table, an equation, a TikZ diagram, a figure) or a
    // BibTeX entry; it goes in as a suggested change.
    ui("edit.ai_make", "Make with AI…", "Insert", None),
    ui("edit.ai_reference", "Add a Reference with AI…", "Insert", None),
    // Not in the menu bar: each has its button in the window (the layout switch, the rail, the
    // PDF pane's toolbar, the app mark), and its shortcut and command for the keyboard and agents.
    ui("view.layout.split", "Editor and PDF", "View", None),
    ui("view.layout.editor", "Editor Only", "View", None),
    ui("view.layout.preview", "PDF Only", "View", None),
    // A narrow window's tabs (a phone); in a wide one, the layout shows that pane too.
    ui("view.pane.source", "Show the Source", "View", None),
    ui("view.pane.pdf", "Show the PDF", "View", None),
    ui("view.side.files", "Project Files", "View", None),
    ui("view.side.outline", "Outline", "View", None),
    ui("view.side.review", "Review", "View", None),
    ui("view.side.search", "Search", "View", None),
    ui("view.side.history", "History", "View", None),
    ui("view.side.ask", "Ask AI", "View", None),
    ui("view.side.hide", "Hide Side Panel", "View", None),
    ui("view.comments", "Show or Hide the Comments Beside the Text", "View", None),
    ui("view.logs", "Build Log", "View", None),
    // The build log's output files: `{"output": "pdf"}` (`build.outputs` lists them). Desktop only,
    // once the file is saved: the build keeps its files in memory.
    ui("view.show_output", "Show Build Output in Folder", "View", None),
    // Desktop only; closing the window, or choosing a layout, puts the PDF back.
    ui("view.pdf_window", "PDF in Its Own Window", "View", None),
    ui("view.zoom_in", "Zoom In", "View", Some("Cmd+=")),
    ui("view.zoom_out", "Zoom Out", "View", Some("Cmd+-")),
    ui("view.fit_width", "Fit Width", "View", None),
    ui("view.fit_page", "Fit Page", "View", None),
    ui("view.actual_size", "Actual Size", "View", Some("Cmd+0")),
    // `{"percent": 200}`: the zoom menu's fixed zooms, and any other from 25 to 400 %.
    ui("view.zoom", "Zoom To", "View", None),
    ui("view.present", "Present the PDF", "View", Some("F5")),
    // In presentation mode the arrows, Space, Page Up and Page Down, and Escape run these.
    ui("view.present.next", "Next Page", "View", None),
    ui("view.present.previous", "Previous Page", "View", None),
    ui("view.present.end", "Leave Presentation", "View", None),
    // Alt pressed and released alone does it too (not on a Mac, whose menus are the system's).
    ui("view.menu_bar", "Go to the Menu Bar", "View", Some("F10")),
    ui("view.next_region", "Next Region", "View", Some("F6")),
    ui("view.previous_region", "Previous Region", "View", Some("Shift+F6")),
    // Hides the menu bar, the rail and the side panel; Escape, when no field has the keyboard,
    // leaves it too.
    ui("view.focus_mode", "Focus Mode", "View", Some("Cmd+Shift+Enter")),
    ui("view.theme", "Switch Light and Dark", "View", None),
    ui("app.start", "Start Page", "View", None),
    // The text's context menu on a misspelled word; `{"text": …}` chooses the spelling, `{"to":
    // "project"}` adds to the project's words.
    ui("edit.spelling.replace", "Change Spelling", "Spelling", None),
    ui("edit.spelling.add", "Add to Dictionary", "Spelling", None),
    ui("edit.spelling.ignore", "Ignore Word", "Spelling", None),
    // The text's context menu on a grammar problem; `{"text": …}` chooses the replacement.
    ui("edit.grammar.replace", "Take the Grammar Replacement", "Grammar", None),
    ui("edit.grammar.ignore", "Ignore Grammar Problem", "Grammar", None),
    ui("edit.grammar.rule_off", "Turn Off This Rule", "Grammar", None),
    // The palette's symbols: `{"command": "\\alpha"}`.
    ui("edit.insert_symbol", "Insert Symbol", "Symbols", None),
    // The user guide online; `{"page": "ai.html"}` opens one of its pages (docs/guide/).
    ui("help.guide", "User Guide", "Help", Some("F1")),
    ui("help.shortcuts", "Keyboard Shortcuts", "Help", None),
    ui("help.manual", "retex User Manual", "Help", None),
    // A short paper of TeXCraft's to explore (`templates/builtin/example`), made a project as a
    // template is: in a folder the user picks, or in the browser under a name.
    ui("help.example", "Open the Example Project…", "Help", None),
    // Shows the hint above the editor that says where the formatted view, the build log and the
    // guide are, or dismisses it.
    ui("help.first_steps", "First Steps", "Help", None),
    ui("help.about", "About TeXCraft", "Help", None),
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

/// The menus of the menu bar, in order. Compiling and its options are on the Compile button, the
/// view's switches on their buttons: the bar keeps what has no button of its own, or is text work.
pub const MENUS: &[&str] = &[tn!("File"), tn!("Edit"), tn!("Insert"), tn!("Help")];

/// An entry of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// A command, by id: an engine command or a view command.
    Command(&'static str),
    /// A line between groups.
    Separator,
    /// A submenu: its title (English, the translation key) and its entries.
    Submenu(&'static str, &'static [Entry]),
    /// Open Recent: the recent projects.
    Recent,
    /// Rewrite with AI: the ways of rewriting the selection, when a model is connected.
    Rewrite,
}

use Entry::{Command as C, Recent, Rewrite, Separator as S, Submenu as Sub};

/// The menus' entries, the one definition the in-window menus and the system's menu bar (macOS)
/// are built from. They keep what is used often; the rest is a level down, and every command,
/// these and the others, is in All Commands (`view.commands`). An entry that cannot apply here
/// (the forge's for a project on none, AI's without a model) is left out ([`in_menus`]).
pub const MENU_BAR: &[(&str, &[Entry])] = &[
    (
        tn!("File"),
        &[
            C("project.new"),
            C("project.from_template"),
            C("project.open"),
            Recent,
            C("forge.open"),
            S,
            C("project.save"),
            C("project.save_as"),
            C("project.download_zip"),
            C("build.save_pdf"),
            S,
            Sub(tn!("Add to Project"), &[C("file.new"), C("file.new_folder"), S, C("file.import"), C("file.unzip")]),
            S,
            C("forge.commit"),
            C("forge.pull"),
            Sub(tn!("Save Project to GitLab or GitHub"), &[C("forge.save_to_gitlab"), C("forge.save_to_github")]),
        ],
    ),
    (
        tn!("Edit"),
        &[
            C("edit.undo"),
            C("edit.redo"),
            S,
            C("edit.cut"),
            C("edit.copy"),
            C("edit.paste"),
            C("edit.select_all"),
            S,
            C("edit.find"),
            C("edit.replace"),
            C("view.find_in_project"),
            Sub(tn!("Go To"), &[C("view.go_to_file"), C("edit.goto_line"), C("view.show_in_pdf"), C("view.show_in_source")]),
            S,
            Sub(tn!("Spelling and Grammar"), &[C("edit.spelling.next"), C("edit.quick_fix"), C("edit.ai_grammar")]),
            Sub(
                tn!("Review"),
                &[
                    C("edit.comment"),
                    C("edit.next_comment"),
                    C("edit.previous_comment"),
                    S,
                    C("edit.suggest_mode"),
                    C("edit.accept_all"),
                    C("edit.reject_all"),
                    S,
                    C("edit.ai_review"),
                ],
            ),
            Sub(tn!("Folding"), &[C("edit.fold"), C("edit.unfold"), S, C("edit.fold_all"), C("edit.unfold_all")]),
            Rewrite,
            S,
            C("view.commands"),
            C("app.settings"),
        ],
    ),
    (
        tn!("Insert"),
        &[
            Sub(
                tn!("Heading"),
                &[
                    C("edit.body"),
                    C("edit.heading_1"),
                    C("edit.heading_2"),
                    C("edit.heading_3"),
                    S,
                    C("edit.chapter"),
                    C("edit.section"),
                    C("edit.subsection"),
                    C("edit.subsubsection"),
                    C("edit.paragraph"),
                ],
            ),
            Sub(tn!("Text Style"), &[C("edit.bold"), C("edit.italic"), C("edit.code")]),
            Sub(tn!("List"), &[C("edit.itemize"), C("edit.enumerate"), S, C("edit.indent"), C("edit.outdent")]),
            S,
            C("edit.figure"),
            C("edit.table"),
            Sub(tn!("Formula"), &[C("edit.math_inline"), C("edit.math_display"), C("edit.equation")]),
            C("edit.symbols"),
            S,
            C("edit.ref"),
            C("edit.cite"),
            C("edit.link"),
            C("edit.footnote"),
            S,
            C("edit.ai_make"),
            C("edit.ai_reference"),
        ],
    ),
    (tn!("Help"), &[C("help.guide"), C("help.shortcuts"), C("help.manual"), S, C("help.example"), C("help.first_steps"), S, C("help.about")]),
];

/// The entries of the menu titled `title` (English).
pub fn menu(title: &str) -> &'static [Entry] {
    MENU_BAR.iter().find(|(t, _)| *t == title).map_or(&[], |(_, entries)| entries)
}

/// Every command id in `entries` and their submenus, in order.
pub fn commands_in(entries: &[Entry]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for e in entries {
        match e {
            Entry::Command(id) => out.push(*id),
            Entry::Submenu(_, inner) => out.extend(commands_in(inner)),
            _ => {}
        }
    }
    out
}

/// A command's menu entry as it shows now: its label, and whether it can run (or why not).
pub(crate) fn menu_entry(app: &App, id: &'static str) -> (&'static str, Result<(), String>) {
    if let Some(spec) = texcraft_engine::cmd::find(id) {
        return (label(app, id, spec.label), command_enabled(app, id));
    }
    let english = UI_COMMANDS.iter().find(|c| c.id == id).map_or(id, |c| c.label);
    (crate::i18n::tid(id, english), enabled(app, id))
}

/// Whether submenu `entries` has an entry to show here.
pub(crate) fn any_shown(app: &App, entries: &[Entry]) -> bool {
    entries.iter().any(|e| match e {
        Entry::Command(id) => in_menus(app, id),
        Entry::Submenu(_, inner) => any_shown(app, inner),
        Entry::Recent => true,
        Entry::Rewrite => crate::ai::offered(app),
        Entry::Separator => false,
    })
}

/// Whether view command `id` can run now, or why not (its menu entry is disabled with the reason).
pub(crate) fn enabled(app: &App, id: &str) -> Result<(), String> {
    match id {
        "edit.cut" | "edit.copy" if app.editor.source.selected().is_none() => Err(tl!("Select some text first.").to_owned()),
        "edit.rewrite" => crate::ai::can_rewrite(app, crate::ai::Kind::Rewrite),
        "edit.instruct" => crate::ai::can_rewrite(app, crate::ai::Kind::Instruct),
        "edit.ai_make" => crate::ai_make::can_make(app, crate::ai::Kind::Make),
        "edit.ai_reference" => crate::ai_make::can_make(app, crate::ai::Kind::Reference),
        "edit.ai_review" => crate::ai_ask::can_review(app),
        "edit.ai_grammar" => crate::grammar::can_check_with_ai(app),
        "edit.accept_all" | "edit.reject_all" if crate::review::open_suggestions_in_current(app) == 0 => {
            Err(tl!("There are no open suggestions in this file.").to_owned())
        }
        "view.pdf_window" if cfg!(target_arch = "wasm32") => {
            Err(tl!("In the browser the PDF stays in the page: a web page cannot open a second window of the app.").to_owned())
        }
        "view.pdf_window" if app.view.narrow && !app.view.pdf_window => {
            Err(tl!("On a screen this narrow the PDF is a tab beside the source: a window of its own would not help.").to_owned())
        }
        "view.show_in_source" if !crate::formatted::shown(app) => {
            Err(tl!("The source is shown already: this goes from the formatted view to it.").to_owned())
        }
        "view.show_output" if !app.file_manager || app.session.library().is_some() => {
            Err(tl!("This window cannot open the system's file manager.").to_owned())
        }
        "view.menu_bar" if app.has_system_menu() => {
            Err(tl!("The menus are in the menu bar at the top of the screen; Ctrl+F2 reaches it from the keyboard.").to_owned())
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
        "project.download_zip" => download_zip(app, params),
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
        "view.show_in_source" => match enabled(app, id) {
            Err(reason) => app.view.status = Some((reason, false)),
            Ok(()) => crate::formatted::show_in_source(app),
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
        "edit.grammar.replace" | "edit.grammar.ignore" | "edit.grammar.rule_off" => crate::grammar::run(app, id, params),
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
        "edit.ai_grammar" => crate::grammar::check_with_ai(app),
        id if id.starts_with("edit.") => {
            if !crate::editor::edit_with(app, id, params) {
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
        "view.commands" => crate::commands::open(app),
        "help.manual" => {
            app.ctx.open_url(egui::OpenUrl::new_tab(RETEX_MANUAL));
            app.view.status = Some((tl!("Opening the retex user manual (PDF) in your browser.").to_owned(), false));
        }
        "help.about" => app.view.dialog = Some(Dialog::About),
        // `{"tab": "editor"}` opens Settings on that tab (by its English name, in lower case).
        "app.settings" => {
            if let Some(tab) = params.get("tab").and_then(serde_json::Value::as_str) {
                crate::settings::choose_tab(app, tab);
            }
            app.view.dialog = Some(Dialog::Settings);
        }
        "help.example" => app.open_example(),
        "help.first_steps" => crate::first_steps::toggle(app),
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
        // In the browser the PDF is downloaded, as the build log's output files are.
        "build.save_pdf" if app.session.library().is_some() => {
            let kind = if app.session.outputs().iter().any(|o| o.kind == "pdfx") { "pdfx" } else { "pdf" };
            save_output(app, &json!({"output": kind}));
        }
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

/// Why engine command `id` cannot run now, if it cannot, as the window offers it: in the browser
/// the PDF is downloaded rather than written to a file, so it needs only a PDF.
pub(crate) fn command_enabled(app: &App, id: &str) -> Result<(), String> {
    if id == "build.save_pdf" && app.session.library().is_some() {
        return if app.session.outputs().iter().any(|o| o.kind == "pdf") {
            Ok(())
        } else {
            Err(tl!("There is no PDF yet: compile the project first.").to_owned())
        };
    }
    texcraft_engine::cmd::find(id).map_or_else(|| enabled(app, id), |c| (c.enabled)(&app.session))
}

/// The label of command `id` where the window words it differently from the registry: what Save
/// and Save PDF do in the browser, and the forge's name on its commands.
pub(crate) fn label(app: &App, id: &'static str, english: &'static str) -> &'static str {
    let browser = app.session.library().is_some();
    match id {
        "build.save_pdf" if browser => tl!("Download PDF"),
        "project.save" if browser => tl!("Keep in This Browser"),
        _ => crate::forge::label(app, id).unwrap_or_else(|| crate::i18n::tid(id, english)),
    }
}

/// Whether the menus offer command `id` here: downloading the project as a zip is the browser's
/// way to keep a copy elsewhere (a desktop project is a folder already); committing to and
/// updating from a forge are for a project opened from one, saving to one for the others; AI's
/// entries need a connected model. All Commands lists them all the same.
pub fn in_menus(app: &App, id: &str) -> bool {
    match id {
        "project.download_zip" => app.session.library().is_some(),
        "forge.commit" | "forge.pull" => app.session.forge().is_some(),
        "forge.save_to_github" | "forge.save_to_gitlab" => app.session.forge().is_none(),
        "edit.ai_grammar" | "edit.ai_review" | "edit.ai_make" | "edit.ai_reference" | "edit.instruct" => crate::ai::offered(app),
        _ => true,
    }
}

/// Downloads the project as a zip file (in the browser; with a `path`, writes it there) and says
/// so on the status line.
fn download_zip(app: &mut App, params: &serde_json::Value) {
    let Some(done) = app.run("project.download_zip", params.clone()) else { return };
    let file = done["path"].as_str().unwrap_or_default();
    let said = if done["download"].as_bool() == Some(true) {
        tl!("Downloading {file}.").replace("{file}", file)
    } else {
        tl!("Saved to {path}.").replace("{path}", file)
    };
    app.view.status = Some((said, false));
}

/// Exports to `format`: asks where (a native picker, or a typed path), then runs `build.export`.
/// The PDF is the last build's (`build.save_pdf`).
pub(crate) fn export(app: &mut App, format: texcraft_engine::Export) {
    if format == texcraft_engine::Export::Pdf {
        return run(app, "build.save_pdf");
    }
    // In the browser there is no file to choose: the shell downloads what the export makes.
    if app.session.library().is_some() {
        let name = format!("{}.{}", app.main_stem(), format.extension());
        if let Some(done) = app.run("build.export", json!({"format": format, "path": name, "background": true})) {
            export_started(app, &done);
        }
        return;
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
