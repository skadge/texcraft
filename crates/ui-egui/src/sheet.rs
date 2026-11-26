//! Where a modal dialog is drawn. In a wide window: a box in the middle, no wider than the window
//! less a margin. Below the theme's `narrow_width` (a phone): a sheet over the whole window, with
//! a bar at the top holding Close (or Back) and the dialog's title, its content scrolling under
//! it, and its actions in a bar along the bottom, always on the screen (docs/ROADMAP.md, A12).
//!
//! The dialogs themselves do not change between the two: their [`crate::dialogs::title`] goes to
//! the sheet's top bar and their [`actions`] row to its bottom bar. Each is drawn where it falls
//! in the dialog, so the keyboard meets them in the order they read; the top bar shows the title
//! the dialog gave on the pass before (a new one has egui draw the frame again).

use egui::{Align, Id, Layout, Rect, RichText, Ui, UiBuilder, vec2};

use crate::theme::Tokens;
use crate::widgets;

/// The margin kept between a dialog and the window's edges, in a wide window.
const MARGIN: f32 = 24.0;

/// What the sheet open this pass lends its dialog: where its actions go.
#[derive(Clone, Copy)]
struct Open {
    id: Id,
    footer: Rect,
}

fn open_key() -> Id {
    Id::unique("texcraft-sheet-open")
}

/// Whether a dialog shown now is drawn as a sheet over the whole window.
pub(crate) fn narrow(ctx: &egui::Context, t: &Tokens) -> bool {
    ctx.content_rect().width() < t.narrow_width
}

/// What a sheet's top bar offers besides its title.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leave {
    /// Close, which closes the dialog.
    Close,
    /// Back, its whole name saying where it goes ("Back to Settings": a sentence each, so every
    /// language can say it its own way), which the dialog handles; a wide window shows nothing
    /// for it.
    Back(&'static str),
}

/// How the user asked to leave the dialog this pass.
pub(crate) struct Shown {
    /// Escape, a click beside the dialog, or the sheet's Close.
    pub close: bool,
    /// The sheet's Back.
    pub back: bool,
}

/// Shows a modal dialog `width` wide at most, as a box or (narrow) a sheet.
pub(crate) fn show(ctx: &egui::Context, t: &Tokens, id: &str, width: f32, leave: Leave, add: impl FnOnce(&mut Ui)) -> Shown {
    let id = Id::unique(id);
    if !narrow(ctx, t) {
        let screen = ctx.content_rect();
        let frame = widgets::dialog_frame(ctx);
        let room = (screen.width() - 2.0 * MARGIN - frame.inner_margin.sum().x).max(120.0);
        let modal = egui::Modal::new(id).frame(frame).show(ctx, |ui| {
            ui.set_width(width.min(room));
            add(ui);
        });
        let close = modal.should_close();
        return Shown { close, back: false };
    }
    let screen = ctx.content_rect();
    let area = egui::Modal::default_area(id).anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO);
    let pad = 4.0 * t.unit;
    let frame = egui::Frame::new().fill(t.surface).inner_margin(egui::Margin::symmetric(pad as i8, 0));
    let mut pressed = (false, false);
    let modal = egui::Modal::new(id).area(area).frame(frame).show(ctx, |ui| {
        let inner = screen.shrink2(vec2(pad, 0.0));
        ui.set_min_size(inner.size());
        ui.set_max_size(inner.size());
        pressed = top_bar(ui, t, id, leave);
        let footer_height: f32 = ui.data(|d| d.get_temp(id.with("footer"))).unwrap_or(t.toolbar_height + 2.0 * t.unit);
        let footer = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - footer_height), inner.max);
        if footer_height > 0.0 {
            ui.painter().hline(footer.expand2(vec2(pad, 0.0)).x_range(), footer.top(), egui::Stroke::new(1.0, t.separator));
        }
        let body = (footer.top() - ui.cursor().top() - t.unit).max(t.row_height);
        ui.data_mut(|d| d.insert_temp(open_key(), Open { id, footer }));
        let mut area = egui::ScrollArea::vertical().id_salt(id.with("body")).max_height(body).auto_shrink([false, false]);
        if ui.data_mut(|d| d.remove_temp::<bool>(id.with("to-top"))).unwrap_or(false) {
            area = area.vertical_scroll_offset(0.0);
        }
        area.show(ui, |ui| {
            ui.add_space(t.unit * 3.0);
            add(ui);
            ui.add_space(t.unit * 3.0);
        });
        ui.data_mut(|d| d.remove::<Open>(open_key()));
        // A dialog with no actions this pass leaves no bar for them.
        let used = ui.data_mut(|d| d.remove_temp::<bool>(id.with("footer-used"))).unwrap_or(false);
        if !used && footer_height > 0.0 {
            ui.data_mut(|d| d.insert_temp(id.with("footer"), 0.0_f32));
            ui.ctx().request_discard("a sheet has no actions");
        }
    });
    let close = modal.should_close() || pressed.0;
    Shown { close, back: pressed.1 }
}

