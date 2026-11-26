//! All Commands (`view.commands`, Cmd+Shift+P): every command the window offers in one searchable
//! list, so the menus can keep only what is used often. A typed query keeps the commands whose
//! name holds its letters in order (as Go to File does), best first; each row says where the
//! command lives (its menu) and its shortcut, and a command that cannot run now is listed dimmed
//! with the reason. Up and Down move, Enter or a click runs the command through
//! [`ui_cmd::run`], Escape closes. The selected command and the count of matches are announced
//! through a live region (standards/accessibility.md).
//!
//! The list is the engine's commands that have a menu or a shortcut (those for agents only are
//! left out) and the view's commands, without those that need what only their own control gives
//! (a zoom's percentage, a spelling to change to, a symbol).

use egui::text::{LayoutJob, TextFormat};
use egui::{Key, Modifiers, RichText};
use texcraft_engine::cmd;

use crate::App;
use crate::dialogs::Dialog;
use crate::ui_cmd::{self, UI_COMMANDS};

/// The list's state while it is open.
pub(crate) struct Palette {
    query: String,
    selected: usize,
    focus: bool,
    /// The selection was moved by a key: scroll it into view.
    scroll: bool,
}

/// A command as the list shows it.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub id: &'static str,
    pub label: String,
    /// The menu it belongs to, translated.
    pub place: String,
    pub shortcut: Option<String>,
    pub enabled: Result<(), String>,
    /// Another name it is found by and that is not shown: a label it had before
    /// (standards/naming.md rule 6).
    alias: Option<String>,
    /// Byte offsets of the label's characters the query matched.
    marks: Vec<usize>,
}

/// The label a command had before it was renamed or folded into another, by the command that
/// does it now: a search finds it under its old name too.
fn alias(id: &str) -> Option<String> {
    match id {
        // Open from GitLab or GitHub… is a card of Import a Project now.
        "project.import_dialog" => Some(tl!("Open from GitLab or GitHub").to_owned()),
        _ => None,
    }
}

/// View commands that need what only their own control gives, or that only make sense where
/// that control is (a presentation's keys, the text's context menu).
fn listed_view_command(id: &str, menu: &str) -> bool {
    !matches!(id, "view.zoom" | "edit.rewrite" | "view.commands" | "view.present.next" | "view.present.previous" | "view.present.end")
        && !matches!(menu, "Spelling" | "Grammar" | "Symbols")
}

/// Every command the list offers, in the registry's order then the view's.
pub(crate) fn rows(app: &App) -> Vec<Row> {
    let browser_only = |id: &str| id == "project.download_zip" && app.session.library().is_none();
    let engine = cmd::all().iter().filter(|c| (!c.menu.is_empty() || c.shortcut.is_some()) && !browser_only(c.id)).map(|c| {
        let (label, enabled) = ui_cmd::menu_entry(app, c.id);
        let place = c.menu.first().map_or_else(String::new, |m| crate::i18n::t(m).to_owned());
        Row { id: c.id, label: label.to_owned(), place, shortcut: app.keys.shortcut_text(c.id), enabled, alias: alias(c.id), marks: Vec::new() }
    });
    let view = UI_COMMANDS.iter().filter(|c| listed_view_command(c.id, c.menu)).map(|c| {
        let (label, enabled) = ui_cmd::menu_entry(app, c.id);
        Row {
            id: c.id,
            label: label.to_owned(),
            place: crate::i18n::t(c.menu).to_owned(),
            shortcut: app.keys.shortcut_text(c.id),
            enabled,
            alias: alias(c.id),
            marks: Vec::new(),
        }
    });
    engine.chain(view).collect()
}

/// The commands the list offers, by id, in its order.
pub fn listed(app: &App) -> Vec<&'static str> {
    rows(app).into_iter().map(|r| r.id).collect()
}

/// The rows whose label holds `query`'s letters in order (ignoring case and spaces), best first;
/// all of them, in order, for an empty query.
pub(crate) fn filter(query: &str, rows: Vec<Row>) -> Vec<Row> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return rows;
    }
    let mut scored: Vec<(i64, Row)> = rows
        .into_iter()
        .filter_map(|mut r| match crate::goto_file::score(&query, &r.label) {
            Some((score, marks)) => {
                r.marks = marks;
                Some((score, r))
            }
            None => r.alias.as_deref().and_then(|a| crate::goto_file::score(&query, a)).map(|(score, _)| (score, r)),
        })
        .collect();
    // Commands that can run now first, then the best matches, shorter names breaking ties.
    scored.sort_by(|(a, ra), (b, rb)| rb.enabled.is_ok().cmp(&ra.enabled.is_ok()).then(b.cmp(a)).then(ra.label.len().cmp(&rb.label.len())));
    scored.into_iter().map(|(_, r)| r).collect()
}

/// Opens the list.
pub(crate) fn open(app: &mut App) {
    app.view.dialog = Some(Dialog::Commands(Palette { query: String::new(), selected: 0, focus: true, scroll: false }));
}

