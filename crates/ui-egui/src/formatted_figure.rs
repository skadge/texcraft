//! Figures in the formatted view (docs/ROADMAP.md, F4): a card with the image at its width, its
//! caption edited in place under it, and the figure's other parts at hand.
//!
//! - **Width:** the image is drawn at its `width=…\linewidth`; the handle at its lower right
//!   corner resizes it, snapping to a quarter, a half, three quarters and the whole line (Shift
//!   for any width), and `-` and `+` do the same from the keyboard while the image has it. The
//!   width goes in with `figure.edit` when the drag ends.
//! - **Caption:** a line of the view's text like a paragraph; a figure without one gets Add
//!   Caption, which adds an empty `\caption{}` with the caret in it.
//! - **Context menu** (right-click, Shift+F10 or the Menu key on the image): the width, where
//!   LaTeX may place the figure, its label, and the figure dialog.

use std::ops::Range;

use egui::{Align, Color32, Rect, Sense, StrokeKind, Vec2, pos2, vec2};
use serde_json::{Value, json};
use texcraft_latex::view::{Inline, InlineKind, Style};

use super::{Cx, Para, activate, paragraph, picture};
use crate::theme::blend;

/// The widths the handle snaps to, as shares of the line.
const SNAPS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// `f` snapped to the nearest of [`SNAPS`], or (`free`) to a hundredth, from 5 % to the line.
pub(crate) fn snap(f: f32, free: bool) -> f32 {
    if free {
        return (f.clamp(0.05, 1.0) * 100.0).round() / 100.0;
    }
    SNAPS.iter().copied().min_by(|a, b| (a - f).abs().total_cmp(&(b - f).abs())).unwrap_or(1.0)
}

/// The next width up (`grow`) or down from `f` among [`SNAPS`].
fn step(f: f32, grow: bool) -> f32 {
    if grow {
        SNAPS.iter().copied().find(|s| *s > f + 0.001).unwrap_or(1.0)
    } else {
        SNAPS.iter().copied().rev().find(|s| *s < f - 0.001).unwrap_or(SNAPS[0])
    }
}

/// The placement letters of the figure whose source is `src` (`htbp`), empty for none.
fn placement(src: &str) -> String {
    let rest = src.strip_prefix("\\begin{figure*}").or_else(|| src.strip_prefix("\\begin{figure}")).unwrap_or_default();
    rest.trim_start().strip_prefix('[').and_then(|r| r.split_once(']')).map(|(p, _)| p.trim().to_owned()).unwrap_or_default()
}

/// "50 % of the line".
fn share(f: f32) -> String {
    tl!("{n} % of the line").replace("{n}", &format!("{}", (f * 100.0).round()))
}

