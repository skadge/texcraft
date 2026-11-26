//! Editing in place in the formatted view (docs/ROADMAP.md, F3): what the view shows as source (an
//! environment or a command it does not render, a table the grid cannot show, the preamble) and
//! formulas are edited where they stand, in a small source editor with the code font and the
//! syntax colours, a formula with its typeset preview under it.
//!
//! - **Opening:** a click, or Enter on the part (a run in a paragraph selected first, as Backspace
//!   does), opens the editor in place of a block or under the paragraph holding the run.
//! - **Closing:** Escape or a click outside gives the formatted view back; what was typed goes in
//!   as one edit of the bytes that changed alone ([`edit::minimal_change`]), undone with the
//!   source's undo.
//! - **New formulas** (the toolbar's Formula menu) are typed in the same editor with its preview,
//!   in a dialog, then inserted.
//! - **Preview:** retex typesets the formula after the main document's preamble
//!   ([`texcraft_engine::Session::formula_preview`]) in the desktop app; the browser has no engine
//!   for formulas yet, and the editor says so.

use std::ops::Range;

use egui::text::{LayoutJob, TextFormat};
use egui::{FontFamily, FontId, Rect, RichText, Sense, StrokeKind, pos2, vec2};
use texcraft_buffer::{EditKind, Selection, Transaction};
use texcraft_engine::FormulaState;
use texcraft_engine::project::ProjectPath;
use texcraft_latex::edit;

use super::{Cx, Ink, Inked};
use crate::App;
use crate::theme::{Tokens, blend};

/// What is edited.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum What {
    /// A part the view shows as its source.
    Source,
    /// A formula's math, without its delimiters; displayed (in environment `env`) or inline.
    Formula { display: bool, env: Option<String> },
}

/// Where the editor is drawn.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// In place of the block starting at this byte.
    Block(usize),
    /// Under the paragraph holding the run of these bytes.
    Run(Range<usize>),
}

/// An editor open in the view.
pub(crate) struct InPlace {
    pub path: ProjectPath,
    /// The source edited, in bytes of the text when it opened, and that source.
    pub range: Range<usize>,
    pub original: String,
    pub buffer: String,
    pub what: What,
    pub anchor: Anchor,
    /// Its visible title: what it is and why the view shows it as source.
    pub header: String,
    /// Take the keyboard next frame.
    pub focus: bool,
}

/// A part of a paragraph that opens in the editor: its bytes, and for a formula its math and
/// whether it is displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Atom {
    pub range: Range<usize>,
    pub math: Option<(Range<usize>, bool)>,
}

impl InPlace {
    /// An editor on `range` of `text`.
    pub(crate) fn new(path: &ProjectPath, text: &str, range: Range<usize>, what: What, anchor: Anchor, header: String) -> InPlace {
        let original = text.get(range.clone()).unwrap_or_default().to_owned();
        InPlace { path: path.clone(), range, buffer: original.clone(), original, what, anchor, header, focus: true }
    }

    /// The editor of a run in a paragraph: a formula's math, or the run's source.
    pub(crate) fn of_atom(path: &ProjectPath, text: &str, atom: &Atom) -> InPlace {
        let anchor = Anchor::Run(atom.range.clone());
        match &atom.math {
            Some((math, display)) => InPlace::new(path, text, math.clone(), What::Formula { display: *display, env: None }, anchor, formula_title()),
            None => InPlace::new(path, text, atom.range.clone(), What::Source, anchor, tl!("LaTeX: a command the view does not show").to_owned()),
        }
    }
}

fn formula_title() -> String {
    tl!("Formula (LaTeX)").to_owned()
}

/// Opens `ip`, after closing the editor open already.
pub(crate) fn open(app: &mut App, ip: InPlace) {
    close(app);
    app.editor.formatted.in_place = Some(ip);
    // The editor takes the keyboard, not the view's text.
    app.editor.formatted.editing.focus = false;
    app.ctx.request_repaint();
}

