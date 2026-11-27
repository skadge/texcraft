//! The find and replace bar (docs/ROADMAP.md), above the source: the text to find
//! with Match Case, Regular Expression, Whole Word and In Selection, the count and the current
//! match, Previous and Next, and with Replace open, Replace and Replace All.
//!
//! Matching is the engine's ([`texcraft_engine::find::Query`]), as `file.find` and `file.replace`
//! use it. Replacing edits the editor's buffer, so Undo takes it back: Replace All in one step.
//! Enter goes to the next match and Shift+Enter to the previous one; Escape closes the bar and
//! gives the keyboard back to the editor.

use egui::{RichText, vec2};
use texcraft_buffer::{Change, EditKind, Selection, Transaction};
use texcraft_engine::find::{MAX_MATCHES, Query};

use crate::source::SourceEditor;
use crate::theme::Tokens;
use crate::{App, widgets};

/// A span of chars.
type Span = (usize, usize);

/// What a list of matches was found for: the text's version, the query and the range searched.
type Searched = (u64, Query, Option<Span>);

#[derive(Default)]
pub(crate) struct FindBar {
    pub query: Query,
    pub replacement: String,
    /// The replace row is shown.
    pub replace: bool,
    /// Only matches inside these chars.
    pub within: Option<(usize, usize)>,
    /// The matches, chars, and which one is current.
    pub matches: Vec<(usize, usize)>,
    /// In the formatted view, the source runs of each match (bytes): more than one when it goes
    /// over a style's boundary (`\textbf{bold} text`), whose markup a replacement keeps.
    pieces: Vec<Vec<std::ops::Range<usize>>>,
    pub current: Option<usize>,
    pub error: Option<String>,
    /// What `matches` was computed for.
    computed: Option<Searched>,
    /// Where the caret was when the bar opened: typing finds from there.
    origin: usize,
    /// Give the keyboard to the find field (and select its text) next frame.
    pub focus_find: bool,
    focus_replace: bool,
    /// In the formatted view: its lines as last drawn, with the buffer's version they show. Only
    /// matches in text the view shows are found there (not in commands' names), and a replacement
    /// is text, escaped as typing escapes it.
    pub view: Option<(u64, texcraft_latex::edit::Doc)>,
    /// The find field changed while the view's lines were of an older text: go to the first match
    /// once they are found.
    go_to_first: bool,
}

const FIND_ID: &str = "texcraft-find-field";
const REPLACE_ID: &str = "texcraft-replace-field";

impl FindBar {
    /// Opens on the editor's selection: its text to find when it is on one line.
    pub fn open(source: &SourceEditor, replace: bool, old: Option<FindBar>) -> FindBar {
        let mut bar = old.unwrap_or_default();
        let r = source.doc.selection.primary();
        let selected = source.doc.buffer.slice(r.start(), r.end());
        if !selected.is_empty() && !selected.contains('\n') {
            bar.query.pattern = if bar.query.regex { texcraft_engine::find::escape(&selected) } else { selected };
        }
        bar.replace |= replace;
        bar.origin = r.start();
        bar.focus_find = !replace || bar.query.pattern.is_empty();
        bar.focus_replace = replace && !bar.focus_find;
        bar.computed = None;
        bar
    }

    /// Finds the matches again when the text or the query changed; true when they changed.
    pub fn refresh(&mut self, source: &SourceEditor) -> bool {
        let b = &source.doc.buffer;
        let key = (b.version(), self.query.clone(), self.within);
        if self.computed.as_ref() == Some(&key) {
            return false;
        }
        // The formatted view's lines are of an older text: found again once it shows this one.
        let doc = match &self.view {
            Some((version, doc)) if *version == b.version() => Some(doc),
            Some(_) => {
                self.matches.clear();
                self.current = None;
                return true;
            }
            None => None,
        };
        self.computed = Some(key);
        self.matches.clear();
        self.pieces.clear();
        self.error = None;
        if self.query.pattern.is_empty() {
            self.current = None;
            return true;
        }
        let text = b.text();
        if let Some(doc) = doc {
            // What the view shows, line by line: a match may go over a style's boundary.
            for (shown, sources) in doc.shown_text(&text) {
                let ranges = match self.query.matches(&shown) {
                    Ok(r) => r,
                    Err(e) => {
                        self.error = Some(e);
                        break;
                    }
                };
                for r in ranges {
                    let (first, last) = (shown[..r.start].chars().count(), shown[..r.end].chars().count());
                    let runs: Vec<std::ops::Range<usize>> = sources.get(first..last).unwrap_or_default().to_vec();
                    let (Some(start), Some(end)) = (runs.first().map(|r| r.start), runs.last().map(|r| r.end)) else { continue };
                    let (s, e) = (b.byte_to_char(start), b.byte_to_char(end));
                    if self.within.is_some_and(|(ws, we)| s < ws || e > we) || self.matches.len() >= MAX_MATCHES {
                        continue;
                    }
                    let mut pieces: Vec<std::ops::Range<usize>> = Vec::new();
                    for run in runs {
                        match pieces.last_mut() {
                            Some(p) if p.end == run.start => p.end = run.end,
                            _ => pieces.push(run),
                        }
                    }
                    self.matches.push((s, e));
                    self.pieces.push(pieces);
                }
            }
            self.current = self.current.filter(|i| *i < self.matches.len());
            return true;
        }
        match self.query.matches(&text) {
            Ok(ranges) => {
                self.matches = ranges
                    .into_iter()
                    .map(|r| (b.byte_to_char(r.start), b.byte_to_char(r.end)))
                    .filter(|(s, e)| self.within.is_none_or(|(ws, we)| *s >= ws && *e <= we))
                    .collect();
            }
            Err(e) => self.error = Some(e),
        }
        self.current = self.current.filter(|i| *i < self.matches.len());
        true
    }

