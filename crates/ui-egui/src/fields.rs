//! The window's text fields (egui's own: in dialogs, Settings, the find bar, the side panels) and
//! the Edit menu. The clipboard's keys reach a focused field by themselves: the keymap leaves
//! them to it while it types ([`crate::keys`]). A menu entry chosen with the pointer, though, takes
//! the keyboard from the field first, and the menu's Cut, Copy, Paste, Select All, Undo and Redo
//! would act on the source editor. Instead they act on the field that had the keyboard: next frame
//! (the click that chose the entry has made the field give the keyboard up in this one) it gets
//! the keyboard back, with the selection it had (a field that gives the keyboard up drops its
//! selection), and the input it would have had from the keys.

use egui::text::CCursorRange;
use egui::text_edit::TextEditState;
use egui::{Event, Id, Key, Modifiers};

use crate::App;

#[derive(Default)]
pub(crate) struct Fields {
    /// The text field that had the keyboard last, while no other control has taken it, and its
    /// selection then.
    last: Option<(Id, Option<CCursorRange>)>,
    /// A field, its selection, and the Edit menu command to run on it next frame.
    queued: Option<((Id, Option<CCursorRange>), &'static str)>,
}

/// Whether `id` is one of egui's text fields, drawn this frame or the last.
fn is_field(ctx: &egui::Context, id: Id) -> bool {
    TextEditState::load(ctx, id).is_some() && ctx.read_response(id).is_some()
}

/// The Edit menu commands a text field does itself.
fn acts_on_field(id: &str) -> bool {
    matches!(id, "edit.cut" | "edit.copy" | "edit.paste" | "edit.select_all" | "edit.undo" | "edit.redo")
}

/// The field the Edit menu acts on now: the focused one, or the one the menu took the keyboard
/// from.
fn menu_target(app: &App, ctx: &egui::Context) -> Option<(Id, Option<CCursorRange>)> {
    match ctx.memory(egui::Memory::focused) {
        Some(id) if is_field(ctx, id) => Some((id, selection(ctx, id))),
        _ => app.fields.last.filter(|(id, _)| is_field(ctx, *id)),
    }
}

fn selection(ctx: &egui::Context, id: Id) -> Option<CCursorRange> {
    TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range())
}

/// At the end of the frame: notes the field that has the keyboard. A menu open, or the menu bar
/// reached from the keyboard, keeps the field noted; another control taking the keyboard, or a
/// click elsewhere, forgets it.
pub(crate) fn track(app: &mut App, ctx: &egui::Context) {
    let in_menu = egui::Popup::is_any_open(ctx) || app.view.menu_nav.is_some();
    match ctx.memory(egui::Memory::focused) {
        Some(id) if is_field(ctx, id) => app.fields.last = Some((id, selection(ctx, id))),
        _ if in_menu => {}
        Some(_) => app.fields.last = None,
        None => {
            let clicked = ctx.input(|i| i.pointer.any_click());
            if clicked || app.fields.last.is_some_and(|(id, _)| !is_field(ctx, id)) {
                app.fields.last = None;
            }
        }
    }
}

/// Whether Edit menu entry `id` can be chosen now: with a field to act on, Cut and Copy do not
/// wait for a selection in the editor.
pub(crate) fn menu_enabled(app: &App, ctx: &egui::Context, id: &str) -> Result<(), String> {
    if matches!(id, "edit.cut" | "edit.copy") && menu_target(app, ctx).is_some() {
        return Ok(());
    }
    crate::ui_cmd::enabled(app, id)
}

/// Runs Edit menu entry `id` on the field the menu took the keyboard from; `false` when there is
/// none, or the field does not do `id`, and the command runs as usual.
pub(crate) fn menu_command(app: &mut App, id: &'static str) -> bool {
    let ctx = app.ctx.clone();
    if !acts_on_field(id) || (id == "edit.paste" && crate::ui_cmd::enabled(app, id).is_err()) {
        return false;
    }
    let Some(field) = menu_target(app, &ctx) else { return false };
    app.fields.queued = Some((field, id));
    ctx.request_repaint();
    true
}

/// After the window's shortcuts ran, before the field is drawn: gives the field the keyboard
/// back, and the input of the menu's command.
pub(crate) fn deliver(app: &mut App, ctx: &egui::Context) {
    let Some(((field, selected), id)) = app.fields.queued.take() else { return };
    let Some(mut state) = TextEditState::load(ctx, field).filter(|_| is_field(ctx, field)) else { return };
    state.cursor.set_char_range(selected);
    state.store(ctx, field);
    ctx.memory_mut(|m| m.request_focus(field));
    let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    let input = match id {
        "edit.cut" => Event::Cut,
        "edit.copy" => Event::Copy,
        "edit.select_all" => key(Key::A, Modifiers::COMMAND),
        "edit.undo" => key(Key::Z, Modifiers::COMMAND),
        "edit.redo" => key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
        // The window reads the clipboard and sends its text as a paste, to the widget that has
        // the keyboard at the end of this frame: the field.
        _ => {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            return;
        }
    };
    ctx.input_mut(|i| i.events.push(input));
}
