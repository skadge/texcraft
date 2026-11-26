//! The top bar (docs/design.md, Regions): the app mark and the menus on the left, the project's
//! name with its menu in the centre, and the layout switch on the right, then the window's
//! controls when the app draws the window's frame (`window.rs`).

use egui::{Align, Layout as EguiLayout, RichText, Sense, vec2};

use crate::theme::Tokens;
use crate::ui_cmd::{self, Entry, MENUS};
use crate::{App, Layout, icons, widgets};

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame).inner_margin(egui::Margin::symmetric(12, 0));
    let shown = egui::Panel::top("top-bar").exact_size(t.top_bar_height).frame(frame).show(ui, |ui| {
        let bar = ui.max_rect();
        // Where the menus end: a narrow window's title goes after them.
        let mut menus_end = bar.left() + 30.0;
        // The bar's empty space moves the window; the controls drawn after it take the pointer.
        crate::window::bar_space(app, ui, ui.max_rect().expand2(vec2(12.0, 0.0)));
        ui.horizontal_centered(|ui| {
            if mark(ui, &t) {
                ui_cmd::run(app, "app.start");
            }
            ui.add_space(t.unit * 2.0);
            // On macOS the menus are in the system's menu bar, when the shell installed it.
            if app.has_system_menu() {
                return;
            }
            navigate(app, ui.ctx());
            let mut ids = Vec::with_capacity(MENUS.len());
            let mut entered = None;
            let narrow = app.view.narrow;
            egui::MenuBar::new().ui(ui, |ui| {
                // A narrow window has room for one menu, holding the others.
                if narrow {
                    let r = ui
                        .menu_button(tl!("Menu"), |ui| {
                            for menu in MENUS {
                                ui.menu_button(crate::i18n::t(menu), |ui| menu_items(app, ui, menu));
                            }
                        })
                        .response;
                    widgets::focus_ring(ui, &t, &r, r.rect, t.corner());
                    if r.clicked() && !r.clicked_by(egui::PointerButton::Primary) && egui::Popup::is_id_open(ui.ctx(), popup_id(r.id)) {
                        entered = Some(0);
                    }
                    ids.push(r.id);
                    menus_end = r.rect.right();
                    return;
                }
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
            // Narrow, the title is between the menu and the controls at the bar's end (the
            // window's, without the layout switch, which the tabs replace).
            let room = if app.view.narrow {
                let controls: f32 = ui.data(|d| d.get_temp(ui.scope_id().with("bar-end"))).unwrap_or(0.0);
                egui::Rect::from_min_max(egui::pos2(menus_end + 2.0 * t.unit, bar.top()), egui::pos2(bar.right() - controls - t.unit, bar.bottom()))
            } else {
                bar
            };
            if let Some(title) = project_title(app, ui, room) {
                beside_title(app, ui, title, room);
            }
        }
        let rect = egui::Rect::from_min_max(egui::pos2(bar.center().x, bar.top()), bar.max);
        // At the bar's end, in reading order: the repository's badge, the layout switch, then the
        // window's buttons.
        let end = ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(EguiLayout::left_to_right(Align::Center)), |ui| {
            widgets::right_aligned(ui, "bar-end-controls", |ui| {
                if !app.start {
                    crate::forge::badge(app, ui);
                    if !app.view.narrow {
                        ui.add_space(t.unit * 2.0);
                        layout_switch(app, ui);
                    }
                }
                if app.own_frame() {
                    if !app.start {
                        ui.add_space(t.unit);
                        ui.add(egui::Separator::default().vertical().shrink(t.unit * 3.0));
                        ui.add_space(t.unit);
                    }
                    crate::window::controls(app, ui);
                }
                ui.min_rect().width()
            })
        });
        let width = end.inner;
        ui.data_mut(|d| d.insert_temp(ui.scope_id().with("bar-end"), width));
    });
    app.region_area(crate::Region::TopBar, shown.response.rect);
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
    if let Err(reason) = ui_cmd::enabled(app, "view.menu_bar") {
        app.view.status = Some((reason, false));
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
        Err(reason) => response.on_disabled_hover_text(crate::i18n::msg(&reason)),
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
        "edit.symbols" => "omega",
        "edit.spelling.add" => "plus",
        "edit.spelling.ignore" | "edit.grammar.ignore" => "eraser",
        "edit.grammar.rule_off" => "x",
        "project.open" => "folder-open",
        "forge.open" | "project.import_dialog" => "import",
        "forge.commit" => "git-commit-horizontal",
        "forge.pull" => "refresh-cw",
        "project.save" => "download",
        "file.import" => "upload",
        "file.unzip" => "upload",
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
        "help.guide" => "circle-question-mark",
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
        "edit.ai_make" | "edit.ai_reference" | "edit.ai_review" | "edit.ai_grammar" | "edit.instruct" | "edit.rewrite" => crate::ai::ICON,
        _ => return None,
    })
}

