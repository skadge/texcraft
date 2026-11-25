//! AI4 (docs/ROADMAP.md): making LaTeX from a description, and drafting a BibTeX entry.
//!
//! Making sends the description, what kind of thing it is, the main document's preamble (so the
//! model uses the packages it loads) and, for a figure, the names of the project's images. The
//! answer is typeset after the preamble in a scratch document ([`Typeset`]) before it is offered:
//! its preview, or retex's messages, which can be sent back to ask again. Inserted by the person who
//! asked for it and saw it (the window's Insert), it is an edit at the caret, undone by Undo, and
//! the packages it needs go before `\begin{document}` in the same way. Inserted without being looked
//! at (an agent, `ai.make` with `insert`), it is a suggested change by "AI (<model>)" instead.
//!
//! A reference sends the title or DOI alone. The entry the model drafts goes at the end of the
//! chosen `.bib` (created when it does not exist) as a suggested change, under a comment line
//! saying a model drafted it: models invent references, so nothing here calls it verified.

use std::sync::{Arc, OnceLock};

use craft_ai_core::history::{EndRecord, HISTORY_SCHEMA, Outcome as Logged};
use craft_ai_core::{HistoryRecord, InputRole, Request};
use serde_json::{Value, json};
use texcraft_project::{Contents, Project, ProjectPath};

use super::write::{between, unfenced};
use super::{Answer, Event, MAKE, MAX_PREAMBLE_CHARS, REFERENCE, Task, Work, kind_of, now_ms, prompts};
use crate::{Change, Origin, Result, Session, Transaction};

/// The longest description or query sent, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 2000;
/// The most image names listed for a figure.
const MAX_IMAGES: usize = 200;

/// What to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MakeKind {
    Table,
    Equation,
    Tikz,
    Figure,
}

impl MakeKind {
    pub const ALL: [MakeKind; 4] = [MakeKind::Table, MakeKind::Equation, MakeKind::Tikz, MakeKind::Figure];

    /// From `ai.make`'s `kind`.
    pub fn parse(kind: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.id() == kind)
            .ok_or_else(|| format!("There is no `{kind}` to make: there are table, equation, tikz and figure."))
    }

    /// The name `ai.make` takes.
    pub fn id(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Equation => "equation",
            Self::Tikz => "tikz",
            Self::Figure => "figure",
        }
    }

    fn instruction(self) -> &'static str {
        match self {
            Self::Table => "Write a table: a table environment holding a centred tabular, with a \\caption and a \\label.",
            Self::Equation => "Write the equation: an equation environment (align for several lines) with a \\label.",
            Self::Tikz => "Write a diagram drawn with TikZ: a tikzpicture inside a figure environment, with a \\caption and a \\label.",
            Self::Figure => {
                "Write a figure environment for the image the description names: \\includegraphics of one of the project's images listed, by its path, with a \\caption and a \\label."
            }
        }
    }
}

/// What making sends: exactly these parts, in [`Making::prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Making {
    pub kind: MakeKind,
    pub description: String,
    /// Where it goes: the file and the caret, in characters from 0.
    pub path: String,
    pub at: usize,
    /// The main document's preamble, at most [`MAX_PREAMBLE_CHARS`].
    pub preamble: String,
    /// For a figure: the project's images.
    pub images: Vec<String>,
    /// Asking again: the LaTeX the model gave last, and why it did not typeset.
    pub again: Option<(String, Vec<String>)>,
}

impl Making {
    fn parts(&self) -> prompts::Parts {
        let preamble = if self.preamble.trim().is_empty() {
            String::new()
        } else {
            format!("The document's preamble:\n<<<PREAMBLE\n{}\nPREAMBLE>>>", self.preamble)
        };
        let images = if self.kind == MakeKind::Figure { format!("The project's images:\n{}", self.image_list()) } else { String::new() };
        let again = match &self.again {
            Some((latex, messages)) => format!(
                "Your last answer did not typeset:\n<<<LAST\n{latex}\nLAST>>>\nThe typesetting engine said:\n{}\nCorrect it.",
                messages.join("\n")
            ),
            None => String::new(),
        };
        vec![
            ("kind", self.kind.instruction().to_owned(), None),
            ("description", self.description.clone(), None),
            ("preamble", preamble, Some(InputRole::Context)),
            ("images", images, Some(InputRole::Context)),
            ("again", again, Some(InputRole::Context)),
        ]
    }

