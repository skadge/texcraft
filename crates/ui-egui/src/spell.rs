//! Spelling in the editor (docs/ROADMAP.md, E11): the session's misspellings of the lines in view
//! as problems underlined in the spelling colour, their spellings as the problem's fixes, and the
//! commands of the text's context menu (change, add to the dictionary, ignore, next).

use serde_json::{Value, json};
use texcraft_engine::Severity;
use texcraft_engine::project::ProjectPath;

use crate::App;
use crate::source::Problem;

/// Lines checked beyond those in view, above and below, so a short scroll shows marks at once.
const MARGIN_LINES: usize = 40;

/// Spellings offered for a word.
const SUGGESTIONS: usize = 6;

/// Whether `path` is checked: LaTeX sources.
pub(crate) fn checked(path: &ProjectPath) -> bool {
    matches!(path.extension().as_str(), "tex" | "ltx")
}

/// The message of a misspelled word.
fn message(word: &str) -> String {
    tl!("“{word}” is not in the dictionary").replace("{word}", word)
}

/// The misspelled words of the lines in view (and a margin) as problems; none when spelling is off
/// or the language has no dictionary (Settings › Editor says why).
pub(crate) fn problems(app: &mut App, path: &ProjectPath) -> Vec<Problem> {
    if !checked(path) {
        return Vec::new();
    }
    let shown = app.editor.source.visible_lines();
    let lines = shown.start.saturating_sub(MARGIN_LINES)..shown.end + MARGIN_LINES;
    let Ok(found) = app.session.misspellings(path, Some(lines)) else { return Vec::new() };
    let b = &app.editor.source.doc.buffer;
    let last = b.len_lines().saturating_sub(1);
    found
        .into_iter()
        .filter(|m| m.line <= last)
        .map(|m| {
            let base = b.line_to_char(m.line);
            let end = b.line_end(m.line);
            Problem {
                start: (base + m.column).min(end),
                end: (base + m.end_column).min(end),
                line: m.line,
                severity: Severity::Hint,
                message: message(&m.word),
                help: Vec::new(),
                fixes: Vec::new(),
                build: false,
                spelling: true,
                grammar: None,
            }
        })
        .collect()
}

/// The word a problem underlines, as the buffer has it.
fn word_of(app: &App, p: &Problem) -> String {
    app.editor.source.doc.buffer.slice(p.start, p.end)
}

/// A misspelling's fixes: each spelling the dictionary offers, replacing the word.
pub(crate) fn fixes(app: &mut App, p: &Problem) -> Vec<texcraft_engine::Fix> {
    let word = word_of(app, p);
    let column = p.start - app.editor.source.doc.buffer.line_to_char(p.line);
    let end_column = column + (p.end - p.start);
    suggestions(app, &word)
        .into_iter()
        .enumerate()
        .map(|(i, s)| texcraft_engine::Fix {
            title: tl!("Change to “{text}”").replace("{text}", &s),
            edits: vec![texcraft_engine::TextEdit { line: p.line, column, end_line: p.line, end_column, text: s }],
            preferred: i == 0,
            problem: Some(p.message.clone()),
        })
        .collect()
}

/// The spellings offered for `word`, kept while it is the word asked about (a menu asks every
/// frame it is open).
pub(crate) fn suggestions(app: &mut App, word: &str) -> Vec<String> {
    if let Some((w, s)) = &app.editor.spelling_suggestions
        && w == word
    {
        return s.clone();
    }
    let s = app.session.spelling_suggestions(word, SUGGESTIONS);
    app.editor.spelling_suggestions = Some((word.to_owned(), s.clone()));
    s
}

