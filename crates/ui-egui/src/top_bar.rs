//! The top bar (docs/design.md, Regions): the app mark and the menus on the left, the project's
//! name with its menu in the centre, and the layout switch on the right, then the window's
//! controls when the app draws the window's frame (`window.rs`).

use egui::{Align, Layout as EguiLayout, RichText, Sense, vec2};
use texcraft_engine::cmd;

use crate::theme::Tokens;
use crate::ui_cmd::{self, MENUS, UI_COMMANDS};
use crate::{App, Layout, icons, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(12, 0));
    egui::Panel::top("top-bar").exact_size(t.top_bar_height).frame(frame).show(ui, |ui| {
        let bar = ui.max_rect();
        // The bar's empty space moves the window; the controls drawn after it take the pointer.
        crate::window::bar_space(app, ui, ui.max_rect().expand2(vec2(12.0, 0.0)));
        ui.horizontal_centered(|ui| {
            if mark(ui, &t) {
                ui_cmd::run(app, "app.start");
            }
            ui.add_space(t.unit * 2.0);
            navigate(app, ui.ctx());
            let mut ids = Vec::with_capacity(MENUS.len());
            let mut entered = None;
            egui::MenuBar::new().ui(ui, |ui| {
                for (i, menu) in MENUS.iter().enumerate() {
                    let r = ui.menu_button(crate::i18n::t(menu), |ui| menu_items(app, ui, menu)).response;
                    widgets::focus_ring(ui, &t, &r, r.rect, t.corner());
                    // Enter or Space on a menu's button opens it with its first entry focused.
                    if r.clicked() && !r.clicked_by(egui::PointerButton::Primary) && egui::Popup::is_id_open(ui.ctx(), popup_id(r.id)) {
                        entered = Some(i);
                    }
                    ids.push(r.id);
                }
            });
            app.view.menu_ids = ids;
            settle(app, ui.ctx(), entered);
        });
        if !app.start {
            project_title(app, ui, bar);
        }
        let rect = egui::Rect::from_min_max(egui::pos2(bar.center().x, bar.top()), bar.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(EguiLayout::right_to_left(Align::Center)), |ui| {
            if app.own_frame() {
                crate::window::controls(app, ui);
                if !app.start {
                    ui.add_space(t.unit);
                    ui.add(egui::Separator::default().vertical().shrink(t.unit * 3.0));
                    ui.add_space(t.unit);
                }
            }
            if !app.start {
                layout_switch(app, ui);
                ui.add_space(t.unit * 2.0);
                crate::forge::badge(app, ui);
            }
        });
    });
}

/// The menu bar while the keyboard is in it: reached with F10 (`view.menu_bar`) or Alt alone; Left
/// and Right move between the menus, Down, Enter or Space open one with its first entry focused,
/// and Escape closes the open menu, then gives the keyboard back to where it was.
#[derive(Debug, Default)]
pub(crate) struct MenuNav {
    /// The control that had the keyboard before; it gets it back on leaving.
    return_to: Option<egui::Id>,
    /// Focus this menu's button this frame.
    focus: Option<usize>,
    /// Move the keyboard into this open menu's first entry.
    enter: Option<usize>,
    /// Last frame: the menu whose button had the keyboard, and the open menu. Escape takes the
    /// focus away before the frame sees it, so the keys go by these.
    focused: Option<usize>,
    open: Option<usize>,
}

/// `view.menu_bar`: the keyboard goes to the first menu, or back to where it was when it is in
/// the menu bar already.
pub(crate) fn toggle_menu_bar(app: &mut App) {
    if app.view.dialog.is_some() || app.pdf.presenting().is_some() {
        return;
    }
    if let Some(nav) = app.view.menu_nav.take() {
        leave(app, nav.return_to);
        return;
    }
    let return_to = app.ctx.memory(egui::Memory::focused);
    app.view.menu_nav = Some(MenuNav { return_to, focus: Some(0), ..MenuNav::default() });
    app.view.status = Some((tl!("Menu bar").to_owned(), false));
}

/// Closes the menus and gives the keyboard back to `to`.
fn leave(app: &mut App, to: Option<egui::Id>) {
    for id in &app.view.menu_ids {
        egui::Popup::close_id(&app.ctx, popup_id(*id));
    }
    if let Some(id) = to {
        app.ctx.memory_mut(|m| m.request_focus(id));
    }
}

/// The popup of the menu whose button is `button`.
fn popup_id(button: egui::Id) -> egui::Id {
    button.with("popup")
}

