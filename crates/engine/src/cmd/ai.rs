//! AI assistance (docs/ROADMAP.md, AI1 to AI3): the connection, explaining build messages,
//! rewriting a selection and continuing a paragraph.
//! The work is in [`crate::ai`]; these are its commands, for the window and for agents alike.

use serde_json::{Value, json};

use super::{CommandSpec, always, no_params, string};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    let spec = |id, label, params, enabled, run, changes| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled, run, changes };
    vec![
        spec("ai.status", "AI Status", no_params, always, status, false),
        spec("ai.connect", "Connect a Model", connect_schema, platform, connect, true),
        spec("ai.model", "Choose the Model", model_schema, connected, model, true),
        spec("ai.models", "List the Models", background_schema, connected, models, true),
        spec("ai.forget", "Forget the AI Connection", no_params, always, forget, true),
        spec("ai.project", "AI in This Project", project_schema, always, project, true),
        spec("ai.disclose", "What an AI Request Sends", disclose_schema, connected, disclose, false),
        spec("ai.consent", "Agree to Send", consent_schema, connected, consent, true),
        spec("ai.explain", "Explain", explain_schema, connected, explain, true),
        spec("ai.rewrite", "Rewrite with AI", rewrite_schema, connected, rewrite, true),
        spec("ai.complete", "Suggest the Next Words", complete_schema, connected, complete, true),
        spec("ai.cancel", "Cancel the AI Request", cancel_schema, always, cancel, true),
    ]
}

fn platform(_: &Session) -> Result<()> {
    if crate::ai::available() { Ok(()) } else { Err(crate::ai::UNAVAILABLE.to_owned()) }
}

fn connected(s: &Session) -> Result<()> {
    platform(s)?;
    match s.ai_saved().0.connection {
        Some(_) => Ok(()),
        None => Err("AI is off: connect a model first (Settings › AI).".to_owned()),
    }
}

fn connect_schema() -> Value {
    json!({"type": "object", "properties": {
        "endpoint": {"type": "string", "description": "The OpenAI-compatible API's base address, such as https://api.example.com/v1, or http://127.0.0.1:11434/v1 for a server on this computer."},
        "token": {"type": "string", "description": "The API token, kept in the system's keychain (in this session only where there is none). Leave it out to keep the saved one; empty for an endpoint that needs none."},
        "model": {"type": "string", "description": "The model, as the endpoint lists it (ai.models)."}
    }, "required": ["endpoint"], "additionalProperties": false})
}

fn model_schema() -> Value {
    json!({"type": "object", "properties": {"model": {"type": "string", "description": "As the endpoint lists it (ai.models)."}}, "required": ["model"], "additionalProperties": false})
}

