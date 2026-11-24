//! Go to File (docs/ROADMAP.md, P7): a palette of the project's files. A typed query keeps the
//! paths that hold its letters in order, best matches first, with the matched letters marked;
//! with no query the files opened most recently in this session come first, then the others by
//! path. Up and Down move the selection, Enter or a click opens the file through `file.open`,
//! Escape closes. The palette traps focus like the other dialogs, and the selected file and the
//! count of matches are announced through a live region (standards/accessibility.md).

use egui::text::{LayoutJob, TextFormat};
use egui::{Key, Modifiers, RichText};
use serde_json::json;
use texcraft_engine::project::ProjectPath;

use crate::App;
use crate::dialogs::Dialog;

/// How many recently opened files the palette remembers.
const RECENT: usize = 10;

/// The palette's state while it is open.
pub(crate) struct Palette {
    query: String,
    selected: usize,
    focus: bool,
    /// The selection was moved by a key: scroll it into view.
    scroll: bool,
}

/// Opens the palette.
pub(crate) fn open(app: &mut App) {
    app.view.dialog = Some(Dialog::GoToFile(Palette { query: String::new(), selected: 0, focus: true, scroll: false }));
}

/// Puts `path` first in the list of recently opened files.
pub(crate) fn opened(recent: &mut Vec<ProjectPath>, path: ProjectPath) {
    recent.retain(|p| *p != path);
    recent.insert(0, path);
    recent.truncate(RECENT);
}

/// A path that matches the query, and the byte offsets of its matched characters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Match {
    pub path: String,
    pub marks: Vec<usize>,
}

/// The paths that hold the query's characters in order (ignoring case), best first: matches in
/// the file's name, at the start of a word and in runs rank higher, shorter paths break ties.
/// With an empty query: `recent` in its order, then the other paths sorted.
pub(crate) fn filter(query: &str, paths: &[String], recent: &[String]) -> Vec<Match> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        let mut rest: Vec<&String> = paths.iter().filter(|p| !recent.contains(p)).collect();
        rest.sort();
        let first = recent.iter().filter(|r| paths.contains(r));
        return first.chain(rest).map(|p| Match { path: p.clone(), marks: Vec::new() }).collect();
    }
    let mut scored: Vec<(i64, Match)> =
        paths.iter().filter_map(|p| score(&query, p).map(|(s, marks)| (s, Match { path: p.clone(), marks }))).collect();
    scored.sort_by(|(a, ma), (b, mb)| b.cmp(a).then(ma.path.len().cmp(&mb.path.len())).then(ma.path.cmp(&mb.path)));
    scored.into_iter().map(|(_, m)| m).collect()
}

/// A greedy subsequence match of `query` (lowercase) in `path`, and its score.
fn score(query: &[char], path: &str) -> Option<(i64, Vec<usize>)> {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    // Try from the file's name first, so a query that fits the name marks it there.
    let candidates = [name_start, 0];
    let mut best: Option<(i64, Vec<usize>)> = None;
    for from in candidates {
        if let Some(found) = subsequence(query, path, from, name_start)
            && best.as_ref().is_none_or(|(s, _)| found.0 > *s)
        {
            best = Some(found);
        }
    }
    best
}

fn subsequence(query: &[char], path: &str, from: usize, name_start: usize) -> Option<(i64, Vec<usize>)> {
    let mut marks = Vec::with_capacity(query.len());
    let mut score = 0i64;
    let mut want = query.iter().peekable();
    let mut previous: Option<(usize, char)> = None;
    let mut last_mark: Option<usize> = None;
    for (i, c) in path.char_indices() {
        let Some(&&q) = want.peek() else { break };
        if i >= from && c.to_lowercase().eq(std::iter::once(q)) {
            score += 1;
            if i >= name_start {
                score += 2;
            }
            let word_start = previous.is_none_or(|(_, p)| matches!(p, '/' | '_' | '-' | '.' | ' ') || (p.is_lowercase() && c.is_uppercase()));
            if word_start {
                score += 3;
            }
            if let (Some(l), Some((pi, _))) = (last_mark, previous)
                && l == pi
            {
                score += 4;
            }
            marks.push(i);
            last_mark = Some(i);
            want.next();
        }
        previous = Some((i, c));
    }
    want.peek().is_none().then_some((score, marks))
}

