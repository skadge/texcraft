//! The side panel (docs/design.md, Regions): the project's files as a tree, and the outline of the
//! shown file below it in a resizable section; or the review's threads, Search, or History.

use std::collections::BTreeMap;

use egui::{Align, Layout};
use serde_json::json;
use texcraft_engine::project::ProjectPath;

use crate::dialogs::{Confirm, Dialog, Prompt};
use crate::theme::Tokens;
use crate::widgets::{self, Row};
use crate::{App, Side, ui_cmd};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 4 });
    let shown =
        egui::Panel::left("side").resizable(true).default_size(248.0).min_size(180.0).max_size(480.0).frame(frame).show(ui, |ui| content(app, ui));
    widgets::name_splitter(ui, "side", tl!("Resize the side panel"));
    app.region_area(crate::Region::Side, shown.response.rect);
}

/// The side panel in a narrow window, while it is open: a drawer over the panes, beside the
/// rail. It closes on Escape, on a tap outside it, and once a file or a place in one is chosen
/// (the editor then shows it); the keyboard goes into it when it opens, and back when it closes.
pub(crate) struct Drawer {
    /// The control that had the keyboard before it opened (the rail's button).
    return_to: Option<egui::Id>,
    /// The file shown when it opened.
    file: Option<ProjectPath>,
    /// Frames since it opened.
    frames: u8,
}

pub(crate) fn drawer(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    if app.view.side.is_none() {
        if let Some(d) = app.view.drawer.take()
            && let Some(id) = d.return_to
        {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        return;
    }
    let return_to = app.view.focused_last.or(app.view.rail_active);
    let file = app.session.current().cloned();
    let state = app.view.drawer.get_or_insert(Drawer { return_to, file, frames: 0 });
    state.frames = state.frames.saturating_add(1);
    let first = state.frames == 2;
    if state.frames < 2 {
        // Once more, to give it the keyboard once it is drawn.
        ctx.request_repaint();
    }
    let screen = ctx.content_rect();
    let top = screen.top() + if app.view.focus_mode { 0.0 } else { t.top_bar_height };
    let left = screen.left() + t.rail_width;
    // A strip of the panes stays beside it, to tap to close it.
    let width = t.drawer_width.min(screen.width() - t.rail_width - 12.0 * t.unit).max(160.0);
    let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + width, screen.bottom() - t.status_height));
    let margin = egui::Margin { left: 4, right: 4, top: 0, bottom: 4 };
    let inner = egui::vec2(rect.width() - margin.sum().x, rect.height() - margin.sum().y);
    let area = egui::Modal::default_area(egui::Id::unique("side-drawer")).anchor(egui::Align2::LEFT_TOP, rect.min - screen.min);
    let frame = egui::Frame::new().fill(t.frame).inner_margin(margin).shadow(egui::Shadow { offset: [2, 0], blur: 16, spread: 0, color: t.shadow });
    let modal =
        egui::Modal::new(egui::Id::unique("side-drawer")).area(area).frame(frame).backdrop_color(t.shadow.gamma_multiply(0.6)).show(ctx, |ui| {
            ui.set_min_size(inner);
            ui.set_max_size(inner);
            content(app, ui);
        });
    app.region_area(crate::Region::Side, rect);
    let chosen = app.session.current() != app.view.drawer.as_ref().and_then(|d| d.file.as_ref()) || app.view.goto.is_some();
    if chosen {
        // The editor shows what was chosen, and takes the keyboard.
        app.view.side = None;
        app.view.pane = crate::Pane::Source;
        app.editor.focus = true;
        app.view.drawer = None;
    } else if modal.should_close() {
        app.view.side = None;
    } else if first {
        // Drawn once: its first control takes the keyboard, as a dialog's does.
        let focused = ctx.memory(egui::Memory::focused);
        if focused.is_none() || focused == return_to {
            app.view.focus_into_modal = true;
            ctx.request_repaint();
        }
    }
}

