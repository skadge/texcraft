//! AI assistance (docs/ROADMAP.md, AI1 and AI2): off until the user connects a model, any
//! OpenAI-compatible endpoint (a local server at a loopback address is the same, with no token).
//!
//! Built on the suite's AI layer: `craft-ai-core` checks the connection (its form,
//! [`ConnectionDraft`], derives whether data leaves the computer from the address), says what a
//! request sends ([`Disclosure`]), keeps the user's agreements ([`ConsentLedger`]) and the request
//! log ([`HistoryRecord`]); `craft-ai`'s `OpenAiCompat` talks to the endpoint on the desktop. The
//! token is never in the saved settings: they hold the connection's `SecretRef`, the token is in
//! [`Secrets`] (the system's keychain on the desktop, this session only elsewhere).
//!
//! Nothing is sent before the user agreed to that kind of request for that connection: the first
//! explanation shows what it sends (the build message, its help, the source lines around it, the
//! preamble when the error is in it) and where. A fix the model proposes becomes a suggested
//! change by "AI (<model>)", never an edit. A project can turn AI off for itself
//! (`.texcraft/ai.json`).
//!
//! AI3 ([`write`]): a selection rewritten (clearer, shorter, more formal, corrected, translated)
//! comes back as a suggested change on exactly that selection, and only when it kept the
//! selection's LaTeX ([`texcraft_latex::keep`]); a paragraph being typed can be continued, the
//! continuation shown after the caret until it is accepted or typed over. Each is its own kind of
//! request, agreed to on its own.
//!
//! AI4 ([`make`]): a table, an equation, a TikZ diagram or a figure made from a description,
//! typeset after the preamble before it is offered and inserted as a suggested change; a BibTeX
//! entry drafted from a title or DOI, suggested at the end of a `.bib` under a line saying a model
//! drafted it. AI5 ([`ask`]): questions about the open file or the project, answered with links
//! to the lines they name, and a section reviewed as comments on the words each remark quotes.

mod ask;
mod make;
mod write;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

use craft_ai_core::endpoint::Endpoint;
use craft_ai_core::history::{EndRecord, HISTORY_SCHEMA, Outcome as Logged, StartRecord};
use craft_ai_core::{AppId, CapabilityId, ConnectionDraft, Disclosure, ErrorKind, HistoryRecord, InputRole, KeyChange, Locality, Payload, Request};
/// The suite's types the settings and the shell see.
pub use craft_ai_core::{Connection, ConsentLedger, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use texcraft_compile::Diagnostic;
use texcraft_project::{Project, ProjectPath};

use crate::{Result, Session};

/// The app's name in consent records and the request log.
pub const APP: &str = "texcraft";
/// The kind of request an explanation is, for consent: agreeing to it covers explanations only.
pub const EXPLAIN: &str = "text.explain";
/// The kind of request a rewrite is.
pub const REWRITE: &str = "text.rewrite";
/// The kind of request a continuation is.
pub const COMPLETE: &str = "text.complete";
/// The kind of request making LaTeX from a description is.
pub const MAKE: &str = "text.make";
/// The kind of request drafting a BibTeX entry is.
pub const REFERENCE: &str = "text.reference";
/// The kind of request a question about the project is.
pub const ASK: &str = "text.ask";
/// The kind of request reviewing a section is.
pub const REVIEW: &str = "text.review";
pub use ask::{
    Answered, Link, MAX_ASK_CHARS, MAX_QUESTION_CHARS, MAX_REVIEW_CHARS, Question, Remark, Reviewed, Scope, Section, find_quote, links, remarks,
    section_at,
};
pub use make::{MAX_DESCRIPTION_CHARS, Made, MakeKind, Making, Preview, Referenced, Scratch, Typeset, UNVERIFIED, entry, made, retex_typeset};
pub use write::{
    COMPLETE_EVERY_MS, Ghost, How, MAX_PARAGRAPH_CHARS, MAX_REWRITE_CHARS, Paragraph, Passage, REWRITE_CONTEXT_CHARS, Rewritten, continuation,
    rewritten,
};
/// Lines of source sent on each side of the message's line.
pub const CONTEXT_LINES: usize = 6;
/// The most of the preamble sent, in characters.
pub const MAX_PREAMBLE_CHARS: usize = 12_000;
/// The project's own AI setting, in its folder.
pub const PROJECT_FILE: &str = ".texcraft/ai.json";
/// Explanations kept (the oldest go first).
const MAX_EXPLANATIONS: usize = 64;
/// Request log lines kept in the session.
const MAX_LOG: usize = 200;

/// What is kept between runs (in the preferences): the connection, which holds a reference to its
/// token and never the token, the model, and the agreements.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub connection: Option<Connection>,
    pub model: Option<String>,
    pub consent: ConsentLedger,
}

/// Where tokens are kept. The desktop uses the system's keychain ([`Keychain`]); the browser, the
/// command line and tests keep them for the session ([`SessionSecrets`]).
pub trait Secrets: Send + Sync {
    fn get(&self, handle: &str) -> std::result::Result<Option<SecretString>, String>;
    fn put(&self, handle: &str, value: &SecretString) -> std::result::Result<(), String>;
    /// Removes it; already gone is success.
    fn remove(&self, handle: &str) -> std::result::Result<(), String>;
    /// Whether a token outlives the session.
    fn lasting(&self) -> bool;
}

/// Tokens kept in memory, for this session only.
#[derive(Default)]
pub struct SessionSecrets(Mutex<std::collections::BTreeMap<String, SecretString>>);

impl Secrets for SessionSecrets {
    fn get(&self, handle: &str) -> std::result::Result<Option<SecretString>, String> {
        Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(handle).cloned())
    }
    fn put(&self, handle: &str, value: &SecretString) -> std::result::Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(handle.to_owned(), value.clone());
        Ok(())
    }
    fn remove(&self, handle: &str) -> std::result::Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(handle);
        Ok(())
    }
    fn lasting(&self) -> bool {
        false
    }
}

/// The system's keychain, through the suite's account store (one namespace for every Craft app).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct Keychain;