    /// The whole text sent to the model, with `template` (the prompt for making).
    pub fn prompt(&self, template: &str) -> String {
        prompts::prompt(template, &self.parts())
    }

    fn image_list(&self) -> String {
        if self.images.is_empty() { "(none)".to_owned() } else { self.images.join("\n") }
    }

    pub(super) fn request(&self, template: &str) -> Result<Request> {
        prompts::request(MAKE, template, &self.parts())
    }

    pub(super) fn describe(&self, template: &str) -> Value {
        json!({
            "kind": self.kind.id(),
            "description": self.description,
            "path": self.path,
            "at": self.at,
            "preamble": self.preamble,
            "images": (self.kind == MakeKind::Figure).then_some(&self.images),
            "again": self.again.as_ref().map(|(latex, messages)| json!({"latex": latex, "messages": messages})),
            "prompt": self.prompt(template),
        })
    }
}

/// A document to typeset for a preview: the project's files, the main document's folder, the
/// preamble and the body.
#[derive(Debug, Clone)]
pub struct Scratch {
    pub files: Vec<(ProjectPath, Arc<[u8]>)>,
    pub main_dir: Option<String>,
    pub preamble: String,
    pub body: String,
}

/// Typesets a scratch document: its PDF, or retex's messages. On the desktop, retex in a copy of
/// the project (`texcraft_compile::scratch`); tests give their own.
pub type Typeset = Arc<dyn Fn(&Scratch) -> std::result::Result<Vec<u8>, Vec<String>> + Send + Sync>;

/// The desktop's: retex in process, each preview in a folder of its own (sessions of one process
/// may typeset at once).
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
pub fn retex_typeset() -> Typeset {
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    Arc::new(|s: &Scratch| {
        let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("texcraft-ai-preview-{}-{run}", std::process::id()));
        let scratch = texcraft_compile::scratch::Scratch {
            files: s.files.clone(),
            main_dir: s.main_dir.clone(),
            preamble: s.preamble.clone(),
            body: s.body.clone(),
        };
        let typeset = texcraft_compile::scratch::typeset(&dir, &scratch);
        let _ = std::fs::remove_dir_all(&dir);
        typeset
    })
}

/// Without retex: nothing is typeset, and that is said.
#[cfg(not(all(feature = "retex", not(target_arch = "wasm32"))))]
pub fn retex_typeset() -> Typeset {
    Arc::new(|_: &Scratch| Err(vec!["This build has no typesetting engine: the LaTeX could not be checked.".to_owned()]))
}

/// How the made LaTeX came out of the engine.
#[derive(Debug, Clone)]
pub enum Preview {
    /// Its PDF.
    Typeset(Arc<Vec<u8>>),
    /// retex's messages.
    NotTypeset(Vec<String>),
}

/// The last thing made.
#[derive(Debug, Clone)]
pub enum Made {
    Running,
    Done {
        kind: MakeKind,
        latex: String,
        /// The `\usepackage` (and `\usetikzlibrary`) lines it needs that the preamble lacks.
        packages: Vec<String>,
        model: String,
        path: String,
        at: usize,
        preview: Preview,
        /// How it was inserted, once it is.
        inserted: Option<Inserted>,
    },
    Failed(String),
    Cancelled,
}

/// How made LaTeX went into the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inserted {
    /// As these suggested changes.
    Suggested(Vec<String>),
    /// As an edit (Undo takes it out).
    Edited,
}

