//! The language server's answers for agents: completion, hover, definition and the lint, as the
//! editor gets them. Lines and columns are from 1, in characters, as `file.find` gives them.

use serde_json::{Value, json};

use super::{CommandSpec, always, path_param};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    let spec =
        |id, label, params: fn() -> Value, run| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, changes: false };
    vec![
        spec("language.complete", "Completions", at_schema, complete),
        spec("language.hover", "Describe", at_schema, hover),
        spec("language.definition", "Go to Definition", at_schema, definition),
        spec("language.lint", "Hints", path_schema, lint),
        spec("language.formulas", "Typeset Formulas", path_schema, formulas),
        spec("language.fixes", "Quick Fixes", line_schema, fixes),
        CommandSpec {
            id: "language.fix",
            label: "Apply Quick Fix",
            menu: &[],
            shortcut: None,
            params: fix_schema,
            enabled: always,
            run: fix,
            changes: true,
        },
        spec("language.spelling", "Check Spelling", spelling_schema, spelling),
        CommandSpec {
            id: "language.spelling_options",
            label: "Spelling Options",
            menu: &[],
            shortcut: None,
            params: options_schema,
            enabled: always,
            run: spelling_options,
            changes: true,
        },
        CommandSpec {
            id: "language.add_word",
            label: "Add to Dictionary",
            menu: &[],
            shortcut: None,
            params: add_word_schema,
            enabled: always,
            run: add_word,
            changes: true,
        },
        CommandSpec {
            id: "language.ignore_word",
            label: "Ignore Word",
            menu: &[],
            shortcut: None,
            params: word_schema,
            enabled: always,
            run: ignore_word,
            changes: true,
        },
    ]
}

fn spelling_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1, "description": "The first line to check, from 1; the whole file without."},
        "end_line": {"type": "integer", "minimum": 1, "description": "The last line to check, from 1."},
        "suggestions": {"type": "integer", "minimum": 0, "maximum": 20, "description": "Spellings offered per word (5 without)."}
    }, "required": ["path"], "additionalProperties": false})
}

fn options_schema() -> Value {
    json!({"type": "object", "properties": {
        "on": {"type": "boolean"},
        "language": {"type": "string", "description": "A BCP 47 tag (en-GB, fr), or `auto`: the document's babel or polyglossia language, else the interface's."},
        "comments": {"type": "boolean", "description": "Check the text of comments too."}
    }, "additionalProperties": false})
}

fn add_word_schema() -> Value {
    json!({"type": "object", "properties": {
        "word": {"type": "string"},
        "to": {"type": "string", "enum": ["user", "project"], "description": "The user's words (every project; the default), or the project's .texcraft/words.txt."}
    }, "required": ["word"], "additionalProperties": false})
}

fn word_schema() -> Value {
    json!({"type": "object", "properties": {"word": {"type": "string"}}, "required": ["word"], "additionalProperties": false})
}

/// The file's misspelled words with their places and suggestions; an error when spelling is off or
/// the language has no dictionary.
fn spelling(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let line = |name: &str| p.get(name).and_then(Value::as_u64).filter(|n| *n >= 1).map(|n| n as usize);
    let lines = match (line("line"), line("end_line")) {
        (None, None) => None,
        (from, to) => Some(from.unwrap_or(1) - 1..to.unwrap_or(usize::MAX)),
    };
    let max = p.get("suggestions").and_then(Value::as_u64).map_or(5, |n| n.min(20) as usize);
    let found = s.misspellings(&path, lines)?;
    let (language, from) = s.spelling_language();
    let words: Vec<Value> = found
        .into_iter()
        .map(|m| {
            let suggestions = if max > 0 { s.spelling_suggestions(&m.word, max) } else { Vec::new() };
            json!({"line": m.line + 1, "column": m.column + 1, "end_line": m.line + 1, "end_column": m.end_column + 1, "word": m.word, "suggestions": suggestions})
        })
        .collect();
    Ok(json!({"language": language, "language_from": from, "misspellings": words}))
}

fn spelling_options(s: &mut Session, p: &Value) -> Result<Value> {
    let mut o = s.settings.spelling.clone();
    if let Some(on) = p.get("on").and_then(Value::as_bool) {
        o.on = on;
    }
    if let Some(language) = p.get("language").and_then(Value::as_str) {
        let language = language.trim();
        if language != "auto" && craft_proof::lang::normalize_tag(language).is_none() {
            return Err(format!("“{language}” is not a language tag: give one such as en-GB or fr, or `auto`."));
        }
        o.language = language.to_owned();
    }
    if let Some(comments) = p.get("comments").and_then(Value::as_bool) {
        o.comments = comments;
    }
    s.settings.spelling = o.clone();
    let (language, from) = s.spelling_language();
    let ready = s.spelling_ready();
    Ok(
        json!({"on": o.on, "language": o.language, "comments": o.comments, "checking": language, "language_from": from, "available": ready.is_ok(), "problem": ready.err()}),
    )
}

