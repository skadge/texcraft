//! The pickers of the formatted view (docs/ROADMAP.md, F3) and of the source (E13): a citation
//! from the project's `.bib` entries, a cross-reference to a label of its `.tex` files (with its
//! caption or heading), an image for a new figure. A typed query keeps the entries holding it (in
//! the key or the description, ignoring case); Up and Down move the selection, Enter or a click
//! inserts it through `edit.cite {key}`, `edit.ref {key}` or `edit.figure {image}`, Escape closes.
//! A key the project does not have yet can be typed and used. Like Go to File, the picker traps
//! focus and announces the selected entry through a live region.

use egui::{Key, Modifiers, RichText};
use serde_json::json;

use crate::App;
use crate::dialogs::Dialog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Citation,
    Reference,
    Image,
}

/// An entry: what is inserted and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub key: String,
    pub detail: String,
}

pub(crate) struct Chooser {
    kind: Kind,
    choices: Vec<Choice>,
    query: String,
    selected: usize,
    focus: bool,
    scroll: bool,
}

/// The project's entries of `kind`.
pub(crate) fn choices(app: &App, kind: Kind) -> Vec<Choice> {
    let project = app.session.project();
    let mut out = Vec::new();
    for path in project.paths() {
        match kind {
            Kind::Citation if path.extension() == "bib" => {
                let Ok(text) = project.text(path) else { continue };
                for e in texcraft_latex::refs::entries(text) {
                    let title = e.title.clone().unwrap_or_default();
                    let detail = format!("{}, {}", texcraft_latex::refs::author_year(&e), title.trim_matches(['{', '}']));
                    out.push(Choice { key: e.key, detail: detail.trim_end_matches([',', ' ']).to_owned() });
                }
            }
            Kind::Reference if path.extension() == "tex" => {
                let Ok(text) = project.text(path) else { continue };
                for l in texcraft_latex::refs::labels(text) {
                    out.push(Choice { key: l.key, detail: format!("{}, {}", l.what, path.file_name()).trim_start_matches([',', ' ']).to_owned() });
                }
            }
            Kind::Image if texcraft_engine::cmd::is_image(path) => out.push(Choice { key: path.as_str().to_owned(), detail: String::new() }),
            _ => {}
        }
    }
    out
}

/// Opens the picker of `kind`; for an image with none in the project, says how to add one.
pub(crate) fn open(app: &mut App, kind: Kind) {
    let choices = choices(app, kind);
    if kind == Kind::Image && choices.is_empty() {
        let said = tl!("The project has no images yet: drop an image file on the view, or add one with Add Files, to make a figure of it.");
        app.view.status = Some((said.to_owned(), true));
        return;
    }
    app.view.dialog = Some(Dialog::Choose(Box::new(Chooser { kind, choices, query: String::new(), selected: 0, focus: true, scroll: false })));
}

/// The entries holding `query` (ignoring case), and a typed key when nothing has it exactly.
pub(crate) fn filter(query: &str, choices: &[Choice], free: bool) -> Vec<Choice> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<Choice> =
        choices.iter().filter(|c| q.is_empty() || c.key.to_lowercase().contains(&q) || c.detail.to_lowercase().contains(&q)).cloned().collect();
    out.sort_by_key(|c| !c.key.to_lowercase().starts_with(&q));
    let typed = query.trim();
    let usable = !typed.is_empty() && !typed.contains(['{', '}', '%', '\\', ',', ' ']);
    if free && usable && !choices.iter().any(|c| c.key == typed) {
        out.push(Choice { key: typed.to_owned(), detail: tl!("not in the project yet").to_owned() });
    }
    out
}

