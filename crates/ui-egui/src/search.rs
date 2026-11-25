//! The side panel's Search (docs/ROADMAP.md, E10): the text to find in every text file of the
//! project, with Match Case, Regular Expression and Whole Word, the matches grouped by file, and
//! Replace All across the files after a confirmation that says how many.
//!
//! Finding is `project.find` and replacing `project.replace`, with the same matching as the find
//! bar. Choosing a match (a click, or Enter) opens its file at its line.

use std::collections::HashSet;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Layout, RichText, Sense, vec2};
use serde_json::{Value, json};
use texcraft_engine::find::Query;
use texcraft_engine::project::ProjectPath;

use crate::dialogs::{Confirm, Dialog};
use crate::i18n::trn;
use crate::theme::Tokens;
use crate::widgets::{self, Row};
use crate::{App, find_bar};

/// The Search section's state: what is looked for, and what was found for it.
#[derive(Default)]
pub(crate) struct Search {
    pub query: Query,
    pub replacement: String,
    /// The replace row is shown.
    pub replace: bool,
    /// What `project.find` returned, and for which session revision and query.
    found: Option<Found>,
    computed: Option<(u64, Query)>,
    error: Option<String>,
    /// Files whose matches are folded away.
    collapsed: HashSet<String>,
    /// Give the keyboard to the query field (and select its text) next frame.
    pub focus: bool,
}

/// One match: its line (from 1), its text, and its line's text around it.
struct Match {
    line: u32,
    text: String,
    before: String,
    after: String,
}

struct FileMatches {
    path: String,
    matches: Vec<Match>,
}

#[derive(Default)]
struct Found {
    files: Vec<FileMatches>,
    count: usize,
    truncated: bool,
}