#[cfg(not(target_arch = "wasm32"))]
impl Secrets for Keychain {
    fn get(&self, handle: &str) -> std::result::Result<Option<SecretString>, String> {
        use craft_suite_account::TokenStore;
        Ok(craft_suite_account::Keychain.get(handle)?.map(|s| SecretString::new(s.expose())))
    }
    fn put(&self, handle: &str, value: &SecretString) -> std::result::Result<(), String> {
        use craft_suite_account::TokenStore;
        craft_suite_account::Keychain.put(handle, &craft_suite_account::Secret::new(value.expose()))
    }
    fn remove(&self, handle: &str) -> std::result::Result<(), String> {
        use craft_suite_account::TokenStore;
        craft_suite_account::Keychain.remove(handle)
    }
    fn lasting(&self) -> bool {
        true
    }
}

/// What a provider is made for.
pub struct Target {
    pub endpoint: String,
    pub token: Option<SecretString>,
    pub model: String,
}

/// Makes the provider a request goes through: `craft-ai`'s OpenAI-compatible one, or a fake in
/// tests. Called on the session's thread; the provider then runs on the request's thread.
pub type Connect = Arc<dyn Fn(&Target) -> std::result::Result<Box<dyn craft_ai::Provider>, String> + Send + Sync>;

/// The suite's OpenAI-compatible provider (desktop).
#[cfg(not(target_arch = "wasm32"))]
pub fn openai_compatible() -> Connect {
    Arc::new(|t: &Target| {
        let token = t.token.as_ref().map(|s| s.expose().to_owned());
        Ok(Box::new(craft_ai::OpenAiCompat::new(t.endpoint.clone(), token, t.model.clone())) as Box<dyn craft_ai::Provider>)
    })
}

/// In the browser: the suite's AI layer has no provider for the page yet.
#[cfg(target_arch = "wasm32")]
pub fn openai_compatible() -> Connect {
    Arc::new(|_: &Target| Err(UNAVAILABLE.to_owned()))
}

/// Why AI does not work in this build, if it does not.
pub const UNAVAILABLE: &str =
    "AI is not available in the browser yet: the suite's AI layer cannot reach an endpoint from a web page. Use the desktop app.";

/// Whether this build can reach an endpoint.
pub fn available() -> bool {
    !cfg!(target_arch = "wasm32")
}

/// Whether `endpoint` is on this computer (a loopback address); `None` while it is not a usable
/// address. As the suite decides it: never by name resolution.
pub fn is_local(endpoint: &str) -> Option<bool> {
    Endpoint::parse(endpoint.trim()).ok().map(|e| e.is_loopback())
}

/// The message an explanation is about: its file, line and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

impl Key {
    pub fn of(d: &Diagnostic) -> Self {
        Self { file: d.file.clone(), line: d.line, message: d.message.clone() }
    }
}

/// An explanation of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Explanation {
    /// The model is answering.
    Running,
    Done {
        text: String,
        /// The model's name.
        model: String,
        /// The suggested change made of its fix.
        suggestion: Option<String>,
        /// Why its fix is not offered, when it gave one that could not be.
        note: Option<String>,
    },
    Failed(String),
    Cancelled,
}

/// What finished in the background, for the window's status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The endpoint's models were listed (how many), or why not.
    Models(std::result::Result<usize, String>),
    /// An explanation arrived (with a suggested change or not), or why not.
    Explained(std::result::Result<bool, String>),
    /// A rewrite became a suggested change (its id), or why not.
    Rewritten(std::result::Result<String, String>),
    /// A continuation is shown after the caret (or was dropped: the text moved on), or why not.
    Completed(std::result::Result<bool, String>),
    /// LaTeX was made: whether it typeset; or why not.
    Made(std::result::Result<bool, String>),
    /// A reference became a suggested change in this `.bib`, or why not.
    Referenced(std::result::Result<String, String>),
    /// A question was answered (with this many links), or why not.
    Answered(std::result::Result<usize, String>),
    /// A review left this many comments and dropped this many remarks, or why not.
    Reviewed(std::result::Result<(usize, usize), String>),
}

/// What an explanation sends: exactly these parts, in [`Context::prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub message: String,
    pub origin: Option<String>,
    pub help: Vec<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// The source lines around the message's line: the first one's number (from 1) and their text.
    pub lines: Option<(u32, String)>,
    /// The main document's preamble, when the message is in it.
    pub preamble: Option<String>,
}

const INSTRUCTIONS: &str = "You help someone writing a LaTeX document understand a message from building it. \
Explain in plain language, in a few sentences, what the message means and what in the source causes it. \
If changing the source lines shown would fix it, end your answer with all of those lines as they should be, \
between a line <<<FIX and a line FIX>>>, changing nothing else. If no change to them fixes it, leave the markers out.";

impl Context {
    /// What explaining `d` sends, from `project`'s text.
    pub fn of(d: &Diagnostic, project: &Project) -> Self {
        let text = d.file.as_deref().and_then(|f| ProjectPath::new(f).ok()).and_then(|p| project.text(&p).ok().map(str::to_owned));
        let lines = text.as_deref().zip(d.line).map(|(text, line)| window(text, line));
        let in_preamble = d.file.as_deref().is_some_and(|f| project.main().is_some_and(|m| m.as_str() == f))
            && text.as_deref().zip(d.line).is_some_and(|(text, line)| preamble_end(text).is_none_or(|end| (line as usize) <= end));
        let preamble = if in_preamble { text.as_deref().map(preamble) } else { None };
        Self { message: d.message.clone(), origin: d.origin.clone(), help: d.help.clone(), file: d.file.clone(), line: d.line, lines, preamble }
    }

    /// The whole text sent to the model.
    pub fn prompt(&self) -> String {
        let mut out = format!("{INSTRUCTIONS}\n\n{}\n", self.message_part());
        if let Some((from, text)) = &self.lines {
            let to = from + text.lines().count().saturating_sub(1) as u32;
            out.push_str(&format!("\nSource lines {from} to {to}:\n<<<SOURCE\n{text}\nSOURCE>>>\n"));
        }
        if let Some(preamble) = &self.preamble {
            out.push_str(&format!("\nThe main document's preamble:\n<<<PREAMBLE\n{preamble}\nPREAMBLE>>>\n"));
        }
        out
    }