/// The menu bar's keys, before it is drawn, from where the keyboard was last frame.
fn navigate(app: &mut App, ctx: &egui::Context) {
    let ids = app.view.menu_ids.clone();
    let Some(nav) = app.view.menu_nav.as_mut() else { return };
    let n = ids.len();
    if n == 0 {
        return;
    }
    let none = egui::Modifiers::NONE;
    let key = |k| ctx.input_mut(|i| i.consume_key(none, k));
    let stay = |ctx: &egui::Context| ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    if key(egui::Key::Escape) {
        if let Some(i) = nav.open {
            egui::Popup::close_id(ctx, popup_id(ids[i]));
            nav.focus = Some(i);
            nav.enter = None;
        } else if nav.focused.is_some() {
            let to = nav.return_to;
            app.view.menu_nav = None;
            leave(app, to);
        }
        return;
    }
    let focused = ctx.memory(egui::Memory::focused);
    // The menu just opened was shown once: its entries can take the keyboard.
    if let Some(i) = nav.enter {
        if focused == Some(ids[i]) && nav.open == Some(i) {
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Down));
            nav.enter = None;
        } else if focused != Some(ids[i]) {
            nav.enter = None;
        }
    }
    let Some(i) = nav.focused.filter(|i| focused == Some(ids[*i])) else { return };
    let (left, right) = (key(egui::Key::ArrowLeft), key(egui::Key::ArrowRight));
    if left || right {
        let j = if right { (i + 1) % n } else { (i + n - 1) % n };
        stay(ctx);
        nav.focus = Some(j);
        if nav.open.is_some() {
            egui::Popup::open_id(ctx, popup_id(ids[j]));
            nav.enter = Some(j);
        }
    } else if nav.open != Some(i) && key(egui::Key::ArrowDown) {
        stay(ctx);
        egui::Popup::open_id(ctx, popup_id(ids[i]));
        nav.enter = Some(i);
    }
}

/// After the menu bar is drawn: moves the keyboard where [`navigate`] asked, and notes where it
/// is. Once it has left the menu bar and its menus, the menu bar lets go of it, giving it back to
/// where it was when nothing else took it (a command chosen in a menu).
fn settle(app: &mut App, ctx: &egui::Context, entered: Option<usize>) {
    let ids = &app.view.menu_ids;
    let Some(nav) = app.view.menu_nav.as_mut() else { return };
    if let Some(i) = nav.focus.take().filter(|i| *i < ids.len()) {
        ctx.memory_mut(|m| m.request_focus(ids[i]));
    }
    if entered.is_some() {
        nav.enter = entered;
    }
    let focused = ctx.memory(egui::Memory::focused);
    nav.focused = ids.iter().position(|id| focused == Some(*id));
    nav.open = ids.iter().position(|id| egui::Popup::is_id_open(ctx, popup_id(*id)));
    if nav.focused.is_none() && nav.open.is_none() && nav.enter.is_none() {
        let to = nav.return_to.filter(|_| focused.is_none());
        app.view.menu_nav = None;
        if let Some(id) = to {
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }
}

/// Alt pressed and released with nothing between (no key, click or other modifier) goes to the
/// menu bar, as F10 does. Not on a Mac, where Option is for typing and the menus are the system's.
#[derive(Debug, Default)]
pub(crate) struct AltTap {
    armed: bool,
    held: egui::Modifiers,
}

impl AltTap {
    /// Whether this frame's input completes an Alt tap.
    pub fn tapped(&mut self, ctx: &egui::Context) -> bool {
        if ctx.os() == egui::os::OperatingSystem::Mac {
            return false;
        }
        let mut tapped = false;
        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::ModifiersChanged(m) => {
                        if *m == egui::Modifiers::ALT {
                            self.armed = !self.held.alt;
                        } else if !m.any() && self.armed && self.held.alt {
                            tapped = true;
                            self.armed = false;
                        } else {
                            self.armed = false;
                        }
                        self.held = *m;
                    }
                    egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::WindowFocused(_) => self.armed = false,
                    _ => {}
                }
            }
        });
        tapped
    }
}

/// The app mark: the app icon, small. It is a button to the start page (the projects); returns
/// whether it was clicked.
fn mark(ui: &mut egui::Ui, t: &Tokens) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let name = tl!("Start Page");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if response.hovered() {
        ui.painter().rect_filled(rect, t.corner(), t.hover);
    }
    crate::app_icon::paint(ui, rect.shrink(2.0));
    widgets::focus_ring(ui, t, &response, rect, t.corner());
    let clicked = response.clicked();
    let _ = response.on_hover_text(tl!("TeXCraft: back to the start page")).on_hover_cursor(egui::CursorIcon::PointingHand);
    clicked
}

