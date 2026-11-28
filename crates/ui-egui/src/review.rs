//! Review in the editor (docs/collaboration-roadmap.md, R1 and R2): commented passages tinted in
//! the text, suggested changes marked in it (Suggest mode, [`crate::suggest`], shows them whole),
//! the margin of threads and suggestions beside it, the composer, and the Review panel that takes
//! the margin's place when the pane is narrow.
//!
//! - **The margin** is a column right of the text. Each thread's card is level with its passage,
//!   pushed down when the one above it is long, with a thin line from the passage to it. Selecting
//!   a card selects its passage in the text, and putting the caret in a passage selects its card.
//! - **Cards** show the author's badge (the colour craft-ui's presence badges give the author's
//!   key), name, how long ago, the messages, Reply and Resolve, and Edit and Delete for one's own
//!   messages. Resolved threads fold to one line. The filter shows all, open, resolved or one's own.
//! - **Suggestion cards** say what would change ("Replace “colour” with “color”", the old text
//!   struck through and the new underlined), with Accept and Reject; replies are as on a comment.
//!   An outdated suggestion (its passage changed since) says so and can only be rejected.
//! - Every change goes through the engine's `review.*` commands (AGENTS.md rule 14). Each card is
//!   an accessibility group named by its author and quote; replies and resolutions that arrive,
//!   and the thread the caret enters, are announced on the status line.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use egui::{Align, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};
use serde_json::json;
use texcraft_engine::project::ProjectPath;
use texcraft_engine::review::{Author, Id, Message, Placement, State, Thread};

use crate::source::{Mark, Passage};
use crate::theme::{Tokens, blend, contrast};
use crate::{App, icons, widgets};

/// Below this pane width the threads are in the Review panel instead of a margin.
pub(crate) const MARGIN_MIN_PANE: f32 = 760.0;
/// The margin's width.
const MARGIN_WIDTH: f32 = 288.0;
/// The space between two cards.
const GAP: f32 = 8.0;

/// Which threads are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Show {
    #[default]
    All,
    Open,
    Resolved,
    Mine,
}

const SHOWS: [Show; 4] = [Show::All, Show::Open, Show::Resolved, Show::Mine];

impl Show {
    fn label(self) -> &'static str {
        match self {
            Show::All => tl!("All"),
            Show::Open => tl!("Open"),
            Show::Resolved => tl!("Resolved"),
            Show::Mine => crate::i18n::tc("comments", "Mine"),
        }
    }
}

/// A comment being written on a passage.
pub(crate) struct Composer {
    path: ProjectPath,
    start: usize,
    end: usize,
    quote: String,
    text: String,
    /// The name to comment under, while the user has none yet.
    name: String,
    focus: bool,
}