/// Closes the open editor: what was typed goes in as one edit of the bytes that changed.
pub(crate) fn close(app: &mut App) {
    let Some(ip) = app.editor.formatted.in_place.take() else { return };
    app.editor.formatted.editing.focus = true;
    let Some(change) = edit::minimal_change(ip.range.start, &ip.original, &ip.buffer) else { return };
    if app.session.current() == Some(&ip.path) {
        let d = &mut app.editor.source.doc;
        let text = d.buffer.text();
        if text.get(ip.range.clone()) != Some(ip.original.as_str()) {
            return changed_meanwhile(app, &ip);
        }
        let (a, b) = (d.buffer.byte_to_char(change.range.start), d.buffer.byte_to_char(change.range.end));
        let before = d.selection.clone();
        let head = d.buffer.char_to_byte(before.primary().head);
        let caret = edit::map_offset(std::slice::from_ref(&change), head);
        let new = edit::apply(&text, std::slice::from_ref(&change));
        let caret = new.get(..caret).map_or(0, |s| s.chars().count());
        d.buffer.seal();
        if d.buffer.edit(Transaction::replace(a, b, &change.text), &before, &Selection::single(caret, caret), EditKind::Other).is_ok() {
            d.selection = Selection::single(caret, caret);
            d.buffer.seal();
        }
        return;
    }
    // The file shown changed while the editor was open: the edit goes to the session.
    let Ok(text) = app.session.project().text(&ip.path).map(str::to_owned) else { return };
    if text.get(ip.range.clone()) != Some(ip.original.as_str()) {
        return changed_meanwhile(app, &ip);
    }
    let chars = |b: usize| text.get(..b).map_or(0, |s| s.chars().count());
    let edits = serde_json::json!([{"start": chars(change.range.start), "end": chars(change.range.end), "text": change.text}]);
    app.run("file.edit", serde_json::json!({"path": ip.path.as_str(), "edits": edits}));
}

/// The source changed under the editor (another window, a collaborator): its text is put on the
/// clipboard instead, and the status line says so.
fn changed_meanwhile(app: &mut App, ip: &InPlace) {
    app.ctx.copy_text(ip.buffer.clone());
    let said = tl!("The source changed while you edited it, so your edit was not made; it is on the clipboard.");
    app.view.status = Some((said.to_owned(), true));
}

/// Whether the editor stands in for the block starting at `start`.
pub(super) fn at_block(cx: &Cx<'_>, start: usize) -> bool {
    cx.in_place.as_deref().is_some_and(|ip| ip.anchor == Anchor::Block(start))
}

/// Whether the editor is under the paragraph of `atoms`.
pub(super) fn under(cx: &Cx<'_>, atoms: &[Atom]) -> bool {
    cx.in_place.as_deref().is_some_and(|ip| matches!(&ip.anchor, Anchor::Run(r) if atoms.iter().any(|a| &a.range == r)))
}

/// The editor, drawn where it stands.
pub(super) fn draw(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    let Some(ip) = cx.in_place.take() else { return };
    let t = cx.t;
    let drawn = editor(ui, &t, ip, Preview { state: cx.preview, ink: &mut *cx.ink, body_pt: cx.body_pt });
    cx.close_in_place |= drawn.closed;
    cx.in_place = Some(ip);
}

/// What the preview of a formula is drawn from.
pub(super) struct Preview<'a> {
    pub state: Option<&'a FormulaState>,
    pub ink: &'a mut Ink,
    pub body_pt: f32,
}

struct Drawn {
    /// Escape, or a click outside.
    closed: bool,
}