/// One menu item with an optional icon and shortcut; disabled items say why on hover.
pub(crate) fn item(ui: &mut egui::Ui, t: &Tokens, icon: Option<&str>, label: &str, shortcut: Option<String>, enabled: Result<(), String>) -> bool {
    item_response(ui, t, icon, label, shortcut, enabled).clicked()
}

/// [`item`]'s response, for giving it the keyboard.
pub(crate) fn item_response(
    ui: &mut egui::Ui,
    t: &Tokens,
    icon: Option<&str>,
    label: &str,
    shortcut: Option<String>,
    enabled: Result<(), String>,
) -> egui::Response {
    let mut button = match icon {
        Some(name) => egui::Button::image_and_text(icons::image(name, t.icon_size, t.text_muted), label),
        None => egui::Button::new(label),
    };
    if let Some(s) = shortcut {
        button = button.shortcut_text(RichText::new(s).color(t.text_muted));
    }
    let response = ui.add_enabled(enabled.is_ok(), button.frame_when_inactive(false));
    match enabled {
        Err(reason) => response.on_disabled_hover_text(reason),
        Ok(()) => response,
    }
}

/// The icon of an engine or view command in menus.
pub(crate) fn icon_for(id: &str) -> Option<&'static str> {
    Some(match id {
        "project.new" => "file-plus",
        "project.from_template" => "book-open",
        "edit.undo" => "undo-2",
        "edit.copy" => "copy",
        "edit.comment" => "message-square-text",
        "edit.suggest_mode" => "pencil",
        "edit.accept_all" => "check",
        "edit.reject_all" => "x",
        "edit.redo" => "redo-2",
        "edit.find" => "search",
        "project.open" => "folder-open",
        "forge.open" => "download",
        "forge.commit" => "git-commit-horizontal",
        "forge.pull" => "refresh-cw",
        "project.save" => "download",
        "file.import" => "upload",
        "file.new" => "file-code",
        "file.close" => "x",
        "app.settings" => "settings",
        "app.start" => "columns-2",
        "project.save_as" => "folder-plus",
        "project.rename" => "pencil",
        "build.save_pdf" => "file-text",
        "build.compile" => "play",
        "build.auto" => "refresh-cw",
        "view.logs" => "scroll-text",
        "view.pdf_window" => "window-out",
        "view.zoom_in" => "zoom-in",
        "view.zoom_out" => "zoom-out",
        "help.shortcuts" => "text-cursor-input",
        "help.manual" => "book-open",
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
        if item(ui, &t, icon_for(c.id), crate::i18n::t(c.label), app.keys.shortcut_text(c.id), ui_cmd::enabled(app, c.id)) {
            chosen = Some(c.id);
        }
    }
    if let Some(id) = chosen {
        ui.close();
        ui_cmd::run(app, id);
    }
}

/// The project's name being edited in place.
pub(crate) struct Rename {
    pub text: String,
    /// Focus the field and select its text next frame.
    pub focus: bool,
}

impl App {
    /// Starts editing the project's name in the top bar.
    pub(crate) fn start_rename(&mut self) {
        let text = if self.session.dir().is_some() { self.project_name() } else { String::new() };
        self.view.rename = Some(Rename { text, focus: true });
    }
}

