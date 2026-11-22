//! The control channel (docs/control-protocol.md): craft-control's loopback JSON-lines server,
//! started only when asked (`--control [PORT]` or `TEXCRAFT_CONTROL_PORT`). A connection shows the
//! launch's token first; the token is random per launch (or `TEXCRAFT_CONTROL_TOKEN`) and written
//! to `<data folder>/TeXCraft/control/<port>.token`, readable by the user only, and removed when
//! the window closes. Requests are answered on the UI thread by `App::control`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};

use craft_control::auth::{self, Listen};
use craft_control::server::{Hub, Protocol, Stop, Summary};
use craft_control::{ControlRequest, Effect};
use serde_json::{Value, json};

/// The first port tried without `--control PORT`.
const DEFAULT_PORT: u16 = 7841;
/// Longest request line, in bytes.
const MAX_LINE: usize = 4 << 20;
/// Most requests answered in one frame; the rest wait for the next.
const PER_FRAME: usize = 64;

fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("TeXCraft"))
}

fn env(name: &str) -> Option<OsString> {
    std::env::var_os(format!("TEXCRAFT_{name}"))
}

fn utc(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
        .map_or_else(|| seconds.to_string(), |t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
}

const APP: auth::App = auth::App { name: "TeXCraft", data_dir, env, utc, default_port: DEFAULT_PORT };

struct TexProtocol;

impl Protocol for TexProtocol {
    fn summary(&self, method: &str, params: &Value) -> Summary {
        let command = params.get("command").and_then(Value::as_str);
        let effect = match method {
            "engine.commands" | "document.inspect" | "document.revision" | "ui.inspect" => Effect::ReadsOnly,
            _ => Effect::Changes,
        };
        let what = match command {
            Some(c) => format!("`{method}` ({c})"),
            None => format!("`{method}`"),
        };
        Summary { what, effect, command: None, steps: None, long: false }
    }
    fn document(&self) -> &'static str {
        "project"
    }
    fn change_event(&self) -> &'static str {
        "project.changed"
    }
    fn revision_method(&self) -> &'static str {
        "document.revision"
    }
    fn docs(&self) -> &'static str {
        "docs/control-protocol.md"
    }
    fn capabilities(&self) -> Value {
        json!({"methods": ["engine.commands", "engine.execute", "document.inspect", "document.revision", "ui.inspect"]})
    }
}

pub struct Control {
    requests: Receiver<ControlRequest>,
    stop: Option<Stop>,
    port: u16,
    token: String,
    token_file: Option<PathBuf>,
}

/// A fresh token: 32 random bytes as 64 hex digits.
fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random source for the control token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl Control {
    /// Starts the server on `explicit` or the default port. The launch asked for it.
    pub fn start(explicit: Option<u16>, ctx: egui::Context) -> Result<Option<Control>, String> {
        // Called only when the launch asked for the channel, so a missing port means the default one.
        let Some(listen) = auth::plan(explicit, true) else { return Ok(None) };
        let listener = match listen {
            Listen::Exact(port) => std::net::TcpListener::bind(("127.0.0.1", port)),
            Listen::Default => auth::bind_with_fallback(DEFAULT_PORT, auth::PORT_TRIES),
        }
        .map_err(|e| format!("the control channel cannot listen: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = match APP.env_token() {
            Some(t) => t,
            None => new_token()?,
        };
        let token_file = APP.token_path(port);
        if let Some(path) = &token_file {
            APP.write_token_file(path, &token).map_err(|e| format!("writing {}: {e}", path.display()))?;
            let _ = APP.write_current_port(port);
        }
        let (tx, requests) = channel();
        let hub = Arc::new(Hub::new(TexProtocol, move || ctx.request_repaint(), MAX_LINE));
        let stop = hub.listen_stoppable(listener, token.clone(), tx, false).map_err(|e| e.to_string())?;
        log::info!(
            "control channel on 127.0.0.1:{port}, token in {}",
            token_file.as_ref().map_or("TEXCRAFT_CONTROL_TOKEN".to_owned(), |p| p.display().to_string())
        );
        Ok(Some(Control { requests, stop: Some(stop), port, token, token_file }))
    }

    /// Answers the requests that arrived, on the UI thread.
    pub fn answer(&self, app: &mut texcraft_ui_egui::App) {
        for request in self.requests.try_iter().take(PER_FRAME) {
            request.started.store(true, Ordering::SeqCst);
            let reply = match texcraft_engine::guard::catch_panic(|| app.control(&request.method, &request.params)) {
                Ok(reply) => reply,
                Err(e) => json!({"ok": false, "error": e}),
            };
            let _ = request.reply.send(reply);
        }
    }

    /// Stops listening and removes the token files this launch wrote.
    pub fn shut_down(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.stop();
        }
        if let Some(path) = &self.token_file {
            auth::remove_token_file(path, &self.token);
        }
        APP.remove_current_port(self.port, &self.token);
    }
}
