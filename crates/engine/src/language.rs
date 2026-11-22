//! The language server (docs/plan.md, phase 2, item 5): retex-lsp's `Server`, in process, with no
//! JSON-RPC transport. It sees the project's text files as they change and answers completion,
//! hover and definition requests; its lint reports problems as the text is typed, before a build.
//!
//! Positions are lines and columns in characters (the LSP's UTF-32 encoding), the editor's own
//! units. Files are named by URIs under the project's folder, or under a fixed root for a project
//! that has none (the web, a new project).
//!
//! What the server knows of commands, environments and bibliography entries comes from a build's
//! `symbols` record ([`Language::build_finished`]), which retex makes with each build. Until a build
//! has given one, completion offers the labels written in the open files.

use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use retex_lsp::{BuildId, Input, Output, Server, uri};
use serde_json::{Value, json};
use texcraft_compile::Severity;
use texcraft_project::{Contents, Project, ProjectPath};

/// A problem the lint found, in characters from 0.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Lint {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub severity: Severity,
    pub message: String,
}

/// A completion: `text` replaces the line's characters from `from` (a column, from 0) to the caret.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    pub text: String,
    pub from: usize,
}

/// One replacement of a fix: lines and columns in characters, from 0.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextEdit {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub text: String,
}

/// A suggested fix: its title, the edits it makes, and for which problem (its message).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Fix {
    pub title: String,
    pub edits: Vec<TextEdit>,
    pub preferred: bool,
    pub problem: Option<String>,
}

pub(crate) struct Language {
    server: Server,
    /// The URI of the project's folder, ending with `/`.
    root: String,
    /// Each synced file's text hash and version.
    synced: HashMap<ProjectPath, (u64, i32)>,
    /// The session revision last synced.
    revision: Option<u64>,
    lint: HashMap<ProjectPath, Vec<Lint>>,
    /// Builds the server asked for, answered by the next build that finishes.
    requested: Vec<BuildId>,
    next_request: i64,
}