/// What a frame's cards asked for, done once they are drawn.
enum Action {
    Select(Id),
    Run(&'static str, serde_json::Value),
    Fold(Id, bool),
    Edit(Id, Id, String),
    CancelEdit,
    Post,
    CancelComposer,
    Identify(String),
}

/// Each thread's message count and whether it is open.
type Seen = HashMap<Id, (usize, bool)>;

/// The review's view state.
#[derive(Default)]
pub(crate) struct ReviewView {
    pub selected: Option<Id>,
    pub show: Show,
    pub composer: Option<Composer>,
    /// Replies being written, by thread.
    replies: HashMap<Id, String>,
    /// The message being edited: thread, message, text.
    editing: Option<(Id, Id, String)>,
    /// Resolved threads shown whole.
    unfolded: HashSet<Id>,
    /// Each thread's message count and openness when last looked at, for announcing changes.
    seen: Option<(u64, Seen)>,
    /// Where the caret was last frame.
    caret: Option<(ProjectPath, usize)>,
    /// The editor pane was wide enough for the margin last frame.
    wide: bool,
    /// Select this thread's passage once its file is shown.
    reveal: Option<Id>,
    /// Give the keyboard to this thread's reply field next frame.
    focus_reply: Option<Id>,
}

/// The user's review identity, as an author.
fn me(app: &App) -> Option<Author> {
    app.prefs.identity.as_ref().map(crate::prefs::Identity::author)
}

/// The author's colour: the one craft-ui's presence badges give the same key.
pub(crate) fn colour(author: &Author) -> Color32 {
    craft_ui::presence::colour_for(author.colour_id())
}

/// The theme's text colour that reads best on `fill`.
fn ink(t: &Tokens, fill: Color32) -> Color32 {
    if contrast(t.surface, fill) >= contrast(t.text, fill) { t.surface } else { t.text }
}

fn shown(app: &App, thread: &Thread, me: Option<&Author>) -> bool {
    match app.review.show {
        Show::All => true,
        Show::Open => thread.is_open(),
        Show::Resolved => !thread.is_open(),
        Show::Mine => me.is_some_and(|m| thread.author().is_some_and(|a| a.is(m)) || thread.messages.iter().any(|msg| msg.author.is(m))),
    }
}

/// The threads the filter shows, of `path` or of the whole project, in the order of the text.
fn threads(app: &App, path: Option<&ProjectPath>) -> Vec<Thread> {
    let me = me(app);
    let mut out: Vec<Thread> = app
        .session
        .review()
        .threads()
        .iter()
        .filter(|t| path.is_none_or(|p| t.anchor.path == p.as_str()) && shown(app, t, me.as_ref()))
        .cloned()
        .collect();
    out.sort_by(|a, b| (&a.anchor.path, a.anchor.start, a.id).cmp(&(&b.anchor.path, b.anchor.start, b.id)));
    out
}

/// The passages of `path` for the editor to mark, in the text it shows: open comments and the
/// selected one tinted, open suggestions struck through and underlined.
pub(crate) fn passages(app: &App, path: &ProjectPath) -> Vec<Passage> {
    let mut out: Vec<Passage> = threads(app, Some(path))
        .iter()
        .filter(|t| !t.is_suggestion() && t.anchor.placement != Placement::Unanchored && (t.is_open() || app.review.selected == Some(t.id)))
        .filter_map(|t| {
            let author = t.author()?;
            let start = crate::suggest::to_view(app, path, t.anchor.start);
            let end = if t.anchor.end == t.anchor.start { start } else { crate::suggest::to_view_end(app, path, t.anchor.end).max(start) };
            Some(Passage { start, end, color: colour(author), selected: app.review.selected == Some(t.id), mark: Mark::Comment })
        })
        .collect();
    out.extend(crate::suggest::passages(app, path, app.review.selected));
    out.sort_by_key(|p| (p.start, p.end));
    out
}

/// What a suggestion would do, in words: "Replace “a” with “b”", "Insert “b”", "Delete “a”".
pub(crate) fn change_text(t: &Thread, max: usize) -> String {
    let Some(s) = &t.suggestion else { return String::new() };
    let (old, new) = (short(&t.anchor.quote.exact, max), short(&s.replacement, max));
    match (old.is_empty(), new.is_empty()) {
        (true, _) => tl!("Insert “{new}”").replace("{new}", &new),
        (false, true) => tl!("Delete “{old}”").replace("{old}", &old),
        (false, false) => tl!("Replace “{old}” with “{new}”").replace("{old}", &old).replace("{new}", &new),
    }
}

/// The open suggestions of the file shown, for Accept All and Reject All.
pub(crate) fn open_suggestions_in_current(app: &App) -> usize {
    let Some(path) = app.session.current() else { return 0 };
    app.session.review().threads().iter().filter(|t| t.is_open() && t.is_suggestion() && t.anchor.path == path.as_str()).count()
}

/// A short quote of a passage, for names and lists.
fn short(text: &str, max: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max { one_line } else { one_line.chars().take(max.saturating_sub(1)).collect::<String>() + "…" }
}

/// The name of a thread for screen readers: who started it, on which words (a suggestion: what
/// it would change).
fn group_name(t: &Thread) -> String {
    let who = t.author().map(|a| a.name.clone()).unwrap_or_default();
    if let Some(s) = &t.suggestion {
        let state = match t.state {
            State::Accepted { .. } => crate::i18n::tc("suggestion", ", accepted"),
            State::Rejected { .. } => crate::i18n::tc("suggestion", ", rejected"),
            _ if s.outdated => crate::i18n::tc("suggestion", ", outdated"),
            _ => "",
        };
        return tl!("Suggestion by {name}: {change}{state}")
            .replace("{name}", &who)
            .replace("{change}", &change_text(t, 60))
            .replace("{state}", state);
    }
    let state = if t.is_open() { "" } else { crate::i18n::tc("comment", ", resolved") };
    tl!("Comment by {name} on “{quote}”{state}")
        .replace("{name}", &who)
        .replace("{quote}", &short(&t.anchor.quote.exact, 60))
        .replace("{state}", state)
}

/// Starts a comment on the shown file's selection, or on the caret's line without one (Edit ›
/// Comment, Cmd+Alt+M).
pub(crate) fn start(app: &mut App) {
    let Some(path) = app.session.current().cloned() else { return };
    let b = &app.editor.source.doc.buffer;
    let r = app.editor.source.doc.selection.primary();
    let (mut start, mut end) = (r.start(), r.end());
    if start == end {
        let line = b.char_to_line(app.editor.source.caret());
        (start, end) = (b.line_to_char(line), b.line_end(line));
    }
    // In Suggest mode the text shown is not the file's: the comment is on the file's passage.
    let (start, end) = (crate::suggest::to_source(app, &path, start), crate::suggest::to_source(app, &path, end));
    let quote = match app.session.project().text(&path) {
        Ok(text) => text.chars().skip(start).take(end.saturating_sub(start)).collect(),
        Err(_) => String::new(),
    };
    open_composer(app, path, start, end, quote);
}

fn open_composer(app: &mut App, path: ProjectPath, start: usize, end: usize, quote: String) {
    let name = crate::prefs::system_user_name();
    app.review.composer = Some(Composer { path, start, end, quote, text: String::new(), name, focus: true });
    app.review.selected = None;
    if !in_margin(app) {
        app.view.side = Some(crate::Side::Review);
    }
}

/// Whether the margin shows the comments and the one being written: the pane is wide enough and
/// the user did not hide it. Otherwise the Review panel has them.
fn in_margin(app: &App) -> bool {
    app.review.wide && app.prefs.review_margin
}

/// `:comment [text]` in Vim: on `start..end`, posted at once when there is a text.
pub(crate) fn from_vim(app: &mut App, path: &ProjectPath, start: usize, end: usize, text: &str) {
    let quote = match app.session.project().text(path) {
        Ok(source) => source.chars().skip(start).take(end.saturating_sub(start)).collect(),
        Err(_) => String::new(),
    };
    if text.trim().is_empty() || app.prefs.identity.is_none() {
        open_composer(app, path.clone(), start, end, quote);
        if let Some(c) = app.review.composer.as_mut() {
            c.text = text.to_owned();
        }
        return;
    }
    if let Some(r) = app.run("review.comment", json!({"path": path.as_str(), "start": start, "end": end, "text": text})) {
        app.review.selected = r.get("id").and_then(serde_json::Value::as_str).and_then(Id::parse);
        app.view.status = Some((tl!("Comment added").to_owned(), false));
    }
}

/// Selects the next (or previous) thread the filter shows, in the order of the project's files
/// and their text, and its passage (F8 and Shift+F8).
pub(crate) fn step(app: &mut App, forward: bool) {
    let list: Vec<Thread> = threads(app, None).into_iter().filter(|t| t.anchor.placement != Placement::Unanchored).collect();
    if list.is_empty() {
        app.view.status = Some((tl!("There are no comments to go to.").to_owned(), false));
        return;
    }
    let caret = app.session.current().map(|p| (p.as_str().to_owned(), crate::suggest::to_source(app, p, app.editor.source.caret())));
    let at = app.review.selected.and_then(|id| list.iter().position(|t| t.id == id));
    let next = match (at, forward) {
        (Some(i), true) => (i + 1) % list.len(),
        (Some(i), false) => (i + list.len() - 1) % list.len(),
        (None, true) => caret.as_ref().and_then(|(p, c)| list.iter().position(|t| (&t.anchor.path, t.anchor.start) > (p, *c))).unwrap_or(0),
        (None, false) => {
            caret.as_ref().and_then(|(p, c)| list.iter().rposition(|t| (&t.anchor.path, t.anchor.end) < (p, *c))).unwrap_or(list.len() - 1)
        }
    };
    let Some(t) = list.get(next) else { return };
    select(app, t);
    let text = if t.is_suggestion() { change_text(t, 80) } else { t.messages.first().map(|m| short(&m.text, 80)).unwrap_or_default() };
    let who = t.author().map(|a| a.name.clone()).unwrap_or_default();
    let said = if t.is_suggestion() { tl!("Suggestion {n} of {total}, by {name}: {text}") } else { tl!("Comment {n} of {total}, by {name}: {text}") };
    let said =
        said.replace("{n}", &(next + 1).to_string()).replace("{total}", &list.len().to_string()).replace("{name}", &who).replace("{text}", &text);
    app.view.status = Some((said, false));
}

/// Selects a thread and its passage, showing its file.
fn select(app: &mut App, t: &Thread) {
    app.review.selected = Some(t.id);
    let shown = app.session.current().is_some_and(|p| p.as_str() == t.anchor.path);
    if shown {
        let Some(path) = app.session.current().cloned() else { return };
        let start = crate::suggest::to_view(app, &path, t.anchor.start);
        let end = if t.anchor.end == t.anchor.start { start } else { crate::suggest::to_view_end(app, &path, t.anchor.end).max(start) };
        app.editor.source.select(start, end);
        app.review.caret = app.session.current().cloned().map(|p| (p, app.editor.source.caret()));
    } else {
        app.review.reveal = Some(t.id);
        app.run("file.open", json!({"path": t.anchor.path}));
    }
}

/// After the editor is drawn: a thread to reveal in the file now shown, and the thread the caret
/// moved into, which becomes the selected one (and is announced).
pub(crate) fn follow_caret(app: &mut App, path: &ProjectPath) {
    if let Some(id) = app.review.reveal
        && let Some(t) = app.session.review().get(id).filter(|t| t.anchor.path == path.as_str()).cloned()
    {
        app.review.reveal = None;
        select(app, &t);
        return;
    }
    let caret = crate::suggest::to_source(app, path, app.editor.source.caret());
    let now = Some((path.clone(), caret));
    if app.review.caret == now {
        return;
    }
    app.review.caret = now;
    let me = me(app);
    let inside = app
        .session
        .review()
        .threads()
        .iter()
        .filter(|t| t.anchor.path == path.as_str() && t.anchor.placement == Placement::Anchored && t.is_open() && shown(app, t, me.as_ref()))
        .find(|t| t.anchor.start <= caret && caret <= t.anchor.end && t.anchor.start < t.anchor.end)
        .cloned();
    match inside {
        Some(t) if app.review.selected != Some(t.id) => {
            app.review.selected = Some(t.id);
            let who = t.author().map(|a| a.name.clone()).unwrap_or_default();
            let said = if t.is_suggestion() {
                tl!("Suggested by {name}: {text}").replace("{text}", &change_text(&t, 80))
            } else {
                let text = t.messages.first().map(|m| short(&m.text, 80)).unwrap_or_default();
                tl!("Commented by {name}: {text}").replace("{text}", &text)
            };
            app.view.status = Some((said.replace("{name}", &who), false));
        }
        Some(_) => {}
        None => app.review.selected = None,
    }
}

/// Announces replies and resolutions that arrived since the last frame (from an agent, or the
/// files of another session), once per change of the review.
pub(crate) fn announce_changes(app: &mut App) {
    let revision = app.session.review_revision();
    if app.review.seen.as_ref().is_some_and(|(r, _)| *r == revision) {
        return;
    }
    let now: HashMap<Id, (usize, bool)> = app.session.review().threads().iter().map(|t| (t.id, (t.messages.len(), t.is_open()))).collect();
    let me = me(app);
    if let Some((_, before)) = &app.review.seen {
        for t in app.session.review().threads() {
            let Some((count, open)) = before.get(&t.id) else { continue };
            let last = t.messages.last();
            let by_other = |a: &Author| me.as_ref().is_none_or(|m| !a.is(m));
            if t.messages.len() > *count
                && let Some(m) = last.filter(|m| by_other(&m.author))
            {
                app.view.status =
                    Some((tl!("{name} replied: {text}").replace("{name}", &m.author.name).replace("{text}", &short(&m.text, 80)), false));
            }
            let closed = match &t.state {
                State::Resolved { by, .. } => Some((by, tl!("{name} resolved a comment"))),
                State::Accepted { by, .. } => Some((by, tl!("{name} accepted a suggestion"))),
                State::Rejected { by, .. } => Some((by, tl!("{name} rejected a suggestion"))),
                State::Open => None,
            };
            if let (true, Some((by, text))) = (*open, closed)
                && by_other(by)
            {
                app.view.status = Some((text.replace("{name}", &by.name), false));
            }
        }
    }
    app.review.seen = Some((revision, now));
}

/// Shows or hides the margin of comments (the View command; the margin's own button hides it). The
/// comments stay in the Review panel, and a comment being written moves there with the margin.
pub(crate) fn toggle_margin(app: &mut App) {
    app.prefs.review_margin = !app.prefs.review_margin;
    app.prefs_changed = true;
    if app.review.composer.is_some() && !in_margin(app) {
        app.view.side = Some(crate::Side::Review);
    }
    let message = if app.prefs.review_margin {
        tl!("Comments are shown beside the text.")
    } else {
        tl!("Comments beside the text are hidden; the Review panel lists them.")
    };
    app.view.status = Some((message.to_owned(), false));
}

/// Shows the source editor, with the margin at its right when the pane is wide enough, the user did
/// not hide the margin, and the file has threads or a comment being written.
pub(crate) fn with_margin<T>(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath, show: impl FnOnce(&mut App, &mut egui::Ui) -> T) -> T {
    announce_changes(app);
    app.editor.source.passages = passages(app, path);
    let full = ui.available_rect_before_wrap();
    app.review.wide = full.width() >= MARGIN_MIN_PANE;
    let composing = app.review.composer.as_ref().is_some_and(|c| &c.path == path);
    let any = app.session.review().threads().iter().any(|t| t.anchor.path == path.as_str());
    if !in_margin(app) || !(composing || any) {
        return show(app, ui);
    }
    let split = full.right() - MARGIN_WIDTH;
    let text_rect = Rect::from_min_max(full.min, pos2(split, full.bottom()));
    let margin_rect = Rect::from_min_max(pos2(split, full.top()), full.max);
    let out = ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| show(app, ui)).inner;
    ui.scope_builder(UiBuilder::new().max_rect(margin_rect).accessibility_label(tl!("Comments")), |ui| margin(app, ui, margin_rect, path));
    out
}