/// The LaTeX and the packages in a model's answer. A whole document is cut to its body, its
/// preamble's package lines kept; lines the preamble already has are dropped.
pub fn made(answer: &str, preamble: &str) -> (String, Vec<String>) {
    let mut latex = between(answer, "LATEX").unwrap_or_else(|| unfenced(answer.trim()));
    let mut wanted: Vec<String> = between(answer, "PACKAGES").map(|p| p.lines().map(str::to_owned).collect()).unwrap_or_default();
    if let (Some(b), Some(e)) = (latex.find("\\begin{document}"), latex.rfind("\\end{document}"))
        && b < e
    {
        wanted.extend(latex[..b].lines().map(str::to_owned));
        latex = latex[b + "\\begin{document}".len()..e].to_owned();
    }
    let have: Vec<String> = preamble.lines().map(|l| l.split('%').next().unwrap_or_default().trim().to_owned()).collect();
    let mut packages: Vec<String> = Vec::new();
    for line in wanted {
        let line = line.trim().to_owned();
        let fits = line.starts_with("\\usepackage") || line.starts_with("\\usetikzlibrary");
        if fits && !have.contains(&line) && !packages.contains(&line) {
            packages.push(line);
        }
    }
    (latex.trim_matches('\n').trim_end().to_owned(), packages)
}

/// Whether `path` names an image `\includegraphics` takes.
fn is_image(path: &ProjectPath) -> bool {
    matches!(path.extension().to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "pdf" | "eps" | "svg")
}

/// The main document's preamble, and its folder; the default article's when there is none.
fn preamble_of(project: &Project) -> (String, Option<String>) {
    let main = project.main();
    let main_dir = main.and_then(|m| m.as_str().rsplit_once('/').map(|(d, _)| d.to_owned()));
    let text = main.and_then(|m| project.text(m).ok()).unwrap_or_default();
    let preamble = text.find("\\begin{document}").map(|at| &text[..at]).unwrap_or_default();
    (preamble.chars().take(MAX_PREAMBLE_CHARS).collect(), main_dir)
}

/// The preamble a preview is typeset after: the project's, else an article with the usual packages.
fn preview_preamble(preamble: &str) -> String {
    if preamble.contains("\\documentclass") {
        preamble.to_owned()
    } else {
        "\\documentclass{article}\n\\usepackage{amsmath}\n\\usepackage{graphicx}\n".to_owned()
    }
}

/// What drafting a reference sends: the title or DOI, nothing of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub query: String,
    /// The `.bib` it goes in.
    pub bib: String,
}

impl Lookup {
    fn parts(&self) -> prompts::Parts {
        vec![("work", self.query.clone(), Some(InputRole::Source))]
    }

    /// The whole text sent to the model, with `template` (the prompt for references).
    pub fn prompt(&self, template: &str) -> String {
        prompts::prompt(template, &self.parts())
    }

    pub(super) fn request(&self, template: &str) -> Result<Request> {
        prompts::request(REFERENCE, template, &self.parts())
    }

    pub(super) fn describe(&self, template: &str) -> Value {
        json!({"query": self.query, "bib": self.bib, "prompt": self.prompt(template)})
    }
}

/// The last reference drafted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Referenced {
    Running,
    Done {
        /// The entry as suggested, with its comment line.
        entry: String,
        key: String,
        bib: String,
        suggestion: String,
        /// The `.bib` was made for it.
        created: bool,
        model: String,
    },
    Failed(String),
    Cancelled,
}

/// What every drafted reference comes with, in the dialog and in `ai.reference`'s answer.
pub const UNVERIFIED: &str =
    "Models invent references: this entry is a draft, not a verified record. Check every field against the work itself before you cite it.";

fn entry_head() -> Option<&'static regex::Regex> {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?m)^\s*@([A-Za-z]+)\s*\{\s*([^,\s{}]+)\s*,").ok()).as_ref()
}