/// The frame with its title, the source editor and, for a formula, its preview.
fn editor(ui: &mut egui::Ui, t: &Tokens, ip: &mut InPlace, preview: Preview<'_>) -> Drawn {
    let pad = 3.0 * t.unit;
    let width = ui.available_width();
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let frame_shape = ui.painter().add(egui::Shape::Noop);
    ui.add_space(pad);
    let mut closed = false;
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(width - 2.0 * pad);
            let title = ui.label(RichText::new(&ip.header).font(t.small()).strong().color(t.text));
            let hint = tl!("Escape or a click outside returns to the formatted view.");
            ui.label(RichText::new(hint).font(t.small()).color(t.text_muted));
            ui.add_space(t.unit * 2.0);
            let id = egui::Id::unique(("formatted-in-place", ip.path.as_str()));
            let r = field(ui, t, id, &mut ip.buffer, (matches!(ip.what, What::Formula { .. }), false)).labelled_by(title.id);
            if std::mem::take(&mut ip.focus) {
                r.request_focus();
                ui.ctx().request_repaint();
            }
            closed = r.lost_focus();
            if let What::Formula { .. } = ip.what {
                ui.add_space(t.unit * 2.0);
                show_preview(ui, t, &ip.buffer, preview);
            }
        });
    });
    ui.add_space(pad);
    let rect = Rect::from_min_max(pos2(left, top), pos2(left + width, ui.cursor().top()));
    ui.painter().set(
        frame_shape,
        egui::Shape::Rect(egui::epaint::RectShape::new(
            rect,
            t.corner_large(),
            blend(t.surface, t.current_line),
            egui::Stroke::new(1.5, t.accent),
            StrokeKind::Inside,
        )),
    );
    Drawn { closed }
}

/// The source field: several lines in the code font with the source's syntax colours; Tab
/// indents in it. With `shift_newline`, Shift+Enter starts a line (Enter is the dialog's).
fn field(ui: &mut egui::Ui, t: &Tokens, id: egui::Id, buffer: &mut String, (math, shift_newline): (bool, bool)) -> egui::Response {
    let font = FontId::new(t.code_size, FontFamily::Monospace);
    let tok = *t;
    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
        let s = buf.as_str();
        let mut job = LayoutJob::default();
        for token in texcraft_latex::lex(s) {
            let color = if math && token.kind == texcraft_latex::Kind::Text { tok.syntax_math } else { tok.syntax(token.kind) };
            job.append(s.get(token.range.clone()).unwrap_or_default(), 0.0, TextFormat::simple(font.clone(), color));
        }
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let rows = buffer.lines().count().clamp(1, 16);
    let mut edit = egui::TextEdit::multiline(buffer);
    if shift_newline {
        edit = edit.return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter));
    }
    ui.add(edit.id(id).code_editor().lock_focus(true).desired_rows(rows).desired_width(f32::INFINITY).layouter(&mut layouter))
}

/// The typeset formula under its source, or why it is not shown.
fn show_preview(ui: &mut egui::Ui, t: &Tokens, source: &str, p: Preview<'_>) {
    let say = |ui: &mut egui::Ui, text: &str, color| {
        ui.label(RichText::new(text).font(t.small()).color(color));
    };
    if source.trim().is_empty() {
        return say(ui, tl!("Type a formula to see it typeset here."), t.text_muted);
    }
    match p.state {
        Some(FormulaState::Typeset(b)) => {
            let scale = t.reading_size / p.body_pt.max(1.0);
            let natural = vec2(b.width_pt, b.height_pt + b.depth_pt) * scale;
            let room = ui.available_width();
            let fit = (room / natural.x.max(1.0)).min(1.0);
            let (row, r) = ui.allocate_exact_size(vec2(room, natural.y * fit + 2.0 * t.unit), Sense::hover());
            let rect = Rect::from_center_size(row.center(), natural * fit);
            let label = tl!("Preview of the formula");
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, label));
            let px = scale * fit * ui.ctx().pixels_per_point();
            match p.ink.texture(ui.ctx(), b, px) {
                Inked::Ready(id) => {
                    ui.painter().image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), t.text);
                }
                Inked::Later => {}
                Inked::Broken => {
                    ui.painter().rect_stroke(rect, t.corner(), egui::Stroke::new(1.0, t.text_muted), StrokeKind::Inside);
                }
            }
        }
        Some(FormulaState::Pending) => {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            say(ui, tl!("Typesetting…"), t.text_muted);
        }
        Some(FormulaState::Failed(messages)) => {
            let message = messages.iter().map(|m| crate::i18n::msg(m)).collect::<Vec<_>>().join("\n");
            say(ui, &tl!("This formula has an error: {message}").replace("{message}", &message), t.error);
        }
        Some(FormulaState::Unavailable(_)) | None => {
            say(ui, tl!("The preview is typeset in the desktop app; this version shows the formula's source."), t.text_muted);
        }
    }
}