fn content(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    if app.view.side == Some(Side::Review) {
        crate::review::panel(app, ui);
        return;
    }
    if app.view.side == Some(Side::Search) {
        crate::search::panel(app, ui);
        return;
    }
    if app.view.side == Some(Side::History) {
        crate::history::panel(app, ui);
        return;
    }
    if app.view.side == Some(Side::Ask) {
        if !crate::ai::offered(app) {
            // AI turned off (or its model forgotten) while the section was open.
            app.view.side = None;
            return;
        }
        crate::ai_ask::panel(app, ui);
        return;
    }
    if app.view.side == Some(Side::Symbols) {
        crate::symbols::panel(app, ui);
        return;
    }
    // The files above the outline, in the order the keyboard visits them too.
    let outline_first = app.view.side == Some(Side::Outline);
    let height = ui.available_height();
    egui::Panel::top("files")
        .resizable(true)
        .default_size(if outline_first { height * 0.3 } else { (height - 220.0).max(80.0) })
        .min_size(80.0)
        .frame(egui::Frame::new().fill(t.frame))
        .show(ui, |ui| files(app, ui));
    widgets::name_splitter(ui, "files", tl!("Resize the files and the outline"));
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.frame)).show(ui, |ui| outline(app, ui));
}

fn header(ui: &mut egui::Ui, t: &Tokens, title: &str, actions: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, t, title);
        // At the row's end, in reading order.
        widgets::right_aligned(ui, "header-actions", actions);
    });
}

/// A folder of the tree: its subfolders and files, sorted.
#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<ProjectPath>,
}

fn tree<'a>(paths: impl Iterator<Item = &'a ProjectPath>, empty: impl Iterator<Item = &'a ProjectPath>) -> Folder {
    let mut root = Folder::default();
    for path in paths {
        let mut node = &mut root;
        let parts: Vec<&str> = path.as_str().split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            node = node.folders.entry((*part).to_owned()).or_default();
        }
        node.files.push(path.clone());
    }
    for folder in empty {
        let mut node = &mut root;
        for part in folder.as_str().split('/') {
            node = node.folders.entry(part.to_owned()).or_default();
        }
    }
    root
}

/// The icon for a file, from its extension.
fn file_icon(path: &ProjectPath) -> &'static str {
    match path.extension().as_str() {
        "tex" | "sty" | "cls" | "ltx" | "dtx" => "file-code",
        "bib" | "bst" | "bbx" | "cbx" => "book-open",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "eps" | "webp" => "image",
        "pdf" | "txt" | "md" => "file-text",
        _ => "file",
    }
}

/// An image of the project being dragged from the file list, to the editor (`file.insert_image`)
/// or onto a folder (moved there).
pub(crate) struct ImageDrag(pub ProjectPath);

/// Another file being dragged from the file list, onto a folder (moved there).
struct FileDrag(ProjectPath);

/// The file dropped on a row this frame, of either kind of drag.
fn dropped(r: &egui::Response) -> Option<ProjectPath> {
    r.dnd_release_payload::<ImageDrag>().map(|d| d.0.clone()).or_else(|| r.dnd_release_payload::<FileDrag>().map(|d| d.0.clone()))
}

/// Outlines `r` while a file is dragged over it, to show where it would go.
fn drop_target(ui: &egui::Ui, t: &Tokens, r: &egui::Response) {
    if r.dnd_hover_payload::<ImageDrag>().is_some() || r.dnd_hover_payload::<FileDrag>().is_some() {
        ui.painter().rect_stroke(r.rect, t.radius, ui.visuals().selection.stroke, egui::StrokeKind::Inside);
    }
}

/// Where a file named like `path` goes in `folder` ("" is the project's top level).
fn moved_to(path: &ProjectPath, folder: &str) -> String {
    let folder = folder.trim().trim_matches('/');
    if folder.is_empty() { path.file_name().to_owned() } else { format!("{folder}/{}", path.file_name()) }
}

/// The folder `path` is in ("" for the top level).
fn folder_of(path: &ProjectPath) -> &str {
    path.as_str().rsplit_once('/').map_or("", |(dir, _)| dir)
}