/// The first entry in a model's answer and its key: from its `@type{key,` to its closing brace.
pub fn entry(answer: &str) -> Option<(String, String)> {
    let body = between(answer, "BIB").unwrap_or_else(|| unfenced(answer.trim()));
    let c = entry_head()?.captures(&body)?;
    let kind = c.get(1)?.as_str().to_ascii_lowercase();
    if matches!(kind.as_str(), "comment" | "string" | "preamble") {
        return None;
    }
    let start = c.get(0)?.start() + c.get(0)?.as_str().find('@')?;
    let mut depth = 0usize;
    for (i, ch) in body[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((body[start..=start + i].to_owned(), c.get(2)?.as_str().to_owned()));
                }
            }
            _ => {}
        }
    }
    None
}

/// `key`, or the first of `key` + a, b, c … that `bib` does not use yet.
fn free_key(bib: &str, key: &str) -> String {
    let used: Vec<&str> = entry_head().map(|re| re.captures_iter(bib).filter_map(|c| c.get(2)).map(|m| m.as_str()).collect()).unwrap_or_default();
    if !used.contains(&key) {
        return key.to_owned();
    }
    ('a'..='z').map(|s| format!("{key}{s}")).find(|k| !used.contains(&k.as_str())).unwrap_or_else(|| format!("{key}-ai"))
}