/// The filter, as a segmented switch.
fn filter_bar(app: &mut App, ui: &mut egui::Ui, salt: &str) {
    let t = app.tokens;
    let options: Vec<(&str, Option<&str>)> = SHOWS.iter().map(|s| (s.label(), None)).collect();
    let selected = SHOWS.iter().position(|s| *s == app.review.show).unwrap_or(0);
    if let Some(i) = widgets::segmented(ui, &t, salt, &options, selected) {
        app.review.show = SHOWS.get(i).copied().unwrap_or_default();
    }
}

/// Accept All and Reject All, for the suggestions of the file shown.
fn all_buttons(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut chosen = None;
    if link_button(ui, &t, tl!("Accept All")).on_hover_text(tl!("Accept every open suggestion in this file (one undoable edit)")).clicked() {
        chosen = Some("edit.accept_all");
    }
    if link_button(ui, &t, tl!("Reject All")).on_hover_text(tl!("Reject every open suggestion in this file")).clicked() {
        chosen = Some("edit.reject_all");
    }
    if let Some(id) = chosen {
        crate::ui_cmd::run(app, id);
    }
}

/// The margin: a header with the counts and the filter, and the cards beside their passages.
fn margin(app: &mut App, ui: &mut egui::Ui, rect: Rect, path: &ProjectPath) {
    let t = app.tokens;
    ui.spacing_mut().item_spacing = vec2(2.0 * t.unit, t.unit);
    ui.painter().rect_filled(rect, 0.0, t.frame);
    ui.painter().vline(rect.left(), rect.y_range(), Stroke::new(1.0, t.separator));
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), t.toolbar_height));
    ui.scope_builder(UiBuilder::new().max_rect(header.shrink2(vec2(3.0 * t.unit, 0.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
        // The counts are in the status bar and the file tree: the narrow margin keeps the filter.
        filter_bar(app, ui, "review-margin-filter");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if open_suggestions_in_current(app) > 0 {
                let more = widgets::icon_button(ui, &t, "ellipsis", tl!("Suggestions in this file"), true);
                crate::widgets::keyboard_menu(ui, &more);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(180.0);
                    ui.vertical(|ui| all_buttons(app, ui));
                });
            }
            let hide = ui.add(egui::Button::image(icons::image("x", t.icon_size, t.text_muted)).frame_when_inactive(false));
            hide.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Hide the comments beside the text")));
            if hide.on_hover_text(tl!("Hide the comments beside the text (the Review panel still lists them)")).clicked() {
                toggle_margin(app);
            }
        });
    });
    ui.painter().hline(rect.x_range(), header.bottom(), Stroke::new(1.0, t.separator));
    let area = Rect::from_min_max(pos2(rect.left(), header.bottom() + 1.0), rect.max);
    let list = threads(app, Some(path));
    let width = rect.width() - 6.0 * t.unit;
    let x = rect.left() + 3.0 * t.unit;
    let text_right = rect.left();
    let mut actions = Vec::new();
    let mut bottom = area.top() + 2.0 * t.unit;
    // The cards and the composer, in the order of their passages.
    let composer_at = app.review.composer.as_ref().filter(|c| &c.path == path).map(|c| crate::suggest::to_view(app, path, c.start));
    let mut items: Vec<(usize, Option<&Thread>)> = list
        .iter()
        .filter(|th| th.anchor.placement != Placement::Unanchored)
        .map(|th| (crate::suggest::to_view(app, path, th.anchor.start), Some(th)))
        .collect();
    if let Some(at) = composer_at {
        items.push((at, None));
    }
    items.sort_by_key(|(at, _)| *at);
    let clip = area;
    for (at, thread) in items {
        let passage = app.editor.source.screen_rect(at);
        let want = passage.map_or(bottom, |r| r.top() - 4.0);
        let top = want.max(bottom);
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, top), vec2(width, 4000.0))));
        child.set_clip_rect(clip);
        let (card, colour, selected) = match thread {
            Some(th) => {
                let selected = app.review.selected == Some(th.id);
                let colour = th.author().map(colour).unwrap_or(t.accent);
                (card(app, &mut child, th, false, &mut actions), colour, selected)
            }
            None => (composer(app, &mut child, &mut actions), me(app).as_ref().map_or(t.accent, colour), true),
        };
        // The line from the passage to its card.
        if let Some(p) = passage {
            // The line leaves the passage's line after its last character, never across its text.
            let end = thread.map_or(at, |th| crate::suggest::to_view_end(app, path, th.anchor.end).max(at));
            let b = &app.editor.source.doc.buffer;
            let line_end = b.line_end(b.char_to_line(end.min(b.len_chars())));
            let after = app.editor.source.screen_rect(line_end);
            let y = after.map_or(p.center().y, |r| r.center().y);
            let to = pos2(card.left(), card.top() + 16.0);
            let stroke = if selected { Stroke::new(1.25, colour) } else { Stroke::new(1.0, blend(t.frame, t.separator)) };
            let left = app.editor.source.shown().map_or(rect.left(), |(_, shown)| shown.left());
            let painter = ui.painter().with_clip_rect(Rect::from_min_max(pos2(left, area.top()), area.max));
            if selected && let Some((_, shown)) = app.editor.source.shown() {
                let from = after.map_or(text_right, |r| r.right() + t.unit);
                if y > shown.top() && y < shown.bottom() {
                    painter.line_segment([pos2(from + 2.0, y), pos2(text_right, y)], Stroke::new(1.0, colour.gamma_multiply(0.6)));
                }
            }
            if y > area.top() && to.y > area.top() {
                painter.line_segment([pos2(text_right, y), pos2(text_right + 1.5 * t.unit, y)], stroke);
                painter.line_segment([pos2(text_right + 1.5 * t.unit, y), to], stroke);
            }
        }
        bottom = card.bottom() + GAP;
    }
    apply(app, actions);
}

