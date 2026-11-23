//! TeXCraft's own controls, drawn from the theme tokens (docs/design.md). Each reports its role,
//! a name that starts with its visible label (or its tooltip, for icon-only controls), its state,
//! and draws a 2 px accent ring when it has keyboard focus (AGENTS.md rule 21).

use egui::{Align2, Color32, CornerRadius, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, vec2};

use crate::icons;
use crate::theme::{Tokens, blend};

/// Background for an interactive control in its current state.
fn state_fill(t: &Tokens, r: &Response, base: Color32) -> Color32 {
    if r.is_pointer_button_down_on() {
        blend(base, t.pressed)
    } else if r.hovered() || r.has_focus() {
        blend(base, t.hover)
    } else {
        base
    }
}

/// The focus ring, outside `rect`.
pub fn focus_ring(ui: &Ui, t: &Tokens, r: &Response, rect: Rect, radius: impl Into<CornerRadius>) {
    if r.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), radius, Stroke::new(2.0, t.accent), StrokeKind::Outside);
    }
}

/// A square button showing one icon; `label` is its tooltip and accessible name.
pub fn icon_button(ui: &mut Ui, t: &Tokens, icon: &str, label: &str, enabled: bool) -> Response {
    let side = t.toolbar_height - 2.0 * t.unit;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), sense);
    r.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let base = t.frame;
        let fill = if enabled { state_fill(t, &r, base) } else { base };
        if fill != base {
            ui.painter().rect_filled(rect, t.corner(), fill);
        }
        let color = if !enabled {
            t.text_muted.gamma_multiply(0.55)
        } else if r.hovered() {
            t.text
        } else {
            t.text_muted
        };
        icons::paint(ui, rect, icon, t.icon_size, color);
        focus_ring(ui, t, &r, rect, t.corner());
    }
    r.on_hover_text(label)
}

/// A square icon button that stays highlighted while `on`.
pub fn icon_toggle(ui: &mut Ui, t: &Tokens, icon: &str, label: &str, on: bool) -> Response {
    let side = t.toolbar_height - 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    r.widget_info(|| WidgetInfo::selected(Role::Button, true, on, label));
    if ui.is_rect_visible(rect) {
        let base = if on { blend(t.frame, t.accent_soft) } else { t.frame };
        let fill = state_fill(t, &r, base);
        if fill != t.frame {
            ui.painter().rect_filled(rect, t.corner(), fill);
        }
        let color = if on {
            t.accent
        } else if r.hovered() {
            t.text
        } else {
            t.text_muted
        };
        icons::paint(ui, rect, icon, t.icon_size, color);
        focus_ring(ui, t, &r, rect, t.corner());
    }
    r.on_hover_text(label)
}

/// A button showing `text` in the interface font, without a frame until hovered.
pub fn text_button(ui: &mut Ui, t: &Tokens, text: &str, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), t.body(), t.text);
    let size = vec2(galley.size().x + 4.0 * t.unit, t.toolbar_height - 2.0 * t.unit);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, r) = ui.allocate_exact_size(size, sense);
    r.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, text));
    if ui.is_rect_visible(rect) {
        let fill = state_fill(t, &r, t.frame);
        if fill != t.frame && enabled {
            ui.painter().rect_filled(rect, t.corner(), fill);
        }
        let color = if enabled { t.text } else { t.text_muted.gamma_multiply(0.55) };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, t.body(), color);
        focus_ring(ui, t, &r, rect, t.corner());
    }
    r
}

/// A rail section: an 18 px icon in a 36 px square, with an accent bar on its inner edge when
/// active.
pub fn rail_button(ui: &mut Ui, t: &Tokens, icon: &str, label: &str, active: bool) -> Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(t.rail_button), Sense::click());
    r.widget_info(|| WidgetInfo::selected(Role::Tab, true, active, label));
    if ui.is_rect_visible(rect) {
        let base = if active { blend(t.frame, t.accent_soft) } else { t.frame };
        let fill = state_fill(t, &r, base);
        if fill != t.frame {
            ui.painter().rect_filled(rect, t.corner(), fill);
        }
        if active {
            let bar = Rect::from_min_size(rect.left_top() + vec2(-(t.rail_width - t.rail_button) / 2.0, 6.0), vec2(3.0, rect.height() - 12.0));
            ui.painter().rect_filled(bar, CornerRadius::same(2), t.accent);
        }
        let color = if active {
            t.accent
        } else if r.hovered() {
            t.text
        } else {
            t.text_muted
        };
        icons::paint(ui, rect, icon, t.rail_icon_size, color);
        focus_ring(ui, t, &r, rect, t.corner());
    }
    r.on_hover_text(label)
}