enum Action {
    Open(ProjectPath),
    InsertImage(ProjectPath),
    SetMain(ProjectPath),
    Rename(ProjectPath),
    MoveTo(ProjectPath),
    Move(ProjectPath, String),
    NewIn(&'static str, String),
    Delete(ProjectPath),
    Toggle(String),
}

fn files(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    header(ui, &t, tl!("Files"), |ui| {
        let new_file = widgets::icon_button(ui, &t, "file-plus", tl!("New File…"), true);
        app.region(crate::Region::Side, new_file.id);
        if new_file.clicked() {
            ui_cmd::run(app, "file.new");
        }
        if widgets::icon_button(ui, &t, "folder-plus", tl!("New Folder…"), true).clicked() {
            ui_cmd::run(app, "file.new_folder");
        }
        let import = texcraft_engine::cmd::find("file.import").map_or(Ok(()), |c| (c.enabled)(&app.session));
        let add = widgets::icon_button(ui, &t, "upload", tl!("Add Files…"), import.is_ok());
        if let Err(reason) = &import {
            let _ = add.on_disabled_hover_text(reason.as_str());
        } else if add.clicked() {
            ui_cmd::run(app, "file.import");
        }
    });
    let root = tree(app.session.project().paths(), app.session.empty_folders());
    let mut action = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        folder(app, ui, &root, "", 0, &mut action);
        // Below the rows: the project's top level, where a dragged file can be dropped.
        let rest = ui.allocate_response(ui.available_size().max(egui::vec2(0.0, t.row_height)), egui::Sense::hover());
        drop_target(ui, &t, &rest);
        if let Some(p) = dropped(&rest) {
            action = Some(Action::Move(p, String::new()));
        }
    });
    match action {
        Some(Action::Open(p)) => {
            app.run("file.open", json!({"path": p.as_str()}));
        }
        Some(Action::InsertImage(p)) => crate::editor::insert_image(app, &p, None),
        Some(Action::SetMain(p)) => {
            app.run("file.set_main", json!({"path": p.as_str()}));
        }
        Some(Action::Toggle(dir)) if !app.view.collapsed.remove(&dir) => {
            app.view.collapsed.insert(dir);
        }
        Some(Action::Rename(p)) => {
            let from = p.as_str().to_owned();
            app.view.dialog = Some(Dialog::Prompt(Prompt {
                title: tl!("Rename File"),
                label: tl!("Its new path in the project"),
                value: from.clone(),
                action: tl!("Rename"),
                command: "file.rename",
                params: Box::new(move |to| json!({"from": from, "to": to})),
                focus: true,
            }));
        }
        Some(Action::Move(p, folder)) => {
            let to = moved_to(&p, &folder);
            if to != p.as_str() {
                app.run("file.rename", json!({"from": p.as_str(), "to": to}));
            }
        }
        Some(Action::MoveTo(p)) => {
            let from = p.as_str().to_owned();
            app.view.dialog = Some(Dialog::Prompt(Prompt {
                title: tl!("Move File"),
                label: tl!("The folder to move it to (empty for the top level)"),
                value: folder_of(&p).to_owned(),
                action: tl!("Move"),
                command: "file.rename",
                params: Box::new(move |folder| json!({"from": from, "to": moved_to(&p, folder)})),
                focus: true,
            }));
        }
        Some(Action::NewIn(command, dir)) => {
            ui_cmd::run(app, command);
            if let Some(Dialog::Prompt(prompt)) = &mut app.view.dialog {
                prompt.value = format!("{dir}/");
            }
        }
        Some(Action::Delete(p)) => {
            app.view.dialog = Some(Dialog::Confirm(Confirm {
                title: tl!("Delete {file}?").replace("{file}", p.file_name()),
                message: tl!("The file is removed from the project. This cannot be undone.").to_owned(),
                action: tl!("Delete"),
                command: "file.delete",
                params: json!({"path": p.as_str()}),
            }));
        }
        Some(Action::Toggle(_)) | None => {}
    }
}