    /// The message, where it comes from, its help and its place.
    fn message_part(&self) -> String {
        let mut out = match &self.origin {
            Some(o) => format!("Message ({o}): {}", self.message),
            None => format!("Message: {}", self.message),
        };
        for h in &self.help {
            out.push_str(&format!("\nHelp: {h}"));
        }
        match (&self.file, self.line) {
            (Some(f), Some(l)) => out.push_str(&format!("\nIn {f}, line {l}.")),
            (Some(f), None) => out.push_str(&format!("\nIn {f}.")),
            _ => {}
        }
        out
    }

    /// The request as the suite's consent and log see it: its parts by role and size.
    fn request(&self) -> Result<Request> {
        let capability = CapabilityId::parse(EXPLAIN).map_err(|e| e.to_string())?;
        let mut request = Request::new(capability).with_input(InputRole::Prompt, Payload::text(format!("{INSTRUCTIONS}\n\n{}", self.message_part())));
        if let Some((_, text)) = &self.lines {
            request = request.with_input(InputRole::Context, Payload::text(text.clone()));
        }
        if let Some(preamble) = &self.preamble {
            request = request.with_input(InputRole::Context, Payload::text(preamble.clone()));
        }
        Ok(request)
    }

    /// For automation and the consent dialog: each part and the whole text.
    fn describe(&self) -> Value {
        json!({
            "message": self.message,
            "origin": self.origin,
            "help": self.help,
            "file": self.file,
            "line": self.line,
            "lines": self.lines.as_ref().map(|(from, text)| json!({"from": from, "to": from + text.lines().count().saturating_sub(1) as u32, "text": text})),
            "preamble": self.preamble,
            "prompt": self.prompt(),
        })
    }
}

/// The lines around `line` (from 1): its first line's number and their text.
fn window(text: &str, line: u32) -> (u32, String) {
    let all: Vec<&str> = text.lines().collect();
    let at = (line as usize).saturating_sub(1).min(all.len().saturating_sub(1));
    let from = at.saturating_sub(CONTEXT_LINES);
    let to = (at + CONTEXT_LINES).min(all.len().saturating_sub(1));
    let lines = all.get(from..=to).unwrap_or_default();
    (from as u32 + 1, lines.join("\n"))
}

/// The line (from 1) of `\begin{document}`, if the text has one.
fn preamble_end(text: &str) -> Option<usize> {
    text.lines().position(|l| l.trim_start().starts_with("\\begin{document}")).map(|i| i + 1)
}

/// The text before `\begin{document}`, at most [`MAX_PREAMBLE_CHARS`].
fn preamble(text: &str) -> String {
    let lines: Vec<&str> = text.lines().take(preamble_end(text).map_or(usize::MAX, |e| e.saturating_sub(1))).collect();
    lines.join("\n").chars().take(MAX_PREAMBLE_CHARS).collect()
}

/// A model's answer: its explanation and the corrected lines, when it gave some.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub explanation: String,
    pub fix: Option<String>,
}

/// Reads a model's answer: the text, and the last `<<<FIX` … `FIX>>>` block as its fix (a code
/// fence around the lines is dropped).
pub fn parse_reply(text: &str) -> Reply {
    let lines: Vec<&str> = text.lines().collect();
    let open = lines.iter().rposition(|l| l.trim() == "<<<FIX");
    let close = open.and_then(|o| lines.iter().skip(o + 1).position(|l| l.trim() == "FIX>>>").map(|c| o + 1 + c));
    let (Some(open), Some(close)) = (open, close) else {
        return Reply { explanation: text.trim().to_owned(), fix: None };
    };
    let mut fix: Vec<&str> = lines.get(open + 1..close).unwrap_or_default().to_vec();
    if fix.first().is_some_and(|l| l.trim_start().starts_with("```")) && fix.last().is_some_and(|l| l.trim() == "```") && fix.len() >= 2 {
        fix = fix.get(1..fix.len() - 1).unwrap_or_default().to_vec();
    }
    let mut explanation: Vec<&str> = lines.get(..open).unwrap_or_default().to_vec();
    explanation.extend(lines.get(close + 1..).unwrap_or_default());
    Reply { explanation: explanation.join("\n").trim().to_owned(), fix: Some(fix.join("\n")) }
}

/// The smallest change that turns `old` into `new`: characters `start..end` of `old` (from 0)
/// become the returned text; `None` when they are the same.
pub fn smallest_change(old: &str, new: &str) -> Option<(usize, usize, String)> {
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if prefix == a.len() && prefix == b.len() {
        return None;
    }
    let room = a.len().min(b.len()) - prefix;
    let suffix = a.iter().rev().zip(b.iter().rev()).take(room).take_while(|(x, y)| x == y).count();
    let text: String = b.get(prefix..b.len() - suffix).unwrap_or_default().iter().collect();
    Some((prefix, a.len() - suffix, text))
}

/// A request in flight.
enum Work {
    Models { endpoint: String },
    Explain { key: Key, context: Box<Context>, model: String, run: String },
    Rewrite { passage: Box<Passage>, model: String, run: String },
    Complete { paragraph: Box<Paragraph>, run: String },
    Make { making: Box<Making>, model: String, run: String, insert: bool },
    Reference { lookup: Box<make::Lookup>, model: String, run: String },
    Ask { question: Box<Question>, model: String, run: String },
    Review { section: Box<Section>, model: String, run: String },
}

/// Where a request's answer comes.
type Pending = Receiver<std::result::Result<Answer, String>>;

struct Task {
    work: Work,
    answer: Pending,
    cancelled: Arc<AtomicBool>,
}

enum Answer {
    Models(Vec<String>),
    Text(String),
    /// Made LaTeX, its packages, and how it typeset.
    Made {
        latex: String,
        packages: Vec<String>,
        preview: Preview,
    },
}

