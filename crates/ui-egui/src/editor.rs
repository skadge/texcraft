//! The source editor: tabs of the open files and the text of the shown one, highlighted with
//! `texcraft_latex::lex`, with the lines that have build errors marked.
//!
//! This is egui's multi-line text field with a highlighting layouter, an interim surface. It will
//! be replaced by TeXCraft's own editor widget (rope buffer, line numbers, folding, completion,
//! IME through craft-ui's `ime::Composer`, an editable AccessKit node with caret and selection),
//! docs/plan.md phase 2.

use std::collections::HashSet;
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use serde_json::json;
use texcraft_engine::project::{Contents, ProjectPath};

use crate::App;
use crate::theme::Tokens;

/// The text being edited and where it came from.
#[derive(Default)]
pub(crate) struct Editor {
    path: Option<ProjectPath>,
    text: String,
    /// The session revision `text` matches.
    revision: u64,
}

impl Editor {
    /// Reloads the text when another file is shown or the file changed outside the editor.
    fn sync(&mut self, app_path: Option<&ProjectPath>, revision: u64, project: &texcraft_engine::project::Project) {
        if self.path.as_ref() == app_path && self.revision == revision {
            return;
        }
        let same_file = self.path.as_ref() == app_path;
        self.path = app_path.cloned();
        self.revision = revision;
        let fresh = match app_path.and_then(|p| project.get(p)) {
            Some(Contents::Text(t)) => t.clone(),
            _ => String::new(),
        };
        if !same_file || fresh != self.text {
            self.text = fresh;
        }
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    tabs(app, ui);
    let current = app.session.current().cloned();
    app.editor.sync(current.as_ref(), app.session.revision(), app.session.project());
    let Some(path) = current else {
        ui.centered_and_justified(|ui| ui.label(tl!("No file is open. Choose one in the project files.")));
        return;
    };
    match app.session.project().get(&path) {
        Some(Contents::Text(_)) => {}
        Some(Contents::Binary(b)) => {
            ui.label(format!("{} · {} {}", path, b.len(), tl!("bytes")));
            ui.label(tl!("This file is not text and cannot be edited here."));
            return;
        }
        None => return,
    }
    let errors = error_lines(app, &path);
    // The tab shows the file name, so the editor's name starts with it (AGENTS.md rule 21).
    let name = format!("{} {}", path.file_name(), tl!("source"));
    let tokens = app.tokens;
    let id = egui::Id::unique(("editor", path.as_str()));
    if let Some((goto_path, line)) = app.view.goto.take() {
        if goto_path == path {
            move_cursor_to_line(ui.ctx(), id, &app.editor.text, line);
        } else {
            app.view.goto = Some((goto_path, line));
        }
    }
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
        let mut job = highlight(text.as_str(), &tokens, &errors);
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let response = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut app.editor.text)
                    .id(id)
                    .code_editor()
                    .font(tokens.code_font())
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter),
            )
            .accessible_name(name)
        })
        .inner;
    if response.changed() {
        let text = app.editor.text.clone();
        if let Some(r) = app.run("file.write", json!({"path": path.as_str(), "text": text})) {
            app.editor.revision = r.get("revision").and_then(serde_json::Value::as_u64).unwrap_or(app.editor.revision);
        }
        app.view.last_edit = Some(ui.ctx().input(|i| i.time));
    }
}

fn tabs(app: &mut App, ui: &mut egui::Ui) {
    let open: Vec<ProjectPath> = app.session.open_files().to_vec();
    let current = app.session.current().cloned();
    ui.horizontal(|ui| {
        for path in open {
            let selected = current.as_ref() == Some(&path);
            let tab = ui.selectable_label(selected, path.file_name()).on_hover_text(path.as_str());
            if tab.clicked() {
                app.run("file.open", json!({"path": path.as_str()}));
            }
            if ui.small_button("×").on_hover_text(tl!("Close File")).clicked() {
                app.run("file.close", json!({"path": path.as_str()}));
            }
        }
    });
    ui.separator();
}

/// Lines (from 0) of `path` that the last build reported errors on.
fn error_lines(app: &App, path: &ProjectPath) -> HashSet<u32> {
    let Some(outcome) = app.session.last_outcome() else { return HashSet::new() };
    outcome
        .diagnostics
        .iter()
        .filter(|d| d.severity == texcraft_engine::Severity::Error && d.file.as_deref() == Some(path.as_str()))
        .filter_map(|d| d.line.map(|l| l.saturating_sub(1)))
        .collect()
}

fn highlight(text: &str, tokens: &Tokens, errors: &HashSet<u32>) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = tokens.code_font();
    let mut line = 0u32;
    for token in texcraft_latex::lex(text) {
        let color = tokens.syntax(token.kind);
        let Some(run) = text.get(token.range.clone()) else { continue };
        // Split at line ends so each line gets its own background.
        for piece in run.split_inclusive('\n') {
            let background = if errors.contains(&line) { tokens.error_line } else { egui::Color32::TRANSPARENT };
            job.append(piece, 0.0, TextFormat { font_id: font.clone(), color, background, ..Default::default() });
            if piece.ends_with('\n') {
                line += 1;
            }
        }
    }
    job
}

fn move_cursor_to_line(ctx: &egui::Context, id: egui::Id, text: &str, line: u32) {
    let target = line.saturating_sub(1) as usize;
    let chars: usize = text.split_inclusive('\n').take(target).map(|l| l.chars().count()).sum();
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(chars))));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}