/// The Review panel (the rail's Review section): every thread of the project the filter shows, by
/// file, with the composer when the margin has no room for it.
pub(crate) fn panel(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.spacing_mut().item_spacing = vec2(1.5 * t.unit, t.unit);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, &t, tl!("Review"));
    });
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 2.0);
        filter_bar(app, ui, "review-panel-filter");
    });
    if open_suggestions_in_current(app) > 0 {
        ui.horizontal(|ui| {
            ui.add_space(t.unit * 2.0);
            all_buttons(app, ui);
        });
    }
    if app.session.review().threads().iter().any(|th| th.is_open() && th.is_suggestion()) {
        ui.horizontal(|ui| {
            ui.add_space(t.unit * 2.0);
            crate::suggest::pdf_switch(app, ui);
        });
    }
    ui.add_space(t.unit * 2.0);
    let mut actions = Vec::new();
    let list = threads(app, None);
    egui::ScrollArea::vertical().id_salt("review-panel").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = GAP;
        let width = ui.available_width() - 2.0 * t.unit;
        if app.review.composer.is_some() && !in_margin(app) {
            let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(ui.cursor().min + vec2(t.unit, 0.0), vec2(width, 4000.0))));
            let r = composer(app, &mut child, &mut actions);
            ui.allocate_space(vec2(width, r.height()));
        }
        // "No comments" under a comment being written would contradict it.
        if list.is_empty() && app.review.composer.is_none() {
            let how = tl!("Select text and choose Edit › Comment, or turn on Suggest to propose changes.");
            widgets::panel_empty(ui, &t, tl!("No comments or suggestions"), Some(how));
        }
        let mut last_path = None;
        for th in &list {
            if last_path != Some(&th.anchor.path) {
                last_path = Some(&th.anchor.path);
                ui.horizontal(|ui| {
                    ui.add_space(t.unit * 2.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(t.icon_size, t.icon_size), Sense::hover());
                    icons::paint(ui, rect, "file-code", t.icon_size - 2.0, t.text_muted);
                    ui.label(RichText::new(&th.anchor.path).font(t.small()).color(t.text_muted));
                });
            }
            let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(ui.cursor().min + vec2(t.unit, 0.0), vec2(width, 4000.0))));
            let r = card(app, &mut child, th, true, &mut actions);
            ui.allocate_space(vec2(width, r.height()));
        }
    });
    apply(app, actions);
}

