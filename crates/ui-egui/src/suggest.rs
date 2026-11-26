//! Suggest mode (docs/collaboration-roadmap.md, R2): the editor shows the file with its open
//! suggestions in it (each passage struck through, its replacement after it, underlined), and an
//! edit of that text is not made in the file but becomes suggestions: `review.suggest` to make or
//! extend one's own, `review.delete` to take one back (AGENTS.md rule 14). The text shown is
//! made again from the session whenever the file or the review changes.
//!
//! Outside Suggest mode the editor shows the file as it is; suggestions are marked on their
//! passages and shown whole in their cards.

use egui::{Align, Layout, RichText, Sense, vec2};
use serde_json::json;
use texcraft_buffer::{Origin, Selection, Transaction};
use texcraft_engine::project::{Contents, ProjectPath};
use texcraft_engine::review::Id;
use texcraft_engine::review::view::{Caret, Kind, Op, View};

use crate::source::{Doc, Mark, Passage};
use crate::{App, icons};

/// The text the editor shows in Suggest mode, and what it was made from.
pub(crate) struct Shown {
    pub path: ProjectPath,
    pub view: View,
    /// The session's revision and review revision it was made at.
    stamp: Option<(u64, u64)>,
}

/// Where the caret goes once the text is shown again: after an edit read as suggestions.
pub(crate) type CaretHint = (Caret, Option<Id>);

/// The text shown for `path` in Suggest mode, when it is the editor's.
pub(crate) fn view<'a>(app: &'a App, path: &ProjectPath) -> Option<&'a View> {
    app.editor.shown.as_ref().filter(|s| &s.path == path).map(|s| &s.view)
}

/// Position `s` of the file, in the text the editor shows.
pub(crate) fn to_view(app: &App, path: &ProjectPath, s: usize) -> usize {
    view(app, path).map_or(s, |v| v.to_view(s))
}

/// The end of a passage ending at `s` of the file, in the text the editor shows.
pub(crate) fn to_view_end(app: &App, path: &ProjectPath, s: usize) -> usize {
    view(app, path).map_or(s, |v| v.to_view_end(s))
}

/// Position `v` of the text the editor shows, in the file.
pub(crate) fn to_source(app: &App, path: &ProjectPath, v: usize) -> usize {
    view(app, path).map_or(v, |view| view.to_source(v))
}

/// Shows `target` in `doc` as one change that is not undone by Undo and not sent to the session.
pub(crate) fn show_text(doc: &mut Doc, target: &str) {
    let old = doc.buffer.text();
    if old == target {
        return;
    }
    let tx = Transaction::diff(&old, target);
    let after = doc.selection.map(&tx);
    if doc.buffer.receive(tx, Origin::Remote).is_ok() {
        doc.selection = after.clamp(doc.buffer.len_chars());
    }
    doc.buffer.take_edits();
}

/// Turns Suggest mode on or off (Edit › Suggest Changes, the toolbar's Suggest).
pub(crate) fn toggle(app: &mut App) {
    app.editor.suggest = !app.editor.suggest;
    let message = if app.editor.suggest {
        if app.prefs.identity.is_none() {
            tl!("Suggesting. Choose the name your suggestions carry in Settings › Review first.")
        } else {
            tl!("Suggesting: your edits become suggestions; the file does not change until one is accepted.")
        }
    } else {
        tl!("Editing: your edits change the file.")
    };
    app.view.status = Some((message.to_owned(), false));
    app.editor.focus = true;
}

