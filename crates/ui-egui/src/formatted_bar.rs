//! The formatted view's toolbar (docs/ROADMAP.md, F3): the paragraph style (body text, or
//! Heading 1 to 3 as the document's sectioning levels), Bold, Italic and Code, the lists and their
//! nesting, a link, a footnote, a citation and a cross-reference, formulas, a table and a figure.
//! Each button runs its `edit.*` command, which the view makes as a minimal edit of the source
//! ([`crate::formatted_edit`]), and shows its shortcut in its tooltip. The styles, the paragraph
//! style and the lists show where the caret is (Bold pressed in `\textbf`).

use egui::{Align, Align2, Layout, Sense, vec2};
use texcraft_latex::edit::Here;
use texcraft_latex::view::ListKind;

use crate::App;
use crate::theme::{Tokens, blend};
use crate::widgets;

/// Where the caret is, for the buttons' state: worked out again when the text or the caret moves.
#[derive(Default)]
pub(crate) struct State {
    /// The buffer's version and the caret's byte it is for.
    of: Option<(u64, usize)>,
    here: Here,
    /// The session revision the heading commands are for, and them.
    headings: Option<(u64, [&'static str; 3])>,
}

/// The sectioning commands of Heading 1, 2 and 3 for the main document's class.
pub(crate) fn heading_commands(app: &mut App) -> [&'static str; 3] {
    let revision = app.session.revision();
    if let Some((r, names)) = app.editor.formatted.bar.headings
        && r == revision
    {
        return names;
    }
    let project = app.session.project();
    let main = project.main().and_then(|m| project.text(m).ok()).or_else(|| app.session.current().and_then(|p| project.text(p).ok()));
    let names = texcraft_latex::edit::heading_commands(main.unwrap_or_default());
    app.editor.formatted.bar.headings = Some((revision, names));
    names
}

/// What encloses the caret.
pub(crate) fn here(app: &mut App) -> Here {
    let d = &app.editor.source.doc;
    let key = (d.buffer.version(), d.buffer.char_to_byte(d.selection.primary().head));
    let state = &app.editor.formatted.bar;
    if state.of == Some(key) {
        return state.here;
    }
    let here = texcraft_latex::edit::here(&d.buffer.text(), key.1);
    app.editor.formatted.bar.of = Some(key);
    app.editor.formatted.bar.here = here;
    here
}

/// The paragraph style at the caret: 0 for body text, 1 to 3 for the headings, `None` for
/// another level (a `\paragraph`, a `\part`).
fn style_index(here: &Here, names: &[&'static str; 3]) -> Option<usize> {
    let Some(level) = here.heading else { return Some(0) };
    names.iter().position(|n| texcraft_latex::view::heading_level(n) == Some(level)).map(|i| i + 1)
}

/// The toolbar's groups, in the order they give way to the More menu when the pane is narrow:
/// each with its number of buttons and its entries for that menu.
const GROUPS: [(u8, &[(&str, &str)]); 5] = [
    (
        // And the paragraph style's drop-down, as wide as its words.
        3,
        &[
            (tn!("Body Text"), "edit.body"),
            (tn!("Heading 1"), "edit.heading_1"),
            (tn!("Heading 2"), "edit.heading_2"),
            (tn!("Heading 3"), "edit.heading_3"),
            (tn!("Bold"), "edit.bold"),
            (tn!("Italic"), "edit.italic"),
            (tn!("Code"), "edit.code"),
        ],
    ),
    (
        4,
        &[
            (tn!("Bulleted List"), "edit.itemize"),
            (tn!("Numbered List"), "edit.enumerate"),
            (tn!("Outdent Item"), "edit.outdent"),
            (tn!("Indent Item"), "edit.indent"),
        ],
    ),
    (4, &[(tn!("Link"), "edit.link"), (tn!("Footnote"), "edit.footnote"), (tn!("Citation"), "edit.cite"), (tn!("Cross-Reference"), "edit.ref")]),
    (1, &[(tn!("Inline Formula"), "edit.math_inline"), (tn!("Displayed Formula"), "edit.math_display"), (tn!("Numbered Equation"), "edit.equation")]),
    (2, &[(tn!("Table"), "edit.table"), (tn!("Figure"), "edit.figure")]),
];

/// The toolbar's buttons after the mode switch; the command chosen goes in `chosen`.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, chosen: &mut Option<&'static str>) {
    let t = app.tokens;
    let here = here(app);
    let names = heading_commands(app);
    let tip = |app: &App, label: &str, id: &str| match app.keys.shortcut_text(id) {
        Some(keys) => format!("{label} ({keys})"),
        None => label.to_owned(),
    };
    // Room for the More menu on the right: its button and the margin after it. Each group's
    // width is what its widgets take, so a group is never sent to the menu while it fits (the
    // faces' widths change the paragraph style's, and a few pixels decided it, #47's test).
    let spacing = ui.spacing().item_spacing.x;
    let button = t.toolbar_height - 2.0 * t.unit + spacing;
    let mut room = ui.max_rect().right() - ui.cursor().left() - (button + 2.0 * t.unit);
    let style_width = style_width(ui, &t);
    let mut hidden: Vec<usize> = Vec::new();
    // On a phone, Bold, Italic and Code stay when the paragraph style's drop-down does not fit:
    // it goes to the More menu alone.
    let mut style_hidden = false;
    for (i, (buttons, _)) in GROUPS.iter().enumerate() {
        // The divider before it: a line between two margins.
        let mut width = 2.0 * t.unit + 1.0 + spacing + f32::from(*buttons) * button;
        if i == 0 {
            if width + style_width + spacing <= room {
                width += style_width + spacing;
            } else {
                style_hidden = true;
            }
        }
        if !(hidden.is_empty() && width <= room) {
            hidden.push(i);
            // The whole group goes to the menu.
            style_hidden &= i != 0;
            continue;
        }
        room -= width;
        divider(ui, &t);
        match i {
            0 => {
                if !style_hidden {
                    paragraph_style(app, ui, (&here, &names, style_width), chosen);
                }
                let bold = tl!("Bold");
                if glyph_toggle(ui, &t, "B", &tip(app, bold, "edit.bold"), bold, here.bold).clicked() {
                    *chosen = Some("edit.bold");
                }
                for (icon, label, id, on) in [("italic", tl!("Italic"), "edit.italic", here.italic), ("code", tl!("Code"), "edit.code", here.code)] {
                    if widgets::icon_toggle(ui, &t, icon, label, on).on_hover_text(tip(app, label, id)).clicked() {
                        *chosen = Some(id);
                    }
                }
            }
            1 => {
                let kind = here.list.map(|(k, _)| k);
                for (icon, label, id, on) in [
                    ("list", tl!("Bulleted List"), "edit.itemize", kind == Some(ListKind::Bullets)),
                    ("list-ordered", tl!("Numbered List"), "edit.enumerate", kind == Some(ListKind::Numbers)),
                ] {
                    if widgets::icon_toggle(ui, &t, icon, label, on).on_hover_text(tip(app, label, id)).clicked() {
                        *chosen = Some(id);
                    }
                }
                let in_list = here.list.is_some();
                for (icon, label, id) in [("chevron-left", tl!("Outdent Item"), "edit.outdent"), ("chevron-right", tl!("Indent Item"), "edit.indent")]
                {
                    if widgets::icon_button(ui, &t, icon, label, in_list).on_hover_text(tip(app, label, id)).clicked() {
                        *chosen = Some(id);
                    }
                }
            }
            2 => {
                for (icon, label, id) in [
                    ("link", tl!("Link"), "edit.link"),
                    ("", tl!("Footnote"), "edit.footnote"),
                    ("book-open", tl!("Citation"), "edit.cite"),
                    ("tag", tl!("Cross-Reference"), "edit.ref"),
                ] {
                    let r = if icon.is_empty() {
                        glyph_toggle(ui, &t, "¹", &tip(app, label, id), label, false)
                    } else {
                        widgets::icon_button(ui, &t, icon, label, true).on_hover_text(tip(app, label, id))
                    };
                    if r.clicked() {
                        *chosen = Some(id);
                    }
                }
            }
            3 => {
                let math = widgets::icon_button(ui, &t, "sigma", tl!("Formula"), true);
                widgets::keyboard_menu(ui, &math);
                egui::Popup::menu(&math).show(|ui| {
                    ui.set_min_width(200.0);
                    for (label, id) in GROUPS[3].1 {
                        if menu_entry(app, ui, crate::i18n::t(label), id) {
                            *chosen = Some(id);
                        }
                    }
                });
            }
            _ => {
                for (icon, label, id) in [("table", tl!("Table"), "edit.table"), ("image", tl!("Figure"), "edit.figure")] {
                    if widgets::icon_button(ui, &t, icon, label, true).on_hover_text(tip(app, label, id)).clicked() {
                        *chosen = Some(id);
                    }
                }
            }
        }
    }
    if hidden.is_empty() && !style_hidden {
        return;
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        let more = widgets::icon_button(ui, &t, "ellipsis", tl!("More formatting"), true);
        widgets::keyboard_menu(ui, &more);
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(200.0);
            // The paragraph styles, when only they went to the menu.
            let styles = GROUPS[0].1.get(..4).unwrap_or_default();
            if style_hidden {
                for (label, id) in styles {
                    if menu_entry(app, ui, crate::i18n::t(label), id) {
                        *chosen = Some(id);
                    }
                }
            }
            for (n, i) in hidden.iter().enumerate() {
                if n > 0 || style_hidden {
                    ui.separator();
                }
                for (label, id) in GROUPS[*i].1 {
                    if menu_entry(app, ui, crate::i18n::t(label), id) {
                        *chosen = Some(id);
                    }
                }
            }
        });
    });
}