fn folder(app: &App, ui: &mut egui::Ui, node: &Folder, prefix: &str, depth: usize, action: &mut Option<Action>) {
    let t = app.tokens;
    for (name, sub) in &node.folders {
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let open = !app.view.collapsed.contains(&path);
        let row = Row {
            depth,
            icon: if open { "folder-open" } else { "folder" },
            label: name,
            selected: false,
            disclosure: Some(open),
            tag: None,
            icon_color: None,
            muted: false,
        };
        let r = widgets::row(ui, &t, &row);
        if r.clicked() {
            *action = Some(Action::Toggle(path.clone()));
        }
        drop_target(ui, &t, &r);
        if let Some(p) = dropped(&r) {
            *action = Some(Action::Move(p, path.clone()));
        }
        context_menu(ui, &r, |ui| {
            if crate::top_bar::item(ui, &t, Some("file-plus"), tl!("New File…"), None, Ok(())) {
                *action = Some(Action::NewIn("file.new", path.clone()));
            }
            if crate::top_bar::item(ui, &t, Some("folder-plus"), tl!("New Folder…"), None, Ok(())) {
                *action = Some(Action::NewIn("file.new_folder", path.clone()));
            }
        });
        if open {
            folder(app, ui, sub, &path, depth + 1, action);
        }
    }
    let main = app.session.project().main();
    let current = app.session.current();
    for path in &node.files {
        let is_main = main == Some(path);
        let row = Row {
            depth,
            icon: file_icon(path),
            label: path.file_name(),
            selected: current == Some(path),
            disclosure: None,
            tag: is_main.then(|| tl!("main")),
            icon_color: is_main.then_some(t.accent),
            muted: false,
        };
        let image = texcraft_engine::cmd::is_image(path);
        let r = widgets::row_sensing(ui, &t, &row, egui::Sense::click_and_drag());
        // A finger's drag scrolls the list (#26); a mouse's moves the file.
        let finger = crate::touch::scroll(ui, &r);
        if finger {
            // Nothing is moved.
        } else if image {
            r.dnd_set_drag_payload(ImageDrag(path.clone()));
        } else {
            r.dnd_set_drag_payload(FileDrag(path.clone()));
        }
        if r.dragged() && !finger {
            drag_label(ui, &t, row.icon, path.file_name());
        }
        let comments = crate::review::open_in(app, path);
        if comments > 0 {
            widgets::badge(ui, &t, egui::pos2(r.rect.right() - 4.0 * t.unit, r.rect.center().y), comments, t.text_muted);
            let said = crate::i18n::trn(comments, "{n} open comment", "{n} open comments");
            ui.ctx().accesskit_node_builder(r.id, |node| node.set_description(said));
        }
        if r.clicked() {
            *action = Some(Action::Open(path.clone()));
        }
        context_menu(ui, &r, |ui| {
            if crate::top_bar::item(ui, &t, Some("file-text"), tl!("Open"), None, Ok(())) {
                *action = Some(Action::Open(path.clone()));
            }
            let can_main = if path.extension() != "tex" {
                Err(tl!("Only a .tex file can be the main document.").to_owned())
            } else if is_main {
                Err(tl!("This is already the main document.").to_owned())
            } else {
                Ok(())
            };
            if crate::top_bar::item(ui, &t, Some("star"), tl!("Set as Main Document"), None, can_main) {
                *action = Some(Action::SetMain(path.clone()));
            }
            if image && crate::top_bar::item(ui, &t, Some("image"), tl!("Insert in Document"), None, crate::editor::can_insert(app)) {
                *action = Some(Action::InsertImage(path.clone()));
            }
            ui.separator();
            if crate::top_bar::item(ui, &t, Some("pencil"), tl!("Rename…"), None, Ok(())) {
                *action = Some(Action::Rename(path.clone()));
            }
            if crate::top_bar::item(ui, &t, Some("folder"), tl!("Move to…"), None, Ok(())) {
                *action = Some(Action::MoveTo(path.clone()));
            }
            if crate::top_bar::item(ui, &t, Some("trash"), tl!("Delete…"), None, Ok(())) {
                *action = Some(Action::Delete(path.clone()));
            }
        });
    }
}