fn add_word(s: &mut Session, p: &Value) -> Result<Value> {
    let word = super::string(p, "word")?;
    let to_project = match p.get("to").and_then(Value::as_str) {
        None | Some("user") => false,
        Some("project") => true,
        Some(other) => return Err(format!("`to` is `user` or `project`, not `{other}`.")),
    };
    let kept = s.add_word(word, to_project)?;
    Ok(json!({"word": word.trim(), "kept": kept}))
}

fn ignore_word(s: &mut Session, p: &Value) -> Result<Value> {
    let word = super::string(p, "word")?;
    s.ignore_word(word)?;
    Ok(json!({"word": word.trim()}))
}

fn at_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1, "description": "From 1."},
        "column": {"type": "integer", "minimum": 1, "description": "From 1, in characters; the caret is before this character."}
    }, "required": ["path", "line", "column"], "additionalProperties": false})
}

fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false})
}

fn line_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}, "line": {"type": "integer", "minimum": 1}}, "required": ["path", "line"], "additionalProperties": false})
}

fn fix_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1},
        "index": {"type": "integer", "minimum": 0, "description": "Which of language.fixes's fixes for the line."}
    }, "required": ["path", "line", "index"], "additionalProperties": false})
}

fn line_of(p: &Value) -> Result<(texcraft_project::ProjectPath, usize)> {
    let path = path_param(p, "path")?;
    let line = p.get("line").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or("`line` (from 1) is missing")?;
    Ok((path, line as usize - 1))
}

fn fixes(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line) = line_of(p)?;
    let fixes: Vec<Value> = s.fixes(&path, line).iter().map(|f| json!({"title": f.title, "problem": f.problem, "preferred": f.preferred})).collect();
    Ok(json!({"fixes": fixes}))
}

/// Applies a fix to the file's text.
fn fix(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line) = line_of(p)?;
    let index = p.get("index").and_then(Value::as_u64).ok_or("`index` is missing")? as usize;
    let fix = s.fixes(&path, line).into_iter().nth(index).ok_or("There is no such fix on that line.")?;
    let text = s.project().text(&path).map_err(|e| e.to_string())?.to_owned();
    let new = crate::apply_edits(&text, &fix.edits)?;
    s.execute("file.write", &json!({"path": path.as_str(), "text": new}))?;
    Ok(json!({"applied": fix.title}))
}

fn at(p: &Value) -> Result<(texcraft_project::ProjectPath, usize, usize)> {
    let path = path_param(p, "path")?;
    let n = |name: &str| -> Result<usize> {
        p.get(name).and_then(Value::as_u64).filter(|n| *n >= 1).map(|n| n as usize - 1).ok_or_else(|| format!("`{name}` (from 1) is missing"))
    };
    Ok((path, n("line")?, n("column")?))
}

fn complete(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line, column) = at(p)?;
    let items: Vec<Value> = s
        .complete(&path, line, column)
        .into_iter()
        .map(|c| json!({"label": c.label, "detail": c.detail, "text": c.text, "replace_from_column": c.from + 1}))
        .collect();
    Ok(json!({"items": items}))
}

fn hover(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line, column) = at(p)?;
    Ok(json!({"markdown": s.hover(&path, line, column)}))
}

fn definition(s: &mut Session, p: &Value) -> Result<Value> {
    let (path, line, column) = at(p)?;
    Ok(match s.definition(&path, line, column) {
        Some((file, l, c)) => json!({"path": file.as_str(), "line": l + 1, "column": c + 1}),
        None => Value::Null,
    })
}

fn lint(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let hints: Vec<Value> = s
        .lint(&path)
        .into_iter()
        .map(|l| json!({"line": l.line + 1, "column": l.column + 1, "end_line": l.end_line + 1, "end_column": l.end_column + 1, "severity": l.severity, "message": l.message}))
        .collect();
    Ok(json!({"hints": hints}))
}

/// The file's formulas typeset as the formatted view draws them, after waiting for the batch:
/// each one's box in TeX points, or why it was not typeset.
fn formulas(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    s.formulas(&path)?;
    s.wait_for_formulas();
    let found = s.formulas(&path)?;
    let text = s.project().text(&path).map_err(|e| e.to_string())?;
    let line_of = |byte: usize| text.get(..byte).map_or(0, |t| t.matches('\n').count()) + 1;
    let items: Vec<Value> = found
        .formulas
        .iter()
        .map(|f| {
            let mut v = json!({"line": line_of(f.range.start), "display": f.display, "source": f.source});
            let state = match &f.state {
                crate::FormulaState::Typeset(b) => {
                    json!({"status": "typeset", "width_pt": b.width_pt, "height_pt": b.height_pt, "depth_pt": b.depth_pt})
                }
                crate::FormulaState::Failed(messages) => json!({"status": "error", "messages": messages}),
                crate::FormulaState::Pending => json!({"status": "pending"}),
                crate::FormulaState::Unavailable(why) => json!({"status": "unavailable", "messages": [why]}),
            };
            if let (Some(v), Some(state)) = (v.as_object_mut(), state.as_object()) {
                v.extend(state.clone());
            }
            v
        })
        .collect();
    Ok(json!({"body_pt": found.body_pt, "formulas": items}))
}
