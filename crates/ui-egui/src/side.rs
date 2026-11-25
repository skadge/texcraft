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
    let shown = egui::Panel::left("side").resizable(true).default_size(248.0).min_size(180.0).max_size(480.0).frame(frame).show(ui, |ui| {
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
    });
    widgets::name_splitter(ui, "side", tl!("Resize the side panel"));
    app.region_area(crate::Region::Side, shown.response.rect);
}

fn header(ui: &mut egui::Ui, t: &Tokens, title: &str, actions: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, t, title);
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
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
        let import = texcraft_engine::cmd::find("file.import").map_or(Ok(()), |c| (c.enabled)(&app.session));
        let add = widgets::icon_button(ui, &t, "upload", tl!("Add Files…"), import.is_ok());
        app.region(crate::Region::Side, add.id);
        if let Err(reason) = &import {
            let _ = add.on_disabled_hover_text(reason.as_str());
        } else if add.clicked() {
            ui_cmd::run(app, "file.import");
        }
        if widgets::icon_button(ui, &t, "folder-plus", tl!("New Folder…"), true).clicked() {
            ui_cmd::run(app, "file.new_folder");
        }
        if widgets::icon_button(ui, &t, "file-plus", tl!("New File…"), true).clicked() {
            ui_cmd::run(app, "file.new");
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
        };
        let image = texcraft_engine::cmd::is_image(path);
        let r = widgets::row_sensing(ui, &t, &row, egui::Sense::click_and_drag());
        if image {
            r.dnd_set_drag_payload(ImageDrag(path.clone()));
        } else {
            r.dnd_set_drag_payload(FileDrag(path.clone()));
        }
        if r.dragged() {
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

fn outline(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    widgets::hairline(ui, &t);
    header(ui, &t, tl!("Outline"), |_| {});
    let Some(path) = app.session.current().cloned() else { return };
    let Ok(text) = app.session.project().text(&path) else {
        empty(ui, &t, tl!("This file has no outline."));
        return;
    };
    let headings = texcraft_latex::outline(text);
    if headings.is_empty() {
        empty(ui, &t, tl!("Sections of the shown file appear here."));
        return;
    }
    // The section the caret is in: the last heading at or before its line.
    let caret = app.view.cursor.map(|(line, _)| line.saturating_sub(1));
    let current = caret.and_then(|c| headings.iter().rposition(|h| h.line <= c));
    let top = headings.iter().map(|h| h.level).min().unwrap_or(0);
    let mut goto = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for (i, h) in headings.iter().enumerate() {
            let row = Row {
                depth: usize::from(h.level.saturating_sub(top)),
                icon: if h.level <= top { "heading-1" } else { "list" },
                label: &h.title,
                selected: current == Some(i),
                disclosure: None,
                tag: None,
                icon_color: None,
            };
            if widgets::row(ui, &t, &row).clicked() {
                goto = Some(h.line + 1);
            }
        }
    });
    if let Some(line) = goto {
        app.view.goto = Some((path, line));
    }
}

fn empty(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(t.unit * 2.0);
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 3.0);
        ui.label(egui::RichText::new(text).font(t.small()).color(t.text_muted));
    });
}
