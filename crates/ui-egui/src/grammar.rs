//! Grammar in the editor (docs/ROADMAP.md, E12): the session's grammar problems on the lines in
//! view (and a margin), as problems underlined with a double line in the grammar colour, their
//! replacements as the problem's fixes, and the commands of the text's context menu (replace,
//! ignore, turn off the rule; check the paragraph with AI).

use serde_json::{Value, json};
use texcraft_engine::Severity;
use texcraft_engine::project::ProjectPath;

use crate::App;
use crate::ai::Kind;
use crate::source::{GrammarMark, Problem};

/// Lines checked beyond those in view, above and below.
const MARGIN_LINES: usize = 40;

/// The grammar problems of the lines in view (and a margin) as problems; none when grammar is off
/// or the document is not in English and no model checked it. Passages not checked yet go to the
/// checker in the background; the window repaints until they are back.
pub(crate) fn problems(app: &mut App, path: &ProjectPath, ctx: &egui::Context) -> Vec<Problem> {
    if !crate::spell::checked(path) {
        return Vec::new();
    }
    let shown = app.editor.source.visible_lines();
    let lines = shown.start.saturating_sub(MARGIN_LINES)..shown.end + MARGIN_LINES;
    let found = app.session.grammar_problems(path, Some(lines)).unwrap_or_default();
    if app.session.grammar_busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
    let b = &app.editor.source.doc.buffer;
    let last = b.len_lines().saturating_sub(1);
    let at = |line: usize, column: usize| {
        let line = line.min(last);
        (b.line_to_char(line) + column).min(b.line_end(line))
    };
    found
        .into_iter()
        .filter(|g| g.line <= last)
        .map(|g| {
            let ai = matches!(g.origin, texcraft_engine::grammar::Origin::Ai { .. });
            let fixes = g
                .replacements
                .iter()
                .enumerate()
                .map(|(i, r)| texcraft_engine::Fix {
                    title: if r.is_empty() { tl!("Remove it").to_owned() } else { tl!("Change to “{text}”").replace("{text}", r) },
                    edits: vec![texcraft_engine::TextEdit {
                        line: g.line,
                        column: g.column,
                        end_line: g.end_line,
                        end_column: g.end_column,
                        text: r.clone(),
                    }],
                    preferred: i == 0,
                    problem: Some(g.message.clone()),
                })
                .collect();
            let help = match &g.origin {
                texcraft_engine::grammar::Origin::Ai { model } => vec![tl!("Found by AI ({model}).").replace("{model}", model)],
                texcraft_engine::grammar::Origin::Harper => Vec::new(),
            };
            Problem {
                start: at(g.line, g.column),
                end: at(g.end_line, g.end_column),
                line: g.line,
                severity: Severity::Hint,
                message: g.message,
                help,
                fixes,
                build: false,
                spelling: false,
                grammar: Some(GrammarMark { id: g.id, rule: if ai { None } else { Some(g.rule) } }),
            }
        })
        .collect()
}

/// Runs a grammar command of the editor (`edit.grammar.*`) on the grammar problem at the caret.
pub(crate) fn run(app: &mut App, id: &str, params: &Value) {
    let Some(problem) = app.editor.source.grammar_at_caret().cloned() else {
        app.view.status = Some((tl!("The caret is not on a grammar problem.").to_owned(), true));
        return;
    };
    let Some(mark) = problem.grammar.clone() else { return };
    match id {
        "edit.grammar.replace" => {
            let text = params.get("text").and_then(Value::as_str);
            let fix = problem.fixes.iter().find(|f| text.is_none_or(|t| f.edits.first().is_some_and(|e| e.text == t)));
            match fix {
                Some(fix) => {
                    app.editor.source.apply_fix(fix);
                    app.editor.focus = true;
                }
                None => app.view.status = Some((tl!("There is no such replacement for this problem.").to_owned(), true)),
            }
        }
        "edit.grammar.ignore" if app.run("grammar.ignore", json!({"id": mark.id})).is_some() => {
            app.view.status = Some((tl!("This grammar problem is ignored until TeXCraft closes.").to_owned(), false));
        }
        "edit.grammar.rule_off" => match &mark.rule {
            Some(rule) => {
                if app.run("grammar.rule", json!({"rule": rule, "on": false})).is_some() {
                    app.view.status = Some((tl!("The rule {rule} is off: turn it on again in Settings › Editor.").replace("{rule}", rule), false));
                    app.prefs_changed = true;
                }
            }
            None => app.view.status = Some((tl!("This problem was found by AI, not by a rule: ignore it instead.").to_owned(), true)),
        },
        _ => {}
    }
}

/// Why the paragraph at the caret (or the selection) cannot be checked with AI now, if it cannot.
pub(crate) fn can_check_with_ai(app: &App) -> Result<(), String> {
    crate::ai_make::can_make(app, Kind::Grammar)
}

/// `edit.ai_grammar`: sends the selection, or the paragraph at the caret, for a grammar check.
pub(crate) fn check_with_ai(app: &mut App) {
    if let Err(why) = can_check_with_ai(app) {
        app.view.status = Some((why, true));
        return;
    }
    crate::editor::send_edits(app);
    let Some(path) = app.session.current().cloned() else { return };
    let selection = app.editor.source.doc.selection.primary();
    let (a, z) = (selection.start(), selection.end());
    let (start, end) = (crate::suggest::to_source(app, &path, a), crate::suggest::to_source(app, &path, z));
    let Ok(text) = app.session.project().text(&path) else { return };
    let (start, end) = if end > start { (start, end) } else { paragraph_at(text, start) };
    crate::ai::request(app, Kind::Grammar, json!({"path": path.as_str(), "start": start, "end": end}), None);
}

/// The paragraph around character `at` of `text`: its lines between blank lines, in characters
/// from 0 (empty on a blank line).
fn paragraph_at(text: &str, at: usize) -> (usize, usize) {
    let mut c = 0;
    let mut para: Option<(usize, usize)> = None;
    for line in text.split_inclusive('\n') {
        if line.trim().is_empty() {
            if let Some((s, e)) = para.take()
                && at <= e
            {
                return if at >= s { (s, e) } else { (at, at) };
            }
        } else {
            let body = line.trim_end_matches(['\n', '\r']).chars().count();
            let s = para.map_or(c, |(s, _)| s);
            para = Some((s, c + body));
        }
        c += line.chars().count();
    }
    para.filter(|(s, _)| *s <= at).unwrap_or((at, at))
}