    /// The first match at or after `pos`, wrapping.
    fn from(&self, pos: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        Some(self.matches.iter().position(|(s, _)| *s >= pos).unwrap_or(0))
    }

    fn step(&mut self, source: &SourceEditor, forward: bool) -> Option<usize> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        let next = match (self.current, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => self.from(source.caret() + 1)?,
            (None, false) => self.matches.iter().rposition(|(s, _)| *s < source.caret()).unwrap_or(n - 1),
        };
        self.current = Some(next);
        Some(next)
    }

    fn count_text(&self) -> String {
        match (self.current, self.matches.len()) {
            (_, 0) if self.query.pattern.is_empty() => String::new(),
            (_, 0) => tl!("No results").to_owned(),
            (Some(i), n) if n >= MAX_MATCHES => tl!("{i} of {n}+").replace("{i}", &(i + 1).to_string()).replace("{n}", &n.to_string()),
            (Some(i), n) => tl!("{i} of {n}").replace("{i}", &(i + 1).to_string()).replace("{n}", &n.to_string()),
            (None, n) => crate::i18n::trn(n, "{n} result", "{n} results"),
        }
    }
}

/// The replacement as source: in the formatted view (and not a regular expression's, which may
/// name its groups), the text escaped as typing escapes it.
fn replacement(bar: &FindBar) -> String {
    if bar.view.is_some() && !bar.query.regex { texcraft_latex::edit::escape(&bar.replacement) } else { bar.replacement.clone() }
}

/// Shows the current match: selected, or with Vim the caret on its start.
fn show_match(source: &mut SourceEditor, (s, e): (usize, usize)) {
    if source.vim.is_some() {
        source.select(s, s);
    } else {
        source.select(s, e);
    }
}

/// The changes replacing a match (chars `s..e`) with `with`: in the formatted view, a match over a
/// style's boundary has its first run replaced and the others deleted, the markup between kept.
fn match_changes(b: &texcraft_buffer::Buffer, pieces: Option<&Vec<std::ops::Range<usize>>>, (s, e): (usize, usize), with: String) -> Vec<Change> {
    match pieces.filter(|p| p.len() > 1) {
        Some(pieces) => pieces
            .iter()
            .enumerate()
            .map(|(k, r)| Change {
                start: b.byte_to_char(r.start),
                end: b.byte_to_char(r.end),
                text: if k == 0 { with.clone() } else { String::new() },
            })
            .collect(),
        None => vec![Change { start: s, end: e, text: with }],
    }
}

/// Replaces the current match, then goes to the next one.
fn replace_one(bar: &mut FindBar, source: &mut SourceEditor) {
    let Some((s, e)) = bar.current.and_then(|i| bar.matches.get(i).copied()) else {
        if let Some(i) = bar.step(source, true) {
            show_match(source, bar.matches[i]);
        }
        return;
    };
    let b = &source.doc.buffer;
    let text = b.text();
    let (bs, be) = (b.char_to_byte(s), b.char_to_byte(e));
    let with = match bar.query.replacement_for(&text, bs..be, &replacement(bar)) {
        Ok(w) => w,
        Err(err) => {
            bar.error = Some(err);
            return;
        }
    };
    let before = source.doc.selection.clone();
    let i = bar.current.unwrap_or(0);
    let changes = match_changes(b, bar.pieces.get(i), (s, e), with.clone());
    let end = changes.first().map_or(s, |c| c.start) + with.chars().count();
    let Ok(tx) = Transaction::new(changes) else { return };
    if source.doc.buffer.edit(tx, &before, &Selection::point(end), EditKind::Other).is_ok() {
        source.doc.buffer.seal();
        source.doc.selection = Selection::point(end);
        bar.refresh(source);
        bar.current = None;
        if let Some(i) = bar.from(end) {
            bar.current = Some(i);
            show_match(source, bar.matches[i]);
        }
    }
}