/// Runs a spelling command of the editor (`edit.spelling.*`) on the misspelled word at the caret.
pub(crate) fn run(app: &mut App, id: &str, params: &Value) {
    if id == "edit.spelling.next" {
        return next(app);
    }
    let Some(problem) = app.editor.source.misspelling_at_caret().cloned() else {
        app.view.status = Some((tl!("The caret is not on a misspelled word.").to_owned(), true));
        return;
    };
    let word = word_of(app, &problem);
    match id {
        "edit.spelling.replace" => {
            let fixes = fixes(app, &problem);
            let chosen = match params.get("text").and_then(Value::as_str) {
                Some(text) => fixes.into_iter().find(|f| f.edits.first().is_some_and(|e| e.text == text)).or_else(|| {
                    // A spelling the dictionary did not offer is still the user's choice.
                    let column = problem.start - app.editor.source.doc.buffer.line_to_char(problem.line);
                    Some(texcraft_engine::Fix {
                        title: String::new(),
                        edits: vec![texcraft_engine::TextEdit {
                            line: problem.line,
                            column,
                            end_line: problem.line,
                            end_column: column + (problem.end - problem.start),
                            text: text.to_owned(),
                        }],
                        preferred: true,
                        problem: None,
                    })
                }),
                None => fixes.into_iter().next(),
            };
            match chosen {
                Some(fix) => {
                    app.editor.source.apply_fix(&fix);
                    app.editor.focus = true;
                }
                None => app.view.status = Some((tl!("The dictionary has no spelling to offer for “{word}”.").replace("{word}", &word), true)),
            }
        }
        "edit.spelling.add" => {
            let to = params.get("to").and_then(Value::as_str).unwrap_or("user");
            if let Some(r) = app.run("language.add_word", json!({"word": word, "to": to})) {
                let said = match r["kept"].as_str() {
                    Some("project") => tl!("Added “{word}” to the project's words (.texcraft/words.txt)."),
                    Some("session") => tl!("Added “{word}” to the project's words for this session: the project has no folder yet."),
                    _ => tl!("Added “{word}” to your dictionary."),
                };
                app.view.status = Some((said.replace("{word}", &word), false));
                app.prefs_changed = true;
            }
        }
        "edit.spelling.ignore" if app.run("language.ignore_word", json!({"word": word})).is_some() => {
            app.view.status = Some((tl!("“{word}” is ignored until TeXCraft closes.").replace("{word}", &word), false));
        }
        _ => {}
    }
}

/// Moves the caret to the next misspelled word or grammar problem of the file, from the top after
/// the last one.
fn next(app: &mut App) {
    let Some(path) = app.session.current().cloned().filter(checked) else {
        app.view.status = Some((tl!("Open a LaTeX file to check its spelling.").to_owned(), true));
        return;
    };
    let spelling = app.session.misspellings(&path, None);
    // What the grammar checker found so far (it goes on in the background).
    let grammar = app.session.grammar_problems(&path, None);
    let (misspelled, grammar) = match (spelling, grammar) {
        (Err(why), Err(_)) => {
            app.view.status = Some((why, true));
            return;
        }
        (s, g) => (s.unwrap_or_default(), g.unwrap_or_default()),
    };
    let source = &mut app.editor.source;
    let b = &source.doc.buffer;
    let last = b.len_lines().saturating_sub(1);
    let at = |line: usize, column: usize| (b.line_to_char(line.min(last)) + column).min(b.line_end(line.min(last)));
    // Each problem: where it starts and ends, and what to say.
    let mut all: Vec<(usize, usize, String)> =
        misspelled.iter().map(|m| (at(m.line, m.column), at(m.line, m.end_column), message(&m.word))).collect();
    all.extend(grammar.into_iter().map(|g| (at(g.line, g.column), at(g.end_line, g.end_column), g.message)));
    all.sort_by_key(|(s, ..)| *s);
    let caret = source.caret();
    let target = all.iter().find(|(s, ..)| *s > caret).or(all.first()).cloned();
    match target {
        Some((start, end, said)) => {
            source.select(start, end);
            source.reveal = true;
            app.editor.focus = true;
            app.view.status = Some((said, false));
        }
        None => app.view.status = Some((tl!("No misspelled words or grammar problems in this file.").to_owned(), false)),
    }
}