/// The session's AI state.
pub(crate) struct State {
    pub(crate) saved: Saved,
    /// The models the endpoint last listed.
    models: Vec<String>,
    secrets: Arc<dyn Secrets>,
    connect: Connect,
    task: Option<Task>,
    explanations: Vec<(Key, Explanation)>,
    log: Vec<HistoryRecord>,
    /// The last rewrite.
    rewrite: Option<Rewritten>,
    /// The continuation offered after the caret.
    ghost: Option<Ghost>,
    /// When the last continuation was asked for (unix milliseconds), for the rate limit.
    last_complete: u64,
    /// Typesets what is made, for its preview.
    typeset: Typeset,
    made: Option<Made>,
    reference: Option<Referenced>,
    answers: Vec<Answered>,
    reviewed: Option<Reviewed>,
    /// The project's own setting: the folder it was read from, and whether AI is off.
    project_off: Option<(Option<PathBuf>, bool)>,
    /// Goes up when `saved` changes, so the window keeps it in the preferences.
    revision: u64,
    runs: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            saved: Saved::default(),
            models: Vec::new(),
            secrets: Arc::new(SessionSecrets::default()),
            connect: openai_compatible(),
            task: None,
            explanations: Vec::new(),
            log: Vec::new(),
            rewrite: None,
            ghost: None,
            last_complete: 0,
            typeset: retex_typeset(),
            made: None,
            reference: None,
            answers: Vec::new(),
            reviewed: None,
            project_off: None,
            revision: 0,
            runs: 0,
        }
    }
}

fn app() -> Result<AppId> {
    AppId::new(APP).map_err(|e| e.to_string())
}

fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or_default()
}

/// The keychain entry of a connection's token.
fn handle(c: &Connection) -> Option<String> {
    c.auth.secret_ref().map(|r| format!("texcraft.ai.{}", r.as_str()))
}

/// `scheme://host[:port]` of an address, which consent is bound to.
fn destination(c: &Connection) -> String {
    c.endpoint.as_deref().and_then(|e| Endpoint::parse(e).ok()).map_or_else(|| c.provider.clone(), |e| e.fingerprint())
}

/// The kinds of request, as `ai.disclose` and `ai.consent` name them.
pub const REQUESTS: [&str; 7] = ["explain", "rewrite", "complete", "make", "reference", "ask", "review"];

/// The kind of request `name` is (one of [`REQUESTS`], or its capability id).
fn capability(name: &str) -> Result<&'static str> {
    match name {
        "explain" | EXPLAIN => Ok(EXPLAIN),
        "rewrite" | REWRITE => Ok(REWRITE),
        "complete" | COMPLETE => Ok(COMPLETE),
        "make" | MAKE => Ok(MAKE),
        "reference" | REFERENCE => Ok(REFERENCE),
        "ask" | ASK => Ok(ASK),
        "review" | REVIEW => Ok(REVIEW),
        _ => Err(format!("There is no AI request `{name}`: there are {}.", REQUESTS.join(", "))),
    }
}

fn kind_of(error: &str) -> ErrorKind {
    if error.starts_with("network error") { ErrorKind::Network } else { ErrorKind::Provider }
}

impl Session {
    /// Keeps AI tokens in `secrets` (the desktop shell gives the system's keychain).
    pub fn set_ai_secrets(&mut self, secrets: Arc<dyn Secrets>) {
        self.ai.secrets = secrets;
    }

    /// Typesets what the model makes with `typeset` (tests give a fake).
    pub fn set_ai_typeset(&mut self, typeset: Typeset) {
        self.ai.typeset = typeset;
    }

    /// Sends AI requests through `connect` (tests give a fake).
    pub fn set_ai_connect(&mut self, connect: Connect) {
        self.ai.connect = connect;
    }

    /// Restores the AI settings the shell kept.
    pub fn restore_ai(&mut self, saved: Saved) {
        self.ai.saved = saved;
    }

    /// The AI settings to keep, and a number that goes up when they change.
    pub fn ai_saved(&self) -> (&Saved, u64) {
        (&self.ai.saved, self.ai.revision)
    }

    /// The models the endpoint last listed.
    pub fn ai_models(&self) -> &[String] {
        &self.ai.models
    }

    /// The explanation of message `d`, if one was asked for.
    pub fn ai_explanation(&self, d: &Diagnostic) -> Option<&Explanation> {
        let key = Key::of(d);
        self.ai.explanations.iter().rev().find(|(k, _)| *k == key).map(|(_, e)| e)
    }

    /// Whether a request is in flight.
    pub fn ai_busy(&self) -> bool {
        self.ai.task.is_some()
    }

    /// Whether the open project turned AI off for itself.
    pub fn ai_project_off(&mut self) -> bool {
        let dir = self.dir.clone().filter(|_| self.library.is_none());
        match &self.ai.project_off {
            Some((d, off)) if *d == dir => *off,
            _ => {
                let off = dir.as_ref().is_some_and(|d| read_project_off(&d.join(PROJECT_FILE)));
                self.ai.project_off = Some((dir, off));
                off
            }
        }
    }

    /// Why AI requests cannot be made now, if they cannot.
    pub(crate) fn ai_ready(&mut self) -> Result<()> {
        if !available() {
            return Err(UNAVAILABLE.to_owned());
        }
        if self.ai.saved.connection.is_none() {
            return Err("AI is off: connect a model first (Settings › AI).".to_owned());
        }
        if self.ai_project_off() {
            return Err("AI is turned off for this project (Settings › AI).".to_owned());
        }
        // A continuation being asked for gives way to any other request.
        if self.ai.task.as_ref().is_some_and(|t| !matches!(t.work, Work::Complete { .. })) {
            return Err("The model is still answering the last request: wait for it or cancel it.".to_owned());
        }
        Ok(())
    }

    fn changed_ai(&mut self) {
        self.ai.revision = self.ai.revision.wrapping_add(1);
    }

