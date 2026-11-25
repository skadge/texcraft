//! AI5 (docs/ROADMAP.md): asking about the project, and a review left as comments.
//!
//! A question sends the open file, or by the user's choice every text file of the project, with
//! its lines numbered so the answer can name places as `path:line`; those become links
//! ([`links`]). A review sends one section; each remark the model makes quotes the words it is
//! about, and becomes a comment by "AI (<model>)" on the first place those words are found at or
//! after the section's start. A remark whose quote is not in the text is dropped, and counted.

use std::sync::OnceLock;

use craft_ai_core::history::{EndRecord, HISTORY_SCHEMA, Outcome as Logged};
use craft_ai_core::{CapabilityId, HistoryRecord, InputRole, Payload, Request};
use serde_json::{Value, json};
use texcraft_project::{Contents, Project, ProjectPath};

use super::write::bytes_of;
use super::{ASK, Answer, Event, REVIEW, Task, Work, kind_of, now_ms};
use crate::{Result, Session};

/// The longest question, in characters.
pub const MAX_QUESTION_CHARS: usize = 2000;
/// The most text a question sends with it, in characters.
pub const MAX_ASK_CHARS: usize = 150_000;
/// The longest section reviewed, in characters.
pub const MAX_REVIEW_CHARS: usize = 16_000;
/// Answers kept.
const MAX_ANSWERS: usize = 32;
/// Remarks placed from one review, at most.
const MAX_REMARKS: usize = 20;

/// What a question is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The open file.
    File,
    /// Every text file of the project.
    Project,
}

impl Scope {
    pub fn parse(scope: &str) -> Result<Self> {
        match scope {
            "file" => Ok(Self::File),
            "project" => Ok(Self::Project),
            _ => Err(format!("`scope` is file or project, not `{scope}`.")),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Project => "project",
        }
    }
}

const ASK_INSTRUCTIONS: &str = "You answer questions about a LaTeX project from its files, shown below with their lines numbered. \
Answer in plain language, briefly. When you refer to a place in a file, write it as path:line (such as main.tex:12), with the numbers shown. \
If the files do not answer the question, say so.";

/// What a question sends: exactly these parts, in [`Question::prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub question: String,
    pub scope: Scope,
    /// The files sent, with their text.
    pub files: Vec<(String, String)>,
}

/// `text` with its lines numbered from 1.
fn numbered(text: &str) -> String {
    text.lines().enumerate().map(|(i, l)| format!("{}| {l}", i + 1)).collect::<Vec<_>>().join("\n")
}

/// Whether `path` is a text file a question about the project sends.
fn sent_with_project(path: &ProjectPath, contents: &Contents) -> bool {
    matches!(contents, Contents::Text(_))
        && !path.as_str().starts_with(".texcraft/")
        && matches!(path.extension().to_ascii_lowercase().as_str(), "tex" | "bib" | "sty" | "cls" | "ltx" | "dtx" | "bbx" | "cbx" | "md" | "txt")
}

