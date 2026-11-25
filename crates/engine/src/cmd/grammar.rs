//! Grammar for agents (docs/ROADMAP.md, E12): a file's grammar problems with their places and
//! replacements, taking one, ignoring one, and the rules and groups switched on or off. Lines and
//! columns are from 1, in characters, as `language.spelling` gives them.

use serde_json::{Value, json};

use super::{CommandSpec, always, path_param, string};
use crate::grammar::{GROUPS, GrammarProblem, Origin};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    let spec =
        |id, label, params: fn() -> Value, run, changes| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, changes };
    vec![
        spec("grammar.check", "Check Grammar", path_schema, check, false),
        spec("grammar.fix", "Take a Grammar Replacement", fix_schema, fix, true),
        spec("grammar.ignore", "Ignore Grammar Problem", ignore_schema, ignore, true),
        spec("grammar.rule", "Switch a Grammar Rule", rule_schema, rule, true),
        spec("grammar.options", "Grammar Options", options_schema, options, true),
    ]
}

fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false})
}

fn fix_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string"},
        "id": {"type": "string", "description": "The problem's id, as grammar.check gives it."},
        "replacement": {"type": "string", "description": "One of the problem's replacements; its first without."}
    }, "required": ["path", "id"], "additionalProperties": false})
}

fn ignore_schema() -> Value {
    json!({"type": "object", "properties": {
        "id": {"type": "string", "description": "The problem's id, as grammar.check gives it: ignored until TeXCraft closes."}
    }, "required": ["id"], "additionalProperties": false})
}

fn rule_schema() -> Value {
    let groups: Vec<&str> = GROUPS.iter().map(|(g, _)| *g).collect();
    json!({"type": "object", "properties": {
        "rule": {"type": "string", "description": "A Harper rule's key, as grammar.check gives it (`rule`)."},
        "group": {"type": "string", "enum": groups},
        "on": {"type": "boolean"}
    }, "required": ["on"], "additionalProperties": false})
}

fn options_schema() -> Value {
    json!({"type": "object", "properties": {
        "on": {"type": "boolean", "description": "Check English documents' grammar offline."}
    }, "additionalProperties": false})
}

fn problem_json(p: &GrammarProblem) -> Value {
    let from = match &p.origin {
        Origin::Harper => json!("harper"),
        Origin::Ai { model } => json!(format!("AI ({model})")),
    };
    json!({
        "line": p.line + 1, "column": p.column + 1, "end_line": p.end_line + 1, "end_column": p.end_column + 1,
        "rule": p.rule, "group": p.group, "message": p.message, "replacements": p.replacements, "id": p.id, "from": from,
    })
}

/// Every grammar problem of the file, after the checker is done with it.
fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let found = s.check_grammar(&path)?;
    let (language, _) = s.spelling_language();
    let problems: Vec<Value> = found.iter().map(problem_json).collect();
    Ok(json!({"language": language, "problems": problems}))
}

/// Puts a problem's replacement in place of the words it flags.
fn fix(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let id = string(p, "id")?;
    let found = s.check_grammar(&path)?;
    let problem = found.into_iter().find(|f| f.id == id).ok_or("There is no grammar problem with that id in the file (it may be fixed).")?;
    let replacement = match p.get("replacement").and_then(Value::as_str) {
        Some(r) if problem.replacements.iter().any(|x| x == r) => r.to_owned(),
        Some(_) => return Err("That is not one of the problem's replacements.".to_owned()),
        None => problem.replacements.first().cloned().ok_or("This problem has no replacement to take: change the text yourself.")?,
    };
    let mut text = s.project().text(&path).map_err(|e| e.to_string())?.to_owned();
    text.replace_range(problem.range.clone(), &replacement);
    s.execute("file.write", &json!({"path": path.as_str(), "text": text}))?;
    Ok(json!({"replaced": problem.message, "with": replacement}))
}

fn ignore(s: &mut Session, p: &Value) -> Result<Value> {
    let id = string(p, "id")?;
    s.ignore_grammar(id)?;
    Ok(json!({"ignored": id}))
}

fn rule(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).ok_or("the parameter `on` (true or false) is missing")?;
    let rule = p.get("rule").and_then(Value::as_str);
    let group = p.get("group").and_then(Value::as_str);
    s.set_grammar_rule(rule, group, on)?;
    options(s, &json!({}))
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(on) = p.get("on").and_then(Value::as_bool) {
        s.settings.grammar.on = on;
    }
    let o = &s.settings.grammar;
    let groups: Vec<Value> = GROUPS.iter().map(|(id, label)| json!({"group": id, "label": label, "on": !o.groups_off.contains(*id)})).collect();
    let ready = s.grammar_ready();
    Ok(json!({
        "on": o.on,
        "groups": groups,
        "rules_off": o.rules_off,
        "checking": ready.is_ok(),
        "problem": ready.err(),
    }))
}