fn text_of(app: &App, path: &ProjectPath) -> Option<String> {
    match app.session.project().get(path) {
        Some(Contents::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

/// The file's text with its suggestions shown, as the session has them now.
fn fresh(app: &App, path: &ProjectPath) -> Option<View> {
    let source = text_of(app, path)?;
    let threads = app.session.review().threads().iter().filter(|t| t.anchor.path == path.as_str());
    Some(View::new(&source, threads))
}

/// Reads the edits of the text shown in Suggest mode as suggestions; `None` when the editor is
/// not suggesting (its edits then go to the file). True when there were edits.
pub(crate) fn send(app: &mut App) -> Option<bool> {
    let path = app.editor.shown.as_ref()?.path.clone();
    let edits = app.editor.source.doc.buffer.take_edits();
    let edits: Vec<Transaction> = edits.into_iter().filter(|(_, o)| !o.is_foreign()).map(|(tx, _)| tx).collect();
    if edits.is_empty() {
        return Some(false);
    }
    // The text is shown again from the session whatever happens, which also takes back an edit
    // that was refused.
    if let Some(shown) = app.editor.shown.as_mut() {
        shown.stamp = None;
    }
    let Some(me) = app.prefs.identity.as_ref().map(crate::prefs::Identity::author) else {
        app.view.status = Some((tl!("Choose the name your suggestions carry first (Settings › Review).").to_owned(), true));
        return Some(true);
    };
    let before = app.editor.caret_before;
    let mut first = true;
    for tx in edits {
        for c in tx.changes().iter().rev() {
            let view = if first { app.editor.shown.as_ref().map(|s| s.view.clone()) } else { fresh(app, &path) };
            first = false;
            let (Some(view), Some(source)) = (view, text_of(app, &path)) else { return Some(true) };
            let forward = before <= c.start;
            match view.interpret(&source, c.start, c.end, &c.text, &me, forward) {
                Err(other) => {
                    let text = tl!("That is {name}’s suggestion: reply to it, or accept or reject it.").replace("{name}", &other.name);
                    app.view.status = Some((text, true));
                }
                Ok(plan) => {
                    let mut made = None;
                    for op in plan.ops {
                        match op {
                            Op::New { start, end, text } => {
                                let params = json!({"path": path.as_str(), "start": start, "end": end, "text": text});
                                made = app.run("review.suggest", params).and_then(|r| r["id"].as_str().and_then(Id::parse));
                            }
                            Op::Revise { id, start, end, text } => {
                                let params = json!({"path": path.as_str(), "start": start, "end": end, "text": text, "id": id.to_string()});
                                made = app.run("review.suggest", params).map(|_| id);
                            }
                            Op::Remove { id } => {
                                app.run("review.delete", json!({"id": id.to_string()}));
                            }
                        }
                    }
                    app.editor.caret_hint = Some((plan.caret, made));
                }
            }
        }
    }
    Some(true)
}

/// After the editor caught up with the session: in Suggest mode, shows the file `path` with its
/// suggestions, made again when the file or the review changed, and puts the caret where the last
/// edit left it.
pub(crate) fn refresh(app: &mut App, path: &ProjectPath) {
    if !app.editor.suggest || app.editor.formatted.shows(path) || text_of(app, path).is_none() {
        return;
    }
    let stamp = (app.session.revision(), app.session.review_revision());
    if app.editor.shown.as_ref().is_some_and(|s| &s.path == path && s.stamp == Some(stamp)) {
        return;
    }
    let Some(view) = fresh(app, path) else { return };
    show_text(&mut app.editor.source.doc, &view.text);
    if let Some((caret, id)) = app.editor.caret_hint.take() {
        let at = match caret {
            Caret::Source(s) => Some(view.to_view(s)),
            Caret::Inserted(k) => id.and_then(|id| view.shown.iter().find(|s| s.id == id)).map(|s| s.inserted_at + k),
        };
        if let Some(at) = at {
            let len = app.editor.source.doc.buffer.len_chars();
            app.editor.source.doc.selection = Selection::point(at.min(len));
            app.editor.source.reveal = true;
        }
    }
    app.editor.shown = Some(Shown { path: path.clone(), view, stamp: Some(stamp) });
}

/// The suggestions' marks in the text the editor shows: in Suggest mode each passage struck
/// through and each replacement underlined; otherwise the passages struck through, and an
/// insertion as a mark where it goes.
pub(crate) fn passages(app: &App, path: &ProjectPath, selected: Option<Id>) -> Vec<Passage> {
    let review = app.session.review();
    let colour_of = |id: Id| review.get(id).and_then(|t| t.author()).map(crate::review::colour);
    if let Some(view) = view(app, path) {
        return view
            .spans
            .iter()
            .filter_map(|span| {
                let (id, mark) = match span.kind {
                    Kind::Text { .. } => return None,
                    Kind::Deleted { id, .. } => (id, Mark::Deleted),
                    Kind::Inserted { id, .. } => (id, Mark::Inserted),
                };
                Some(Passage { start: span.start, end: span.end, color: colour_of(id)?, selected: selected == Some(id), mark })
            })
            .collect();
    }
    review
        .threads()
        .iter()
        .filter(|t| {
            t.anchor.path == path.as_str() && t.is_open() && t.is_suggestion() && t.anchor.placement != texcraft_engine::review::Placement::Unanchored
        })
        .filter_map(|t| {
            let mark = if t.anchor.start == t.anchor.end { Mark::Comment } else { Mark::Deleted };
            Some(Passage { start: t.anchor.start, end: t.anchor.end, color: colour_of(t.id)?, selected: selected == Some(t.id), mark })
        })
        .collect()
}

/// The toolbar's Suggest switch: a button that stays pressed while suggesting. With less than
/// `room` for it and its label (a narrow pane, a language's longer word), the pencil alone, named
/// and explained the same.
pub(crate) fn switch(app: &mut App, ui: &mut egui::Ui, room: f32) -> bool {
    let t = app.tokens;
    let on = app.editor.suggest;
    let colour = if on { t.accent } else { t.text_muted };
    let label = tl!("Suggest");
    // The label, the icon and the button's padding.
    let wanted = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), t.small(), t.text).size().x) + 14.0 + 4.0 * t.unit + 8.0;
    let compact = room < wanted;
    let icon = icons::image("pencil", 14.0, colour);
    let r = if compact {
        ui.add(egui::Button::selectable(on, egui::Atoms::new(icon)))
    } else {
        let text = RichText::new(label).font(t.small()).color(if on { t.text } else { t.text_muted });
        ui.add(egui::Button::selectable(on, egui::Atoms::new((icon, text))))
    };
    let tip = match (on, app.keys.shortcut_text("edit.suggest_mode")) {
        (true, Some(keys)) => tf!("Suggesting: edits become suggestions ({keys} to edit the file)", keys = keys),
        (true, None) => tl!("Suggesting: edits become suggestions").to_owned(),
        (false, _) => tl!("Suggest changes instead of editing the file").to_owned(),
    };
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, tl!("Suggest changes")));
    r.on_hover_text(tip).clicked()
}