/// A new formula being typed in the dialog: the command that inserts it, and its kind.
pub(crate) struct NewFormula {
    command: &'static str,
    title: &'static str,
    display: bool,
    env: Option<&'static str>,
    buffer: String,
    focus: bool,
}

/// Opens the new formula dialog for `edit.math_inline`, `edit.math_display` or `edit.equation`,
/// its field holding `start` (the selection).
pub(crate) fn ask_formula(app: &mut App, command: &str, start: String) {
    let (command, title, display, env) = match command {
        "edit.math_inline" => ("edit.math_inline", tl!("Inline Formula"), false, None),
        "edit.math_display" => ("edit.math_display", tl!("Displayed Formula"), true, None),
        _ => ("edit.equation", tl!("Numbered Equation"), true, Some("equation")),
    };
    app.editor.formatted.new_formula = Some(NewFormula { command, title, display, env, buffer: start, focus: true });
}

/// The new formula dialog: the field and its preview, Insert (Enter) and Cancel (Escape).
pub(crate) fn new_formula_dialog(app: &mut App, ctx: &egui::Context, path: &ProjectPath) {
    let Some(nf) = app.editor.formatted.new_formula.as_ref() else { return };
    let (math, display, env) = (nf.buffer.clone(), nf.display, nf.env);
    let state = (!math.trim().is_empty()).then(|| app.session.formula_preview(path, &math, display, env));
    let body_pt = app.session.formulas(path).map_or(10.0, |f| f.body_pt.max(1.0));
    let t = app.tokens;
    let (Some(nf), ink) = (app.editor.formatted.new_formula.as_mut(), &mut app.editor.formatted.ink) else { return };
    let mut outcome = None;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-new-formula")).frame(crate::widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(460.0);
        crate::dialogs::title(ui, &t, nf.title);
        let label = ui.label(RichText::new(tl!("The formula in LaTeX, such as x^2 + \\alpha")).color(t.text_muted));
        // Enter inserts it (Shift+Enter starts a line), before the field takes it.
        let field_id = egui::Id::unique("formatted-new-formula");
        if ui.ctx().memory(|m| m.has_focus(field_id)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            outcome = Some(true);
        }
        let r = field(ui, &t, field_id, &mut nf.buffer, (true, true)).labelled_by(label.id);
        if std::mem::take(&mut nf.focus) {
            r.request_focus();
        }
        ui.label(RichText::new(tl!("Enter inserts it; Shift+Enter starts a new line.")).font(t.small()).color(t.text_muted));
        ui.add_space(t.unit * 2.0);
        ink.begin();
        show_preview(ui, &t, &nf.buffer, Preview { state: state.as_ref(), ink, body_pt });
        match crate::dialogs::buttons(ui, &t, tl!("Insert"), false, !nf.buffer.trim().is_empty()) {
            crate::dialogs::Outcome::Run => outcome = Some(true),
            crate::dialogs::Outcome::Cancel => outcome = Some(false),
            _ => {}
        }
    });
    if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = outcome.or(Some(false));
    }
    let Some(insert) = outcome else { return };
    let Some(nf) = app.editor.formatted.new_formula.take() else { return };
    app.editor.formatted.editing.focus = true;
    if insert && !nf.buffer.trim().is_empty() {
        let latex = nf.buffer.trim().to_owned();
        crate::ui_cmd::run_with(app, nf.command, &serde_json::json!({ "latex": latex }));
    }
}
