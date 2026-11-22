//! The top bar (docs/design.md, Regions): the app mark and the menus on the left, the project's
//! name with its menu in the centre, and the layout switch on the right.

use egui::{Align, Align2, Layout as EguiLayout, RichText, Sense, vec2};
use texcraft_engine::cmd;

use crate::theme::Tokens;
use crate::ui_cmd::{self, MENUS, UI_COMMANDS};
use crate::{App, Layout, icons, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(12, 0));
    egui::Panel::top("top-bar").exact_size(t.top_bar_height).frame(frame).show(ui, |ui| {
        let bar = ui.max_rect();
        ui.horizontal_centered(|ui| {
            mark(ui, &t);
            ui.add_space(t.unit * 2.0);
            egui::MenuBar::new().ui(ui, |ui| {
                for menu in MENUS {
                    ui.menu_button(crate::i18n::t(menu), |ui| menu_items(app, ui, menu));
                }
            });
        });
        project_title(app, ui, bar);
        let rect = egui::Rect::from_min_max(egui::pos2(bar.center().x, bar.top()), bar.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(EguiLayout::right_to_left(Align::Center)), |ui| {
            layout_switch(app, ui);
        });
    });
}

/// The app mark: a rounded square in the accent colour.
fn mark(ui: &mut egui::Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    ui.painter().rect_filled(rect, 6, t.accent);
    ui.painter().text(rect.center() + vec2(0.0, 0.5), Align2::CENTER_CENTER, "T", egui::FontId::new(15.0, crate::fonts::semibold()), t.on_accent);
}

/// One menu item with an optional icon and shortcut; disabled items say why on hover.
pub(crate) fn item(ui: &mut egui::Ui, t: &Tokens, icon: Option<&str>, label: &str, shortcut: Option<String>, enabled: Result<(), String>) -> bool {
    let mut button = match icon {
        Some(name) => egui::Button::image_and_text(icons::image(name, t.icon_size, t.text_muted), label),
        None => egui::Button::new(label),
    };
    if let Some(s) = shortcut {
        button = button.shortcut_text(RichText::new(s).color(t.text_muted));
    }
    let response = ui.add_enabled(enabled.is_ok(), button.frame_when_inactive(false));
    match enabled {
        Err(reason) => {
            response.on_disabled_hover_text(reason);
            false
        }
        Ok(()) => response.clicked(),
    }
}

/// The icon of an engine or view command in menus.
fn icon_for(id: &str) -> Option<&'static str> {
    Some(match id {
        "project.new" => "file-plus",
        "project.open" => "folder-open",
        "project.save" => "download",
        "file.import" => "upload",
        "file.new" => "file-code",
        "file.close" => "x",
        "app.settings" => "settings",
        "project.save_as" => "folder-plus",
        "build.save_pdf" => "file-text",
        "build.compile" => "play",
        "build.auto" => "refresh-cw",
        "view.logs" => "scroll-text",
        "view.zoom_in" => "zoom-in",
        "view.zoom_out" => "zoom-out",
        "help.shortcuts" => "text-cursor-input",
        "help.about" => "info",
        "edit.figure" => "image",
        "edit.table" => "table",
        "edit.math_inline" | "edit.math_display" | "edit.equation" => "sigma",
        "edit.ref" => "tag",
        "edit.cite" => "book-open",
        "edit.link" => "link",
        "edit.itemize" => "list",
        "edit.enumerate" => "list-ordered",
        "edit.italic" => "italic",
        "edit.section" => "heading-1",
        _ => return None,
    })
}

fn menu_items(app: &mut App, ui: &mut egui::Ui, menu: &str) {
    let t = app.tokens;
    ui.set_min_width(220.0);
    let mut chosen = None;
    for spec in cmd::all().iter().filter(|c| c.menu.first() == Some(&menu)) {
        let mut label = crate::i18n::t(spec.label).to_owned();
        if spec.id == "build.auto" && app.session.settings.auto_compile {
            label = format!("{label} ✓");
        }
        if item(ui, &t, icon_for(spec.id), &label, app.keys.shortcut_text(spec.id), (spec.enabled)(&app.session)) {
            chosen = Some(spec.id);
        }
    }
    if menu == "File" {
        ui.separator();
        recent_menu(app, ui);
    }
    let mut last_group = "";
    for c in UI_COMMANDS.iter().filter(|c| c.menu == menu) {
        if !last_group.is_empty() && c.group != last_group {
            ui.separator();
        }
        last_group = c.group;
        if item(ui, &t, icon_for(c.id), crate::i18n::t(c.label), app.keys.shortcut_text(c.id), Ok(())) {
            chosen = Some(c.id);
        }
    }
    if let Some(id) = chosen {
        ui.close();
        ui_cmd::run(app, id);
    }
}