/// Replaces every match as one undoable edit; returns how many.
fn replace_all(bar: &mut FindBar, source: &mut SourceEditor) -> usize {
    let b = &source.doc.buffer;
    let text = b.text();
    let mut changes = Vec::with_capacity(bar.matches.len());
    for (i, (s, e)) in bar.matches.iter().enumerate() {
        match bar.query.replacement_for(&text, b.char_to_byte(*s)..b.char_to_byte(*e), &replacement(bar)) {
            Ok(with) => changes.extend(match_changes(b, bar.pieces.get(i), (*s, *e), with)),
            Err(err) => {
                bar.error = Some(err);
                return 0;
            }
        }
    }
    let n = changes.len();
    let Ok(tx) = Transaction::new(changes) else { return 0 };
    let before = source.doc.selection.clone();
    let after = before.map(&tx);
    if n > 0 && source.doc.buffer.edit(tx, &before, &after, EditKind::Other).is_ok() {
        source.doc.buffer.seal();
        source.doc.selection = after;
        bar.current = None;
        return n;
    }
    0
}

/// A toggle of the bar; returns whether it was clicked.
pub(crate) fn toggle(ui: &mut egui::Ui, t: &Tokens, text: &str, name: &str, on: bool) -> bool {
    let fill = if on { t.accent_soft } else { egui::Color32::TRANSPARENT };
    let stroke = if on { egui::Stroke::new(1.0, t.accent) } else { egui::Stroke::NONE };
    let button = egui::Button::new(RichText::new(text).font(egui::FontId::monospace(12.0)).color(if on { t.accent } else { t.text_muted }))
        .fill(fill)
        .stroke(stroke)
        .min_size(vec2(26.0, 24.0));
    let r = ui.add(button);
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, name));
    let clicked = r.clicked();
    let _ = r.on_hover_text(name);
    clicked
}

/// Match Case, Regular Expression, Whole Word and In Selection.
fn toggles(ui: &mut egui::Ui, t: &Tokens, bar: &mut FindBar, source: &SourceEditor) {
    let mut changed = false;
    if toggle(ui, t, "Aa", tl!("Match Case"), bar.query.case) {
        bar.query.case = !bar.query.case;
        changed = true;
    }
    if toggle(ui, t, ".*", tl!("Regular Expression"), bar.query.regex) {
        bar.query.regex = !bar.query.regex;
        changed = true;
    }
    if toggle(ui, t, "W", tl!("Whole Word"), bar.query.word) {
        bar.query.word = !bar.query.word;
        changed = true;
    }
    if toggle(ui, t, "[ ]", tl!("In Selection"), bar.within.is_some()) {
        let r = source.doc.selection.primary();
        bar.within = if bar.within.is_some() || r.is_empty() { None } else { Some((r.start(), r.end())) };
        changed = true;
    }
    if changed {
        bar.current = None;
    }
}