/// The line under the toolbar while suggesting: what the mode does, whether the PDF shows the
/// suggestions, and the switch for that.
pub(crate) fn banner(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut stop = false;
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height * 0.8), Layout::left_to_right(Align::Center), |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0.0, crate::theme::blend(t.surface, t.accent.gamma_multiply(0.12)));
        ui.add_space(t.unit * 3.0);
        let (rect, _) = ui.allocate_exact_size(vec2(t.icon_size, t.icon_size), Sense::hover());
        icons::paint(ui, rect, "pencil", t.icon_size - 2.0, t.accent);
        ui.add_space(t.unit * 2.0);
        let note = if app.session.settings.with_suggestions {
            tl!("Suggesting: edits are proposed, not made. The PDF shows the text with the open suggestions applied.")
        } else {
            tl!("Suggesting: edits are proposed, not made. The PDF shows the text as it is; suggestions are not applied.")
        };
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(t.unit * 2.0);
            stop = ui.add(egui::Button::new(RichText::new(tl!("Stop Suggesting")).font(t.small())).frame_when_inactive(false)).clicked();
            pdf_switch(app, ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(note).font(t.small()).color(t.text)).truncate());
            });
        });
    });
    if stop {
        toggle(app);
    }
}

/// "Show suggestions in the PDF": builds with the open suggestions applied (`build.suggestions`).
pub(crate) fn pdf_switch(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut on = app.session.settings.with_suggestions;
    let text = RichText::new(tl!("Show suggestions in the PDF")).font(t.small()).color(t.text);
    if ui.checkbox(&mut on, text).on_hover_text(tl!("Build with the open suggestions applied; the files do not change")).changed()
        && app.run("build.suggestions", json!({"on": on})).is_some()
    {
        // As after an edit: the automatic build runs again.
        app.view.last_edit = Some(ui.input(|i| i.time));
        let said = if on {
            tl!("The PDF will show the open suggestions applied.")
        } else {
            tl!("The PDF will show the text as it is, without suggestions.")
        };
        app.view.status = Some((said.to_owned(), false));
    }
}