/// The author's badge: their initial on their colour.
fn badge(ui: &mut egui::Ui, t: &Tokens, author: &Author, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let fill = colour(author);
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    let font = egui::FontId::new(size * 0.5, crate::fonts::semibold());
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, author.initial(), font, ink(t, fill));
}

/// A small text button for card actions (Reply, Cancel).
fn link_button(ui: &mut egui::Ui, t: &Tokens, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).font(t.small()).color(t.text)).frame_when_inactive(false).min_size(vec2(0.0, 24.0)))
}

fn primary(ui: &mut egui::Ui, t: &Tokens, label: &str, enabled: bool) -> egui::Response {
    let text = RichText::new(label).font(t.small()).color(if enabled { t.on_accent } else { t.text_muted });
    let fill = if enabled { t.accent } else { blend(t.frame, t.hover) };
    ui.add_enabled(enabled, egui::Button::new(text).fill(fill).stroke(Stroke::NONE).corner_radius(t.corner()).min_size(vec2(0.0, 24.0)))
}

/// Cmd+Enter in a field that has the keyboard.
fn submitted(ui: &egui::Ui, field: &egui::Response) -> bool {
    field.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
}

/// One message: badge, name, time and text, with a menu of Edit and Delete for one's own.
fn message(app: &App, ui: &mut egui::Ui, th: &Thread, m: &Message, first: bool, actions: &mut Vec<Action>) {
    let t = app.tokens;
    let now = Utc::now();
    let mine = me(app).is_some_and(|a| a.is(&m.author));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0 * t.unit;
        badge(ui, &t, &m.author, if first { 24.0 } else { 20.0 });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(&m.author.name).font(t.strong()).color(t.text));
            let mut when = crate::i18n::ago(m.time, now);
            if m.edited.is_some() {
                when.push_str(tl!(" · edited"));
            }
            ui.label(RichText::new(when).font(t.small()).color(t.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if first {
                if th.is_open() {
                    if widgets::icon_button(ui, &t, "check", tl!("Resolve"), true).clicked() {
                        actions.push(Action::Run("review.resolve", json!({"id": th.id.to_string()})));
                    }
                } else if widgets::icon_button(ui, &t, "rotate-ccw", crate::i18n::tc("comment", "Reopen"), true).clicked() {
                    actions.push(Action::Run("review.reopen", json!({"id": th.id.to_string()})));
                }
            }
            if mine && !m.deleted {
                let more = widgets::icon_button(ui, &t, "ellipsis", tl!("More actions"), true);
                crate::widgets::keyboard_menu(ui, &more);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(150.0);
                    if ui.add(egui::Button::new(tl!("Edit")).frame_when_inactive(false)).clicked() {
                        actions.push(Action::Edit(th.id, m.id, m.text.clone()));
                        ui.close();
                    }
                    let delete = if first && th.author().is_some_and(|a| a.is(&m.author)) { tl!("Delete Thread") } else { tl!("Delete") };
                    if ui.add(egui::Button::new(RichText::new(delete).color(t.error)).frame_when_inactive(false)).clicked() {
                        let params =
                            if first { json!({"id": th.id.to_string()}) } else { json!({"id": th.id.to_string(), "message": m.id.to_string()}) };
                        actions.push(Action::Run("review.delete", params));
                        ui.close();
                    }
                });
            }
        });
    });
    let editing = app.review.editing.as_ref().filter(|(tid, mid, _)| *tid == th.id && *mid == m.id);
    if let Some((_, _, text)) = editing {
        let mut text = text.clone();
        let field = ui
            .add(egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(f32::INFINITY).font(t.body()))
            .accessible_name(tl!("Edit comment"));
        let save = submitted(ui, &field);
        actions.push(Action::Edit(th.id, m.id, text.clone()));
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary(ui, &t, tl!("Save"), !text.trim().is_empty()).clicked() || save {
                    actions.push(Action::Run("review.edit", json!({"id": th.id.to_string(), "message": m.id.to_string(), "text": text})));
                    actions.push(Action::CancelEdit);
                }
                if link_button(ui, &t, tl!("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    actions.push(Action::CancelEdit);
                }
            });
        });
    } else if m.deleted {
        ui.label(RichText::new(tl!("Message deleted")).font(t.small()).italics().color(t.text_muted));
    } else {
        ui.add(egui::Label::new(RichText::new(&m.text).font(t.body()).color(t.text)).wrap());
    }
}

