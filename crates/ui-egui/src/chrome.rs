//! The parts around the editor and the preview: the menu bar, the rail, the side panel (project
//! files, outline), the preview's toolbar, the build log and the one-line prompt dialog.

use serde_json::json;
use texcraft_engine::Severity;
use texcraft_engine::project::{Contents, ProjectPath};

use crate::ui_cmd;
use crate::{App, Side};

pub(crate) fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("side").resizable(true).default_size(220.0).min_size(160.0).show(ui, |ui| match app.view.side {
        Some(Side::Files) => files(app, ui),
        Some(Side::Outline) => outline(app, ui),
        None => {}
    });
}

fn files(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.strong(tl!("Project Files"));
        if ui.small_button("+").on_hover_text(tl!("New File…")).clicked() {
            ui_cmd::run(app, "file.new");
        }
    });
    ui.separator();
    let main = app.session.project().main().cloned();
    let current = app.session.current().cloned();
    let entries: Vec<(ProjectPath, bool)> = app.session.project().files().map(|(p, c)| (p.clone(), matches!(c, Contents::Text(_)))).collect();
    let mut action = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut folder = String::new();
        for (path, _is_text) in &entries {
            let dir = path.as_str().rsplit_once('/').map_or("", |(d, _)| d);
            if dir != folder {
                folder = dir.to_owned();
                if !dir.is_empty() {
                    ui.label(egui::RichText::new(format!("{dir}/")).color(app.tokens.text_muted));
                }
            }
            let marker = if main.as_ref() == Some(path) { tl!(" (main)") } else { "" };
            let indent = if dir.is_empty() { 0.0 } else { 12.0 };
            ui.horizontal(|ui| {
                ui.add_space(indent);
                let r = ui.selectable_label(current.as_ref() == Some(path), format!("{}{marker}", path.file_name()));
                if r.clicked() {
                    action = Some(("file.open", path.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button(tl!("Set as Main Document")).clicked() {
                        action = Some(("file.set_main", path.clone()));
                        ui.close();
                    }
                    if ui.button(tl!("Delete")).clicked() {
                        action = Some(("file.delete", path.clone()));
                        ui.close();
                    }
                });
            });
        }
    });
    match action {
        Some(("file.delete", path)) => {
            app.view.dialog = Some(crate::dialogs::Dialog::Confirm(crate::dialogs::Confirm {
                title: tl!("Delete {file}?").replace("{file}", path.file_name()),
                message: tl!("The file is removed from the project. This cannot be undone.").to_owned(),
                action: tl!("Delete"),
                command: "file.delete",
                params: json!({"path": path.as_str()}),
            }));
        }
        Some((id, path)) => {
            app.run(id, json!({"path": path.as_str()}));
        }
        None => {}
    }
}

fn outline(app: &mut App, ui: &mut egui::Ui) {
    ui.strong(tl!("Outline"));
    ui.separator();
    let Some(path) = app.session.current().cloned() else { return };
    let Ok(text) = app.session.project().text(&path) else { return };
    let headings = texcraft_latex::outline(text);
    if headings.is_empty() {
        ui.label(egui::RichText::new(tl!("No sections in this file.")).color(app.tokens.text_muted));
    }
    for h in headings {
        let indent = f32::from(h.level.saturating_sub(1)) * 10.0;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            if ui.selectable_label(false, h.title.as_str()).clicked() {
                app.view.goto = Some((path.clone(), h.line + 1));
            }
        });
    }
}

pub(crate) fn preview_toolbar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let building = app.session.is_building();
        let label = if building { tl!("Compiling…") } else { tl!("Compile") };
        let compile = egui::Button::new(egui::RichText::new(label).color(app.tokens.on_accent)).fill(app.tokens.accent);
        if ui.add_enabled(!building, compile).clicked() {
            app.run("build.compile", json!({}));
        }
        let mut auto = app.session.settings.auto_compile;
        if ui.checkbox(&mut auto, tl!("Automatic")).changed() {
            app.run("build.auto", json!({"on": auto}));
        }
        let errors = app.session.last_outcome().map_or(0, |o| o.errors());
        let logs = if errors > 0 { format!("{} ({errors})", tl!("Log")) } else { tl!("Log").to_owned() };
        if ui.selectable_label(app.view.logs, logs).clicked() {
            app.view.logs = !app.view.logs;
        }
        ui.separator();
        if ui.button("−").on_hover_text(tl!("Zoom Out")).clicked() {
            app.pdf.zoom_by(0.8);
        }
        ui.label(format!("{:.0} %", app.pdf.zoom() * 100.0));
        if ui.button("+").on_hover_text(tl!("Zoom In")).clicked() {
            app.pdf.zoom_by(1.25);
        }
        if ui.button(tl!("Fit Width")).clicked() {
            app.pdf.fit_width();
        }
        if app.pdf.page_count() > 0 {
            ui.label(format!("{} {} / {}", tl!("Page"), app.pdf.page + 1, app.pdf.page_count()));
        }
    });
    if let Some((message, is_error)) = &app.view.status {
        let color = if *is_error { app.tokens.error } else { app.tokens.text_muted };
        ui.colored_label(color, message.as_str());
    }
    ui.separator();
}

pub(crate) fn logs(app: &mut App, ui: &mut egui::Ui) {
    let Some(outcome) = app.session.last_outcome() else {
        ui.label(tl!("Nothing has been compiled yet."));
        return;
    };
    let mut goto = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for severity in [Severity::Error, Severity::Warning, Severity::Note, Severity::Hint] {
            let items: Vec<_> = outcome.diagnostics.iter().filter(|d| d.severity == severity).collect();
            if items.is_empty() {
                continue;
            }
            let (title, color) = match severity {
                Severity::Error => (tl!("Errors"), app.tokens.error),
                Severity::Warning => (tl!("Warnings"), app.tokens.warning),
                Severity::Note => (tl!("Notes"), app.tokens.note),
                Severity::Hint => (tl!("Hints"), app.tokens.text_muted),
            };
            ui.colored_label(color, format!("{title} ({})", items.len()));
            for d in items {
                let place = match (&d.file, d.line) {
                    (Some(f), Some(l)) => format!("{f}:{l}"),
                    (Some(f), None) => f.clone(),
                    _ => String::new(),
                };
                // The severity is written out, so colour is not the only signal (rule 21).
                let text = format!("{}  {place}  {}", title, d.message);
                let r = ui.selectable_label(false, text);
                for h in &d.help {
                    ui.label(egui::RichText::new(h.as_str()).color(app.tokens.text_muted));
                }
                if r.clicked()
                    && let (Some(f), Some(l)) = (&d.file, d.line)
                    && let Ok(p) = ProjectPath::new(f)
                {
                    goto = Some((p, l));
                }
            }
            ui.separator();
        }
        egui::CollapsingHeader::new(tl!("Raw log")).show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(outcome.log.as_str()).monospace()).wrap());
        });
    });
    if let Some((path, line)) = goto {
        app.run("file.open", json!({"path": path.as_str()}));
        app.view.goto = Some((path, line));
    }
}