/// The list, when it is the open dialog.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let all = rows(app);
    let Some(Dialog::Commands(palette)) = app.view.dialog.as_mut() else { return };
    let mut matches = filter(&palette.query, all.clone());
    let mut run: Option<&'static str> = None;
    let mut close = false;
    let field_id = egui::Id::unique("texcraft-all-commands-query");
    if ctx.memory(|m| m.has_focus(field_id)) {
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
            run = matches.get(palette.selected).map(|r| r.id);
        }
    }
    let modal = crate::sheet::show(ctx, &t, "texcraft-all-commands", 560.0, crate::sheet::Leave::Close, |ui| {
        crate::dialogs::title(ui, &t, tl!("All Commands"));
        let label = ui.label(RichText::new(tl!("Type part of a command's name")).color(t.text_muted));
        let edit = ui
            .add(egui::TextEdit::singleline(&mut palette.query).id(field_id).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)))
            .labelled_by(label.id);
        if palette.focus {
            edit.request_focus();
            palette.focus = false;
        }
        if edit.changed() {
            palette.selected = 0;
            matches = filter(&palette.query, all.clone());
        }
        ui.add_space(t.unit * 2.0);
        palette.selected = palette.selected.min(matches.len().saturating_sub(1));
        let summary = match matches.get(palette.selected) {
            Some(r) => tl!("{command}, {n} of {total}")
                .replace("{command}", &r.label)
                .replace("{n}", &(palette.selected + 1).to_string())
                .replace("{total}", &matches.len().to_string()),
            None => tl!("No command matches.").to_owned(),
        };
        let status = ui.label(RichText::new(&summary).font(t.small()).color(t.text_muted));
        crate::status::announce(ui, &status, &summary);
        ui.add_space(t.unit);
        let list = ui.scope(|ui| {
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, r) in matches.iter().enumerate() {
                    let selected = i == palette.selected;
                    let mut button = egui::Button::selectable(selected, row_text(ui, &t, r)).min_size(egui::vec2(ui.available_width(), 26.0));
                    if let Some(k) = &r.shortcut {
                        button = button.shortcut_text(RichText::new(k).color(t.text_muted));
                    }
                    let row = ui.add(button);
                    // Its name starts with the label; where it lives, and why it cannot run, follow.
                    let name = match &r.enabled {
                        Ok(()) => format!("{}, {}", r.label, r.place),
                        Err(why) => format!("{}, {}, {}", r.label, r.place, crate::i18n::msg(why)),
                    };
                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, r.enabled.is_ok(), selected, &name));
                    let row = match &r.enabled {
                        Err(why) => row.on_hover_text(crate::i18n::msg(why)),
                        Ok(()) => row,
                    };
                    if selected && palette.scroll {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        run = Some(r.id);
                    }
                }
            });
        });
        ctx.accesskit_node_builder(list.response.id, |node| {
            node.set_role(egui::accesskit::Role::ListBox);
            node.set_label(tl!("Commands"));
        });
        palette.scroll = false;
    });
    if modal.close || ctx.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if let Some(id) = run {
        app.view.dialog = None;
        // A command that cannot run says why, as its menu entry does on hover.
        match matches.iter().find(|r| r.id == id).map(|r| r.enabled.clone()) {
            Some(Err(why)) => app.view.status = Some((crate::i18n::msg(&why), true)),
            _ => ui_cmd::run(app, id),
        }
    } else if close {
        app.view.dialog = None;
    }
}

/// The command's name with the matched letters in the accent colour, then its menu, dimmed;
/// dimmed throughout when it cannot run now.
fn row_text(ui: &egui::Ui, t: &crate::theme::Tokens, r: &Row) -> LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = if r.enabled.is_ok() { t.text } else { t.text_muted };
    let plain = TextFormat { font_id: font.clone(), color, ..Default::default() };
    let mark = TextFormat { font_id: font, color: t.accent, underline: egui::Stroke::new(1.0, t.accent), ..Default::default() };
    let mut job = LayoutJob::default();
    for (i, c) in r.label.char_indices() {
        let format = if r.marks.contains(&i) { mark.clone() } else { plain.clone() };
        job.append(c.encode_utf8(&mut [0; 4]), 0.0, format);
    }
    if !r.place.is_empty() {
        job.append(&format!("  {}", r.place), 0.0, TextFormat { font_id: t.small(), color: t.text_muted, ..Default::default() });
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &'static str, label: &str, enabled: bool) -> Row {
        let enabled = if enabled { Ok(()) } else { Err(String::new()) };
        Row { id, label: label.to_owned(), place: String::new(), shortcut: None, enabled, alias: None, marks: Vec::new() }
    }

    #[test]
    fn a_query_keeps_the_names_holding_its_letters_and_puts_those_that_can_run_first() {
        let all = vec![row("a", "Fold All", false), row("b", "Find", true), row("c", "Find in Project", true), row("d", "Zoom In", true)];
        let found: Vec<&str> = filter("fi", all.clone()).iter().map(|r| r.id).collect();
        assert!(!found.contains(&"d"), "a name without the letters is kept: {found:?}");
        assert!(found.iter().position(|i| *i == "b") < found.iter().position(|i| *i == "c"), "the shorter name is not first");
        let found: Vec<&str> = filter("fold", all).iter().map(|r| r.id).collect();
        assert_eq!(found, ["a"], "a command that cannot run now is left out");
    }
}