/// A thread's card; returns its rectangle.
fn card(app: &mut App, ui: &mut egui::Ui, th: &Thread, in_panel: bool, actions: &mut Vec<Action>) -> Rect {
    let t = app.tokens;
    let selected = app.review.selected == Some(th.id);
    let author_colour = th.author().map(colour).unwrap_or(t.accent);
    let folded = !th.is_open() && !app.review.unfolded.contains(&th.id) && !selected;
    let builder = UiBuilder::new().sense(Sense::click()).accessibility_role(egui::accesskit::Role::Group).accessibility_label(group_name(th));
    let shadow = if selected { egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: t.shadow } } else { egui::Shadow::NONE };
    let stroke = if selected { Stroke::new(1.5, author_colour) } else { Stroke::new(1.0, blend(t.surface, t.separator)) };
    let frame = egui::Frame::new().fill(t.surface).stroke(stroke).corner_radius(t.corner_large()).inner_margin(egui::Margin::same(10)).shadow(shadow);
    let r = ui.scope_builder(builder, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(1.5 * t.unit, 1.5 * t.unit);
            if folded {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(t.icon_size, t.icon_size), Sense::hover());
                    icons::paint(ui, rect, "circle-check", t.icon_size, t.success);
                    let who = th.state.closed().map(|(by, _)| by.name.clone()).unwrap_or_default();
                    let (summary, text) = match th.state {
                        State::Accepted { .. } => (tl!("Accepted by {name} · {text}"), change_text(th, 30)),
                        State::Rejected { .. } => (tl!("Rejected by {name} · {text}"), change_text(th, 30)),
                        _ => (tl!("Resolved by {name} · {text}"), th.messages.first().map(|m| short(&m.text, 40)).unwrap_or_default()),
                    };
                    ui.add(
                        egui::Label::new(RichText::new(summary.replace("{name}", &who).replace("{text}", &text)).font(t.small()).color(t.text_muted))
                            .truncate(),
                    );
                });
                return;
            }
            if th.is_suggestion() {
                suggestion(app, ui, th, actions);
            }
            if (in_panel && !th.is_suggestion()) || th.anchor.placement != Placement::Anchored {
                let quote = short(&th.anchor.quote.exact, 70);
                let (note, colour) = match th.anchor.placement {
                    Placement::Removed => (tl!("The passage was deleted: “{quote}”").replace("{quote}", &quote), t.warning),
                    Placement::Unanchored => (tl!("Not found in the file: “{quote}”").replace("{quote}", &quote), t.warning),
                    Placement::Anchored => (format!("“{quote}”"), t.text_muted),
                };
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 16.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.5, author_colour);
                    ui.add(egui::Label::new(RichText::new(note).font(egui::FontId::monospace(t.small_size)).color(colour)).wrap());
                });
            }
            // A suggestion's replies come after its change, as a comment's after its first message.
            let first_reply = if th.is_suggestion() { 0 } else { 1 };
            for (i, m) in th.messages.iter().enumerate() {
                if i == first_reply {
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().rect_filled(line, 0.0, blend(t.surface, t.separator));
                }
                message(app, ui, th, m, i == 0 && !th.is_suggestion(), actions);
            }
            if let State::Resolved { by, at } = &th.state {
                let text = tl!("Resolved by {name}, {when}").replace("{name}", &by.name).replace("{when}", &crate::i18n::ago(*at, Utc::now()));
                ui.label(RichText::new(text).font(t.small()).color(t.success));
            }
            if selected && th.is_open() {
                reply_field(app, ui, th, actions);
            }
        });
    });
    let response = r.response;
    app.region(if in_panel { crate::Region::Side } else { crate::Region::Review }, response.id);
    if response.clicked() {
        actions.push(Action::Select(th.id));
        if folded {
            actions.push(Action::Fold(th.id, false));
        }
    }
    if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        actions.push(Action::Select(th.id));
    }
    widgets::focus_ring(ui, &t, &response, response.rect, t.corner_large());
    response.rect
}

