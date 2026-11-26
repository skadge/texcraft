//! Requests from the control channel (docs/control-protocol.md): the same commands the menus run,
//! and what the window shows. The shell owns the transport (the desktop app's loopback server,
//! later the web page's API) and hands each request here on the UI thread.

use serde_json::{Value, json};
use texcraft_engine::cmd;

use crate::ui_cmd::UI_COMMANDS;
use crate::{App, Layout, Side};

impl App {
    /// Answers one control request: `{"ok": true, "result": …}` or `{"ok": false, "error": …}`.
    pub fn control(&mut self, method: &str, params: &Value) -> Value {
        let result = match method {
            // A line with only the token: the connection is open.
            "" => Ok(json!({"app": "TeXCraft", "version": env!("CARGO_PKG_VERSION"), "protocol": "docs/control-protocol.md"})),
            "engine.commands" => Ok(self.commands()),
            "engine.execute" => self.execute(params),
            "document.inspect" => self.session.execute("project.inspect", &json!({})).map(|mut v| {
                if let (Some(obj), Ok(build)) = (v.as_object_mut(), self.session.execute("build.result", &json!({}))) {
                    obj.insert("build".to_owned(), build);
                }
                v
            }),
            "document.revision" => Ok(json!({"document": self.document_id(), "revision": self.session.revision()})),
            "ui.inspect" => Ok(self.inspect()),
            other => Err(format!("unknown method `{other}`; see docs/control-protocol.md")),
        };
        match result {
            Ok(result) => json!({"ok": true, "result": result}),
            Err(error) => json!({"ok": false, "error": error}),
        }
    }

    /// The open project's id for the control channel: its folder, or `untitled`.
    pub fn document_id(&self) -> String {
        self.session.dir().map_or_else(|| "untitled".to_owned(), |d| d.display().to_string())
    }

    fn commands(&self) -> Value {
        let engine = cmd::all().iter().map(|c| c.info(&self.session));
        let ui = UI_COMMANDS.iter().map(|c| {
            let enabled = crate::ui_cmd::enabled(self, c.id);
            json!({"id": c.id, "label": c.label, "menu": [c.menu], "shortcut": self.keys.shortcut_text(c.id), "interface": true, "enabled": enabled.is_ok(), "disabledReason": enabled.err()})
        });
        Value::Array(engine.chain(ui).collect())
    }

    /// `engine.execute {command, params}`: an engine command, or an interface command (view, edit,
    /// help) run as its menu entry runs it.
    fn execute(&mut self, params: &Value) -> Result<Value, String> {
        let id = params.get("command").and_then(Value::as_str).ok_or("the parameter `command` (a string) is missing")?;
        let args = params.get("params").cloned().unwrap_or_else(|| json!({}));
        if cmd::find(id).is_some() {
            return self.session.execute(id, &args);
        }
        if UI_COMMANDS.iter().any(|c| c.id == id) {
            self.view.status = None;
            crate::ui_cmd::run_with(self, id, &args);
            return match self.view.status.take() {
                Some((message, true)) => Err(message),
                other => {
                    self.view.status = other;
                    Ok(json!({}))
                }
            };
        }
        Err(format!("unknown command `{id}`; engine.commands lists them"))
    }

    fn inspect(&self) -> Value {
        let layout = match self.view.layout {
            Layout::Split => "split",
            Layout::EditorOnly => "editor",
            Layout::PreviewOnly => "preview",
        };
        let side = match self.view.side {
            Some(Side::Files) => "files",
            Some(Side::Outline) => "outline",
            Some(Side::Review) => "review",
            Some(Side::Search) => "search",
            Some(Side::History) => "history",
            Some(Side::Symbols) => "symbols",
            Some(Side::Ask) => "ask",
            None => "hidden",
        };
        let dialog = self.view.dialog.as_ref().map(crate::dialogs::Dialog::name);
        json!({
            "layout": layout,
            "side": side,
            "log": self.view.logs,
            "focus_mode": self.view.focus_mode,
            "pdf_window": self.view.pdf_window,
            // A narrow window (a phone) shows one of the editor and the PDF, as tabs.
            "narrow": self.view.narrow,
            "pane": if self.view.pane == crate::Pane::Pdf { "pdf" } else { "source" },
            "dialog": dialog,
            "theme": if self.tokens.scheme == crate::theme::Scheme::Dark { "dark" } else { "light" },
            "cursor": self.view.cursor.map(|(line, column)| json!({"line": line, "column": column})),
            "status": self.view.status.as_ref().map(|(text, error)| json!({"text": text, "error": error})),
            "preview": {"page": self.pdf.page + 1, "pages": self.pdf.page_count(), "zoom": self.pdf.zoom(), "scroll": [self.pdf.offset().x, self.pdf.offset().y]},
            "building": self.session.is_building(),
        })
    }
}
