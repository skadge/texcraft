//! Touch screens (#26): a finger's drag scrolls the view it is in, as on phones and tablets, and
//! does not select; a tap places the caret; a long press selects there (the word, in text), and
//! moving the finger, still down, then extends the selection. A mouse's drag selects as before.
//!
//! The views whose drag selects (the source, the formatted view, the PDF, the file list's rows)
//! ask [`scroll`] with their response, inside their scroll area, before they look at the drag.

use egui::{Context, Id, Response, Ui};

/// The pointer's gesture: whether its last press was a finger's, and whether a long press made
/// that finger select.
#[derive(Clone, Copy, Default)]
struct Gesture {
    /// The frame of the press.
    frame: u64,
    touch: bool,
    selecting: bool,
}

fn key() -> Id {
    Id::unique("texcraft-touch-gesture")
}

/// The current gesture, updated when a press starts one.
fn gesture(ctx: &Context) -> Gesture {
    let frame = ctx.cumulative_frame_nr();
    let (pressed, touches) = ctx.input(|i| (i.pointer.any_pressed(), i.any_touches()));
    ctx.data_mut(|d| {
        let g = d.get_temp_mut_or_default::<Gesture>(key());
        if pressed && g.frame != frame {
            *g = Gesture { frame, touch: touches, selecting: false };
        }
        *g
    })
}

/// Whether the pointer's press (the one down now, or the last one) was a finger's.
pub(crate) fn is_touch(ctx: &Context) -> bool {
    gesture(ctx).touch
}

/// For a view whose drag selects, called inside its scroll area with its response `r`: when a
/// finger presses it, a drag scrolls the area with the finger, and `true` says the view leaves
/// the press and the drag alone (a tap is still `r.clicked()`). A long press on it makes the
/// finger select instead (`false` from then on, and [`selecting`] while it stays down).
pub(crate) fn scroll(ui: &Ui, r: &Response) -> bool {
    let g = gesture(ui.ctx());
    if !g.touch {
        return false;
    }
    if r.long_touched() {
        ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Gesture>(key()).selecting = true);
        return false;
    }
    if g.selecting {
        return false;
    }
    if r.dragged() {
        ui.scroll_with_delta_animation(r.drag_delta(), egui::style::ScrollAnimation::none());
    }
    true
}

/// Whether a finger selects: it was pressed long and is still down. Its place is the pointer's.
pub(crate) fn selecting(ctx: &Context) -> bool {
    let g = gesture(ctx);
    g.touch && g.selecting && ctx.input(|i| i.pointer.primary_down())
}