impl Question {
    /// What asking `question` about `path` (or the whole project) sends.
    pub fn of(project: &Project, question: &str, scope: Scope, path: Option<&str>) -> Result<Self> {
        let question = question.trim();
        if question.is_empty() {
            return Err("Ask a question: `question`.".to_owned());
        }
        if question.chars().count() > MAX_QUESTION_CHARS {
            return Err(format!("Ask in at most {MAX_QUESTION_CHARS} characters."));
        }
        let files: Vec<(String, String)> = match scope {
            Scope::File => {
                let path = path.ok_or("Name the open file: `path`.")?;
                let text = project.text(&ProjectPath::new(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                vec![(path.to_owned(), text.to_owned())]
            }
            Scope::Project => project
                .files()
                .filter(|(p, c)| sent_with_project(p, c))
                .filter_map(|(p, c)| match c {
                    Contents::Text(t) => Some((p.as_str().to_owned(), t.clone())),
                    Contents::Binary(_) => None,
                })
                .collect(),
        };
        let size: usize = files.iter().map(|(_, t)| t.chars().count()).sum();
        if size > MAX_ASK_CHARS {
            return Err(format!(
                "That is {size} characters of text, more than the {MAX_ASK_CHARS} a question sends at most: ask about the open file instead."
            ));
        }
        if files.is_empty() {
            return Err("The project has no text file to ask about.".to_owned());
        }
        Ok(Self { question: question.to_owned(), scope, files })
    }

    fn instructions(&self) -> String {
        format!("{ASK_INSTRUCTIONS}\n\nThe question: {}", self.question)
    }

    /// The whole text sent to the model.
    pub fn prompt(&self) -> String {
        let mut out = format!("{}\n", self.instructions());
        for (path, text) in &self.files {
            out.push_str(&format!("\n<<<FILE {path}\n{}\nFILE>>>\n", numbered(text)));
        }
        out
    }

    pub(super) fn request(&self) -> Result<Request> {
        let capability = CapabilityId::parse(ASK).map_err(|e| e.to_string())?;
        let mut r = Request::new(capability).with_input(InputRole::Prompt, Payload::text(self.instructions()));
        for (_, text) in &self.files {
            r = r.with_input(InputRole::Context, Payload::text(numbered(text)));
        }
        Ok(r)
    }

    pub(super) fn describe(&self) -> Value {
        json!({
            "question": self.question,
            "scope": self.scope.id(),
            "files": self.files.iter().map(|(p, _)| p).collect::<Vec<_>>(),
            "prompt": self.prompt(),
        })
    }
}

/// A place an answer names: characters `start..end` of the answer (from 0) say `path:line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub path: String,
    /// From 1.
    pub line: u32,
}

/// The `path:line` places in `answer` whose path is one of `paths` and whose line it has
/// (`lines` gives a file's line count).
pub fn links(answer: &str, lines: impl Fn(&str) -> Option<usize>) -> Vec<Link> {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let Some(re) = RE.get_or_init(|| regex::Regex::new(r"([A-Za-z0-9_][A-Za-z0-9_./-]*\.[A-Za-z0-9]+):(\d+)").ok()).as_ref() else {
        return Vec::new();
    };
    re.captures_iter(answer)
        .filter_map(|c| {
            let whole = c.get(0)?;
            let path = c.get(1)?.as_str().trim_start_matches("./");
            let line: u32 = c.get(2)?.as_str().parse().ok()?;
            let count = lines(path)?;
            (line >= 1 && line as usize <= count.max(1)).then(|| Link {
                start: answer[..whole.start()].chars().count(),
                end: answer[..whole.end()].chars().count(),
                path: path.to_owned(),
                line,
            })
        })
        .collect()
}

/// An answer, or the question being answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    Running { question: String },
    Done { question: String, scope: Scope, answer: String, links: Vec<Link>, model: String },
    Failed { question: String, error: String },
    Cancelled { question: String },
}

const REVIEW_INSTRUCTIONS: &str = "You review a section of a LaTeX document as a careful colleague would: \
point out unclear sentences, mistakes in reasoning, grammar or LaTeX, and what needs explaining. \
For each remark, copy the exact words it is about, as written in the text (a short passage, at most one sentence), and write the remark, as\n\
QUOTE: the exact words\nREMARK: the remark\n\
with a blank line between remarks. Give at most 12 remarks; when there is nothing to remark, answer NONE.";

