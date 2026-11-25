//! TeXCraft's agent tools over MCP (stdio), on craft-mcp-server. The tools are the engine's
//! command registry: `command_list` describes every command with its parameters and whether it is
//! enabled now, `command_run` runs one. Agents therefore reach exactly what the interface reaches
//! (AGENTS.md rule 14), and nothing else.
//!
//! [`serve`] refuses to start without read and write roots (rule 18); the session it drives is
//! confined to them ([`texcraft_engine::file_access`]).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use craft_mcp_server::{Effect, Info, Server, Tools, object, string, text_result, tool};
use serde_json::{Value, json};
use texcraft_engine::Session;
use texcraft_engine::file_access::AutomationRoots;
use texcraft_engine::guard::catch_panic;

pub struct TexTools {
    session: Session,
}

impl TexTools {
    pub fn new(session: Session) -> Self {
        Self { session }
    }
}

impl Tools for TexTools {
    fn instructions(&self) -> String {
        "TeXCraft edits and builds LaTeX projects. Call `command_list` to see every command and its \
         parameters, then `command_run`. Open a project folder inside the write root with \
         `project.open`; files are addressed by paths relative to the project. Change text with \
         `file.edit` (character offsets); comment on a passage with `review.comment`, or propose \
         a change for the author to accept or reject with `review.suggest`."
            .to_owned()
    }

    fn list(&mut self) -> Vec<Value> {
        vec![
            tool(
                "command_list",
                "List commands",
                "Every TeXCraft command: id, label, parameters (JSON schema), and whether it can run now.",
                object(json!({}), &[]),
                Effect::ReadOnly,
            ),
            tool(
                "command_run",
                "Run a command",
                "Runs one command by id with its parameters and returns its result.",
                object(
                    json!({"command": string("A command id from command_list."), "params": {"type": "object", "description": "The command's parameters."}}),
                    &["command"],
                ),
                Effect::Files,
            ),
        ]
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        let session = &mut self.session;
        let result = catch_panic(|| match name {
            "command_list" => Ok(Value::Array(texcraft_engine::cmd::all().iter().map(|c| c.info(session)).collect())),
            "command_run" => {
                let id = args.get("command").and_then(Value::as_str).unwrap_or_default();
                let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
                session.execute(id, &params)
            }
            other => Err(format!("unknown tool `{other}`")),
        });
        match result {
            Ok(Ok(v)) => text_result(v.to_string(), false),
            Ok(Err(e)) | Err(e) => text_result(e, true),
        }
    }
}

/// Serves the tools on stdin/stdout until stdin closes. Logs go to stderr only. `templates` is
/// where templates are kept; confined sessions use the kept ones and download none.
pub fn serve(roots: AutomationRoots, templates: Option<std::path::PathBuf>) -> std::io::Result<()> {
    eprintln!("texcraft mcp: files confined to {}", roots.describe());
    let mut session = Session::new();
    if let Some(dir) = templates {
        session.set_template_store(dir);
    }
    session.confine(roots);
    // Comments an agent writes carry this name (docs/collaboration-roadmap.md, R5 marks them further).
    session.set_author(Some(texcraft_engine::review::Author::new("Agent", "")));
    let info = Info { name: "texcraft", title: "TeXCraft", version: env!("CARGO_PKG_VERSION"), max_line: 16 * 1024 * 1024 };
    let mut server = Server::new(info, TexTools::new(session));
    let stdin = std::io::stdin();
    server.serve(stdin.lock(), std::io::stdout())
}
