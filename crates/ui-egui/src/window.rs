//! The window's own frame, for a desktop window the system draws no frame around (docs/design.md,
//! Regions): the window's controls at the right of the top bar, the bar's empty space to move the
//! window (a double-click maximizes or restores it), and the window's edges to resize it.
//!
//! The browser has no window to control: there the shell leaves the frame off.

use egui::{Align, Layout, ResizeDirection, Sense, ViewportCommand, vec2};

use crate::App;
use crate::theme::Tokens;

/// How far from the window's edge the pointer resizes it, in points.
const EDGE: f32 = 5.0;
/// From a corner, how far along each side the pointer resizes diagonally.
const CORNER: f32 = 14.0;

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Runs a window command (`window.minimize`, `window.maximize`, `window.close`); false for others.
pub(crate) fn run(app: &mut App, id: &str) -> bool {
    let ctx = app.ctx.clone();
    match id {
        "window.minimize" => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
        "window.maximize" => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized(&ctx))),
        // Closing asks about unsaved changes first, as the system's close button does.
        "window.close" => ctx.send_viewport_cmd(ViewportCommand::Close),
        _ => return false,
    }
    true
}

/// The bar's empty space: dragging it moves the window, a double-click maximizes or restores it.
/// Call before the bar's own controls, which then take the pointer over this.
pub(crate) fn bar_space(app: &mut App, ui: &mut egui::Ui, bar: egui::Rect) {
    if !app.own_frame() {
        return;
    }
    let response = ui.interact(bar, ui.scope_id().with("window-move"), Sense::click_and_drag());
    if response.double_clicked() {
        run(app, "window.maximize");
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// The minimize, maximize (or restore) and close buttons, right to left from the bar's end.
pub(crate) fn controls(app: &mut App, ui: &mut egui::Ui) {
    if !app.own_frame() {
        return;
    }
    let t = app.tokens;
    let is_max = maximized(ui.ctx());
    let restore = if is_max { tl!("Restore") } else { tl!("Maximize") };
    let mut chosen = None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = t.unit;
        for (id, icon, label, danger) in [
            ("window.close", "x", tl!("Close"), true),
            ("window.maximize", if is_max { "copy" } else { "square" }, restore, false),
            ("window.minimize", "minus", tl!("Minimize"), false),
        ] {
            if button(ui, &t, icon, label, danger).clicked() {
                chosen = Some(id);
            }
        }
    });
    if let Some(id) = chosen {
        run(app, id);
    }
}

/// A round window button: the icon in muted text, a soft fill under the pointer (the close button's
/// in the error colour, its icon then white).
fn button(ui: &mut egui::Ui, t: &Tokens, icon: &str, label: &str, danger: bool) -> egui::Response {
    let side = t.toolbar_height - 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let active = r.hovered() || r.has_focus() || r.is_pointer_button_down_on();
        let (fill, color) = match (active, danger) {
            (true, true) => (Some(t.error), egui::Color32::WHITE),
            (true, false) => (Some(crate::theme::blend(t.frame, if r.is_pointer_button_down_on() { t.pressed } else { t.hover })), t.text),
            (false, _) => (None, t.text_muted),
        };
        if let Some(fill) = fill {
            ui.painter().circle_filled(rect.center(), side / 2.0 - 2.0, fill);
        }
        crate::icons::paint(ui, rect, icon, t.icon_size - 2.0, color);
        crate::widgets::focus_ring(ui, t, &r, rect, side / 2.0);
    }
    r.on_hover_text(label)
}

/// Resizing from the window's edges and corners, when it is not maximized: the pointer shows the
/// direction, and a press there hands the resize to the system.
pub(crate) fn edges(app: &App, ctx: &egui::Context) {
    if !app.own_frame() || maximized(ctx) || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let screen = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let Some(direction) = direction(screen, pos) else {
        return;
    };
    // Not while a drag that started elsewhere (a selection, a splitter) passes the edge.
    if ctx.input(|i| i.pointer.any_down() && !i.pointer.any_pressed()) {
        return;
    }
    ctx.set_cursor_icon(match direction {
        ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}

/// The resize direction at `pos` near the edges of `screen`, if any.
pub(crate) fn direction(screen: egui::Rect, pos: egui::Pos2) -> Option<ResizeDirection> {
    if !screen.contains(pos) {
        return None;
    }
    let near = |d: f32, within: f32| d >= 0.0 && d < within;
    let (left, right, top, bottom) = (pos.x - screen.left(), screen.right() - pos.x, pos.y - screen.top(), screen.bottom() - pos.y);
    let on_side = |a: f32, b: f32| near(a, EDGE) || near(b, EDGE);
    if !on_side(left, right) && !on_side(top, bottom) {
        return None;
    }
    let west = near(left, if near(top, CORNER) || near(bottom, CORNER) { CORNER } else { EDGE });
    let east = near(right, if near(top, CORNER) || near(bottom, CORNER) { CORNER } else { EDGE });
    let north = near(top, if near(left, CORNER) || near(right, CORNER) { CORNER } else { EDGE });
    let south = near(bottom, if near(left, CORNER) || near(right, CORNER) { CORNER } else { EDGE });
    Some(match (north, south, west, east) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return None,
    })
}
