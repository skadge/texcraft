//! A narrow window (a phone) shows the editor and the PDF as two tabs, Source and PDF, above the
//! pane; the PDF's tab says how the build went. The tabs run `view.pane.source` and
//! `view.pane.pdf`; Left and Right move between them, as in any row of tabs.

use egui::{Align2, Rect, Role, Sense, WidgetInfo, vec2};

use crate::{App, Pane, icons, ui_cmd, widgets};

/// What the PDF's tab shows of the build.
enum Build {
    Running,
    Errors(usize),
    Warnings(usize),
    Done,
    Nothing,
}

fn build(app: &App) -> Build {
    if app.session.is_building() {
        return Build::Running;
    }
    match app.session.last_outcome().map(|o| (o.errors(), o.warnings())) {
        Some((e, _)) if e > 0 => Build::Errors(e),
        Some((_, w)) if w > 0 => Build::Warnings(w),
        Some(_) if app.pdf.has_pdf() => Build::Done,
        _ => Build::Nothing,
    }
}

pub(crate) fn tabs(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let frame = egui::Frame::new().fill(t.frame);
    egui::Panel::top("pane-tabs").exact_size(t.tab_height).resizable(false).frame(frame).show(ui, |ui| {
        let bar = ui.max_rect();
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, t.hairline());
        let half = bar.width() / 2.0;
        let state = build(app);
        let (pdf_name, mark) = match state {
            Build::Running => (format!("{}, {}", tl!("PDF"), tl!("compiling")), None),
            Build::Errors(n) => (
                format!("{}, {}", tl!("PDF"), tl!("{count} errors").replace("{count}", &n.to_string())),
                Some(("circle-exclamation-point", t.error, n)),
            ),
            Build::Warnings(n) => (
                format!("{}, {}", tl!("PDF"), tl!("{count} warnings").replace("{count}", &n.to_string())),
                Some(("triangle-exclamation-point", t.warning, n)),
            ),
            Build::Done => (format!("{}, {}", tl!("PDF"), tl!("compiled")), Some(("circle-check", t.success, 0))),
            Build::Nothing => (tl!("PDF").to_owned(), None),
        };
        let tabs = [(Pane::Source, tl!("Source"), tl!("Source").to_owned()), (Pane::Pdf, tl!("PDF"), pdf_name)];
        let mut responses = Vec::with_capacity(2);
        for (i, (pane, label, name)) in tabs.iter().enumerate() {
            let rect = Rect::from_min_size(egui::pos2(bar.left() + half * i as f32, bar.top()), vec2(half, bar.height()));
            let r = ui.interact(rect, ui.scope_id().with(("pane-tab", i)), Sense::click());
            let selected = app.view.pane == *pane;
            r.widget_info(|| WidgetInfo::selected(Role::Tab, true, selected, name));
            // A tab is selected, not toggled, for screen readers.
            ui.ctx().accesskit_node_builder(r.id, |node| {
                node.clear_toggled();
                node.set_selected(selected);
            });
            if ui.is_rect_visible(rect) {
                let fill = widgets::state_fill(&t, &r, t.frame);
                if fill != t.frame {
                    ui.painter().rect_filled(rect.shrink(2.0), t.corner(), fill);
                }
                let (font, color) =
                    if selected { (egui::FontId::new(t.font_size, crate::fonts::semibold()), t.text) } else { (t.body(), t.text_muted) };
                let galley = ui.painter().layout_no_wrap((*label).to_owned(), font, color);
                // The label, and after it the build's mark on the PDF's tab, centred together.
                let extra = match (*pane, &state, &mark) {
                    (Pane::Pdf, Build::Running, _) => t.icon_size + 2.0 * t.unit,
                    (Pane::Pdf, _, Some((_, _, n))) => t.icon_size + 2.0 * t.unit + if *n > 0 { 3.0 * t.unit } else { 0.0 },
                    _ => 0.0,
                };
                let x = rect.center().x - (galley.size().x + extra) / 2.0;
                let text_rect = Rect::from_min_size(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley.size());
                ui.painter().galley(text_rect.min, galley, color);
                let at = egui::pos2(text_rect.right() + t.unit + t.icon_size / 2.0, rect.center().y);
                if *pane == Pane::Pdf {
                    match (&state, &mark) {
                        (Build::Running, _) => widgets::spinner(ui, at, t.icon_size / 2.0 - 2.0, t.text_muted),
                        (_, Some((icon, tint, n))) => {
                            icons::paint(ui, Rect::from_center_size(at, vec2(t.icon_size, t.icon_size)), icon, t.icon_size, *tint);
                            if *n > 0 {
                                let count = egui::pos2(at.x + t.icon_size / 2.0 + t.unit, at.y);
                                ui.painter().text(count, Align2::LEFT_CENTER, n.to_string(), t.small(), color);
                            }
                        }
                        _ => {}
                    }
                }
                if selected {
                    let line = Rect::from_min_max(
                        egui::pos2(rect.left() + 3.0 * t.unit, rect.bottom() - 2.0),
                        egui::pos2(rect.right() - 3.0 * t.unit, rect.bottom()),
                    );
                    ui.painter().rect_filled(line, 1.0, t.accent);
                }
                widgets::focus_ring(ui, &t, &r, rect.shrink(3.0), t.corner());
            }
            responses.push(r);
        }
        let ids = [responses[0].id, responses[1].id];
        for (i, r) in responses.iter().enumerate() {
            let other = 1 - i;
            if r.has_focus() {
                // Left and Right are the tabs', not egui's moves between controls.
                let keys = egui::EventFilter { horizontal_arrows: true, ..Default::default() };
                ui.memory_mut(|m| m.set_focus_lock_filter(r.id, keys));
            }
            let moved = r.has_focus()
                && ui.input_mut(|inp| {
                    inp.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) | inp.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                });
            if moved {
                ui.memory_mut(|m| m.request_focus(ids[other]));
            }
            if r.clicked() || moved {
                let chosen = if moved { other } else { i };
                ui_cmd::run(app, if chosen == 0 { "view.pane.source" } else { "view.pane.pdf" });
            }
        }
    });
}