pub(super) fn card(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    range: &Range<usize>,
    image: Option<&texcraft_latex::view::Image>,
    caption: &[Inline],
    label: Option<&Inline>,
) {
    let t = cx.t;
    let width = ui.available_width();
    let pad = 4.0 * t.unit;
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    // The card's frame goes under what is drawn in it.
    let frame_shape = ui.painter().add(egui::Shape::Noop);
    ui.add_space(pad);
    let inner = width - 2.0 * pad;
    let max_h = t.reading_size * 20.0;
    let (shown, name) = match image {
        Some(img) => (picture(ui, cx, &img.path), img.path.clone()),
        None => (Err(tl!("This figure has no image.").to_owned()), String::new()),
    };
    let fraction = image.and_then(|i| texcraft_latex::figure::width_fraction(&i.options));
    let drag_id = egui::Id::unique(("figure-width", range.start));
    let dragging: Option<f32> = ui.ctx().data(|d| d.get_temp(drag_id));
    let image_r = match shown {
        Ok((texture, natural)) => {
            // At its width of the line while it has one, else as large as fits.
            let scale = match dragging.or(fraction) {
                Some(f) => (inner * f / natural.x).min(max_h / natural.y),
                None => (inner / natural.x).min(max_h / natural.y).min(1.5),
            };
            let size = natural * scale;
            let (row, r) = ui.allocate_exact_size(vec2(width, size.y), Sense::click());
            let rect = Rect::from_center_size(row.center(), size);
            ui.painter().image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let label = match fraction {
                Some(f) => format!("{}, {}", tl!("Image {name}").replace("{name}", &name), share(f)),
                None => tl!("Image {name}").replace("{name}", &name),
            };
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &label));
            handle(ui, cx, (rect, row.center().x, inner), drag_id, (fraction, range.start));
            if r.has_focus() {
                // - and + step through the widths the handle snaps to.
                let (grow, shrink) =
                    ui.input(|i| (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals), i.key_pressed(egui::Key::Minus)));
                if grow || shrink {
                    let f = step(fraction.unwrap_or(1.0), grow);
                    cx.figure_action = Some(json!({"at": range.start, "width": f}));
                }
            }
            r.on_hover_text(tl!(
                "Drag the corner to resize the image (Shift for any width), or press - and +. Right-click for its placement and label."
            ))
        }
        Err(why) => {
            let (row, r) = ui.allocate_exact_size(vec2(width, t.reading_size * 7.0), Sense::click());
            let rect = row.shrink2(vec2(pad, 0.0));
            ui.painter().rect_filled(rect, t.corner(), blend(t.surface, t.pressed));
            crate::icons::paint(ui, Rect::from_center_size(rect.center() - vec2(0.0, 12.0), Vec2::splat(28.0)), "image", 24.0, t.text_muted);
            let galley = ui.painter().layout(why.clone(), t.small(), t.text_muted, rect.width() - 2.0 * pad);
            let at = pos2(rect.center().x - galley.size().x / 2.0, rect.center().y + 10.0);
            ui.painter().galley(at, galley, t.text_muted);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &why));
            r
        }
    };
    let image_start = image.map_or(range.start, |i| i.range.start);
    let image_rect = image_r.rect;
    activate(ui, cx, &image_r, image_rect, image_start, None);
    menu(ui, cx, &image_r, range, fraction);
    ui.add_space(3.0 * t.unit);
    caption_line(ui, cx, (range, inner, pad), caption, label);
    // The figure dialog: image, width, placement, caption and label.
    let has_caption = !caption.is_empty() || caption_slot(cx, range).is_some();
    let (edit, add) = ui
        .with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(pad);
            let edit = crate::widgets::text_button(ui, &t, tl!("Edit Figure…"), true);
            let add = (!has_caption).then(|| crate::widgets::text_button(ui, &t, tl!("Add Caption"), true));
            (edit, add)
        })
        .inner;
    if edit.clicked() {
        cx.edit_figure = Some(range.start + 1);
    }
    if add.is_some_and(|a| a.clicked()) {
        cx.figure_action = Some(json!({"at": range.start, "caption": "", "caret_in_caption": true}));
    }
    ui.add_space(pad);
    let card = Rect::from_min_max(pos2(left, top), pos2(left + width, ui.cursor().top()));
    ui.painter().set(
        frame_shape,
        egui::Shape::Rect(egui::epaint::RectShape::new(card, t.corner_large(), blend(t.surface, t.current_line), t.hairline(), StrokeKind::Inside)),
    );
}

/// Inside the braces of the `\caption{}` of the figure at `range` (where the caret stands in one
/// that shows nothing: empty, or an empty style just made in it).
pub(super) fn caption_slot(cx: &Cx<'_>, range: &Range<usize>) -> Option<usize> {
    let src = cx.text.get(range.clone())?;
    let at = src.find("\\caption{")? + "\\caption{".len();
    Some(range.start + at)
}

/// "Figure 2." as the document numbers it (the last build's number when it has one), or "Figure."
/// when it has none.
pub(super) fn float_name(cx: &Cx<'_>, range: &Range<usize>, plain: &str, numbered: &str) -> String {
    match cx.refs.numbered.numbers.one(cx.path.as_str(), range.start) {
        Some(n) => numbered.replace("{n}", n),
        None => plain.to_owned(),
    }
}

/// "Figure." and the caption, edited as a line of the view's text, with the label after it.
fn caption_line(ui: &mut egui::Ui, cx: &mut Cx<'_>, (range, inner, pad): (&Range<usize>, f32, f32), caption: &[Inline], label: Option<&Inline>) {
    let t = cx.t;
    let mut inlines = vec![Inline {
        kind: InlineKind::Text {
            text: format!("{} ", float_name(cx, range, tl!("Figure."), tl!("Figure {n}."))),
            style: Style { bold: true, sans: true, ..Style::default() },
        },
        range: range.start..range.start,
    }];
    inlines.extend(caption.iter().cloned());
    if let Some(label) = label {
        inlines.push(Inline {
            kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() },
            range: label.range.start..label.range.start,
        });
        inlines.push(label.clone());
    }
    // An empty caption is a place to type one.
    let empty_at = if caption.is_empty() { caption_slot(cx, range) } else { None };
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(inner);
            let para = Para { size: t.reading_size * 0.9, bold: false, color: t.text, role: egui::Role::FigureCaption, empty_at, cell: None };
            let _ = paragraph(ui, cx, &inlines, range, &para);
            if caption.is_empty() && empty_at.is_none() {
                super::inert(cx);
            }
        });
    });
}