/// The paragraph style drop-down's width: its longest style's, with its chevron.
fn style_width(ui: &egui::Ui, t: &Tokens) -> f32 {
    let widest = [tl!("Body Text"), tl!("Heading 1"), tl!("Heading 2"), tl!("Heading 3"), tl!("Other Heading")]
        .iter()
        .map(|s| ui.painter().layout_no_wrap((*s).to_owned(), t.small(), t.text).size().x)
        .fold(0.0, f32::max);
    widest + 4.0 * t.unit + 20.0
}

/// The paragraph style's drop-down: the style at the caret, and Body Text, Heading 1 to 3 (each
/// named with its sectioning command).
fn paragraph_style(app: &mut App, ui: &mut egui::Ui, (here, names, width): (&Here, &[&'static str; 3], f32), chosen: &mut Option<&'static str>) {
    let t = app.tokens;
    let current = style_index(here, names);
    let styles = [tl!("Body Text"), tl!("Heading 1"), tl!("Heading 2"), tl!("Heading 3")];
    let shown = current.and_then(|i| styles.get(i).copied()).unwrap_or(tl!("Other Heading"));
    let name = tl!("{style}, paragraph style").replace("{style}", shown);
    let height = t.toolbar_height - 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &name));
    if ui.is_rect_visible(rect) {
        let fill = widgets::state_fill(&t, &r, t.frame);
        ui.painter().rect_filled(rect, t.corner(), if fill == t.frame { blend(t.frame, t.hover) } else { fill });
        let text_at = rect.left_center() + vec2(2.0 * t.unit, 0.0);
        ui.painter().text(text_at, Align2::LEFT_CENTER, shown, t.small(), t.text);
        let chevron = egui::Rect::from_center_size(rect.right_center() - vec2(10.0, 0.0), vec2(14.0, 14.0));
        crate::icons::paint(ui, chevron, "chevron-down", 12.0, t.text_muted);
        widgets::focus_ring(ui, &t, &r, rect, t.corner());
    }
    let r = r.on_hover_text(tl!("Paragraph style"));
    widgets::keyboard_menu(ui, &r);
    let ids = ["edit.body", "edit.heading_1", "edit.heading_2", "edit.heading_3"];
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(220.0);
        for (i, (label, id)) in styles.iter().zip(ids).enumerate() {
            let size = [13.0, 17.0, 15.0, 13.5][i];
            let detail = names.get(i.wrapping_sub(1)).map(|n| format!("  \\{n}")).unwrap_or_default();
            let mut job = egui::text::LayoutJob::default();
            let strong = egui::FontId::new(size, if i == 0 { egui::FontFamily::Proportional } else { crate::fonts::semibold() });
            job.append(label, 0.0, egui::TextFormat { font_id: strong, color: t.text, ..Default::default() });
            job.append(&detail, 0.0, egui::TextFormat { font_id: t.small(), color: t.text_muted, ..Default::default() });
            let entry = ui.add(egui::Button::selectable(current == Some(i), job).frame_when_inactive(false));
            entry.widget_info(|| egui::WidgetInfo::selected(egui::Role::MenuItem, true, current == Some(i), *label));
            if entry.clicked() {
                // The style it has already: nothing to change.
                if current != Some(i) {
                    *chosen = Some(id);
                }
                ui.close();
            }
        }
    });
}