/// The project's name, centred: a button that edits it in place, and beside it the project's menu.
fn project_title(app: &mut App, ui: &mut egui::Ui, bar: egui::Rect) {
    if app.view.rename.is_some() {
        rename_field(app, ui, bar);
        return;
    }
    let t = app.tokens;
    let name = app.project_name();
    let galley = ui.painter().layout_no_wrap(name.clone(), t.strong(), t.text);
    let menu_width = t.icon_size + 2.0 * t.unit;
    let width = galley.size().x + 3.0 * t.unit + menu_width + 4.0 * t.unit;
    let rect = egui::Rect::from_center_size(bar.center(), vec2(width, t.toolbar_height - 8.0));
    let (name_rect, menu_rect) = rect.split_left_right_at_x(rect.right() - menu_width - t.unit);
    let fill = crate::theme::blend(t.frame, t.hover);

    let response = ui.interact(name_rect, ui.scope_id().with("project-title"), Sense::click());
    app.region(crate::Region::TopBar, response.id);
    let folder = app.session.dir().is_some() && app.session.library().is_none();
    let action = match (app.session.dir().is_some(), folder) {
        (false, _) => tl!("name and save the project"),
        (true, true) => tl!("rename the project's folder"),
        (true, false) => tl!("rename the project"),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name}: {action}")));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(name_rect, t.corner(), fill);
    }
    let text_pos = egui::pos2(rect.left() + 3.0 * t.unit, rect.center().y - galley.size().y / 2.0);
    let text_right = text_pos.x + galley.size().x;
    ui.painter().galley(text_pos, galley, t.text);
    if app.session.is_dirty() {
        ui.painter().circle_filled(egui::pos2(text_right + 7.0, rect.center().y), 3.0, t.text_muted);
    }
    widgets::focus_ring(ui, &t, &response, name_rect, t.corner());
    let hint = match (app.session.dir().is_some(), folder) {
        (false, _) => tl!("Click to name the project and save it"),
        (true, true) => tl!("Click to rename the project's folder"),
        (true, false) => tl!("Click to rename the project"),
    };
    let hint = if app.session.is_dirty() { format!("{hint}\n{}", tl!("Unsaved changes")) } else { hint.to_owned() };
    if response.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::Text).clicked() {
        app.start_rename();
        return;
    }

    let menu = ui.interact(menu_rect, ui.scope_id().with("project-menu"), Sense::click());
    menu.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} {}", tl!("project menu"))));
    if menu.hovered() || menu.has_focus() {
        ui.painter().rect_filled(menu_rect, t.corner(), fill);
    }
    icons::paint(ui, menu_rect, "chevron-down", t.icon_size - 2.0, t.text_muted);
    widgets::focus_ring(ui, &t, &menu, menu_rect, t.corner());
    let menu = menu.on_hover_text(tl!("Project menu"));
    egui::Popup::menu(&menu).align(egui::RectAlign::BOTTOM).show(|ui| {
        ui.set_min_width(220.0);
        let mut chosen = None;
        recent_menu(app, ui);
        ui.separator();
        if item(ui, &t, icon_for("project.rename"), crate::i18n::t("Rename Project"), None, Ok(())) {
            chosen = Some("project.rename");
        }
        for id in ["project.save", "project.save_as", "build.save_pdf", "project.open", "project.new"] {
            if let Some(spec) = cmd::find(id)
                && item(ui, &t, icon_for(id), crate::i18n::t(spec.label), app.keys.shortcut_text(id), (spec.enabled)(&app.session))
            {
                chosen = Some(id);
            }
        }
        if let Some(id) = chosen {
            ui.close();
            if id == "project.rename" {
                app.start_rename();
            } else {
                ui_cmd::run(app, id);
            }
        }
    });
}

/// The project's name as a text field: Enter gives the project the name typed; Escape, or
/// leaving the field, keeps the name it had.
fn rename_field(app: &mut App, ui: &mut egui::Ui, bar: egui::Rect) {
    let t = app.tokens;
    let Some(rename) = app.view.rename.as_mut() else { return };
    let typed = ui.painter().layout_no_wrap(rename.text.clone(), t.strong(), t.text).size().x;
    let width = (typed + 8.0 * t.unit).clamp(240.0, (bar.width() * 0.4).max(240.0));
    let rect = egui::Rect::from_center_size(bar.center(), vec2(width, t.toolbar_height - 8.0));
    let id = ui.scope_id().with("project-name-field");
    let field = egui::TextEdit::singleline(&mut rename.text)
        .id(id)
        .font(t.strong())
        .hint_text(tl!("Project name"))
        .margin(vec2(3.0 * t.unit, 0.0))
        .align(egui::Align2::LEFT_CENTER);
    let response = ui.put(rect, field);
    // Named for screen readers by what it edits; the field shows the name itself.
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_label(tl!("Project name")));
    let first = std::mem::take(&mut rename.focus);
    if first {
        response.request_focus();
        if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
            state.cursor.set_char_range(Some(egui::text::CCursorRange::select_all_str(&rename.text)));
            state.store(ui.ctx(), id);
        }
    }
    app.region(crate::Region::TopBar, response.id);
    if first {
        return;
    }
    if response.lost_focus() || !response.has_focus() {
        let commit = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let text = app.view.rename.take().map(|r| r.text).unwrap_or_default();
        if commit {
            app.rename_project(&text);
        }
    }
}

fn layout_switch(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    // Right to left: the last entry is drawn first.
    for (layout, icon, label) in [
        (Layout::PreviewOnly, "file-text", tl!("PDF only")),
        (Layout::EditorOnly, "code", tl!("Editor only")),
        (Layout::Split, "columns-2", tl!("Editor and PDF")),
    ] {
        let shown = if app.view.pdf_window { Layout::EditorOnly } else { app.view.layout };
        if widgets::icon_toggle(ui, &t, icon, label, shown == layout).clicked() {
            let id = match layout {
                Layout::Split => "view.layout.split",
                Layout::EditorOnly => "view.layout.editor",
                Layout::PreviewOnly => "view.layout.preview",
            };
            ui_cmd::run(app, id);
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
            let exists = app.project_exists(path);
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