/// A suggestion's part of its card: who suggested it, the change in words and as struck and
/// underlined text, whether it is outdated or closed, and Accept and Reject while it is open.
fn suggestion(app: &App, ui: &mut egui::Ui, th: &Thread, actions: &mut Vec<Action>) {
    let t = app.tokens;
    let Some(s) = &th.suggestion else { return };
    let mine = me(app).is_some_and(|a| a.is(&s.author));
    let id = th.id.to_string();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0 * t.unit;
        badge(ui, &t, &s.author, 24.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(&s.author.name).font(t.strong()).color(t.text));
            let when = tl!("suggested {when}").replace("{when}", &crate::i18n::ago(s.edited.unwrap_or(s.time), Utc::now()));
            ui.label(RichText::new(when).font(t.small()).color(t.text_muted));
        });
        if mine && th.is_open() {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let more = widgets::icon_button(ui, &t, "ellipsis", tl!("More actions"), true);
                crate::widgets::keyboard_menu(ui, &more);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(150.0);
                    if ui.add(egui::Button::new(RichText::new(tl!("Withdraw Suggestion")).color(t.error)).frame_when_inactive(false)).clicked() {
                        actions.push(Action::Run("review.delete", json!({"id": id})));
                        ui.close();
                    }
                });
            });
        }
    });
    ui.add(egui::Label::new(RichText::new(change_text(th, 80)).font(t.body()).color(t.text)).wrap());
    // The change itself: the old text struck through, the new underlined, on the author's tint.
    let code = egui::FontId::monospace(t.small_size);
    let tint = colour(&s.author).gamma_multiply(0.16);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = t.unit;
        if !th.anchor.quote.exact.is_empty() {
            ui.label(RichText::new(short(&th.anchor.quote.exact, 80)).font(code.clone()).strikethrough().color(t.text).background_color(tint));
        }
        if !s.replacement.is_empty() {
            ui.label(RichText::new(short(&s.replacement, 80)).font(code.clone()).underline().color(t.text).background_color(tint));
        }
    });
    match &th.state {
        State::Open if s.outdated => {
            let note = tl!("Outdated: the passage changed since. Reject it, or suggest again.");
            ui.add(egui::Label::new(RichText::new(note).font(t.small()).color(t.warning)).wrap());
        }
        State::Accepted { by, at } => {
            let text = tl!("Accepted by {name}, {when}").replace("{name}", &by.name).replace("{when}", &crate::i18n::ago(*at, Utc::now()));
            ui.label(RichText::new(text).font(t.small()).color(t.success));
        }
        State::Rejected { by, at } => {
            let text = tl!("Rejected by {name}, {when}").replace("{name}", &by.name).replace("{when}", &crate::i18n::ago(*at, Utc::now()));
            ui.label(RichText::new(text).font(t.small()).color(t.text_muted));
        }
        _ => {}
    }
    if th.is_open() {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let accept = primary(ui, &t, tl!("Accept"), th.can_accept());
                if accept.on_hover_text(tl!("Make this change in the file (one undoable edit)")).clicked() {
                    actions.push(Action::Run("review.accept", json!({"id": id})));
                }
                if link_button(ui, &t, tl!("Reject")).on_hover_text(tl!("Leave the text as it is")).clicked() {
                    actions.push(Action::Run("review.reject", json!({"id": id})));
                }
            });
        });
    } else if matches!(th.state, State::Rejected { .. })
        && widgets::icon_button(ui, &t, "rotate-ccw", crate::i18n::tc("suggestion", "Reopen"), true).clicked()
    {
        actions.push(Action::Run("review.reopen", json!({"id": id})));
    }
}

fn reply_field(app: &mut App, ui: &mut egui::Ui, th: &Thread, actions: &mut Vec<Action>) {
    let t = app.tokens;
    let mut text = app.review.replies.get(&th.id).cloned().unwrap_or_default();
    let id = ui.make_persistent_id(("reply", th.id.to_string()));
    let field = ui
        .add(egui::TextEdit::multiline(&mut text).id(id).hint_text(tl!("Reply…")).desired_rows(1).desired_width(f32::INFINITY).font(t.body()))
        .accessible_name(tl!("Reply"));
    if app.review.focus_reply == Some(th.id) {
        field.request_focus();
        app.review.focus_reply = None;
    }
    let send = submitted(ui, &field);
    app.review.replies.insert(th.id, text.clone());
    if !text.trim().is_empty() {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary(ui, &t, tl!("Reply"), true).clicked() || send {
                    actions.push(Action::Run("review.reply", json!({"id": th.id.to_string(), "text": text})));
                    actions.push(Action::Fold(th.id, true));
                }
                if link_button(ui, &t, tl!("Cancel")).clicked() {
                    app.review.replies.remove(&th.id);
                }
            });
        });
    }
}