/// What reviewing sends: the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub path: String,
    /// In characters from 0.
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Section {
    pub fn of(project: &Project, path: &str, start: usize, end: usize) -> Result<Self> {
        let source = project.text(&ProjectPath::new(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if end <= start {
            return Err("There is no text to review there.".to_owned());
        }
        if end - start > MAX_REVIEW_CHARS {
            return Err(format!("The section is longer than the {MAX_REVIEW_CHARS} characters reviewed at once: review a shorter one."));
        }
        let (s, e) = bytes_of(source, start, end).ok_or_else(|| format!("{path} has fewer than {end} characters."))?;
        let text = source.get(s..e).unwrap_or_default().to_owned();
        if text.trim().is_empty() {
            return Err("There is no text to review there.".to_owned());
        }
        Ok(Self { path: path.to_owned(), start, end, text })
    }

    /// The whole text sent to the model.
    pub fn prompt(&self) -> String {
        format!("{REVIEW_INSTRUCTIONS}\n\n<<<SECTION\n{}\nSECTION>>>\n", self.text)
    }

    pub(super) fn request(&self) -> Result<Request> {
        let capability = CapabilityId::parse(REVIEW).map_err(|e| e.to_string())?;
        Ok(Request::new(capability)
            .with_input(InputRole::Prompt, Payload::text(REVIEW_INSTRUCTIONS))
            .with_input(InputRole::Source, Payload::text(self.text.clone())))
    }

    pub(super) fn describe(&self) -> Value {
        json!({"path": self.path, "start": self.start, "end": self.end, "text": self.text, "prompt": self.prompt()})
    }
}

/// The section of `text` around character `at`: from the last heading at or before it to the
/// next heading of the same level or above, or the end of the document's body; the whole body
/// when there is no heading before it. In characters from 0.
pub fn section_at(text: &str, at: usize) -> (usize, usize) {
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let char_of = |byte: usize| text[..byte.min(text.len())].chars().count();
    let line_start = |line: u32| char_of(starts.get(line as usize).copied().unwrap_or(text.len()));
    let body_start = text.find("\\begin{document}").map(|b| text[b..].find('\n').map_or(text.len(), |n| b + n + 1));
    let body_end = text.rfind("\\end{document}").unwrap_or(text.len());
    let headings = texcraft_latex::outline(text);
    let current = headings.iter().rposition(|h| line_start(h.line) <= at);
    let (start, level) = match current.and_then(|i| headings.get(i)) {
        Some(h) => (line_start(h.line), Some(h.level)),
        None => (char_of(body_start.unwrap_or(0)), None),
    };
    let next = headings.iter().map(|h| (line_start(h.line), h.level)).find(|(s, l)| *s > start && level.is_none_or(|level| *l <= level));
    let end = next.map_or(char_of(body_end), |(s, _)| s).max(start);
    (start, end)
}

/// A remark of a review: the words it is about and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remark {
    pub quote: String,
    pub remark: String,
}

/// The remarks in a model's answer (`QUOTE:` and `REMARK:` lines; a line that follows one goes
/// on with it).
pub fn remarks(answer: &str) -> Vec<Remark> {
    let mut out: Vec<Remark> = Vec::new();
    let mut field: Option<bool> = None;
    for line in answer.lines() {
        let t = line.trim().trim_start_matches(['-', '*', ' ']);
        if let Some(q) = strip_label(t, "QUOTE") {
            out.push(Remark { quote: q.to_owned(), remark: String::new() });
            field = Some(true);
        } else if let Some(r) = strip_label(t, "REMARK") {
            if let Some(last) = out.last_mut().filter(|l| l.remark.is_empty()) {
                last.remark = r.to_owned();
                field = Some(false);
            }
        } else if t.is_empty() {
            field = None;
        } else if let (Some(quote), Some(last)) = (field, out.last_mut()) {
            let part = if quote { &mut last.quote } else { &mut last.remark };
            part.push(' ');
            part.push_str(t);
        }
    }
    out.retain(|r| !r.quote.trim().is_empty() && !r.remark.trim().is_empty());
    for r in &mut out {
        r.quote = unquote(&r.quote);
    }
    out
}

/// What follows `label:` on a line (`label` and its colon perhaps in bold).
fn strip_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    Some(line.strip_prefix(label)?.trim_start_matches('*').strip_prefix(':')?.trim_start_matches('*').trim())
}

/// A quote without the quotation marks around it.
fn unquote(q: &str) -> String {
    let q = q.trim();
    for (open, close) in [('"', '"'), ('“', '”'), ('«', '»'), ('`', '`'), ('\'', '\'')] {
        if let Some(inner) = q.strip_prefix(open).and_then(|r| r.strip_suffix(close)) {
            return inner.trim().to_owned();
        }
    }
    q.to_owned()
}

/// Where `quote` is in `text`, in characters from 0: the first place at or after character
/// `from`, else the first; spaces and line breaks match any run of them.
pub fn find_quote(text: &str, quote: &str, from: usize) -> Option<(usize, usize)> {
    let words: Vec<&str> = quote.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let pattern = words.iter().map(|w| regex::escape(w)).collect::<Vec<_>>().join(r"\s+");
    let re = regex::Regex::new(&pattern).ok()?;
    let from_byte = text.char_indices().nth(from).map_or(text.len(), |(b, _)| b);
    let m = re.find_at(text, from_byte).or_else(|| re.find(text))?;
    Some((text[..m.start()].chars().count(), text[..m.end()].chars().count()))
}

/// The last review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reviewed {
    Running,
    Done {
        /// The comments it left.
        comments: Vec<String>,
        /// Remarks whose quote was not found in the text.
        dropped: usize,
        model: String,
    },
    Failed(String),
    Cancelled,
}