fn background_schema() -> Value {
    json!({"type": "object", "properties": {
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_ai` (the window's status line). Default: wait for it."}
    }, "additionalProperties": false})
}

fn project_schema() -> Value {
    json!({"type": "object", "properties": {
        "enabled": {"type": "boolean", "description": "False turns AI off for the open project (kept in its .texcraft/ai.json; for this session when it has no folder)."}
    }, "required": ["enabled"], "additionalProperties": false})
}

const MESSAGE: &str = "The message: `index` in build.result's diagnostics (from 0), or its `path` and `line` (from 1).";

fn disclose_schema() -> Value {
    json!({"type": "object", "description": "What a request would send, where, and whether the user must agree to its kind first. \
explain: the message (`index`, or `path` and `line`); rewrite: as ai.rewrite; complete: as ai.complete (without `path`, an example).", "properties": {
        "request": {"type": "string", "enum": ["explain", "rewrite", "complete"], "description": "Default: explain."},
        "index": {"type": "integer", "minimum": 0},
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1},
        "start": {"type": "integer", "minimum": 0},
        "end": {"type": "integer", "minimum": 0},
        "how": {"type": "string", "enum": HOWS},
        "language": {"type": "string"},
        "at": {"type": "integer", "minimum": 0}
    }, "additionalProperties": false})
}

const HOWS: [&str; 5] = ["clearer", "shorter", "formal", "grammar", "translate"];

fn rewrite_schema() -> Value {
    json!({"type": "object", "description": "Rewrites characters `start..end` of `path` (from 0). The rewrite arrives as a suggested change on exactly them, by \"AI (<model>)\", never as an edit; it is refused, saying why, when it dropped or altered a label, reference, citation, formula or command.", "properties": {
        "path": {"type": "string"},
        "start": {"type": "integer", "minimum": 0},
        "end": {"type": "integer", "minimum": 0},
        "how": {"type": "string", "enum": HOWS, "description": "clearer, shorter, formal (more formal), grammar (fix grammar and spelling only) or translate."},
        "language": {"type": "string", "description": "For translate: the language, in English words (French, Brazilian Portuguese)."},
        "background": {"type": "boolean", "description": "Return at once; the outcome comes from `Session::poll_ai` and `Session::ai_rewritten`. Default: wait for it."}
    }, "required": ["path", "start", "end", "how"], "additionalProperties": false})
}

fn complete_schema() -> Value {
    json!({"type": "object", "description": "Asks how the paragraph at the caret goes on: sends the paragraph up to `at`. At most once every two seconds; the answer is `text` (null when the text moved on meanwhile).", "properties": {
        "path": {"type": "string"},
        "at": {"type": "integer", "minimum": 0, "description": "The caret, in characters from 0."},
        "background": {"type": "boolean", "description": "Return at once; the continuation comes from `Session::poll_ai` and `Session::ai_ghost`. Default: wait for it."}
    }, "required": ["path", "at"], "additionalProperties": false})
}

fn cancel_schema() -> Value {
    json!({"type": "object", "properties": {
        "only": {"type": "string", "enum": ["completion"], "description": "Cancel only a continuation being asked for or shown (as typing on does). Default: whatever is in flight."}
    }, "additionalProperties": false})
}

fn explain_schema() -> Value {
    json!({"type": "object", "description": MESSAGE, "properties": {
        "index": {"type": "integer", "minimum": 0},
        "path": {"type": "string"},
        "line": {"type": "integer", "minimum": 1},
        "background": {"type": "boolean", "description": "Return at once; the explanation comes from `Session::poll_ai` and `Session::ai_explanation`. Default: wait for it."}
    }, "additionalProperties": false})
}

fn consent_schema() -> Value {
    json!({"type": "object", "properties": {
        "request": {"type": "string", "enum": ["explain", "rewrite", "complete"], "description": "The kind of request agreed to, for the connection's endpoint: what ai.disclose shows it sends."}
    }, "required": ["request"], "additionalProperties": false})
}

fn flag(p: &Value, name: &str) -> bool {
    p.get(name).and_then(Value::as_bool).unwrap_or(false)
}

fn status(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(s.ai_status())
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    let endpoint = string(p, "endpoint")?.to_owned();
    let token = match p.get("token") {
        None | Some(Value::Null) => None,
        Some(t) => Some(t.as_str().ok_or("`token` is a string")?),
    };
    let model = match p.get("model") {
        None | Some(Value::Null) => None,
        Some(m) => Some(m.as_str().ok_or("`model` is a string")?),
    };
    s.ai_connect(&endpoint, token, model)
}

fn model(s: &mut Session, p: &Value) -> Result<Value> {
    let m = string(p, "model")?.to_owned();
    s.ai_set_model(&m)
}

fn models(s: &mut Session, p: &Value) -> Result<Value> {
    s.ai_list_models(cfg!(target_arch = "wasm32") || flag(p, "background"))
}

fn forget(s: &mut Session, _: &Value) -> Result<Value> {
    s.ai_forget()
}

fn project(s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or("`enabled` (true or false) is missing")?;
    s.ai_set_project(enabled)
}

fn disclose(s: &mut Session, p: &Value) -> Result<Value> {
    s.ai_disclose(p)
}

fn consent(s: &mut Session, p: &Value) -> Result<Value> {
    // Agreeing to send the project's text elsewhere is the person's to give, at the window: an
    // agent session (confined to its roots) cannot give it for them.
    if s.roots.is_some() {
        return Err("Agreeing to send text to an AI service is for the person at the window, not an agent session.".to_owned());
    }
    let request = string(p, "request")?.to_owned();
    s.ai_consent(&request)
}

fn explain(s: &mut Session, p: &Value) -> Result<Value> {
    s.ai_explain(p, cfg!(target_arch = "wasm32") || flag(p, "background"))
}

fn rewrite(s: &mut Session, p: &Value) -> Result<Value> {
    s.ai_rewrite(p, cfg!(target_arch = "wasm32") || flag(p, "background"))
}

fn complete(s: &mut Session, p: &Value) -> Result<Value> {
    s.ai_complete(p, cfg!(target_arch = "wasm32") || flag(p, "background"))
}

fn cancel(s: &mut Session, p: &Value) -> Result<Value> {
    let only = match p.get("only").and_then(Value::as_str) {
        None => false,
        Some("completion") => true,
        Some(other) => return Err(format!("`only` is \"completion\", not `{other}`.")),
    };
    Ok(json!({"cancelled": s.ai_cancel(only)}))
}