/// The context menu of row `r`, which opens from the keyboard too (Shift+F10, the Menu key), at
/// the row.
fn context_menu(ui: &mut egui::Ui, r: &egui::Response, items: impl FnOnce(&mut egui::Ui)) {
    let by_key = r.id.with("menu-by-key");
    let mut menu = egui::Popup::context_menu(r);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(r));
    let mut opened = false;
    if r.has_focus()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10) || i.consume_key(egui::Modifiers::NONE, egui::Key::Menu))
    {
        ui.data_mut(|d| d.insert_temp(by_key, true));
        menu = menu.open_memory(egui::SetOpenCommand::Bool(true));
        opened = !open;
    }
    widgets::keyboard_popup(ui, r, opened, open);
    if ui.data(|d| d.get_temp::<bool>(by_key)).unwrap_or(false) {
        menu = menu.at_position(r.rect.left_bottom());
    }
    let shown = menu.show(|ui| {
        ui.set_min_width(200.0);
        items(ui);
    });
    if shown.is_none() {
        ui.data_mut(|d| d.remove::<bool>(by_key));
    }
}

/// The name of the file being dragged, by the pointer.
fn drag_label(ui: &egui::Ui, t: &Tokens, icon: &str, name: &str) {
    let Some(p) = ui.ctx().pointer_interact_pos() else { return };
    egui::Area::new(egui::Id::unique("texcraft-file-drag"))
        .order(egui::Order::Tooltip)
        .fixed_pos(p + egui::vec2(12.0, 8.0))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(t.raised).stroke(t.hairline()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(crate::icons::image(icon, t.icon_size, t.text_muted));
                    ui.label(egui::RichText::new(name).font(t.small()).color(t.text));
                });
            });
        });
}

/// The Outline: the whole document's (docs/ROADMAP.md, E14) when the shown file is part of it,
/// else the shown file's own.
fn outline(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    widgets::hairline(ui, &t);
    let mut chosen = None;
    header(ui, &t, tl!("Outline"), |ui| {
        let more = widgets::icon_button(ui, &t, "ellipsis", tl!("Outline options"), true);
        widgets::keyboard_menu(ui, &more);
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(180.0);
            for id in ["outline.collapse_all", "outline.expand_all"] {
                let (label, enabled) = ui_cmd::menu_entry(app, id);
                if crate::top_bar::item(ui, &t, None, label, None, enabled) {
                    chosen = Some(id);
                }
            }
        });
    });
    if let Some(id) = chosen {
        ui_cmd::run(app, id);
    }
    let Some(path) = app.session.current().cloned() else { return };
    let Ok(text) = app.session.project().text(&path) else {
        empty(ui, &t, tl!("This file has no outline."));
        return;
    };
    let text = text.to_owned();
    let document = app.session.document_outline();
    let in_document = document.iter().any(|e| e.file == path.as_str());
    if !in_document {
        file_outline(app, ui, &path, &text);
        return;
    }
    document_outline(app, ui, &path, &document);
}