fn parse(v: &Value) -> Found {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_owned();
    let files: Vec<FileMatches> = v["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .map(|f| FileMatches {
                    path: s(f, "path"),
                    matches: f["matches"]
                        .as_array()
                        .map(|ms| {
                            ms.iter()
                                .map(|m| Match {
                                    line: m["line"].as_u64().and_then(|l| u32::try_from(l).ok()).unwrap_or(1),
                                    text: s(m, "text"),
                                    before: s(m, "before"),
                                    after: s(m, "after"),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let count = files.iter().map(|f| f.matches.len()).sum();
    Found { files, count, truncated: v["truncated"].as_bool().unwrap_or(false) }
}

impl Search {
    /// Finds again when the project or the query changed.
    fn refresh(&mut self, session: &mut texcraft_engine::Session) {
        let key = (session.revision(), self.query.clone());
        if self.computed.as_ref() == Some(&key) {
            return;
        }
        self.computed = Some(key);
        self.error = None;
        self.found = None;
        if self.query.pattern.is_empty() {
            return;
        }
        match session.execute("project.find", &self.params()) {
            Ok(v) => self.found = Some(parse(&v)),
            Err(e) => self.error = Some(e),
        }
    }

    fn params(&self) -> Value {
        let q = &self.query;
        json!({"pattern": q.pattern, "regex": q.regex, "case": q.case, "word": q.word})
    }
}

/// "3 matches in 2 files", for the summary and the confirmation.
fn matches_in_files(found: &Found) -> (String, String) {
    let matches =
        if found.truncated { tl!("{n}+ matches").replace("{n}", &found.count.to_string()) } else { trn(found.count, "{n} match", "{n} matches") };
    (matches, trn(found.files.len(), "{n} file", "{n} files"))
}

/// Opens Search with the keyboard in its field, on the editor's selection when it is on one line.
pub(crate) fn open(app: &mut App) {
    app.view.side = Some(crate::Side::Search);
    if let Some(selected) = app.editor.source.selected().filter(|s| !s.contains('\n')) {
        let search = &mut app.search;
        search.query.pattern = if search.query.regex { texcraft_engine::find::escape(&selected) } else { selected };
    }
    app.search.focus = true;
}

enum Action {
    Open(String, u32),
    Toggle(String),
    ReplaceAll,
}

pub(crate) fn panel(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut search = std::mem::take(&mut app.search);
    ui.spacing_mut().item_spacing = vec2(1.5 * t.unit, t.unit);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, &t, tl!("Search"));
    });
    let mut action = None;
    let mut to_results = false;
    let width = ui.available_width() - 4.0 * t.unit;
    ui.horizontal(|ui| {
        ui.add_space(t.unit);
        let chevron = if search.replace { "chevron-down" } else { "chevron-right" };
        let name = if search.replace { tl!("Hide Replace") } else { tl!("Show Replace") };
        if widgets::icon_button(ui, &t, chevron, name, true).clicked() {
            search.replace = !search.replace;
        }
        let id = egui::Id::unique("texcraft-search-field");
        let field_width = (width - 24.0 - t.unit * 1.5).max(60.0);
        let output = egui::TextEdit::singleline(&mut search.query.pattern)
            .id(id)
            .hint_text(tl!("Search in the project"))
            .desired_width(field_width)
            .font(t.code_font())
            .show(ui);
        let r = output.response.response.clone();
        app.region(crate::Region::Side, r.id);
        let _ = r.clone().accessible_name(tl!("Search in the project"));
        if std::mem::take(&mut search.focus) {
            r.request_focus();
            let mut state = output.state.clone();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(search.query.pattern.chars().count()),
            )));
            state.store(ui.ctx(), id);
        }
        if r.lost_focus() {
            // Enter goes to the results; Escape back to the editor.
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                to_results = true;
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.editor.focus = true;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(t.unit + 24.0 + t.unit * 1.5);
        if find_bar::toggle(ui, &t, "Aa", tl!("Match Case"), search.query.case) {
            search.query.case = !search.query.case;
        }
        if find_bar::toggle(ui, &t, ".*", tl!("Regular Expression"), search.query.regex) {
            search.query.regex = !search.query.regex;
        }
        if find_bar::toggle(ui, &t, "W", tl!("Whole Word"), search.query.word) {
            search.query.word = !search.query.word;
        }
    });
    search.refresh(&mut app.session);
    let any = search.found.as_ref().is_some_and(|f| f.count > 0);
    if search.replace {
        ui.horizontal(|ui| {
            ui.add_space(t.unit + 24.0 + t.unit * 1.5);
            let r = ui.add(
                egui::TextEdit::singleline(&mut search.replacement)
                    .id(egui::Id::unique("texcraft-search-replace"))
                    .hint_text(tl!("Replace with"))
                    .desired_width((width - 24.0 - t.unit * 1.5).max(60.0))
                    .font(t.code_font()),
            );
            let _ = r.clone().accessible_name(tl!("Replace with"));
        });
        ui.horizontal(|ui| {
            ui.add_space(t.unit + 24.0 + t.unit * 1.5);
            if ui.add_enabled(any, egui::Button::new(tl!("Replace All"))).clicked() {
                action = Some(Action::ReplaceAll);
            }
        });
    }
    // How many, said politely when it changes.
    let summary = match (&search.error, &search.found) {
        (Some(e), _) => Some((crate::i18n::msg(e), t.error)),
        (None, Some(f)) if f.count == 0 => Some((tl!("No results").to_owned(), t.text_muted)),
        (None, Some(f)) => {
            let (m, n) = matches_in_files(f);
            Some((tl!("{matches} in {files}").replace("{matches}", &m).replace("{files}", &n), t.text_muted))
        }
        (None, None) => None,
    };
    if let Some((text, color)) = summary {
        ui.horizontal(|ui| {
            ui.add_space(t.unit * 2.0);
            let label = ui.add(egui::Label::new(RichText::new(&text).font(t.small()).color(color)).wrap());
            crate::status::announce(ui, &label, &text);
        });
    }
    ui.add_space(t.unit);
    widgets::hairline(ui, &t);
    if let Some(found) = &search.found {
        results(ui, &t, found, &search.collapsed, to_results, &mut action);
    }
    match action {
        Some(Action::Open(path, line)) => {
            // The key that chose the match must not reach the editor, which takes the keyboard now.
            ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            });
            if let Ok(path) = ProjectPath::new(&path)
                && app.run("file.open", json!({"path": path.as_str()})).is_some()
            {
                app.view.goto = Some((path, line));
            }
        }
        Some(Action::Toggle(path)) if !search.collapsed.remove(&path) => {
            search.collapsed.insert(path);
        }
        Some(Action::ReplaceAll) => {
            if let Some(found) = &search.found {
                let (m, n) = matches_in_files(found);
                let mut params = search.params();
                params["replacement"] = json!(search.replacement);
                params["files"] = json!(found.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>());
                app.view.dialog = Some(Dialog::Confirm(Confirm {
                    title: tl!("Replace {matches} in {files}?").replace("{matches}", &m).replace("{files}", &n),
                    message: tl!("In a file open in the editor, Undo takes back its replacements in one step.").to_owned(),
                    action: tl!("Replace All"),
                    command: "project.replace",
                    params,
                }));
            }
        }
        Some(Action::Toggle(_)) | None => {}
    }
    app.search = search;
}