/// The handle at the image's lower right corner: dragged, it resizes the image (snapping unless
/// Shift is held); at the drag's end the width goes in.
fn handle(ui: &mut egui::Ui, cx: &mut Cx<'_>, (rect, center_x, inner): (Rect, f32, f32), drag_id: egui::Id, (fraction, start): (Option<f32>, usize)) {
    let t = cx.t;
    let side = 2.5 * t.unit + 4.0;
    // Inside the image's corner: the caption under it takes the pointer there.
    let knob = Rect::from_min_size(rect.right_bottom() - Vec2::splat(side + t.unit), Vec2::splat(side));
    let r = ui.interact(knob.expand(2.0), drag_id.with("handle"), Sense::drag());
    let label = tl!("Image width");
    let value = fraction.unwrap_or(1.0);
    r.widget_info(|| egui::WidgetInfo::slider(true, f64::from(value * 100.0), label));
    if r.hovered() || r.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
    }
    let fill = if r.hovered() || r.dragged() { t.accent } else { blend(t.surface, t.accent) };
    ui.painter().rect(knob, t.corner(), fill, egui::Stroke::new(1.5, t.surface), StrokeKind::Middle);
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
    {
        // The image is centred: its half width is the pointer's distance from the middle.
        let free = ui.input(|i| i.modifiers.shift);
        let f = snap(2.0 * (p.x - center_x) / inner.max(1.0), free);
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, f));
        let at = pos2(knob.left(), knob.bottom() + t.unit);
        let tip = ui.painter().layout_no_wrap(share(f), t.small(), t.text);
        let bg = Rect::from_min_size(at, tip.size() + vec2(2.0 * t.unit, t.unit));
        ui.painter().rect_filled(bg, t.corner(), t.frame);
        ui.painter().galley(at + vec2(t.unit, t.unit * 0.5), tip, t.text);
    }
    if r.drag_stopped() {
        let dragged: Option<f32> = ui.ctx().data_mut(|d| d.remove_temp(drag_id));
        if let Some(f) = dragged.filter(|f| Some(*f) != fraction) {
            cx.figure_action = Some(json!({"at": start, "width": f}));
        }
    }
}

/// The image's context menu: the width, the placement, the label and the figure dialog.
fn menu(ui: &egui::Ui, cx: &mut Cx<'_>, r: &egui::Response, range: &Range<usize>, fraction: Option<f32>) {
    let t = cx.t;
    let by_key = r.id.with("menu-by-key");
    let mut popup = egui::Popup::context_menu(r);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(r));
    let mut opened = false;
    if r.has_focus()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10) || i.consume_key(egui::Modifiers::NONE, egui::Key::Menu))
    {
        ui.data_mut(|d| d.insert_temp(by_key, true));
        popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
        opened = !open;
    }
    crate::widgets::keyboard_popup(ui, r, opened, open);
    if ui.data(|d| d.get_temp::<bool>(by_key)).unwrap_or(false) {
        popup = popup.at_position(r.rect.center());
    }
    let place = placement(cx.text.get(range.clone()).unwrap_or_default());
    let mut chosen: Option<Value> = None;
    let mut asked_label = false;
    let mut dialog = false;
    let shown = popup.show(|ui| {
        ui.set_min_width(240.0);
        let item = |ui: &mut egui::Ui, label: &str, on: bool| {
            let r = ui.add(egui::Button::selectable(on, label).frame_when_inactive(false));
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::MenuItem, true, on, label));
            r.clicked()
        };
        ui.label(egui::RichText::new(tl!("Width")).font(t.small()).color(t.text_muted));
        for f in SNAPS {
            let name = if f >= 1.0 { tl!("The whole line").to_owned() } else { share(f) };
            if item(ui, &name, fraction.is_some_and(|w| (w - f).abs() < 0.001)) {
                chosen = Some(json!({"width": f}));
            }
        }
        ui.separator();
        ui.label(egui::RichText::new(tl!("Where LaTeX may place it")).font(t.small()).color(t.text_muted));
        for (letters, name) in [
            ("htbp", tl!("Where it fits best")),
            ("h", tl!("Here, if it fits")),
            ("t", tl!("At the top of a page")),
            ("b", tl!("At the bottom of a page")),
            ("p", tl!("On a page of floats")),
        ] {
            if item(ui, name, place == letters) {
                chosen = Some(json!({"placement": letters}));
            }
        }
        ui.separator();
        if item(ui, tl!("Label…"), false) {
            asked_label = true;
        }
        if item(ui, tl!("Edit Figure…"), false) {
            dialog = true;
        }
        if chosen.is_some() || asked_label || dialog {
            ui.close();
        }
    });
    if shown.is_none() {
        ui.data_mut(|d| d.remove::<bool>(by_key));
    }
    if let Some(mut c) = chosen {
        c["at"] = json!(range.start);
        cx.figure_action = Some(c);
    }
    if asked_label {
        cx.figure_label = Some(range.start);
    }
    if dialog {
        cx.edit_figure = Some(range.start + 1);
    }
}
