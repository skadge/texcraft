//! The side panel (docs/design.md, Regions): the project's files as a tree, and the outline of the
//! shown file below it in a resizable section.

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
    egui::Panel::left("side").resizable(true).default_size(248.0).min_size(180.0).max_size(480.0).frame(frame).show(ui, |ui| {
        if app.view.side == Some(Side::Review) {
            crate::review::panel(app, ui);
            return;
        }
        let outline_first = app.view.side == Some(Side::Outline);
        egui::Panel::bottom("outline")
            .resizable(true)
            .default_size(if outline_first { ui.available_height() * 0.7 } else { 220.0 })
            .min_size(80.0)
            .frame(egui::Frame::new().fill(t.frame))
            .show(ui, |ui| outline(app, ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.frame)).show(ui, |ui| files(app, ui));
    });
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

fn tree(paths: impl Iterator<Item = ProjectPath>) -> Folder {
    let mut root = Folder::default();
    for path in paths {
        let mut node = &mut root;
        let parts: Vec<&str> = path.as_str().split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            node = node.folders.entry((*part).to_owned()).or_default();
        }
        node.files.push(path.clone());
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

enum Action {
    Open(ProjectPath),
    SetMain(ProjectPath),
    Rename(ProjectPath),
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
        if widgets::icon_button(ui, &t, "file-plus", tl!("New File…"), true).clicked() {
            ui_cmd::run(app, "file.new");
        }
    });
    let root = tree(app.session.project().paths().cloned());
    let mut action = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        folder(app, ui, &root, "", 0, &mut action);
    });
    match action {
        Some(Action::Open(p)) => {
            app.run("file.open", json!({"path": p.as_str()}));
        }
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
        if widgets::row(ui, &t, &row).clicked() {
            *action = Some(Action::Toggle(path.clone()));
        }
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
        let r = widgets::row(ui, &t, &row);
        let comments = crate::review::open_in(app, path);
        if comments > 0 {
            widgets::badge(ui, &t, egui::pos2(r.rect.right() - 4.0 * t.unit, r.rect.center().y), comments, t.text_muted);
            let said = crate::i18n::trn(comments, "{n} open comment", "{n} open comments");
            ui.ctx().accesskit_node_builder(r.id, |node| node.set_description(said));
        }
        if r.clicked() {
            *action = Some(Action::Open(path.clone()));
        }
        r.context_menu(|ui| {
            ui.set_min_width(200.0);
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
            ui.separator();
            if crate::top_bar::item(ui, &t, Some("pencil"), tl!("Rename…"), None, Ok(())) {
                *action = Some(Action::Rename(path.clone()));
            }
            if crate::top_bar::item(ui, &t, Some("trash"), tl!("Delete…"), None, Ok(())) {
                *action = Some(Action::Delete(path.clone()));
            }
        });
    }
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