/// A segmented switch. `options` are (label, disabled reason); returns the index clicked.
pub fn segmented(ui: &mut Ui, t: &Tokens, id: &str, options: &[(&str, Option<&str>)], selected: usize) -> Option<usize> {
    let height = t.toolbar_height - 3.0 * t.unit;
    let widths: Vec<f32> =
        options.iter().map(|(label, _)| ui.painter().layout_no_wrap((*label).to_owned(), t.small(), t.text).size().x + 5.0 * t.unit).collect();
    let total = vec2(widths.iter().sum::<f32>() + 4.0, height);
    let (outer, _) = ui.allocate_exact_size(total, Sense::hover());
    ui.painter().rect_filled(outer, t.corner(), blend(t.frame, t.hover));
    let mut x = outer.left() + 2.0;
    let mut clicked = None;
    for (i, ((label, disabled), w)) in options.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(egui::pos2(x, outer.top() + 2.0), vec2(*w, height - 4.0));
        x += w;
        let enabled = disabled.is_none();
        let r = ui.interact(rect, ui.scope_id().with((id, i)), if enabled { Sense::click() } else { Sense::hover() });
        r.widget_info(|| WidgetInfo::selected(Role::RadioButton, enabled, i == selected, *label));
        if i == selected {
            ui.painter().rect_filled(rect, CornerRadius::same(t.radius.saturating_sub(1)), t.surface);
            ui.painter().rect_stroke(rect, CornerRadius::same(t.radius.saturating_sub(1)), t.hairline(), StrokeKind::Inside);
        }
        let color = if !enabled {
            t.text_muted.gamma_multiply(0.6)
        } else if i == selected {
            t.text
        } else {
            t.text_muted
        };
        let font = if i == selected { egui::FontId::new(t.small_size, crate::fonts::semibold()) } else { t.small() };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, *label, font, color);
        focus_ring(ui, t, &r, rect, t.corner());
        let r = match disabled {
            Some(reason) => r.on_hover_text(*reason),
            None => r,
        };
        if r.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// The round primary button with an icon, a label, and a separate menu segment. Returns the
/// main part's and the menu part's responses.
pub fn primary_split(ui: &mut Ui, t: &Tokens, icon: &str, label: &str, busy: bool) -> (Response, Response) {
    let height = t.toolbar_height - 2.0 * t.unit;
    let text = ui.painter().layout_no_wrap(label.to_owned(), t.strong(), t.on_accent);
    let main_w = 3.0 * t.unit + t.icon_size + 2.0 * t.unit + text.size().x + 3.0 * t.unit;
    let menu_w = 7.0 * t.unit;
    let (outer, _) = ui.allocate_exact_size(vec2(main_w + menu_w, height), Sense::hover());
    let main_rect = Rect::from_min_size(outer.min, vec2(main_w, height));
    let menu_rect = Rect::from_min_size(outer.min + vec2(main_w, 0.0), vec2(menu_w, height));
    let main = ui.interact(main_rect, ui.scope_id().with(("primary", label)), if busy { Sense::hover() } else { Sense::click() });
    let menu = ui.interact(menu_rect, ui.scope_id().with(("primary-menu", label)), Sense::click());
    main.widget_info(|| WidgetInfo::labeled(Role::Button, !busy, label));
    menu.widget_info(|| WidgetInfo::labeled(Role::Button, true, format!("{label} {}", tl!("options"))));
    let round = (height / 2.0) as u8;
    let fill = |r: &Response| {
        if r.is_pointer_button_down_on() {
            blend(t.accent, Color32::from_black_alpha(40))
        } else if r.hovered() {
            blend(t.accent, Color32::from_black_alpha(20))
        } else {
            t.accent
        }
    };
    let p = ui.painter();
    p.rect_filled(main_rect, CornerRadius { nw: round, sw: round, ne: 0, se: 0 }, fill(&main));
    p.rect_filled(menu_rect, CornerRadius { nw: 0, sw: 0, ne: round, se: round }, fill(&menu));
    p.line_segment(
        [menu_rect.left_top() + vec2(0.0, 7.0), menu_rect.left_bottom() - vec2(0.0, 7.0)],
        Stroke::new(1.0, t.on_accent.gamma_multiply(0.5)),
    );
    let icon_rect =
        Rect::from_center_size(egui::pos2(main_rect.left() + 3.0 * t.unit + t.icon_size / 2.0, main_rect.center().y), Vec2::splat(t.icon_size));
    if busy {
        spinner(ui, icon_rect.center(), t.icon_size / 2.0 - 1.0, t.on_accent);
    } else {
        icons::paint(ui, icon_rect, icon, t.icon_size, t.on_accent);
    }
    p.galley(egui::pos2(icon_rect.right() + 2.0 * t.unit, main_rect.center().y - text.size().y / 2.0), text, t.on_accent);
    icons::paint(ui, menu_rect, "chevron-down", t.icon_size - 2.0, t.on_accent);
    focus_ring(ui, t, &main, main_rect, CornerRadius::same(round));
    focus_ring(ui, t, &menu, menu_rect, CornerRadius::same(round));
    (main, menu)
}