    fn token(&self, c: &Connection) -> Result<Option<SecretString>> {
        match handle(c) {
            Some(h) => {
                let token = self.ai.secrets.get(&h).map_err(|e| format!("The token could not be read from the keychain: {e}"))?;
                token.map(Some).ok_or_else(|| "The connection's token is not kept here any more: enter it again (Settings › AI).".to_owned())
            }
            None => Ok(None),
        }
    }

    /// `ai.status`.
    pub(crate) fn ai_status(&mut self) -> Value {
        let project_off = self.ai_project_off();
        let a = &self.ai;
        let c = a.saved.connection.as_ref();
        let requests: Vec<Value> = a.log.iter().filter_map(|r| serde_json::to_value(r).ok()).collect();
        json!({
            "available": available(),
            "why_not": (!available()).then_some(UNAVAILABLE),
            "connected": c.is_some(),
            "endpoint": c.and_then(|c| c.endpoint.clone()),
            "destination": c.map(destination),
            "local": c.map(|c| c.locality == Locality::Local),
            "token": c.is_some_and(|c| c.auth.secret_ref().is_some()),
            "token_kept": if a.secrets.lasting() { "the system's keychain" } else { "this session only" },
            "model": a.saved.model,
            "models": a.models,
            "project_off": project_off,
            "busy": a.task.as_ref().map(|t| match t.work {
                Work::Models { .. } => "models",
                Work::Explain { .. } => "explain",
                Work::Rewrite { .. } => "rewrite",
                Work::Complete { .. } => "complete",
                Work::Make { .. } => "make",
                Work::Reference { .. } => "reference",
                Work::Ask { .. } => "ask",
                Work::Review { .. } => "review",
            }),
            "requests": requests,
        })
    }