/// One entry of the list: a file's heading, or one of its matches.
enum Item<'a> {
    File(&'a FileMatches),
    Match(&'a str, &'a Match),
}

fn results(ui: &mut egui::Ui, t: &Tokens, found: &Found, collapsed: &HashSet<String>, focus_first: bool, action: &mut Option<Action>) {
    let mut items = Vec::new();
    for f in &found.files {
        items.push(Item::File(f));
        if !collapsed.contains(&f.path) {
            items.extend(f.matches.iter().map(|m| Item::Match(&f.path, m)));
        }
    }
    let first = items.iter().position(|i| matches!(i, Item::Match(..)));
    egui::ScrollArea::vertical().id_salt("search-results").auto_shrink([false, false]).show_rows(ui, t.row_height, items.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let r = match items.get(i) {
                Some(Item::File(f)) => {
                    let open = !collapsed.contains(&f.path);
                    let count = f.matches.len().to_string();
                    let row = Row {
                        depth: 0,
                        icon: "file-text",
                        label: &f.path,
                        selected: false,
                        disclosure: Some(open),
                        tag: Some(&count),
                        icon_color: None,
                    };
                    let r = widgets::row(ui, t, &row);
                    let said = trn(f.matches.len(), "{n} match", "{n} matches");
                    ui.ctx().accesskit_node_builder(r.id, |node| {
                        node.set_description(said);
                        node.set_expanded(open);
                    });
                    if r.clicked() {
                        *action = Some(Action::Toggle(f.path.clone()));
                    }
                    r
                }
                Some(Item::Match(path, m)) => {
                    let r = match_row(ui, t, m);
                    if r.clicked() {
                        *action = Some(Action::Open((*path).to_owned(), m.line));
                    }
                    r
                }
                None => continue,
            };
            if focus_first && Some(i) == first {
                r.request_focus();
            }
        }
    });
}

/// At most this many characters of the line are shown before a match.
const LEAD: usize = 16;

/// A match: its line number, then its line with the match marked (a fill and an underline).
fn match_row(ui: &mut egui::Ui, t: &Tokens, m: &Match) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), t.row_height), Sense::click());
    let line = m.line.to_string();
    let name = format!("{line} {}{}{}", m.before.trim_start(), m.text, m.after);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TreeItem, true, &name));
    if !ui.is_rect_visible(rect) {
        return r;
    }
    let fill = widgets::state_fill(t, &r, t.frame);
    if fill != t.frame {
        ui.painter().rect_filled(rect.shrink2(vec2(t.unit, 0.0)), t.corner(), fill);
    }
    let x = rect.left() + 6.0 * t.unit;
    let number = ui.painter().layout_no_wrap(line, t.small(), t.text_muted);
    let number_width = number.size().x.max(3.0 * t.unit * 2.0);
    ui.painter().galley(egui::pos2(x, rect.center().y - number.size().y / 2.0), number, t.text_muted);
    let mut job = LayoutJob::default();
    let font = t.code_font();
    let plain = TextFormat { font_id: font.clone(), color: t.text_muted, ..Default::default() };
    let marked =
        TextFormat { font_id: font, color: t.text, background: t.search_match, underline: egui::Stroke::new(1.0, t.text), ..Default::default() };
    // A short lead, so the match shows in a narrow panel.
    let before = m.before.trim_start();
    let before = match before.char_indices().rev().nth(LEAD - 1) {
        Some((i, _)) if i > 0 => format!("…{}", &before[i..]),
        _ => before.to_owned(),
    };
    job.append(&before, 0.0, plain.clone());
    job.append(&m.text.replace('\n', "⏎"), 0.0, marked);
    job.append(&m.after, 0.0, plain);
    let left = x + number_width + 2.0 * t.unit;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - left - t.unit).max(0.0));
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(left, rect.center().y - galley.size().y / 2.0), galley, t.text);
    widgets::focus_ring(ui, t, &r, rect.shrink2(vec2(t.unit, 0.0)), t.corner());
    r
}