fn project_title(app: &mut App, ui: &mut egui::Ui, bar: egui::Rect) {
    let t = app.tokens;
    let name = app.project_name();
    let galley = ui.painter().layout_no_wrap(name.clone(), t.strong(), t.text);
    let width = galley.size().x + 3.0 * t.unit + t.icon_size + 6.0 * t.unit;
    let rect = egui::Rect::from_center_size(bar.center(), vec2(width, t.toolbar_height - 8.0));
    let response = ui.interact(rect, ui.scope_id().with("project-title"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} {}", tl!("project menu"))));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, t.corner(), crate::theme::blend(t.frame, t.hover));
    }
    let text_pos = egui::pos2(rect.left() + 3.0 * t.unit, rect.center().y - galley.size().y / 2.0);
    let text_right = text_pos.x + galley.size().x;
    ui.painter().galley(text_pos, galley, t.text);
    if app.session.is_dirty() {
        ui.painter().circle_filled(egui::pos2(text_right + 7.0, rect.center().y), 3.0, t.text_muted);
    }
    let chevron = egui::Rect::from_min_size(egui::pos2(text_right + 12.0, rect.top()), vec2(t.icon_size, rect.height()));
    icons::paint(ui, chevron, "chevron-down", t.icon_size - 2.0, t.text_muted);
    widgets::focus_ring(ui, &t, &response, rect, t.corner());
    let response = if app.session.is_dirty() { response.on_hover_text(tl!("Unsaved changes")) } else { response };
    egui::Popup::menu(&response).align(egui::RectAlign::BOTTOM).show(|ui| {
        ui.set_min_width(220.0);
        let mut chosen = None;
        recent_menu(app, ui);
        ui.separator();
        for id in ["project.save", "project.save_as", "build.save_pdf", "project.open", "project.new"] {
            if let Some(spec) = cmd::find(id)
                && item(ui, &t, icon_for(id), crate::i18n::t(spec.label), app.keys.shortcut_text(id), (spec.enabled)(&app.session))
            {
                chosen = Some(id);
            }
        }
        if let Some(id) = chosen {
            ui.close();
            ui_cmd::run(app, id);
        }
    });
}

fn layout_switch(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    // Right to left: the last entry is drawn first.
    for (layout, icon, label) in [
        (Layout::PreviewOnly, "file-text", tl!("PDF only")),
        (Layout::EditorOnly, "code", tl!("Editor only")),
        (Layout::Split, "columns-2", tl!("Editor and PDF")),
    ] {
        if widgets::icon_toggle(ui, &t, icon, label, app.view.layout == layout).clicked() {
            app.view.layout = layout;
        }
    }
}

/// The recent projects, as a submenu: folder name, with where it is below in muted text. A folder
/// that no longer exists is shown disabled, with the reason.
pub(crate) fn recent_menu(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut open = None;
    let mut clear = false;
    ui.menu_button(tl!("Open Recent"), |ui| {
        ui.set_min_width(300.0);
        if app.prefs.recent.is_empty() {
            ui.add_enabled(false, egui::Button::new(tl!("No recent projects")));
            return;
        }
        let current = app.session.dir().map(|d| d.display().to_string());
        for path in &app.prefs.recent {
            let p = std::path::Path::new(path);
            let name = p.file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
            let parent = p.parent().map(|d| home_relative(&d.display().to_string())).unwrap_or_default();
            let exists = cfg!(target_arch = "wasm32") || p.is_dir();
            let mut job = egui::text::LayoutJob::default();
            let strong = egui::TextFormat { font_id: t.body(), color: t.text, ..Default::default() };
            let muted = egui::TextFormat { font_id: t.small(), color: t.text_muted, ..Default::default() };
            job.append(&name, 0.0, strong);
            job.append(&format!("\n{parent}"), 0.0, muted);
            let selected = current.as_deref() == Some(path.as_str());
            let r = ui.add_enabled(exists, egui::Button::selectable(selected, job));
            let r = r.on_disabled_hover_text(tl!("This folder no longer exists."));
            if r.clicked() {
                open = Some(path.clone());
                ui.close();
            }
        }
        ui.separator();
        if ui.add(egui::Button::new(tl!("Clear Recent Projects")).frame_when_inactive(false)).clicked() {
            clear = true;
            ui.close();
        }
    });
    if let Some(path) = open {
        app.guard(crate::dialogs::Next::OpenProject(path));
    }
    if clear {
        ui_cmd::run(app, "project.clear_recent");
    }
}

/// `path` with the home folder written as `~`.
fn home_relative(path: &str) -> String {
    match std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        Some(home) if path == home => "~".to_owned(),
        Some(home) if path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_owned(),
    }
}