    /// `ai.connect`: the endpoint, the token (absent: keep the saved one; empty: none) and the model.
    pub(crate) fn ai_connect(&mut self, endpoint: &str, token: Option<&str>, model: Option<&str>) -> Result<Value> {
        let existing: Vec<Connection> = self.ai.saved.connection.iter().cloned().collect();
        let mut draft = existing.first().map(ConnectionDraft::from_connection).unwrap_or_default();
        let moved = draft.endpoint.trim() != endpoint.trim();
        let parsed = Endpoint::parse(endpoint.trim())
            .map_err(|e| format!("That address cannot be used ({e}). Use https://, or http:// only for this computer."))?;
        // A token is for its host: another host never gets it unless it is given again.
        let new_host = Endpoint::parse(draft.endpoint.trim()).ok().map(|e| e.fingerprint()) != Some(parsed.fingerprint());
        draft.endpoint = endpoint.trim().to_owned();
        draft.label = parsed.fingerprint().chars().take(80).collect();
        match token.map(str::trim) {
            Some("") => {
                draft.uses_key = false;
                draft.key.clear();
            }
            Some(t) => {
                draft.uses_key = true;
                draft.key = t.to_owned();
            }
            // A new connection, or a new host, given no token uses none.
            None if existing.is_empty() || new_host => {
                draft.uses_key = false;
                draft.key.clear();
            }
            None => {}
        }
        let prepared = draft.prepare(&existing).map_err(|e| e.message)?;
        let old_handle = existing.first().and_then(handle);
        match &prepared.key {
            KeyChange::Set(secret) => {
                let h = handle(&prepared.connection).ok_or("The connection has no place for its token.")?;
                self.ai.secrets.put(&h, secret).map_err(|e| format!("The token could not be kept in the keychain: {e}"))?;
            }
            KeyChange::Clear => {
                if let Some(h) = &old_handle {
                    self.ai.secrets.remove(h).map_err(|e| format!("The old token could not be removed from the keychain: {e}"))?;
                }
            }
            KeyChange::Keep => {}
        }
        if moved {
            self.ai.models.clear();
        }
        self.ai.saved.connection = Some(prepared.connection);
        if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
            self.ai.saved.model = Some(m.to_owned());
        }
        self.changed_ai();
        Ok(self.ai_status())
    }

    /// `ai.model`.
    pub(crate) fn ai_set_model(&mut self, model: &str) -> Result<Value> {
        let model = model.trim();
        if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err("Name a model: one line, as the endpoint lists it.".to_owned());
        }
        self.ai.saved.model = Some(model.to_owned());
        self.changed_ai();
        Ok(json!({"model": model}))
    }

    /// `ai.forget`: the connection, its token and the agreements given for it.
    pub(crate) fn ai_forget(&mut self) -> Result<Value> {
        self.ai_cancel(false);
        let Some(c) = self.ai.saved.connection.take() else { return Ok(json!({"forgotten": false})) };
        if let Some(h) = handle(&c) {
            self.ai
                .secrets
                .remove(&h)
                .map_err(|e| format!("The connection is forgotten but its token could not be removed from the keychain: {e}"))?;
        }
        self.ai.saved.consent.revoke(&app()?, &c.id);
        self.ai.saved.model = None;
        self.ai.models.clear();
        self.changed_ai();
        Ok(json!({"forgotten": true}))
    }

    /// `ai.project`: turns AI on or off for the open project, in its folder; for this session when
    /// it has none.
    pub(crate) fn ai_set_project(&mut self, enabled: bool) -> Result<Value> {
        let dir = self.dir.clone().filter(|_| self.library.is_none());
        let mut kept = "session";
        if let Some(d) = &dir {
            let file = d.join(PROJECT_FILE);
            if let Some(roots) = &self.roots {
                roots.check(&file, crate::file_access::Access::Write)?;
            }
            write_project_off(&file, !enabled)?;
            kept = "project";
        }
        self.ai.project_off = Some((dir, !enabled));
        Ok(json!({"enabled": enabled, "kept": kept}))
    }

    /// The message `p` names: `index` in the last build's messages, or the first at `path` and `line`.
    pub(crate) fn ai_diagnostic(&self, p: &Value) -> Result<Diagnostic> {
        let outcome = self.last_outcome().ok_or("There is no build yet: compile the project first.")?;
        if let Some(i) = p.get("index") {
            let i =
                i.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or("`index` is a message's position in build.result's diagnostics, from 0.")?;
            return outcome.diagnostics.get(i).cloned().ok_or_else(|| format!("The last build has no message {i}."));
        }
        let path = p.get("path").and_then(Value::as_str).ok_or("Name the message: its `index`, or its `path` and `line`.")?;
        let line = p.get("line").and_then(Value::as_u64).ok_or("Name the message's `line` (from 1).")?;
        outcome
            .diagnostics
            .iter()
            .filter(|d| d.file.as_deref() == Some(path) && d.line.map(u64::from) == Some(line))
            .max_by_key(|d| d.severity)
            .cloned()
            .ok_or_else(|| format!("The last build has no message at {path}:{line}."))
    }

    /// The disclosure of a request and whether the user must still agree to it.
    fn disclosure(&self, request: &Request) -> Result<(Disclosure, bool)> {
        let c = self.ai.saved.connection.as_ref().ok_or("AI is off: connect a model first (Settings › AI).")?;
        let d = Disclosure::for_request(&app()?, c, request);
        let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default();
        let ask = self.ai.saved.consent.needs_consent(&d, now);
        Ok((d, ask))
    }

    /// `ai.disclose`: what a request (explaining a message, by default) sends, where, and whether
    /// the user must agree first.
    pub(crate) fn ai_disclose(&mut self, p: &Value) -> Result<Value> {
        self.ai_ready()?;
        let kind = capability(p.get("request").and_then(Value::as_str).unwrap_or("explain"))?;
        let (request, sends) = match kind {
            REWRITE => {
                let passage = self.ai_passage(p)?;
                (passage.request()?, passage.describe())
            }
            COMPLETE => {
                let paragraph = if p.get("path").is_some() { self.ai_paragraph(p)? } else { Paragraph::example() };
                (paragraph.request()?, paragraph.describe())
            }
            MAKE => {
                let making = self.ai_making(p)?;
                (making.request()?, making.describe())
            }
            REFERENCE => {
                let lookup = self.ai_lookup(p)?;
                (lookup.request()?, lookup.describe())
            }
            ASK => {
                let question = self.ai_question(p)?;
                (question.request()?, question.describe())
            }
            REVIEW => {
                let section = self.ai_section(p)?;
                (section.request()?, section.describe())
            }
            _ => {
                let d = self.ai_diagnostic(p)?;
                let context = Context::of(&d, &self.project);
                (context.request()?, context.describe())
            }
        };
        let (disclosure, ask) = self.disclosure(&request)?;
        let bytes: u64 = disclosure.items.iter().filter_map(|i| i.bytes).sum();
        Ok(json!({
            "request": kind,
            "destination": disclosure.destination,
            "local": disclosure.locality == Locality::Local,
            "model": self.ai.saved.model,
            "needs_consent": ask,
            "bytes": bytes,
            "sends": sends,
        }))
    }

    /// `ai.consent`: the user agrees to send one kind of request (explanations, rewrites,
    /// continuations) to the connection.
    pub(crate) fn ai_consent(&mut self, request: &str) -> Result<Value> {
        let kind = capability(request)?;
        let probe = Request::new(CapabilityId::parse(kind).map_err(|e| e.to_string())?).with_input(InputRole::Prompt, Payload::text(""));
        let (d, _) = self.disclosure(&probe)?;
        self.ai.saved.consent.grant(&d);
        self.changed_ai();
        Ok(json!({"agreed": kind, "destination": d.destination}))
    }

    /// `ai.models`: lists the endpoint's models (a handshake: no document text is sent).
    pub(crate) fn ai_list_models(&mut self, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let c = self.ai.saved.connection.clone().ok_or("AI is off: connect a model first (Settings › AI).")?;
        let token = self.token(&c)?;
        let endpoint = c.endpoint.clone().unwrap_or_default();
        let target = Target { endpoint: endpoint.clone(), token: token.clone(), model: self.ai.saved.model.clone().unwrap_or_default() };
        let mut provider = (self.ai.connect)(&target)?;
        let secrets: Vec<SecretString> = token.into_iter().collect();
        let answer = spawn(move || provider.test().map(|info| Answer::Models(info.models)).map_err(|e| scrub(&e.to_string(), &secrets)));
        self.ai.task = Some(Task { work: Work::Models { endpoint }, answer, cancelled: Arc::new(AtomicBool::new(false)) });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        Ok(json!({"models": self.ai.models}))
    }

    /// `ai.explain`: asks the model to explain message `p`; a fix it proposes becomes a suggested
    /// change. Refused before anything is sent when the user has not agreed to it.
    pub(crate) fn ai_explain(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        self.ai.saved.model.clone().ok_or("Choose a model first (Settings › AI).")?;
        let d = self.ai_diagnostic(p)?;
        let context = Context::of(&d, &self.project);
        let preamble = context.preamble.is_some();
        let opts = craft_ai::GenOpts { max_tokens: 1200, temperature: 0.2 };
        let (model, run, answer) = self.begin(&context.request()?, ("explain", "Explain"), context.prompt(), opts, Answer::Text, |to| {
            format!(
                "Explaining sends the message, the source lines around it{} to {to}. Agree to it first (the window asks; agents: ai.disclose shows it, ai.consent agrees).",
                if preamble { " and the preamble" } else { "" },
            )
        })?;
        let key = Key::of(&d);
        self.set_explanation(key.clone(), Explanation::Running);
        self.ai.task = Some(Task {
            work: Work::Explain { key: key.clone(), context: Box::new(context), model, run },
            answer,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        match self.ai.explanations.iter().rev().find(|(k, _)| *k == key).map(|(_, e)| e.clone()) {
            Some(Explanation::Done { text, model, suggestion, note }) => {
                Ok(json!({"explanation": text, "model": model, "suggestion": suggestion, "note": note}))
            }
            Some(Explanation::Failed(e)) => Err(e),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    /// `ai.cancel`: stops waiting for the request in flight, and takes away the continuation
    /// shown; with `only_completion`, only a continuation (typing on cancels it). An answer that
    /// still comes is dropped (the endpoint may still finish it).
    pub(crate) fn ai_cancel(&mut self, only_completion: bool) -> bool {
        let shown = self.ai.ghost.take().is_some();
        if only_completion && !self.ai.task.as_ref().is_some_and(|t| matches!(t.work, Work::Complete { .. })) {
            return shown;
        }
        let Some(task) = self.ai.task.take() else { return shown };
        task.cancelled.store(true, Ordering::Relaxed);
        let cancelled = |run| HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Cancelled, usage: None });
        match task.work {
            Work::Explain { key, run, .. } => {
                self.set_explanation(key, Explanation::Cancelled);
                self.log(cancelled(run));
            }
            Work::Rewrite { run, .. } => {
                self.ai.rewrite = Some(Rewritten::Cancelled);
                self.log(cancelled(run));
            }
            Work::Complete { run, .. } => self.log(cancelled(run)),
            Work::Make { run, .. } => {
                self.ai.made = Some(Made::Cancelled);
                self.log(cancelled(run));
            }
            Work::Reference { run, .. } => {
                self.ai.reference = Some(Referenced::Cancelled);
                self.log(cancelled(run));
            }
            Work::Ask { question, run, .. } => {
                self.set_last_answer(Answered::Cancelled { question: question.question.clone() });
                self.log(cancelled(run));
            }
            Work::Review { run, .. } => {
                self.ai.reviewed = Some(Reviewed::Cancelled);
                self.log(cancelled(run));
            }
            Work::Models { .. } => {}
        }
        true
    }

    /// Starts `request` on the connection, `prompt` being its whole text: refused, with `ask`'s
    /// message, while the user has not agreed to its kind; logged (its size and destination,
    /// never its text); sent from its own thread, where `after` makes the answer of its text. A
    /// continuation being asked for is dropped first. Gives the model and the run's id.
    fn begin(
        &mut self,
        request: &Request,
        role: (&str, &str),
        prompt: String,
        opts: craft_ai::GenOpts,
        after: impl FnOnce(String) -> Answer + Send + 'static,
        ask: impl FnOnce(&str) -> String,
    ) -> Result<(String, String, Pending)> {
        let model = self.ai.saved.model.clone().ok_or("Choose a model first (Settings › AI).")?;
        let (disclosure, needs) = self.disclosure(request)?;
        if needs {
            return Err(ask(&disclosure.destination));
        }
        let c = self.ai.saved.connection.clone().ok_or("AI is off: connect a model first (Settings › AI).")?;
        let token = self.token(&c)?;
        let target = Target { endpoint: c.endpoint.clone().unwrap_or_default(), token: token.clone(), model: model.clone() };
        let mut provider = (self.ai.connect)(&target)?;
        if self.ai.task.is_some() {
            self.ai_cancel(true);
        }
        self.ai.runs += 1;
        let run = format!("{APP}-{}-{}", now_ms(), self.ai.runs);
        let bytes: u64 = disclosure.items.iter().filter_map(|i| i.bytes).sum();
        log::info!("AI request {run}: {bytes} bytes of text to {} ({model})", disclosure.destination);
        self.log(HistoryRecord::Start(StartRecord {
            v: HISTORY_SCHEMA,
            run: run.clone(),
            at_ms: now_ms(),
            app: APP.to_owned(),
            role_id: role.0.to_owned(),
            role_label: role.1.to_owned(),
            capability: request.capability.as_str().to_owned(),
            connection: c.id.to_string(),
            connection_label: c.label.clone(),
            model: model.clone(),
            locality: c.locality,
            destination: disclosure.destination.clone(),
            sent: disclosure.items.clone(),
            params: Vec::new(),
            provider_job: None,
            estimate: None,
            prompt: None,
        }));
        let secrets: Vec<SecretString> = token.into_iter().collect();
        let answer = spawn(move || provider.complete(&prompt, &opts).map(after).map_err(|e| scrub(&e.to_string(), &secrets)));
        Ok((model, run, answer))
    }

    /// What finished in the background since the last call. The window calls it every frame.
    pub fn poll_ai(&mut self) -> Option<Event> {
        let task = self.ai.task.as_ref()?;
        let answer = match task.answer.try_recv() {
            Ok(a) => a,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("The request stopped without an answer.".to_owned()),
        };
        self.finish(answer)
    }

    fn wait_ai(&mut self) -> Result<()> {
        let Some(task) = self.ai.task.as_ref() else { return Ok(()) };
        let answer = task.answer.recv().unwrap_or_else(|_| Err("The request stopped without an answer.".to_owned()));
        match self.finish(answer) {
            Some(Event::Models(Err(e))) => Err(e),
            _ => Ok(()),
        }
    }

    fn finish(&mut self, answer: std::result::Result<Answer, String>) -> Option<Event> {
        let task = self.ai.task.take()?;
        if task.cancelled.load(Ordering::Relaxed) {
            return None;
        }
        match (task.work, answer) {
            (Work::Models { endpoint }, Ok(Answer::Models(models))) => {
                // An answer for an address the connection no longer has is dropped.
                let current = self.ai.saved.connection.as_ref().and_then(|c| c.endpoint.clone());
                if current.as_deref() != Some(endpoint.as_str()) {
                    return None;
                }
                let n = models.len();
                self.ai.models = models;
                Some(Event::Models(Ok(n)))
            }
            (Work::Models { .. }, Err(e)) => Some(Event::Models(Err(e))),
            (Work::Explain { key, context, model, run }, Ok(Answer::Text(text))) => {
                let reply = parse_reply(&text);
                let (suggestion, note) = match reply.fix.as_deref() {
                    Some(fix) => match self.suggest_fix(&context, fix, &model) {
                        Ok(s) => (s, None),
                        Err(why) => (None, Some(why)),
                    },
                    None => (None, None),
                };
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                let made = suggestion.is_some();
                self.set_explanation(key, Explanation::Done { text: reply.explanation, model, suggestion, note });
                Some(Event::Explained(Ok(made)))
            }
            (Work::Explain { key, run, .. }, answer) => {
                let e = match answer {
                    Err(e) => e,
                    Ok(_) => "The model's answer could not be read.".to_owned(),
                };
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                self.set_explanation(key, Explanation::Failed(e.clone()));
                Some(Event::Explained(Err(e)))
            }
            (Work::Models { .. }, Ok(_)) => Some(Event::Models(Err("The endpoint's answer could not be read.".to_owned()))),
            (Work::Rewrite { passage, model, run }, answer) => Some(self.finish_rewrite(&passage, &model, run, answer)),
            (Work::Complete { paragraph, run }, answer) => Some(self.finish_complete(&paragraph, run, answer)),
            (Work::Make { making, model, run, insert }, answer) => Some(self.finish_make(&making, &model, run, insert, answer)),
            (Work::Reference { lookup, model, run }, answer) => Some(self.finish_reference(&lookup, &model, run, answer)),
            (Work::Ask { question, model, run }, answer) => Some(self.finish_ask(&question, &model, run, answer)),
            (Work::Review { section, model, run }, answer) => Some(self.finish_review(&section, &model, run, answer)),
        }
    }

    /// Makes the model's corrected lines a suggested change by "AI (<model>)": only the part that
    /// differs, and only while the lines are still as they were sent.
    fn suggest_fix(&mut self, context: &Context, fix: &str, model: &str) -> std::result::Result<Option<String>, String> {
        let (Some(file), Some((from, sent))) = (&context.file, &context.lines) else {
            return Err("The model proposed a fix, but the message has no place in the source to put it.".to_owned());
        };
        let path = ProjectPath::new(file).map_err(|e| e.to_string())?;
        let source = self.project.text(&path).map_err(|e| e.to_string())?.to_owned();
        let all: Vec<&str> = source.split('\n').collect();
        let first = (*from as usize).saturating_sub(1);
        let count = sent.split('\n').count();
        let now: Vec<&str> = all.get(first..first + count).unwrap_or_default().to_vec();
        if now.iter().map(|l| l.trim_end_matches('\r')).collect::<Vec<_>>().join("\n") != *sent {
            return Err("The lines changed while the model answered, so its fix is not offered.".to_owned());
        }
        // The file's own line ends, so the change's place counts them.
        let raw = now.join("\n");
        let fix = if raw.contains('\r') { fix.split('\n').collect::<Vec<_>>().join("\r\n") } else { fix.to_owned() };
        let Some((start, end, text)) = smallest_change(&raw, &fix) else { return Ok(None) };
        let base: usize = all.iter().take(first).map(|l| l.chars().count() + 1).sum();
        self.suggest_change(&path, &source, base + start, base + end, &text, model).map(Some)
    }

    /// A suggested change by "AI (<model>)": characters `start..end` of `source` (the text of
    /// `path`) become `text`. Its id.
    fn suggest_change(&mut self, path: &ProjectPath, source: &str, start: usize, end: usize, text: &str, model: &str) -> Result<String> {
        let id = self.new_id()?;
        let anchor = texcraft_review::Anchor::new(path.as_str(), source, start, end);
        let author = texcraft_review::Author::new(format!("AI ({model})"), "");
        let now = self.now();
        self.review.review.suggest(id, anchor, source, text, author, now).map_err(|e| e.to_string())?;
        self.review_changed(id)?;
        Ok(id.to_string())
    }

    fn set_explanation(&mut self, key: Key, e: Explanation) {
        self.ai.explanations.retain(|(k, _)| *k != key);
        self.ai.explanations.push((key, e));
        let over = self.ai.explanations.len().saturating_sub(MAX_EXPLANATIONS);
        self.ai.explanations.drain(..over);
    }

    fn log(&mut self, record: HistoryRecord) {
        self.ai.log.push(record);
        let over = self.ai.log.len().saturating_sub(MAX_LOG);
        self.ai.log.drain(..over);
    }
}

/// Runs `job` on its own thread (in the browser, where nothing reaches this, at once).
fn spawn(job: impl FnOnce() -> std::result::Result<Answer, String> + Send + 'static) -> Receiver<std::result::Result<Answer, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let run = move || {
        let answer = crate::guard::catch_panic(job).unwrap_or_else(Err);
        let _ = tx.send(answer);
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(run);
    #[cfg(target_arch = "wasm32")]
    run();
    rx
}

/// An error's text without the token in it.
fn scrub(text: &str, secrets: &[SecretString]) -> String {
    let refs: Vec<&SecretString> = secrets.iter().collect();
    craft_ai_core::secret::redact(text, &refs)
}

#[derive(Serialize, Deserialize)]
struct ProjectSetting {
    enabled: bool,
}

fn read_project_off(file: &std::path::Path) -> bool {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<ProjectSetting>(&t).ok()).is_some_and(|s| !s.enabled)
}