fn hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl Language {
    pub fn new(dir: Option<&std::path::Path>) -> Self {
        let root = match dir {
            Some(d) => format!("{}/", uri::from_path(&d.to_string_lossy()).trim_end_matches('/')),
            None => "file:///texcraft-project/".to_owned(),
        };
        let mut language = Self {
            server: Server::new(),
            root,
            synced: HashMap::new(),
            revision: None,
            lint: HashMap::new(),
            requested: Vec::new(),
            next_request: 1,
        };
        let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "capabilities": {"general": {"positionEncodings": ["utf-32"]}}
        }});
        language.send(init);
        language.send(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        language
    }

    fn uri(&self, path: &ProjectPath) -> String {
        uri::resolve(&self.root, path.as_str())
    }

    fn path(&self, u: &str) -> Option<ProjectPath> {
        uri::relative(&self.root, &uri::canonical(u)).and_then(|p| ProjectPath::new(&p).ok())
    }

    /// Sends a message; returns the responses, and takes the notifications and build requests.
    fn send(&mut self, message: Value) -> Vec<Value> {
        let mut responses = Vec::new();
        for output in self.server.handle(Input::Message(message)) {
            match output {
                Output::Message(m) if m.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") => self.take_diagnostics(&m),
                Output::Message(m) => responses.push(m),
                Output::Build { id, .. } => self.requested.push(id),
                Output::Cancel(id) => self.requested.retain(|r| *r != id),
                Output::Exit(_) => {}
            }
        }
        responses
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_request;
        self.next_request += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .into_iter()
            .find(|r| r.get("id").and_then(Value::as_i64) == Some(id))
            .and_then(|r| r.get("result").cloned())
            .unwrap_or(Value::Null)
    }

    fn take_diagnostics(&mut self, m: &Value) {
        let Some(path) = m.pointer("/params/uri").and_then(Value::as_str).and_then(|u| self.path(u)) else { return };
        let at = |d: &Value, p: &str| d.pointer(p).and_then(Value::as_u64).map_or(0, |n| n as usize);
        let lints = m
            .pointer("/params/diagnostics")
            .and_then(Value::as_array)
            .map(|ds| {
                ds.iter()
                    .map(|d| Lint {
                        line: at(d, "/range/start/line"),
                        column: at(d, "/range/start/character"),
                        end_line: at(d, "/range/end/line"),
                        end_column: at(d, "/range/end/character"),
                        severity: match d.get("severity").and_then(Value::as_u64) {
                            Some(1) => Severity::Error,
                            Some(2) => Severity::Warning,
                            Some(3) => Severity::Note,
                            _ => Severity::Hint,
                        },
                        message: d.get("message").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.lint.insert(path, lints);
    }

    /// Brings the server's copy of the project's text files up to date.
    pub fn sync(&mut self, project: &Project, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        let source = |p: &ProjectPath| [".tex", ".sty", ".cls", ".bib", ".ltx"].iter().any(|e| p.as_str().ends_with(e));
        let texts: Vec<(ProjectPath, &str)> = project
            .files()
            .filter_map(|(p, c)| match c {
                Contents::Text(t) if source(p) => Some((p.clone(), t.as_str())),
                _ => None,
            })
            .collect();
        for (path, text) in &texts {
            let h = hash(text);
            let uri = self.uri(path);
            match self.synced.get(path).copied() {
                Some((old, _)) if old == h => {}
                Some((_, version)) => {
                    self.synced.insert(path.clone(), (h, version + 1));
                    self.send(json!({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {
                        "textDocument": {"uri": uri, "version": version + 1},
                        "contentChanges": [{"text": text}]
                    }}));
                }
                None => {
                    self.synced.insert(path.clone(), (h, 1));
                    self.send(json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                        "textDocument": {"uri": uri, "languageId": "latex", "version": 1, "text": text}
                    }}));
                }
            }
        }
        let gone: Vec<ProjectPath> = self.synced.keys().filter(|p| !texts.iter().any(|(q, _)| q == *p)).cloned().collect();
        for path in gone {
            self.synced.remove(&path);
            self.lint.remove(&path);
            let uri = self.uri(&path);
            self.send(json!({"jsonrpc": "2.0", "method": "textDocument/didClose", "params": {"textDocument": {"uri": uri}}}));
        }
    }

    pub fn lint(&self, path: &ProjectPath) -> &[Lint] {
        self.lint.get(path).map_or(&[], Vec::as_slice)
    }

    fn position(&self, path: &ProjectPath, line: usize, column: usize) -> Value {
        json!({"textDocument": {"uri": self.uri(path)}, "position": {"line": line, "character": column}})
    }

    /// Completions at a line and column (characters, from 0).
    pub fn complete(&mut self, path: &ProjectPath, line: usize, column: usize) -> Vec<Completion> {
        let result = self.request("textDocument/completion", self.position(path, line, column));
        let items = result.get("items").and_then(Value::as_array).cloned().or_else(|| result.as_array().cloned()).unwrap_or_default();
        items
            .iter()
            .filter_map(|item| {
                let label = item.get("label")?.as_str()?.to_owned();
                let edit = item.get("textEdit");
                let text = edit.and_then(|e| e.get("newText")).and_then(Value::as_str).unwrap_or(&label).to_owned();
                let from = edit.and_then(|e| e.pointer("/range/start/character")).and_then(Value::as_u64).map_or(column, |c| c as usize);
                let detail = item.get("detail").and_then(Value::as_str).unwrap_or_default().to_owned();
                Some(Completion { label, detail, text, from })
            })
            .collect()
    }

    /// What the server says about the thing at a line and column, as Markdown.
    pub fn hover(&mut self, path: &ProjectPath, line: usize, column: usize) -> Option<String> {
        let result = self.request("textDocument/hover", self.position(path, line, column));
        let contents = result.get("contents")?;
        contents.get("value").and_then(Value::as_str).or_else(|| contents.as_str()).map(str::to_owned)
    }

    /// Where the thing at a line and column is defined: a file and a line and column.
    pub fn definition(&mut self, path: &ProjectPath, line: usize, column: usize) -> Option<(ProjectPath, usize, usize)> {
        let result = self.request("textDocument/definition", self.position(path, line, column));
        let location = if result.is_array() { result.get(0)?.clone() } else { result };
        let file = self.path(location.get("uri")?.as_str()?)?;
        let start = location.pointer("/range/start")?;
        Some((file, start.get("line")?.as_u64()? as usize, start.get("character")?.as_u64()? as usize))
    }

    /// The fixes for the problems on `line` (from 0), and the one that fixes every hint of the
    /// file, if any (last, its `problem` none).
    pub fn fixes(&mut self, path: &ProjectPath, line: usize) -> Vec<Fix> {
        let uri = self.uri(path);
        let params = json!({"textDocument": {"uri": uri}, "range": {"start": {"line": line, "character": 0}, "end": {"line": line, "character": 0}}, "context": {"diagnostics": []}});
        let result = self.request("textDocument/codeAction", params);
        let canonical = uri::canonical(&uri);
        let at = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_u64).map_or(0, |n| n as usize);
        result
            .as_array()
            .map(|actions| {
                actions
                    .iter()
                    .filter_map(|a| {
                        let changes = a.pointer("/edit/changes")?.as_object()?;
                        let edits = changes.iter().find(|(u, _)| uri::canonical(u) == canonical)?.1.as_array()?;
                        let edits = edits
                            .iter()
                            .map(|e| TextEdit {
                                line: at(e, "/range/start/line"),
                                column: at(e, "/range/start/character"),
                                end_line: at(e, "/range/end/line"),
                                end_column: at(e, "/range/end/character"),
                                text: e.get("newText").and_then(Value::as_str).unwrap_or_default().to_owned(),
                            })
                            .collect();
                        Some(Fix {
                            title: a.get("title")?.as_str()?.to_owned(),
                            edits,
                            preferred: a.get("isPreferred").and_then(Value::as_bool).unwrap_or(false),
                            problem: a.pointer("/diagnostics/0/message").and_then(Value::as_str).map(str::to_owned),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A build finished: its `symbols` record (when the engine made one) for `main` goes to the
    /// server as a build of its own (`start_build`), and answers the builds the server asked for.
    pub fn build_finished(&mut self, main: Option<&ProjectPath>, symbols: Option<Value>) {
        let main_uri = main.map(|m| self.uri(m));
        let mut builds = std::mem::take(&mut self.requested);
        if let (Some(uri), Some(symbols)) = (&main_uri, &symbols) {
            for id in &builds {
                // Paths in the records are relative to the main file's folder, where it is built.
                self.server.handle(Input::Started { id: *id, main: uri.clone(), dir: None });
            }
            if builds.is_empty() {
                let (id, cancelled) = self.server.start_build(uri, None);
                self.take_outputs(cancelled);
                builds.push(id);
            }
            for id in &builds {
                self.server.handle(Input::Records(*id, vec![symbols.clone()]));
            }
        }
        for id in builds {
            let outputs = self.server.handle(Input::Finished(id));
            self.take_outputs(outputs);
        }
    }

    /// Takes the diagnostics and build requests among the server's outputs.
    fn take_outputs(&mut self, outputs: Vec<Output>) {
        for output in outputs {
            match output {
                Output::Message(m) if m.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") => self.take_diagnostics(&m),
                Output::Build { id, .. } => self.requested.push(id),
                Output::Cancel(id) => self.requested.retain(|r| *r != id),
                Output::Message(_) | Output::Exit(_) => {}
            }
        }
    }
}