/// The picker, when it is the open dialog.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let Some(Dialog::Choose(p)) = app.view.dialog.as_mut() else { return };
    let (title, field, list_name, none) = match p.kind {
        Kind::Citation => (
            tl!("Insert a Citation"),
            tl!("Type part of a key, an author or a title"),
            tl!("Bibliography entries"),
            tl!("No entry matches. The project's .bib files have the entries to cite."),
        ),
        Kind::Reference => {
            (tl!("Insert a Cross-Reference"), tl!("Type part of a label or of what it labels"), tl!("Labels"), tl!("No label matches."))
        }
        Kind::Image => (tl!("Insert a Figure"), tl!("Type part of the image's path"), tl!("Images"), tl!("No image matches.")),
    };
    let matches = filter(&p.query, &p.choices, p.kind != Kind::Image);
    let mut chosen: Option<String> = None;
    let field_id = egui::Id::unique("texcraft-insert-pick-query");
    if ctx.memory(|m| m.has_focus(field_id)) {
        let (down, up, enter) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::Enter))
        });
        if !matches.is_empty() {
            if down {
                p.selected = (p.selected + 1) % matches.len();
                p.scroll = true;
            }
            if up {
                p.selected = (p.selected + matches.len() - 1) % matches.len();
                p.scroll = true;
            }
        }
        if enter {
            chosen = matches.get(p.selected).map(|m| m.key.clone());
        }
    }
    let modal = egui::Modal::new(egui::Id::unique("texcraft-insert-pick")).frame(crate::widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new(title).font(t.title()));
        ui.add_space(t.unit * 2.0);
        let label = ui.label(RichText::new(field).color(t.text_muted));
        let edit = ui
            .add(egui::TextEdit::singleline(&mut p.query).id(field_id).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)))
            .labelled_by(label.id);
        if std::mem::take(&mut p.focus) {
            edit.request_focus();
        }
        if edit.changed() {
            p.selected = 0;
        }
        ui.add_space(t.unit * 2.0);
        let shown = filter(&p.query, &p.choices, p.kind != Kind::Image);
        p.selected = p.selected.min(shown.len().saturating_sub(1));
        let summary = match shown.get(p.selected) {
            Some(m) => tl!("{key}, {n} of {total}")
                .replace("{key}", &m.key)
                .replace("{n}", &(p.selected + 1).to_string())
                .replace("{total}", &shown.len().to_string()),
            None => none.to_owned(),
        };
        let status = ui.label(RichText::new(&summary).font(t.small()).color(t.text_muted));
        crate::status::announce(ui, &status, &summary);
        ui.add_space(t.unit);
        let list = ui.scope(|ui| {
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, m) in shown.iter().enumerate() {
                    let selected = i == p.selected;
                    let mut job = egui::text::LayoutJob::default();
                    let font = egui::TextStyle::Button.resolve(ui.style());
                    job.append(&m.key, 0.0, egui::TextFormat { font_id: font, color: t.text, ..Default::default() });
                    if !m.detail.is_empty() {
                        job.append(
                            &format!("   {}", m.detail),
                            0.0,
                            egui::TextFormat { font_id: t.small(), color: t.text_muted, ..Default::default() },
                        );
                    }
                    let row = ui.add(egui::Button::selectable(selected, job).truncate().min_size(egui::vec2(ui.available_width(), 26.0)));
                    let name = if m.detail.is_empty() { m.key.clone() } else { format!("{}, {}", m.key, m.detail) };
                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, true, selected, &name));
                    if selected && p.scroll {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        chosen = Some(m.key.clone());
                    }
                }
            });
        });
        ctx.accesskit_node_builder(list.response.id, |node| {
            node.set_role(egui::accesskit::Role::ListBox);
            node.set_label(list_name);
        });
        p.scroll = false;
    });
    let kind = p.kind;
    if let Some(key) = chosen {
        app.view.dialog = None;
        let (id, params) = match kind {
            Kind::Citation => ("edit.cite", json!({"key": key})),
            Kind::Reference => ("edit.ref", json!({"key": key})),
            Kind::Image => ("edit.figure", json!({"image": key})),
        };
        crate::ui_cmd::run_with(app, id, &params);
    } else if modal.should_close() || ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.view.dialog = None;
        // The keyboard goes back to the view the picker opened from.
        if app.session.current().is_some_and(|p| app.editor.formatted.shows(p)) {
            app.editor.formatted.editing.focus = true;
        } else {
            app.editor.focus = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(key: &str, detail: &str) -> Choice {
        Choice { key: key.to_owned(), detail: detail.to_owned() }
    }

    #[test]
    fn a_query_keeps_entries_by_key_or_description_and_offers_a_new_key() {
        let all = [c("knuth84", "Knuth (1984), The TeXbook"), c("lamport94", "Lamport (1994), LaTeX")];
        let found = filter("texbook", &all, true);
        assert_eq!(found.first().map(|m| m.key.as_str()), Some("knuth84"));
        assert!(found.iter().any(|m| m.key == "texbook"), "a typed key is offered when no entry has it");
        assert!(!filter("knuth84", &all, true).iter().any(|m| m.detail == tl!("not in the project yet")));
        assert!(filter("a b", &all, true).is_empty(), "a key with a space cannot be cited");
        assert_eq!(filter("", &all, false).len(), all.len());
    }
}