fn menu_items(app: &mut App, ui: &mut egui::Ui, menu: &str) {
    ui.set_min_width(220.0);
    let mut chosen = None;
    let mut rewrite = None;
    entries(app, ui, ui_cmd::menu(menu), &mut chosen, &mut rewrite);
    if let Some(choice) = rewrite {
        ui.close();
        crate::ai::run_rewrite_choice(app, &choice);
    }
    if let Some(id) = chosen {
        ui.close();
        if !crate::fields::menu_command(app, id) {
            ui_cmd::run(app, id);
        }
    }
}

/// Draws `list`'s entries ([`ui_cmd::MENU_BAR`]), those that apply here, with a line between
/// groups; the entry chosen goes to `chosen`, a way of rewriting to `rewrite`.
fn entries(app: &mut App, ui: &mut egui::Ui, list: &[Entry], chosen: &mut Option<&'static str>, rewrite: &mut Option<serde_json::Value>) {
    let t = app.tokens;
    // A line is drawn only between two entries shown.
    let mut line = false;
    let mut drawn = false;
    for entry in list {
        let shown = match entry {
            Entry::Separator => {
                line = drawn;
                continue;
            }
            Entry::Command(id) => ui_cmd::in_menus(app, id),
            Entry::Submenu(_, inner) => ui_cmd::any_shown(app, inner),
            Entry::Recent => true,
            Entry::Rewrite => crate::ai::offered(app),
        };
        if !shown {
            continue;
        }
        if std::mem::take(&mut line) {
            ui.separator();
        }
        drawn = true;
        match entry {
            Entry::Command(id) => {
                let (label, enabled) = ui_cmd::menu_entry(app, id);
                let enabled = if texcraft_engine::cmd::find(id).is_none() { crate::fields::menu_enabled(app, ui.ctx(), id) } else { enabled };
                if item(ui, &t, icon_for(id), label, app.keys.shortcut_text(id), enabled) {
                    *chosen = Some(id);
                }
            }
            Entry::Submenu(title, inner) => {
                ui.menu_button(crate::i18n::t(title), |ui| {
                    ui.set_min_width(220.0);
                    entries(app, ui, inner, chosen, rewrite);
                });
            }
            Entry::Recent => recent_menu(app, ui),
            Entry::Rewrite => {
                ui.menu_image_text_button(crate::icons::image(crate::ai::ICON, 14.0, t.text_muted), tl!("Rewrite with AI"), |ui| {
                    ui.set_min_width(220.0);
                    *rewrite = crate::ai::rewrite_menu(app, ui);
                });
            }
            Entry::Separator => {}
        }
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

/// The project's name, centred in `bar`: a button that edits it in place, and beside it the
/// project's menu. Returns where it is drawn (none while the name is edited).
fn project_title(app: &mut App, ui: &mut egui::Ui, bar: egui::Rect) -> Option<egui::Rect> {
    if app.view.rename.is_some() {
        rename_field(app, ui, bar);
        return None;
    }
    let t = app.tokens;
    let name = app.project_name();
    let menu_width = t.icon_size + 2.0 * t.unit;
    // A name longer than the room is cut short with an ellipsis (a narrow window).
    let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), t.strong(), t.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((bar.width() - 7.0 * t.unit - menu_width).max(4.0 * t.unit));
    let galley = ui.painter().layout_job(job);
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
    // The dot of unsaved changes, where the window does not save by itself (the words beside the
    // title say how saving stands where it does).
    let saving_itself = crate::autosave::state(app).is_some();
    if app.session.is_dirty() && !saving_itself {
        ui.painter().circle_filled(egui::pos2(text_right + 7.0, rect.center().y), 3.0, t.text_muted);
    }
    widgets::focus_ring(ui, &t, &response, name_rect, t.corner());
    let hint = match (app.session.dir().is_some(), folder) {
        (false, _) => tl!("Click to name the project and save it"),
        (true, true) => tl!("Click to rename the project's folder"),
        (true, false) => tl!("Click to rename the project"),
    };
    let hint = if app.session.is_dirty() && !saving_itself { format!("{hint}\n{}", tl!("Unsaved changes")) } else { hint.to_owned() };
    if response.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::Text).clicked() {
        app.start_rename();
        return Some(rect);
    }

    let menu = ui.interact(menu_rect, ui.scope_id().with("project-menu"), Sense::click());
    menu.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} {}", tl!("project menu"))));
    if menu.hovered() || menu.has_focus() {
        ui.painter().rect_filled(menu_rect, t.corner(), fill);
    }
    icons::paint(ui, menu_rect, "chevron-down", t.icon_size - 2.0, t.text_muted);
    widgets::focus_ring(ui, &t, &menu, menu_rect, t.corner());
    let menu = menu.on_hover_text(tl!("Project menu"));
    crate::widgets::keyboard_menu(ui, &menu);
    egui::Popup::menu(&menu).align(egui::RectAlign::BOTTOM).show(|ui| {
        ui.set_min_width(220.0);
        let mut chosen = None;
        recent_menu(app, ui);
        ui.separator();
        if item(ui, &t, icon_for("project.rename"), tl!("Rename Project"), None, Ok(())) {
            chosen = Some("project.rename");
        }
        for id in ["project.save", "project.save_as", "build.save_pdf", "project.open", "project.new"] {
            let (label, enabled) = ui_cmd::menu_entry(app, id);
            if item(ui, &t, icon_for(id), label, app.keys.shortcut_text(id), enabled) {
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
    Some(rect)
}

/// After the project's name: Sync, when the project has a remote, and how saving stands, dim
/// ("All changes saved", "Saving…", or why the last write failed), where the window saves by
/// itself. Kept to the room before the controls at the bar's end.
fn beside_title(app: &mut App, ui: &mut egui::Ui, title: egui::Rect, bar: egui::Rect) {
    let t = app.tokens;
    let controls: f32 = ui.data(|d| d.get_temp(ui.scope_id().with("bar-end"))).unwrap_or(0.0);
    let right = (bar.right() - controls - 2.0 * t.unit).max(title.right());
    let room = egui::Rect::from_min_max(egui::pos2(title.right() + t.unit, bar.top()), egui::pos2(right, bar.bottom()));
    if room.width() < 4.0 * t.unit {
        return;
    }
    let narrow = app.view.narrow;
    ui.scope_builder(egui::UiBuilder::new().max_rect(room).layout(EguiLayout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 1.5 * t.unit;
        crate::sync::button(app, ui, narrow);
        let Some(state) = crate::autosave::state(app) else { return };
        let (text, color) = match &state {
            crate::autosave::State::Saved => (tl!("All changes saved").to_owned(), t.text_muted),
            crate::autosave::State::Saving => (tl!("Saving…").to_owned(), t.text_muted),
            crate::autosave::State::Failed(_) => (tl!("Not saved").to_owned(), t.error),
        };
        if narrow && !matches!(state, crate::autosave::State::Failed(_)) {
            return;
        }
        let label = ui.add(egui::Label::new(RichText::new(&text).font(t.small()).color(color)).truncate().sense(Sense::hover()));
        if let crate::autosave::State::Failed(why) = &state {
            let said = format!("{text}: {why}");
            crate::status::announce(ui, &label, &said);
            label.on_hover_text(said);
        }
    });
}

/// The project's name as a text field: Enter gives the project the name typed; Escape, or
/// leaving the field, keeps the name it had.
fn rename_field(app: &mut App, ui: &mut egui::Ui, bar: egui::Rect) {
    let t = app.tokens;
    let Some(rename) = app.view.rename.as_mut() else { return };
    let typed = ui.painter().layout_no_wrap(rename.text.clone(), t.strong(), t.text).size().x;
    let width = (typed + 8.0 * t.unit).clamp(240.0_f32.min(bar.width()), (bar.width() * 0.4).max(240.0).min(bar.width()));
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
    // In reading order. Named as their commands are, in the menus and the shortcuts list.
    for (layout, icon, id, label) in [
        (Layout::Split, "columns-2", "view.layout.split", tn!("Editor and PDF")),
        (Layout::EditorOnly, "code", "view.layout.editor", tn!("Editor Only")),
        (Layout::PreviewOnly, "file-text", "view.layout.preview", tn!("PDF Only")),
    ] {
        let shown = if app.view.pdf_window { Layout::EditorOnly } else { app.view.layout };
        if widgets::icon_toggle(ui, &t, icon, crate::i18n::tid(id, label), shown == layout).clicked() {
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