/// A turning arc, for work in progress. Asks for the next frame while shown.
pub fn spinner(ui: &Ui, center: egui::Pos2, radius: f32, color: Color32) {
    let time = ui.input(|i| i.time) as f32;
    let reduced = ui.ctx().global_style().animation_time <= 0.0;
    let start = if reduced { 0.0 } else { time * 5.0 };
    let points: Vec<egui::Pos2> = (0..=24)
        .map(|i| {
            let a = start + i as f32 / 24.0 * std::f32::consts::TAU * 0.75;
            center + radius * vec2(a.cos(), a.sin())
        })
        .collect();
    ui.painter().add(egui::Shape::line(points, Stroke::new(2.0, color)));
    if !reduced {
        ui.ctx().request_repaint();
    }
}

/// A small round count, drawn only when `count` is not zero.
pub fn badge(ui: &Ui, t: &Tokens, at: egui::Pos2, count: usize, color: Color32) {
    if count == 0 {
        return;
    }
    let text = if count > 99 { "99+".to_owned() } else { count.to_string() };
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::new(10.5, crate::fonts::semibold()), t.surface);
    let size = vec2((galley.size().x + 8.0).max(16.0), 16.0);
    let rect = Rect::from_center_size(at, size);
    ui.painter().rect_filled(rect, CornerRadius::same(8), color);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.surface);
}

/// A small uppercase heading for a panel section, with room on its right for actions.
pub fn section_title(ui: &mut Ui, t: &Tokens, title: &str) {
    let font = egui::FontId::new(t.small_size - 0.5, crate::fonts::semibold());
    ui.label(egui::RichText::new(title.to_uppercase()).font(font).color(t.text_muted).extra_letter_spacing(0.6));
}

/// One row of a tree or list: indent, an optional disclosure chevron, an icon, the label, and an
/// optional trailing mark. Returns the row's response.
pub struct Row<'a> {
    pub depth: usize,
    pub icon: &'a str,
    pub label: &'a str,
    pub selected: bool,
    /// `Some(open)` for a folder row.
    pub disclosure: Option<bool>,
    /// A muted word after the label (such as "main").
    pub tag: Option<&'a str>,
    pub icon_color: Option<Color32>,
}

pub fn row(ui: &mut Ui, t: &Tokens, row: &Row<'_>) -> Response {
    let width = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(vec2(width, t.row_height), Sense::click());
    r.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, row.selected, row.label));
    if !ui.is_rect_visible(rect) {
        return r;
    }
    let base = if row.selected { blend(t.frame, t.accent_soft) } else { t.frame };
    let fill = state_fill(t, &r, base);
    if fill != t.frame {
        ui.painter().rect_filled(rect.shrink2(vec2(t.unit, 0.0)), t.corner(), fill);
    }
    let mut x = rect.left() + 2.0 * t.unit + row.depth as f32 * 4.0 * t.unit;
    let chevron = Rect::from_min_size(egui::pos2(x, rect.top()), vec2(t.icon_size, t.row_height));
    if let Some(open) = row.disclosure {
        icons::paint(ui, chevron, if open { "chevron-down" } else { "chevron-right" }, t.icon_size - 2.0, t.text_muted);
    }
    x += t.icon_size + t.unit;
    let icon_rect = Rect::from_min_size(egui::pos2(x, rect.top()), vec2(t.icon_size, t.row_height));
    let icon_color = row.icon_color.unwrap_or(if row.selected { t.accent } else { t.text_muted });
    icons::paint(ui, icon_rect, row.icon, t.icon_size, icon_color);
    x += t.icon_size + 2.0 * t.unit;
    let font = if row.selected { t.strong() } else { t.body() };
    let galley = ui.painter().layout_no_wrap(row.label.to_owned(), font, t.text);
    let text_pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
    let label_right = text_pos.x + galley.size().x;
    ui.painter().galley(text_pos, galley, t.text);
    if let Some(tag) = row.tag {
        ui.painter().text(egui::pos2(label_right + 2.0 * t.unit, rect.center().y), Align2::LEFT_CENTER, tag, t.small(), t.text_muted);
    }
    focus_ring(ui, t, &r, rect.shrink2(vec2(t.unit, 0.0)), t.corner());
    r
}

/// A 1 px separator across the available width.
pub fn hairline(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.separator);
}

/// The frame of every dialog: the system's popup look with room around its content, so titles,
/// tabs and buttons sit on one inner edge.
pub fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(20))
}