impl Session {
    /// What `ai.make`'s (or `ai.disclose`'s) parameters ask to make.
    pub(super) fn ai_making(&self, p: &Value) -> Result<Making> {
        let kind = MakeKind::parse(p.get("kind").and_then(Value::as_str).unwrap_or("table"))?;
        let description = p.get("description").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        if description.is_empty() {
            return Err("Describe what to make: `description`.".to_owned());
        }
        if description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(format!("Describe it in at most {MAX_DESCRIPTION_CHARS} characters."));
        }
        let path = p.get("path").and_then(Value::as_str).ok_or("Name the file it goes in: `path`.")?;
        let at = p.get("at").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or("`at` is the caret, in characters from 0.")?;
        let text = self.project.text(&ProjectPath::new(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if at > text.chars().count() {
            return Err(format!("{path} has fewer than {at} characters."));
        }
        let (preamble, _) = preamble_of(&self.project);
        let images = if kind == MakeKind::Figure {
            self.project.paths().filter(|p| is_image(p)).take(MAX_IMAGES).map(|p| p.as_str().to_owned()).collect()
        } else {
            Vec::new()
        };
        let again = if p.get("again").and_then(Value::as_bool).unwrap_or(false) {
            match &self.ai.made {
                Some(Made::Done { latex, preview: Preview::NotTypeset(messages), .. }) => Some((latex.clone(), messages.clone())),
                _ => return Err("There is nothing that did not typeset to ask about again.".to_owned()),
            }
        } else {
            None
        };
        Ok(Making { kind, description: description.to_owned(), path: path.to_owned(), at, preamble, images, again })
    }

    /// The last thing made, if something was asked for.
    pub fn ai_made(&self) -> Option<&Made> {
        self.ai.made.as_ref()
    }

    /// `ai.make`: asks the model for LaTeX, typesets it after the preamble and, with `insert`
    /// (the default) and when it typeset, makes it a suggested change at the caret.
    pub(crate) fn ai_make(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let making = self.ai_making(p)?;
        let insert = p.get("insert").and_then(Value::as_bool).unwrap_or(true);
        let (preamble, main_dir) = preamble_of(&self.project);
        let files: Vec<(ProjectPath, Arc<[u8]>)> = self.project.files().map(|(p, c)| (p.clone(), Arc::<[u8]>::from(c.as_bytes()))).collect();
        let typeset = Arc::clone(&self.ai.typeset);
        let after = move |text: String| {
            let (latex, packages) = made(&text, &preamble);
            let preview = if latex.trim().is_empty() {
                Preview::NotTypeset(vec!["The model gave no LaTeX.".to_owned()])
            } else {
                let mut preamble = preview_preamble(&preamble);
                for line in &packages {
                    preamble.push_str(&format!("\n{line}"));
                }
                match typeset(&Scratch { files, main_dir, preamble, body: latex.clone() }) {
                    Ok(pdf) => Preview::Typeset(Arc::new(pdf)),
                    Err(messages) => Preview::NotTypeset(messages),
                }
            };
            Answer::Made { latex, packages, preview }
        };
        let opts = craft_ai::GenOpts { max_tokens: 2400, temperature: 0.2 };
        let (template, note) = self.ai_template("make");
        let (model, run, answer) = self.begin(&making.request(&template)?, ("make", "Make with AI"), making.prompt(&template), opts, after)?;
        self.ai.prompt_note = note;
        self.ai.made = Some(Made::Running);
        self.ai.task = Some(Task { work: Work::Make { making: Box::new(making), model, run, insert }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        self.describe_made()
    }

    fn describe_made(&self) -> Result<Value> {
        match &self.ai.made {
            Some(Made::Done { kind, latex, packages, model, preview, inserted, .. }) => {
                let messages = match preview {
                    Preview::NotTypeset(m) => m.clone(),
                    Preview::Typeset(_) => Vec::new(),
                };
                Ok(json!({
                    "kind": kind.id(),
                    "latex": latex,
                    "packages": packages,
                    "model": model,
                    "typeset": matches!(preview, Preview::Typeset(_)),
                    "messages": messages,
                    "suggestions": match inserted {
                        Some(Inserted::Suggested(ids)) => ids.clone(),
                        _ => Vec::new(),
                    },
                    "edited": *inserted == Some(Inserted::Edited),
                    "note": (!messages.is_empty()).then_some("It does not typeset, so it was not inserted: ask again with `again: true` to send retex's messages back, or insert it anyway with ai.insert_made {anyway: true}."),
                }))
            }
            Some(Made::Failed(e)) => Err(e.clone()),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    pub(super) fn finish_make(
        &mut self,
        making: &Making,
        model: &str,
        run: String,
        insert: bool,
        answer: std::result::Result<Answer, String>,
    ) -> Event {
        match answer {
            Ok(Answer::Made { latex, packages, preview }) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                let typeset = matches!(preview, Preview::Typeset(_));
                self.ai.made = Some(Made::Done {
                    kind: making.kind,
                    latex,
                    packages,
                    model: model.to_owned(),
                    path: making.path.clone(),
                    at: making.at,
                    preview,
                    inserted: None,
                });
                if insert
                    && typeset
                    && let Err(e) = self.ai_insert_made(false, false)
                {
                    self.ai.made = Some(Made::Failed(e.clone()));
                    return Event::Made(Err(e));
                }
                Event::Made(Ok(typeset))
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                self.ai.made = Some(Made::Failed(e.clone()));
                Event::Made(Err(e))
            }
        }
    }

    /// `ai.insert_made`: the last thing made goes in at its caret, and the packages it needs
    /// before `\begin{document}`: with `edit`, as an edit Undo takes out (the window, whose user
    /// saw it and chose Insert); otherwise as suggested changes. One that did not typeset only
    /// `anyway`.
    pub(crate) fn ai_insert_made(&mut self, anyway: bool, edit: bool) -> Result<Value> {
        let Some(Made::Done { latex, packages, model, path, at, preview, inserted, .. }) = self.ai.made.clone() else {
            return Err("There is nothing made to insert: make something first (ai.make).".to_owned());
        };
        match inserted {
            Some(Inserted::Suggested(_)) => return Err("It is already inserted as a suggested change.".to_owned()),
            Some(Inserted::Edited) => return Err("It is already inserted.".to_owned()),
            None => {}
        }
        if let Preview::NotTypeset(messages) = &preview
            && !anyway
        {
            return Err(format!("It does not typeset ({}): ask again with the error, or insert it anyway.", messages.join("; ")));
        }
        let p = ProjectPath::new(&path).map_err(|e| e.to_string())?;
        let source = self.project.text(&p).map_err(|e| e.to_string())?.to_owned();
        let chars: Vec<char> = source.chars().collect();
        if at > chars.len() {
            return Err(format!("{path} is shorter than when it was made: make it again."));
        }
        // On lines of its own.
        let before = if at == 0 || chars.get(at - 1) == Some(&'\n') { "" } else { "\n" };
        let after = if chars.get(at) == Some(&'\n') { "" } else { "\n" };
        let body = format!("{before}{latex}{after}");
        // Where the packages go: the main document, before `\begin{document}`.
        let mut note = None;
        let mut place = None;
        if !packages.is_empty() {
            let main = self.project.main().cloned();
            let found = main.as_ref().and_then(|m| self.project.text(m).ok().map(|t| (m.clone(), t.to_owned())));
            match found.and_then(|(m, t)| t.find("\\begin{document}").map(|b| (m, t[..b].chars().count(), t))) {
                Some(found) => place = Some(found),
                None => note = Some(format!("It needs {} in the preamble: the main document has none to add them to.", packages.join(", "))),
            }
        }
        let lines = format!("{}\n", packages.join("\n"));
        if !edit {
            let mut ids = vec![self.suggest_change(&p, &source, at, at, &body, &model)?];
            if let Some((m, at, text)) = place {
                ids.push(self.suggest_change(&m, &text, at, at, &lines, &model)?);
            }
            if let Some(Made::Done { inserted, .. }) = &mut self.ai.made {
                *inserted = Some(Inserted::Suggested(ids.clone()));
            }
            return Ok(json!({"suggestions": ids, "note": note}));
        }
        // One edit for each file: in the same file, the packages and the LaTeX go together.
        let added = if place.is_some() { packages.clone() } else { Vec::new() };
        let mut start = at;
        match place {
            Some((m, pat, _)) if m == p => {
                let changes = vec![Change { start: pat, end: pat, text: lines.clone() }, Change { start: at, end: at, text: body.clone() }];
                self.apply_edit(&p, &Transaction::new(changes).map_err(|e| e.to_string())?, Origin::Local)?;
                if pat <= at {
                    start += lines.chars().count();
                }
            }
            Some((m, pat, _)) => {
                self.apply_edit(&p, &Transaction::insert(at, &body), Origin::Local)?;
                self.apply_edit(&m, &Transaction::insert(pat, &lines), Origin::Local)?;
            }
            None => self.apply_edit(&p, &Transaction::insert(at, &body), Origin::Local)?,
        }
        if let Some(Made::Done { inserted, .. }) = &mut self.ai.made {
            *inserted = Some(Inserted::Edited);
        }
        let end = start + body.chars().count();
        Ok(json!({"edited": true, "path": path, "start": start, "end": end, "packages": added, "note": note, "revision": self.revision}))
    }

    /// What `ai.reference`'s (or `ai.disclose`'s) parameters ask for.
    pub(super) fn ai_lookup(&self, p: &Value) -> Result<Lookup> {
        let query = p.get("query").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        if query.is_empty() {
            return Err("Give the work's title or DOI: `query`.".to_owned());
        }
        if query.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(format!("Give the title or DOI in at most {MAX_DESCRIPTION_CHARS} characters."));
        }
        let bib = p.get("bib").and_then(Value::as_str).map(str::trim).unwrap_or("references.bib");
        let path = ProjectPath::new(bib).map_err(|e| e.to_string())?;
        if path.extension() != "bib" {
            return Err(format!("`{bib}` is not a .bib file."));
        }
        if self.project.get(&path).is_some() {
            self.project.text(&path).map_err(|e| e.to_string())?;
        }
        Ok(Lookup { query: query.to_owned(), bib: path.as_str().to_owned() })
    }

    /// The last reference drafted, if one was asked for.
    pub fn ai_referenced(&self) -> Option<&Referenced> {
        self.ai.reference.as_ref()
    }

    /// `ai.reference`: asks the model to draft a BibTeX entry; it becomes a suggested change at
    /// the end of the `.bib`, under a line saying a model drafted it.
    pub(crate) fn ai_reference(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let lookup = self.ai_lookup(p)?;
        let opts = craft_ai::GenOpts { max_tokens: 800, temperature: 0.0 };
        let (template, note) = self.ai_template("reference");
        let (model, run, answer) =
            self.begin(&lookup.request(&template)?, ("reference", "Add a Reference"), lookup.prompt(&template), opts, Answer::Text)?;
        self.ai.prompt_note = note;
        self.ai.reference = Some(Referenced::Running);
        self.ai.task = Some(Task { work: Work::Reference { lookup: Box::new(lookup), model, run }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        match self.ai.reference.clone() {
            Some(Referenced::Done { entry, key, bib, suggestion, created, model }) => Ok(json!({
                "entry": entry, "key": key, "bib": bib, "suggestion": suggestion, "created": created, "model": model, "note": UNVERIFIED,
            })),
            Some(Referenced::Failed(e)) => Err(e),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    pub(super) fn finish_reference(&mut self, lookup: &Lookup, model: &str, run: String, answer: std::result::Result<Answer, String>) -> Event {
        let outcome = match answer {
            Ok(Answer::Text(text)) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                self.offer_reference(lookup, &text, model)
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                Err(e)
            }
        };
        self.ai.reference = Some(match &outcome {
            Ok(done) => done.clone(),
            Err(e) => Referenced::Failed(e.clone()),
        });
        Event::Referenced(outcome.map(|r| match r {
            Referenced::Done { bib, .. } => bib,
            _ => String::new(),
        }))
    }

    fn offer_reference(&mut self, lookup: &Lookup, answer: &str, model: &str) -> Result<Referenced> {
        let (entry, key) = entry(answer).ok_or("The model gave no BibTeX entry.")?;
        let path = ProjectPath::new(&lookup.bib).map_err(|e| e.to_string())?;
        let created = self.project.get(&path).is_none();
        if created {
            self.project.add(path.clone(), Contents::Text(String::new())).map_err(|e| e.to_string())?;
            self.touch();
        }
        let source = self.project.text(&path).map_err(|e| e.to_string())?.to_owned();
        let free = free_key(&source, &key);
        let entry = if free == key { entry } else { entry.replacen(&key, &free, 1) };
        let gap = if source.is_empty() || source.ends_with("\n\n") {
            ""
        } else if source.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let text = format!("{gap}% Drafted by AI ({model}): check it\n{}\n", entry.trim());
        let end = source.chars().count();
        let suggestion = self.suggest_change(&path, &source, end, end, &text, model)?;
        Ok(Referenced::Done { entry: text.trim_start().to_owned(), key: free, bib: lookup.bib.clone(), suggestion, created, model: model.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_document_is_cut_to_its_body_keeping_the_packages_the_preamble_lacks() {
        let answer = "<<<LATEX\n\\documentclass{article}\n\\usepackage{amsmath}\n\\usepackage{tikz}\n\\begin{document}\n\\begin{tikzpicture}\\end{tikzpicture}\n\\end{document}\nLATEX>>>";
        let (latex, packages) = made(answer, "\\documentclass{article}\n\\usepackage{amsmath} % maths\n");
        assert_eq!(latex, "\\begin{tikzpicture}\\end{tikzpicture}");
        assert_eq!(packages, vec!["\\usepackage{tikz}".to_owned()]);
    }

    #[test]
    fn an_entry_is_taken_whole_with_nested_braces_and_a_used_key_is_changed() {
        let answer = "Here:\n<<<BIB\n@article{knuth84,\n  title = {The {TeX}book},\n  year = 1984\n}\nBIB>>>\nNote this.";
        let (e, key) = entry(answer).unwrap_or_default();
        assert_eq!(key, "knuth84");
        assert!(e.starts_with("@article{knuth84,") && e.ends_with('}') && e.contains("{TeX}"), "{e}");
        assert_eq!(free_key("@book{knuth84, title={x}}\n@book{knuth84a, title={y}}", "knuth84"), "knuth84b");
        assert_eq!(entry("@comment{nothing}"), None);
    }
}