/// The shown file's headings alone, for a file the main document does not include.
fn file_outline(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath, text: &str) {
    let t = app.tokens;
    let headings = texcraft_latex::outline(text);
    if headings.is_empty() {
        empty(ui, &t, tl!("No headings in this file yet."));
        return;
    }
    let keys: Vec<FoldKey> = headings.iter().map(|h| Some((fold_key(path.as_str(), h.level, &h.title), h.level))).collect();
    let fold = Folding::new(app, path.as_str(), &keys);
    // The section the caret is in: the last heading at or before its line, or the folded one
    // that holds it.
    let caret = app.view.cursor.map(|(line, _)| line.saturating_sub(1));
    let current = fold.shown(caret.and_then(|c| headings.iter().rposition(|h| h.line <= c)));
    let top = headings.iter().map(|h| h.level).min().unwrap_or(0);
    let mut goto = None;
    let mut toggle = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for (i, h) in headings.iter().enumerate() {
            if fold.hidden[i] {
                continue;
            }
            let depth = usize::from(h.level.saturating_sub(top));
            let row = Row {
                depth,
                icon: if h.level <= top { "heading-1" } else { "list" },
                label: &h.title,
                selected: current == Some(i),
                disclosure: fold.disclosure(i),
                tag: None,
                icon_color: None,
                muted: false,
            };
            let r = widgets::row(ui, &t, &row);
            match fold.react(ui, &t, &r, i, depth) {
                Some(Reaction::Toggle) => toggle = Some(i),
                Some(Reaction::Open) => goto = Some(h.line + 1),
                None => {}
            }
        }
    });
    if let Some(i) = toggle {
        fold.toggle(app, path.as_str(), &keys, i);
    }
    if let Some(line) = goto {
        app.view.goto = Some((path.clone(), line));
        crate::preview::follow_in_pdf(app, path, line);
    }
}

/// The document's headings through its includes, numbered as in the PDF: the shown file's in the
/// text's colour with the heading icons, the other files' muted with a file's icon (and named
/// after their file where a file's part starts), so the shown part does not stand out by colour
/// alone. An include that is not read says why in its place. A click opens the entry's file at
/// its line, and shows the heading in the PDF when the last build has it.
fn document_outline(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath, entries: &[texcraft_latex::document::Entry]) {
    use texcraft_latex::document::EntryKind;
    let t = app.tokens;
    let shown = path.as_str();
    let levels = || entries.iter().filter_map(|e| if let EntryKind::Heading { level, .. } = e.kind { Some(level) } else { None });
    let Some(top) = levels().min() else {
        // Only includes that are not read: say so, under the words for no headings.
        empty(ui, &t, tl!("No headings in this file yet."));
        for e in entries {
            problem(ui, &t, e, 0);
        }
        return;
    };
    let keys = document_keys(entries);
    let doc = document_key(app);
    let fold = Folding::new(app, &doc, &keys);
    // The section the caret is in: the shown file's last heading at or before its line, or the
    // folded one that holds it.
    let caret = app.view.cursor.map(|(line, _)| line.saturating_sub(1));
    let current =
        fold.shown(caret.and_then(|c| entries.iter().rposition(|e| e.file == shown && e.line <= c && matches!(e.kind, EntryKind::Heading { .. }))));
    let several = texcraft_latex::document::files(entries).len() > 1;
    let mut goto: Option<(String, u32)> = None;
    let mut toggle = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let mut depth = 0;
        let mut file_before: Option<&str> = None;
        for (i, e) in entries.iter().enumerate() {
            let EntryKind::Heading { level, title, number } = &e.kind else {
                if !fold.hidden[i] {
                    problem(ui, &t, e, depth + 1);
                }
                continue;
            };
            if fold.hidden[i] {
                continue;
            }
            depth = usize::from(level.saturating_sub(top));
            let ours = e.file == shown;
            let label = match number {
                Some(n) => format!("{n}  {title}"),
                None => title.clone(),
            };
            let name = e.file.rsplit('/').next().unwrap_or(&e.file);
            let starts = several && file_before != Some(e.file.as_str());
            file_before = Some(&e.file);
            let icon = match (ours, *level <= top) {
                (false, _) => "file-text",
                (true, true) => "heading-1",
                (true, false) => "list",
            };
            let row = Row {
                depth,
                icon,
                label: &label,
                selected: current == Some(i),
                disclosure: fold.disclosure(i),
                tag: starts.then_some(name),
                icon_color: None,
                muted: !ours,
            };
            let r = widgets::row(ui, &t, &row);
            // Where it is, for the screen reader: the file's name the row shows at a part's
            // start, and whether it is in the shown file.
            let place = if ours { tf!("in {file}, the file shown", file = name) } else { tf!("in {file}", file = name) };
            ui.ctx().accesskit_node_builder(r.id, |node| node.set_description(place));
            match fold.react(ui, &t, &r, i, depth) {
                Some(Reaction::Toggle) => toggle = Some(i),
                Some(Reaction::Open) => goto = Some((e.file.clone(), e.line + 1)),
                None => {}
            }
        }
    });
    if let Some(i) = toggle {
        fold.toggle(app, &doc, &keys, i);
    }
    if let Some((file, line)) = goto
        && let Ok(file) = ProjectPath::new(&file)
    {
        // The key that chose the entry must not reach the editor, which takes the keyboard now.
        ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
        });
        if file == *path || app.run("file.open", json!({"path": file.as_str()})).is_some() {
            crate::preview::follow_in_pdf(app, &file, line);
            app.view.goto = Some((file, line));
        }
    }
}