/// A menu entry running `id`, with its shortcut; whether it was chosen.
fn menu_entry(app: &App, ui: &mut egui::Ui, label: &str, id: &str) -> bool {
    let t = app.tokens;
    let item = crate::top_bar::item_response(ui, &t, None, label, app.keys.shortcut_text(id), Ok(()));
    let chosen = item.clicked();
    if chosen {
        ui.close();
    }
    chosen
}

fn divider(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(t.unit);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, t.toolbar_height - 4.0 * t.unit), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.separator);
    ui.add_space(t.unit);
}

/// A toolbar button showing a glyph (B, ¹) that stays highlighted while `on`; `name` is its
/// accessible name, `tip` its tooltip.
fn glyph_toggle(ui: &mut egui::Ui, t: &Tokens, glyph: &str, tip: &str, name: &str, on: bool) -> egui::Response {
    let side = t.toolbar_height - 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, name));
    if ui.is_rect_visible(rect) {
        let base = if on { blend(t.frame, t.accent_soft) } else { t.frame };
        let fill = widgets::state_fill(t, &r, base);
        if fill != t.frame {
            ui.painter().rect_filled(rect, t.corner(), fill);
        }
        let color = if on {
            t.accent
        } else if r.hovered() {
            t.text
        } else {
            t.text_muted
        };
        let font = egui::FontId::new(15.0, crate::fonts::semibold());
        let galley = ui.painter().layout_no_wrap(glyph.to_owned(), font, color);
        let at = rect.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley, color);
        widgets::focus_ring(ui, t, &r, rect, t.corner());
    }
    r.on_hover_text(tip)
}