/// The palette, when it is the open dialog.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let paths: Vec<String> = app.session.project().paths().map(|p| p.as_str().to_owned()).collect();
    let recent: Vec<String> = app.recent_files.iter().map(|p| p.as_str().to_owned()).collect();
    let Some(Dialog::GoToFile(palette)) = app.view.dialog.as_mut() else { return };
    let matches = filter(&palette.query, &paths, &recent);
    let mut open: Option<String> = None;
    let mut close = false;
    let field_id = egui::Id::unique("texcraft-goto-file-query");
    let typing = ctx.memory(|m| m.has_focus(field_id));
    if typing {
        let (down, up, enter) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::Enter))
        });
        if !matches.is_empty() {
            if down {
                palette.selected = (palette.selected + 1) % matches.len();
                palette.scroll = true;
            }
            if up {
                palette.selected = (palette.selected + matches.len() - 1) % matches.len();
                palette.scroll = true;
            }
        }
        if enter {
            open = matches.get(palette.selected).map(|m| m.path.clone());
        }
    }
    let modal = egui::Modal::new(egui::Id::unique("texcraft-goto-file")).frame(crate::widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new(tl!("Go to File")).font(t.title()));
        ui.add_space(t.unit * 2.0);
        let label = ui.label(RichText::new(tl!("Type part of a file's path")).color(t.text_muted));
        let edit = ui
            .add(egui::TextEdit::singleline(&mut palette.query).id(field_id).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)))
            .labelled_by(label.id);
        if palette.focus {
            edit.request_focus();
            palette.focus = false;
        }
        if edit.changed() {
            palette.selected = 0;
        }
        ui.add_space(t.unit * 2.0);
        let count = filter(&palette.query, &paths, &recent);
        palette.selected = palette.selected.min(count.len().saturating_sub(1));
        let summary = match count.get(palette.selected) {
            Some(m) => tl!("{path}, {n} of {total}")
                .replace("{path}", &m.path)
                .replace("{n}", &(palette.selected + 1).to_string())
                .replace("{total}", &count.len().to_string()),
            None => tl!("No file matches.").to_owned(),
        };
        let status = ui.label(RichText::new(&summary).font(t.small()).color(t.text_muted));
        crate::status::announce(ui, &status, &summary);
        ui.add_space(t.unit);
        let list = ui.scope(|ui| {
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, m) in count.iter().enumerate() {
                    let selected = i == palette.selected;
                    let row = ui.add(egui::Button::selectable(selected, highlighted(ui, &t, m)).min_size(egui::vec2(ui.available_width(), 26.0)));
                    let name = m.path.clone();
                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, true, selected, &name));
                    if selected && palette.scroll {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        open = Some(m.path.clone());
                    }
                }
            });
        });
        ctx.accesskit_node_builder(list.response.id, |node| {
            node.set_role(egui::accesskit::Role::ListBox);
            node.set_label(tl!("Project files"));
        });
        palette.scroll = false;
    });
    if modal.should_close() || ctx.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if let Some(path) = open {
        app.view.dialog = None;
        if app.run("file.open", json!({"path": path})).is_some() {
            app.editor.focus = true;
        }
    } else if close {
        app.view.dialog = None;
    }
}

/// The path with its matched characters in the accent colour.
fn highlighted(ui: &egui::Ui, t: &crate::theme::Tokens, m: &Match) -> LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let plain = TextFormat { font_id: font.clone(), color: t.text, ..Default::default() };
    let mark = TextFormat { font_id: font, color: t.accent, underline: egui::Stroke::new(1.0, t.accent), ..Default::default() };
    let mut job = LayoutJob::default();
    for (i, c) in m.path.char_indices() {
        let format = if m.marks.contains(&i) { mark.clone() } else { plain.clone() };
        job.append(c.encode_utf8(&mut [0; 4]), 0.0, format);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_match_in_the_file_name_ranks_above_one_spread_over_folders() {
        let all = paths(&["chapters/intro.tex", "images/chart.png", "main.tex"]);
        let found = filter("chap", &all, &[]);
        assert!(found.iter().any(|m| m.path == "images/chart.png"));
        let found = filter("intro", &all, &[]);
        assert_eq!(found.first().map(|m| m.path.as_str()), Some("chapters/intro.tex"));
        let found = filter("ct", &all, &[]);
        assert!(found.iter().all(|m| m.marks.len() == 2), "every match marks each letter of the query");
        assert!(filter("xyz", &all, &[]).is_empty());
    }

    #[test]
    fn with_no_query_the_recent_files_come_first() {
        let all = paths(&["a.tex", "b.tex", "c.tex"]);
        let found: Vec<String> = filter("", &all, &paths(&["c.tex", "gone.tex"])).into_iter().map(|m| m.path).collect();
        assert_eq!(found, paths(&["c.tex", "a.tex", "b.tex"]));
    }
}
