//! The parts around the editor and the preview: the menu bar, the rail, the side panel (project
//! files, outline), the preview's toolbar, the build log and the one-line prompt dialog.

use serde_json::json;
use texcraft_engine::Severity;
use texcraft_engine::project::ProjectPath;

use crate::App;

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