impl Session {
    /// What `ai.ask`'s (or `ai.disclose`'s) parameters ask.
    pub(super) fn ai_question(&self, p: &Value) -> Result<Question> {
        let scope = Scope::parse(p.get("scope").and_then(Value::as_str).unwrap_or("file"))?;
        let path = p.get("path").and_then(Value::as_str).map(str::to_owned).or_else(|| self.current().map(|c| c.as_str().to_owned()));
        Question::of(&self.project, p.get("question").and_then(Value::as_str).unwrap_or_default(), scope, path.as_deref())
    }

    /// What `ai.review`'s (or `ai.disclose`'s) parameters name.
    pub(super) fn ai_section(&self, p: &Value) -> Result<Section> {
        let path = p.get("path").and_then(Value::as_str).ok_or("Name the file: `path`.")?;
        let place = |name: &str| {
            p.get(name)
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("`{name}` is a character position, from 0."))
        };
        Section::of(&self.project, path, place("start")?, place("end")?)
    }

    /// The answers so far, the oldest first.
    pub fn ai_answers(&self) -> &[Answered] {
        &self.ai.answers
    }

    /// The last review, if one was asked for.
    pub fn ai_reviewed(&self) -> Option<&Reviewed> {
        self.ai.reviewed.as_ref()
    }

    /// `ai.ask`.
    pub(crate) fn ai_ask(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let question = self.ai_question(p)?;
        let opts = craft_ai::GenOpts { max_tokens: 1500, temperature: 0.2 };
        let project = question.scope == Scope::Project;
        let (model, run, answer) = self.begin(&question.request()?, ("ask", "Ask"), question.prompt(), opts, Answer::Text, |to| {
            format!(
                "Asking sends the question and {} to {to}. Agree to it first (the window asks; agents: ai.disclose with request ask shows it, ai.consent agrees).",
                if project { "every text file of the project" } else { "the open file" }
            )
        })?;
        self.push_answer(Answered::Running { question: question.question.clone() });
        self.ai.task = Some(Task { work: Work::Ask { question: Box::new(question), model, run }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        match self.ai.answers.last().cloned() {
            Some(Answered::Done { answer, links, model, .. }) => Ok(json!({
                "answer": answer,
                "model": model,
                "links": links.iter().map(|l| json!({"start": l.start, "end": l.end, "path": l.path, "line": l.line})).collect::<Vec<_>>(),
            })),
            Some(Answered::Failed { error, .. }) => Err(error),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    fn push_answer(&mut self, a: Answered) {
        self.ai.answers.push(a);
        let over = self.ai.answers.len().saturating_sub(MAX_ANSWERS);
        self.ai.answers.drain(..over);
    }

    /// The question in flight becomes `a`.
    pub(super) fn set_last_answer(&mut self, a: Answered) {
        match self.ai.answers.last_mut() {
            Some(last @ Answered::Running { .. }) => *last = a,
            _ => self.push_answer(a),
        }
    }

    pub(super) fn finish_ask(&mut self, q: &Question, model: &str, run: String, answer: std::result::Result<Answer, String>) -> Event {
        match answer {
            Ok(Answer::Text(text)) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                let text = text.trim().to_owned();
                let project = &self.project;
                let found = links(&text, |path| ProjectPath::new(path).ok().and_then(|p| project.text(&p).ok()).map(|t| t.lines().count()));
                let n = found.len();
                self.set_last_answer(Answered::Done {
                    question: q.question.clone(),
                    scope: q.scope,
                    answer: text,
                    links: found,
                    model: model.to_owned(),
                });
                Event::Answered(Ok(n))
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                self.set_last_answer(Answered::Failed { question: q.question.clone(), error: e.clone() });
                Event::Answered(Err(e))
            }
        }
    }

    /// `ai.review`: asks for remarks on a section; each becomes a comment on the words it quotes.
    pub(crate) fn ai_review(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let section = self.ai_section(p)?;
        let opts = craft_ai::GenOpts { max_tokens: 2000, temperature: 0.2 };
        let (model, run, answer) = self.begin(&section.request()?, ("review", "Review This Section"), section.prompt(), opts, Answer::Text, |to| {
            format!(
                "Reviewing sends the section to {to}. Agree to it first (the window asks; agents: ai.disclose with request review shows it, ai.consent agrees)."
            )
        })?;
        self.ai.reviewed = Some(Reviewed::Running);
        self.ai.task = Some(Task { work: Work::Review { section: Box::new(section), model, run }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        match self.ai.reviewed.clone() {
            Some(Reviewed::Done { comments, dropped, model }) => Ok(json!({"comments": comments, "dropped": dropped, "model": model})),
            Some(Reviewed::Failed(e)) => Err(e),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    pub(super) fn finish_review(&mut self, section: &Section, model: &str, run: String, answer: std::result::Result<Answer, String>) -> Event {
        let outcome = match answer {
            Ok(Answer::Text(text)) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                self.place_remarks(section, &text, model)
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                Err(e)
            }
        };
        self.ai.reviewed = Some(match &outcome {
            Ok((comments, dropped)) => Reviewed::Done { comments: comments.clone(), dropped: *dropped, model: model.to_owned() },
            Err(e) => Reviewed::Failed(e.clone()),
        });
        Event::Reviewed(outcome.map(|(c, d)| (c.len(), d)))
    }

    /// Each remark a comment on its quote; the ids, and how many were dropped.
    fn place_remarks(&mut self, section: &Section, answer: &str, model: &str) -> Result<(Vec<String>, usize)> {
        let path = ProjectPath::new(&section.path).map_err(|e| e.to_string())?;
        let all = remarks(answer);
        let mut dropped = all.len().saturating_sub(MAX_REMARKS);
        let author = texcraft_review::Author::new(format!("AI ({model})"), "");
        let mut ids = Vec::new();
        for r in all.into_iter().take(MAX_REMARKS) {
            let source = self.project.text(&path).map_err(|e| e.to_string())?.to_owned();
            let Some((start, end)) = find_quote(&source, &r.quote, section.start) else {
                dropped += 1;
                continue;
            };
            let anchor = texcraft_review::Anchor::new(path.as_str(), &source, start, end);
            let (id, message) = (self.new_id()?, self.new_id()?);
            let now = self.now();
            let first = texcraft_review::Message::new(message, author.clone(), now, &r.remark);
            self.review.review.comment(id, anchor, first).map_err(|e| e.to_string())?;
            self.review_changed(id)?;
            ids.push(id.to_string());
        }
        Ok((ids, dropped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remarks_are_read_with_their_continuation_lines_and_without_quotation_marks() {
        let answer = "Here are my remarks.\n\nQUOTE: \"the value is very\nlarge\"\nREMARK: Say how large.\n\n- **QUOTE:** Line two.\n- **REMARK:** Too short.\n\nQUOTE: orphan\n";
        let r = remarks(answer);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(r[0].quote, "the value is very large");
        assert_eq!(r[1].remark, "Too short.");
    }

    #[test]
    fn a_quote_matches_across_line_breaks_and_prefers_the_place_after_the_section_start() {
        let text = "one two\nthree. Later: one two three.";
        assert_eq!(find_quote(text, "one two three", 0), Some((0, 13)));
        assert_eq!(find_quote(text, "one two three", 5).map(|(s, _)| s), Some(22));
        assert_eq!(find_quote(text, "four", 0), None);
    }

    #[test]
    fn a_section_runs_to_the_next_heading_of_its_level_or_above() {
        let text = "\\begin{document}\n\\section{A}\na\n\\subsection{B}\nb\n\\section{C}\nc\n\\end{document}\n";
        let in_b = text.find("b\n").map(|b| text[..b].chars().count()).unwrap_or_default();
        let (s, e) = section_at(text, in_b);
        assert_eq!(&text[s..e], "\\subsection{B}\nb\n");
        let in_a = text.find("a\n").unwrap_or_default();
        let (s, e) = section_at(text, in_a);
        assert_eq!(&text[s..e], "\\section{A}\na\n\\subsection{B}\nb\n");
        let (s, e) = section_at(text, text.find("c\n").unwrap_or_default());
        assert_eq!(&text[s..e], "\\section{C}\nc\n");
    }

    #[test]
    fn links_are_only_to_files_and_lines_there_are() {
        let answer = "See main.tex:3 and `refs.bib:2`; not other.tex:1 nor main.tex:99.";
        let l = links(answer, |p| match p {
            "main.tex" => Some(10),
            "refs.bib" => Some(5),
            _ => None,
        });
        assert_eq!(l.iter().map(|l| (l.path.as_str(), l.line)).collect::<Vec<_>>(), vec![("main.tex", 3), ("refs.bib", 2)]);
        assert_eq!(answer.chars().skip(l[0].start).take(l[0].end - l[0].start).collect::<String>(), "main.tex:3");
    }
}