fn write_project_off(file: &std::path::Path, off: bool) -> Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&ProjectSetting { enabled: !off }).map_err(|e| e.to_string())?;
    std::fs::write(file, text + "\n").map_err(|e| format!("writing {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_without_markers_is_all_explanation() {
        let r = parse_reply("The brace is not closed.\n");
        assert_eq!(r.fix, None);
        assert!(r.explanation.contains("brace"));
    }

    #[test]
    fn the_fix_block_is_taken_out_of_the_explanation_and_its_fence_dropped() {
        let r = parse_reply("It is a typo.\n<<<FIX\n```latex\n\\textbf{x}\n```\nFIX>>>\nThat is all.");
        assert_eq!(r.fix.as_deref(), Some("\\textbf{x}"));
        assert!(!r.explanation.contains("FIX"), "{}", r.explanation);
        assert!(r.explanation.contains("typo") && r.explanation.contains("all"));
    }

    #[test]
    fn the_smallest_change_covers_only_what_differs() {
        let (s, e, t) = smallest_change("a\n\\textbf{x\nc", "a\n\\textbf{x}\nc").unwrap_or_default();
        assert!(s >= 2 && e <= "a\n\\textbf{x\nc".chars().count(), "{s}..{e}");
        assert_eq!(t, "}");
        assert_eq!(smallest_change("same", "same"), None);
    }
}