/// An outline entry's key and level ([`fold_key`]); `None` for an include not read.
type FoldKey = Option<(String, u8)>;

/// What tells an outline entry apart across edits: its file, level and title (lines move as
/// the text changes).
fn fold_key(file: &str, level: u8, title: &str) -> String {
    format!("{file}\u{1f}{level}\u{1f}{title}")
}

/// The keys and levels of the document outline's entries; `None` for an include not read.
fn document_keys(entries: &[texcraft_latex::document::Entry]) -> Vec<FoldKey> {
    use texcraft_latex::document::EntryKind;
    entries
        .iter()
        .map(|e| match &e.kind {
            EntryKind::Heading { level, title, .. } => Some((fold_key(&e.file, *level, title), *level)),
            EntryKind::Include { .. } => None,
        })
        .collect()
}

/// The document the folded entries are kept for: the main file's.
fn document_key(app: &App) -> String {
    app.session.project().main().map(|m| m.as_str().to_owned()).unwrap_or_default()
}

/// The outline shown now: the document whose folds it keeps and its entries' keys and levels,
/// as [`outline`] lists them.
fn shown_outline(app: &mut App) -> Option<(String, Vec<FoldKey>)> {
    let path = app.session.current().cloned()?;
    let document = app.session.document_outline();
    if document.iter().any(|e| e.file == path.as_str()) {
        return Some((document_key(app), document_keys(&document)));
    }
    let text = app.session.project().text(&path).ok()?;
    let keys = texcraft_latex::outline(text).iter().map(|h| Some((fold_key(path.as_str(), h.level, &h.title), h.level))).collect();
    Some((path.as_str().to_owned(), keys))
}

/// `outline.collapse_all` and `outline.expand_all`: folds every entry that has entries under it,
/// or unfolds them all, in the outline shown.
pub(crate) fn fold_all(app: &mut App, folded: bool) {
    let Some((doc, keys)) = shown_outline(app) else { return };
    let parents = parents(&keys);
    let set = app.view.outline_folded.entry(doc).or_default();
    set.clear();
    if folded {
        set.extend(keys.into_iter().zip(parents).filter(|(_, p)| *p).filter_map(|(k, _)| k.map(|(k, _)| k)));
    }
}

/// Whether each entry has entries under it: the next one is deeper, or an include not read.
fn parents(keys: &[FoldKey]) -> Vec<bool> {
    keys.iter()
        .enumerate()
        .map(|(i, k)| match (k, keys.get(i + 1)) {
            (Some((_, level)), Some(Some((_, next)))) => next > level,
            (Some(_), Some(None)) => true,
            _ => false,
        })
        .collect()
}

/// What a row of the outline asks for.
enum Reaction {
    /// Fold or unfold it: its chevron clicked, or Left or Right.
    Toggle,
    /// Open its file at its line: clicked elsewhere, or Enter.
    Open,
}

/// The outline's folds this frame: which entries have entries under them, which are folded, and
/// which a fold above hides.
struct Folding {
    parent: Vec<bool>,
    folded: Vec<bool>,
    hidden: Vec<bool>,
}

