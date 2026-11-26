//! The command registry. Each command has a stable id (`file.new`), an English label (the
//! translation key), the menu it appears in, an optional default shortcut, a description of its
//! parameters for agents, an enablement check that says why it is disabled, and the function that
//! runs it. Frontends list commands from here to build menus, the command palette and the agent
//! tool list, so the three never disagree.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::{Result, Session};

mod ai;
mod build;
pub(crate) mod clone;
mod file;
mod floats;
pub(crate) mod forge;
pub(crate) mod forge_auth;
pub(crate) mod forge_pick;
pub(crate) mod git;
mod grammar;
mod history;
mod image;
mod language;
mod pdf;
mod project;
mod review;
mod search;
mod template;

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> Result<()>;

pub struct CommandSpec {
    pub id: &'static str,
    /// English, the translation key.
    pub label: &'static str,
    /// Menu path, outermost first; empty for commands that are not in a menu.
    pub menu: &'static [&'static str],
    /// Default shortcut in craft-keymap's notation (`Cmd+S`: Ctrl on Linux and Windows).
    pub shortcut: Option<&'static str>,
    /// The parameters, for agents: a JSON-schema object.
    pub params: fn() -> Value,
    pub enabled: Enabled,
    pub run: Run,
    /// Whether the command changes the session (false for queries).
    pub changes: bool,
}

impl CommandSpec {
    /// What `engine.commands` and the agent tools list.
    pub fn info(&self, session: &Session) -> Value {
        let enabled = (self.enabled)(session);
        json!({
            "id": self.id,
            "label": self.label,
            "menu": self.menu,
            "shortcut": self.shortcut,
            "params": (self.params)(),
            "changes": self.changes,
            "enabled": enabled.is_ok(),
            "disabledReason": enabled.err(),
        })
    }
}

pub use image::{is_image, relative as image_path};

pub fn always(_: &Session) -> Result<()> {
    Ok(())
}

pub fn no_params() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

/// Every command, in menu order.
pub fn all() -> &'static [CommandSpec] {
    static ALL: OnceLock<Vec<CommandSpec>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v = project::specs();
        v.extend(file::specs());
        v.extend(search::specs());
        v.extend(image::specs());
        v.extend(floats::specs());
        v.extend(build::specs());
        v.extend(template::specs());
        v.extend(language::specs());
        v.extend(grammar::specs());
        v.extend(pdf::specs());
        v.extend(review::specs());
        v.extend(history::specs());
        v.extend(forge::specs());
        v.extend(forge_auth::specs());
        v.extend(forge_pick::specs());
        v.extend(git::specs());
        v.extend(clone::specs());
        v.extend(ai::specs());
        v
    })
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    static INDEX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| all().iter().enumerate().map(|(i, c)| (c.id, i)).collect());
    index.get(id).and_then(|i| all().get(*i))
}

/// The string parameter `name`, or an error naming it.
pub(crate) fn string<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params.get(name).and_then(Value::as_str).ok_or_else(|| format!("the parameter `{name}` (a string) is missing"))
}

pub(crate) fn path_param(params: &Value, name: &str) -> Result<texcraft_project::ProjectPath> {
    texcraft_project::ProjectPath::new(string(params, name)?).map_err(|e| e.to_string())
}