/// The sheet's top bar: Close or Back, and the title [`title`] gave last pass. Returns whether
/// Close and Back were pressed.
fn top_bar(ui: &mut Ui, t: &Tokens, id: Id, leave: Leave) -> (bool, bool) {
    let title: String = ui.data(|d| d.get_temp(id.with("title"))).unwrap_or_default();
    let mut pressed = (false, false);
    let focus = ui.data_mut(|d| d.remove_temp::<bool>(id.with("focus-leave"))).unwrap_or(false);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height + t.unit), Layout::left_to_right(Align::Center), |ui| {
        let r = match leave {
            Leave::Close => widgets::icon_button(ui, t, "x", tl!("Close"), true),
            Leave::Back(name) => widgets::icon_button(ui, t, "arrow-left", name, true),
        };
        if focus {
            r.request_focus();
        }
        match leave {
            Leave::Close => pressed.0 = r.clicked(),
            Leave::Back(_) => pressed.1 = r.clicked(),
        }
        ui.add(egui::Label::new(RichText::new(title).font(t.title())).truncate());
    });
    let y = ui.cursor().top();
    let x = ui.max_rect().expand2(vec2(4.0 * t.unit, 0.0)).x_range();
    ui.painter().hline(x, y, egui::Stroke::new(1.0, t.separator));
    pressed
}

/// A dialog's title: in a wide window, a heading at the top of its box; on a sheet, the top bar's.
pub(crate) fn title(ui: &mut Ui, t: &Tokens, text: &str) {
    let Some(open) = ui.data(|d| d.get_temp::<Open>(open_key())) else {
        ui.label(RichText::new(text).font(t.title()));
        ui.add_space(t.unit * 2.0);
        return;
    };
    let key = open.id.with("title");
    let last: Option<String> = ui.data(|d| d.get_temp(key));
    if last.as_deref() != Some(text) {
        ui.data_mut(|d| d.insert_temp(key, text.to_owned()));
        ui.ctx().request_discard("a sheet's title changed");
    }
}

/// A dialog's actions, from the right: in a wide window a row where they fall, on a sheet its
/// bottom bar (wrapping onto a second row when they need more room than the screen's width).
pub(crate) fn actions(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let Some(open) = ui.data(|d| d.get_temp::<Open>(open_key())) else {
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), Layout::right_to_left(Align::Center), add);
        return;
    };
    let gap = 8.0;
    let room = open.footer.shrink2(vec2(0.0, gap));
    let mut bar = ui.new_child(UiBuilder::new().max_rect(room).layout(Layout::right_to_left(Align::Center).with_main_wrap(true)));
    bar.set_clip_rect(open.footer);
    add(&mut bar);
    let height = bar.min_rect().height() + 2.0 * gap;
    let key = open.id.with("footer");
    ui.data_mut(|d| d.insert_temp(open.id.with("footer-used"), true));
    let last: Option<f32> = ui.data(|d| d.get_temp(key));
    if last.is_none_or(|h| (h - height).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(key, height));
        ui.ctx().request_discard("a sheet's actions were measured");
    }
}

/// Gives the keyboard to the Close or Back of sheet `id` next pass (a page of it opened).
pub(crate) fn focus_leave(ctx: &egui::Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::unique(id).with("focus-leave"), true));
}

/// The sheet `id`'s content starts at its top when it is next drawn (a dialog that shows other
/// content in it: Settings' sections).
pub(crate) fn to_top(ctx: &egui::Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::unique(id).with("to-top"), true));
}

/// Whether `ui` is a sheet's content (a phone's dialog).
pub(crate) fn in_sheet(ui: &Ui) -> bool {
    ui.data(|d| d.get_temp::<Open>(open_key())).is_some()
}