impl Folding {
    fn new(app: &App, doc: &str, keys: &[FoldKey]) -> Self {
        let parent = parents(keys);
        let set = app.view.outline_folded.get(doc);
        let folded: Vec<bool> =
            keys.iter().zip(&parent).map(|(k, p)| *p && k.as_ref().is_some_and(|(k, _)| set.is_some_and(|s| s.contains(k)))).collect();
        // Under a folded entry: everything deeper than it, up to the next entry at its level.
        let mut hidden = Vec::with_capacity(keys.len());
        let mut under: Option<u8> = None;
        for (i, k) in keys.iter().enumerate() {
            match k {
                Some((_, level)) if under.is_some_and(|f| *level > f) => hidden.push(true),
                Some((_, level)) => {
                    hidden.push(false);
                    under = folded[i].then_some(*level);
                }
                None => hidden.push(under.is_some()),
            }
        }
        Self { parent, folded, hidden }
    }

    /// The chevron of entry `i`: open or folded, for an entry with entries under it.
    fn disclosure(&self, i: usize) -> Option<bool> {
        self.parent[i].then_some(!self.folded[i])
    }

    /// Entry `i`, or the folded entry that hides it.
    fn shown(&self, i: Option<usize>) -> Option<usize> {
        let i = i?;
        (0..=i).rev().find(|j| !self.hidden[*j])
    }

    /// What row `r` of entry `i`, at `depth`, asks for: its chevron clicked or Left and Right
    /// with the keyboard fold and unfold it; elsewhere a click opens it.
    fn react(&self, ui: &egui::Ui, t: &Tokens, r: &egui::Response, i: usize, depth: usize) -> Option<Reaction> {
        if self.parent[i] && r.has_focus() {
            // Left and Right reach the row instead of moving the keyboard to its neighbours.
            ui.memory_mut(|m| m.set_focus_lock_filter(r.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() }));
            let (left, right) = ui.input_mut(|input| {
                (input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft), input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
            });
            if (left && !self.folded[i]) || (right && self.folded[i]) {
                return Some(Reaction::Toggle);
            }
        }
        if !r.clicked() {
            return None;
        }
        let on_chevron = r.interact_pointer_pos().is_some_and(|p| widgets::disclosure_rect(t, r.rect, depth).contains(p));
        Some(if self.parent[i] && on_chevron { Reaction::Toggle } else { Reaction::Open })
    }

    /// Folds or unfolds entry `i`, for the rest of the session.
    fn toggle(&self, app: &mut App, doc: &str, keys: &[FoldKey], i: usize) {
        let Some(Some((key, _))) = keys.get(i) else { return };
        let set = app.view.outline_folded.entry(doc.to_owned()).or_default();
        if !set.remove(key) {
            set.insert(key.clone());
        }
    }
}

/// An include the outline does not read, in words, at `depth` under the heading before it.
fn problem(ui: &mut egui::Ui, t: &Tokens, e: &texcraft_latex::document::Entry, depth: usize) {
    use texcraft_latex::document::{EntryKind, Problem};
    let EntryKind::Include { name, problem, .. } = &e.kind else { return };
    let said = match problem {
        Problem::Missing => tl!("{name} is not among the project's files: the build will stop here."),
        Problem::Loop => tl!("{name} includes itself, so it is not shown again here."),
        Problem::TooDeep => tl!("{name} is included too deep to be shown here."),
    }
    .replace("{name}", name);
    ui.horizontal(|ui| {
        ui.add_space(2.0 * t.unit + depth as f32 * 4.0 * t.unit + t.icon_size + t.unit);
        ui.add(crate::icons::image("triangle-exclamation-point", t.icon_size, t.text_muted));
        ui.add(egui::Label::new(egui::RichText::new(said).font(t.small()).color(t.text_muted)).wrap());
    });
}

fn empty(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(t.unit * 2.0);
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 3.0);
        ui.label(egui::RichText::new(text).font(t.small()).color(t.text_muted));
    });
}
