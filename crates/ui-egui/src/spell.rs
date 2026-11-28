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

/// Spellings offered for a word: a few, so the popup and the menu stay small.
const SUGGESTIONS: usize = crate::source::POPUP_FIXES;

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
pub(crate) fn word_of(app: &App, p: &Problem) -> String {
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
            everywhere: 0,
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
    // The formatted view's caret is on its own words.
    if crate::formatted_edit::spelling(app, id, params) {
        return;
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
                        everywhere: 0,
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
        _ => keep(app, id, &word, params),
    }
}

/// Add to Dictionary (`edit.spelling.add`, `{to?}`) or Ignore (`edit.spelling.ignore`) for
/// `word`, saying what was done.
pub(crate) fn keep(app: &mut App, id: &str, word: &str, params: &Value) {
    match id {
        "edit.spelling.add" => {
            let to = params.get("to").and_then(Value::as_str).unwrap_or("user");
            if let Some(r) = app.run("language.add_word", json!({"word": word, "to": to})) {
                let said = match r["kept"].as_str() {
                    Some("project") => tl!("Added “{word}” to the project's words (.texcraft/words.txt)."),
                    Some("session") => tl!("Added “{word}” to the project's words for this session: the project has no folder yet."),
                    _ => tl!("Added “{word}” to your dictionary."),
                };
                app.view.status = Some((said.replace("{word}", word), false));
                app.prefs_changed = true;
            }
        }
        "edit.spelling.ignore" if app.run("language.ignore_word", json!({"word": word})).is_some() => {
            app.view.status = Some((tl!("“{word}” is ignored until TeXCraft closes.").replace("{word}", word), false));
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

/// The status bar's spelling language (issue #37): what is checked now, or that spelling is off;
/// a click (or Enter) opens a menu to take the document's language, another installed one, or to
/// turn spelling off, through `language.spelling_options`.
pub(crate) fn status(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    let options = app.session.settings.spelling.clone();
    let (language, _) = app.session.spelling_language();
    let shown = if options.on { crate::settings::spelling_language_name(&language) } else { tl!("Spelling off").to_owned() };
    let ready = app.session.spelling_ready().is_ok();
    let color = if ready || !options.on { t.text_muted } else { t.warning };
    let icon = if ready || !options.on { "type" } else { "triangle-exclamation-point" };
    let button = egui::Button::image_and_text(crate::icons::image(icon, 13.0, color), egui::RichText::new(&shown).font(t.small()).color(color))
        .frame_when_inactive(false);
    let r = ui.add(button);
    crate::widgets::focus_ring(ui, &t, &r, r.rect, t.corner());
    let help = if options.on { crate::settings::spelling_help(app) } else { tl!("Spelling is off.").to_owned() };
    let r = r.accessible_name(format!("{shown}, {}", tl!("spelling language"))).on_hover_text(help);
    crate::widgets::keyboard_menu(ui, &r);
    let mut chosen: Option<Value> = None;
    egui::Popup::menu(&r).show(|ui| {
        let (automatic, _) = app.session.automatic_spelling_language();
        let auto = tl!("From the document ({language})").replace("{language}", &crate::settings::spelling_language_name(&automatic));
        if ui.selectable_label(options.on && options.language == "auto", auto).clicked() {
            chosen = Some(json!({"on": true, "language": "auto"}));
        }
        ui.separator();
        for (tag, name) in crate::settings::SPELLING_LANGUAGES {
            // Only the languages there is a dictionary for: the others say why in Settings.
            if app.session.spelling_available(tag).is_err() {
                continue;
            }
            if ui.selectable_label(options.on && options.language.eq_ignore_ascii_case(tag), crate::i18n::t(name)).clicked() {
                chosen = Some(json!({"on": true, "language": tag}));
            }
        }
        ui.separator();
        if ui.selectable_label(!options.on, tl!("Off")).clicked() {
            chosen = Some(json!({"on": false}));
        }
        ui.separator();
        crate::settings::menu_link(app, ui, tl!("Spelling Settings…"), "editor", Some(tl!("spelling")));
    });
    if let Some(params) = chosen
        && let Some(done) = app.run("language.spelling_options", params)
    {
        let said = match done["on"].as_bool() {
            Some(false) => tl!("Spelling is off.").to_owned(),
            _ => tl!("Checking {language}.")
                .replace("{language}", &crate::settings::spelling_language_name(done["checking"].as_str().unwrap_or_default())),
        };
        app.view.status = Some((said, false));
    }
}