/// The comment being written: the name to comment under first, when the user has none.
fn composer(app: &mut App, ui: &mut egui::Ui, actions: &mut Vec<Action>) -> Rect {
    let t = app.tokens;
    let me = me(app);
    let accent = me.as_ref().map_or(t.accent, colour);
    let frame = egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.5, accent))
        .corner_radius(t.corner_large())
        .inner_margin(egui::Margin::same(10))
        .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: t.shadow });
    let builder = UiBuilder::new().accessibility_role(egui::accesskit::Role::Group).accessibility_label(tl!("New comment"));
    let r = ui.scope_builder(builder, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(2.0 * t.unit, 2.0 * t.unit);
            let Some(c) = app.review.composer.as_mut() else { return };
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            match &me {
                None => {
                    ui.label(RichText::new(tl!("Your name on comments")).font(t.strong()));
                    let field = ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(f32::INFINITY).font(t.body())).accessible_name(tl!("Your name on comments"));
                    if std::mem::take(&mut c.focus) {
                        field.request_focus();
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(tl!("Shown with everything you write in a review. TeXCraft also makes a key that tells your comments from those of someone with the same name; change the name in Settings › Review."))
                                .font(t.small())
                                .color(t.text_muted),
                        )
                        .wrap(),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if primary(ui, &t, tl!("Continue"), !c.name.trim().is_empty()).clicked() || (enter && !c.name.trim().is_empty()) {
                                actions.push(Action::Identify(c.name.clone()));
                            }
                            if link_button(ui, &t, tl!("Cancel")).clicked() || escape {
                                actions.push(Action::CancelComposer);
                            }
                        });
                    });
                }
                Some(author) => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0 * t.unit;
                        badge(ui, &t, author, 24.0);
                        ui.label(RichText::new(&author.name).font(t.strong()));
                    });
                    if !c.quote.is_empty() {
                        ui.horizontal(|ui| {
                            let (bar, _) = ui.allocate_exact_size(vec2(3.0, 16.0), Sense::hover());
                            ui.painter().rect_filled(bar, 1.5, accent);
                            ui.add(egui::Label::new(RichText::new(short(&c.quote, 90)).font(egui::FontId::monospace(t.small_size)).color(t.text_muted)).wrap());
                        });
                    }
                    let id = ui.make_persistent_id("review-composer");
                    let field = ui.add(
                        egui::TextEdit::multiline(&mut c.text).id(id).hint_text(tl!("Add a comment…")).desired_rows(3).desired_width(f32::INFINITY).font(t.body()),
                    ).accessible_name(crate::i18n::tc("noun", "Comment"));
                    if std::mem::take(&mut c.focus) {
                        field.request_focus();
                    }
                    let post = submitted(ui, &field);
                    let ready = !c.text.trim().is_empty();
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if primary(ui, &t, tl!("Comment"), ready).clicked() || (post && ready) {
                                actions.push(Action::Post);
                            }
                            if link_button(ui, &t, tl!("Cancel")).clicked() || escape {
                                actions.push(Action::CancelComposer);
                            }
                        });
                    });
                }
            }
        });
    });
    r.response.rect
}

/// Does what the cards asked for.
fn apply(app: &mut App, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Select(id) => {
                if let Some(th) = app.session.review().get(id).cloned() {
                    select(app, &th);
                    app.review.focus_reply = Some(id);
                }
            }
            Action::Run(command, params) => {
                let announce = match command {
                    "review.resolve" => Some(tl!("Comment resolved")),
                    "review.reopen" => Some(tl!("Reopened")),
                    "review.accept" => Some(tl!("Suggestion accepted")),
                    "review.reject" => Some(tl!("Suggestion rejected")),
                    "review.reply" => Some(tl!("Reply added")),
                    "review.delete" => Some(crate::i18n::tc("comment", "Deleted")),
                    _ => None,
                };
                if app.run(command, params.clone()).is_some() {
                    if let Some(text) = announce {
                        app.view.status = Some((text.to_owned(), false));
                    }
                    if command == "review.reply"
                        && let Some(id) = params.get("id").and_then(serde_json::Value::as_str).and_then(Id::parse)
                    {
                        app.review.replies.remove(&id);
                    }
                    if command == "review.resolve" {
                        app.review.selected = None;
                    }
                }
            }
            Action::Fold(id, fold) => {
                if fold {
                    app.review.unfolded.remove(&id);
                } else {
                    app.review.unfolded.insert(id);
                }
            }
            Action::Edit(thread, message, text) => app.review.editing = Some((thread, message, text)),
            Action::CancelEdit => app.review.editing = None,
            Action::CancelComposer => {
                app.review.composer = None;
                app.editor.focus = true;
            }
            Action::Identify(name) => match crate::prefs::Identity::new(&name) {
                Ok(identity) => {
                    app.session.set_author(Some(identity.author()));
                    app.prefs.identity = Some(identity);
                    app.prefs_changed = true;
                    if let Some(c) = app.review.composer.as_mut() {
                        c.focus = true;
                    }
                }
                Err(e) => app.view.status = Some((e, true)),
            },
            Action::Post => {
                let Some(c) = app.review.composer.take() else { continue };
                let params = json!({"path": c.path.as_str(), "start": c.start, "end": c.end, "text": c.text});
                match app.run("review.comment", params) {
                    Some(r) => {
                        app.review.selected = r.get("id").and_then(serde_json::Value::as_str).and_then(Id::parse);
                        app.review.caret = None;
                        app.view.status = Some((tl!("Comment added").to_owned(), false));
                        app.editor.focus = true;
                    }
                    None => app.review.composer = Some(c),
                }
            }
        }
    }
}

/// The status bar's count of open threads, when there are any; clicking it opens the Review panel.
pub(crate) fn status(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let open = app.session.review().threads().iter().filter(|th| th.is_open() && !th.is_suggestion()).count();
    let suggested = app.session.review().threads().iter().filter(|th| th.is_open() && th.is_suggestion()).count();
    if open + suggested == 0 {
        return;
    }
    let text = match (open, suggested) {
        (_, 0) => crate::i18n::trn(open, "{n} comment", "{n} comments"),
        (0, _) => crate::i18n::trn(suggested, "{n} suggestion", "{n} suggestions"),
        _ => format!(
            "{} · {}",
            crate::i18n::trn(open, "{n} comment", "{n} comments"),
            crate::i18n::trn(suggested, "{n} suggestion", "{n} suggestions")
        ),
    };
    let r = ui.add(
        egui::Button::image_and_text(
            icons::image("message-square-text", 13.0, t.text_muted),
            RichText::new(&text).font(t.small()).color(t.text_muted),
        )
        .frame_when_inactive(false),
    );
    if r.clicked() {
        app.view.side = Some(crate::Side::Review);
    }
}

/// The open threads in `path`, for the file tree.
pub(crate) fn open_in(app: &App, path: &ProjectPath) -> usize {
    app.session.review().threads().iter().filter(|th| th.is_open() && th.anchor.path == path.as_str()).count()
}