/// The bar, when it is open.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(mut bar) = app.editor.find.take() else { return };
    let formatted = app.session.current().is_some_and(|p| app.editor.formatted.shows(p));
    bar.view = formatted.then(|| app.editor.formatted.editing.shown.clone()).flatten();
    if formatted && bar.view.is_none() {
        // Not drawn yet: an older text's version, found again next frame.
        bar.view = Some((u64::MAX, texcraft_latex::edit::Doc::default()));
    }
    let shown_before = app.editor.source.doc.selection.clone();
    let t = app.tokens;
    let source = &mut app.editor.source;
    let mut close = false;
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let shift = ui.input(|i| i.modifiers.shift);
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let mut replaced = None;
    let frame = egui::Frame::new().fill(t.raised).inner_margin(egui::Margin::symmetric(10, 6));
    // The fields take what the toggles, the count and the buttons leave.
    // (The pane may be wider than what is shown of it: measure what is visible.)
    let visible = ui.available_width().min(ui.clip_rect().right() - ui.cursor().left());
    // Narrow, the toggles take a row of their own.
    let narrow = visible < 520.0;
    let field_width = (visible - 20.0 - if narrow { 220.0 } else { 360.0 }).clamp(90.0, 360.0);
    frame.show(ui, |ui| {
        ui.set_width(visible - 20.0);
        ui.spacing_mut().item_spacing = vec2(t.unit * 1.5, t.unit * 1.5);
        ui.horizontal(|ui| {
            let chevron = if bar.replace { "chevron-down" } else { "chevron-right" };
            let name = if bar.replace { tl!("Hide Replace") } else { tl!("Show Replace") };
            if widgets::icon_button(ui, &t, chevron, name, true).clicked() {
                bar.replace = !bar.replace;
                bar.focus_replace = bar.replace;
            }
            let id = egui::Id::unique(FIND_ID);
            let field =
                egui::TextEdit::singleline(&mut bar.query.pattern).id(id).hint_text(tl!("Find")).desired_width(field_width).font(t.code_font());
            let output = field.show(ui);
            let r = output.response.response.clone();
            let _ = r.clone().accessible_name(tl!("Find"));
            if std::mem::take(&mut bar.focus_find) {
                r.request_focus();
                let mut state = output.state.clone();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(bar.query.pattern.chars().count()),
                )));
                state.store(ui.ctx(), id);
            }
            if r.changed() {
                bar.current = None;
                bar.refresh(source);
                bar.go_to_first = bar.computed.is_none();
                if let Some(i) = bar.from(bar.origin) {
                    bar.current = Some(i);
                    show_match(source, bar.matches[i]);
                }
            }
            if r.lost_focus() && enter {
                bar.refresh(source);
                if let Some(i) = bar.step(source, !shift) {
                    show_match(source, bar.matches[i]);
                }
                r.request_focus();
            }
            if r.lost_focus() && escape {
                close = true;
            }
            if !narrow {
                toggles(ui, &t, &mut bar, source);
            }
            let count = bar.count_text();
            let color = if bar.error.is_some() { t.error } else { t.text_muted };
            let shown = bar.error.as_deref().map_or(count, crate::i18n::msg);
            let label = ui.add(egui::Label::new(RichText::new(&shown).font(t.small()).color(color)).truncate());
            crate::status::announce(ui, &label, &shown);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, &t, "x", tl!("Close Find"), true).clicked() {
                    close = true;
                }
                let any = !bar.matches.is_empty();
                if widgets::icon_button(ui, &t, "chevron-down", tl!("Next Match"), any).clicked()
                    && let Some(i) = bar.step(source, true)
                {
                    show_match(source, bar.matches[i]);
                }
                if widgets::icon_button(ui, &t, "chevron-up", tl!("Previous Match"), any).clicked()
                    && let Some(i) = bar.step(source, false)
                {
                    show_match(source, bar.matches[i]);
                }
            });
        });
        if narrow {
            ui.horizontal(|ui| {
                ui.add_space(24.0 + t.unit * 1.5);
                toggles(ui, &t, &mut bar, source);
            });
        }
        if bar.replace {
            ui.horizontal(|ui| {
                ui.add_space(24.0 + t.unit * 1.5);
                let id = egui::Id::unique(REPLACE_ID);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut bar.replacement)
                        .id(id)
                        .hint_text(tl!("Replace with"))
                        .desired_width(field_width)
                        .font(t.code_font()),
                );
                let _ = r.clone().accessible_name(tl!("Replace with"));
                if std::mem::take(&mut bar.focus_replace) {
                    r.request_focus();
                }
                if r.lost_focus() && enter {
                    replace_one(&mut bar, source);
                    r.request_focus();
                }
                if r.lost_focus() && escape {
                    close = true;
                }
                let any = !bar.matches.is_empty();
                if ui.add_enabled(any, egui::Button::new(tl!("Replace"))).clicked() {
                    replace_one(&mut bar, source);
                }
                if ui.add_enabled(any, egui::Button::new(tl!("Replace All"))).clicked() {
                    replaced = Some(replace_all(&mut bar, source));
                }
            });
        }
    });
    widgets::hairline(ui, &t);
    if bar.refresh(source)
        && bar.computed.is_some()
        && std::mem::take(&mut bar.go_to_first)
        && let Some(i) = bar.from(bar.origin)
    {
        bar.current = Some(i);
        show_match(source, bar.matches[i]);
    }
    if let Some(n) = replaced {
        app.view.status = Some((crate::i18n::trn(n, "Replaced {n} match", "Replaced {n} matches"), false));
    }
    if formatted && app.editor.source.doc.selection != shown_before {
        // The formatted view shows the match the bar went to.
        app.editor.formatted.editing.reveal = true;
    }
    if close {
        app.editor.source.matches.clear();
        app.editor.source.current_match = None;
        app.editor.focus = true;
        app.editor.formatted.editing.focus = formatted;
        return;
    }
    app.editor.source.matches = bar.matches.clone();
    app.editor.source.current_match = bar.current.and_then(|i| bar.matches.get(i).copied());
    app.editor.find = Some(bar);
}
