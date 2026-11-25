//! Grammar checked by the connected model (docs/ROADMAP.md, E12), for any language and for what
//! Harper does not see: a passage (the selection, or the paragraph at the caret) is sent, and each
//! problem the model reports quotes the words it is about. A problem becomes a grammar problem on
//! the first place those words are in the passage, with the model's replacement when it keeps the
//! LaTeX of the words it replaces; a problem whose quote is not in the passage is dropped, and
//! counted.

use craft_ai_core::history::{EndRecord, HISTORY_SCHEMA, Outcome as Logged};
use craft_ai_core::{HistoryRecord, InputRole, Request};
use serde_json::{Value, json};
use texcraft_project::{Project, ProjectPath};

use super::write::bytes_of;
use super::{Answer, Event, GRAMMAR, Task, Work, kind_of, now_ms, prompts};
use crate::grammar::AiIssue;
use crate::{Result, Session};

/// The longest passage checked at once, in characters.
pub const MAX_GRAMMAR_CHARS: usize = 12_000;
/// Problems kept from one answer, at most.
const MAX_ISSUES: usize = 40;

/// What checking grammar with AI sends: the passage, and the document's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarPassage {
    pub path: String,
    /// In characters from 0.
    pub start: usize,
    pub end: usize,
    pub text: String,
    /// The document's language, as a tag (`en-GB`, `fr`).
    pub language: String,
    /// Where the text starts, in bytes of the file.
    byte: usize,
}

impl GrammarPassage {
    pub fn of(project: &Project, path: &str, start: usize, end: usize, language: &str) -> Result<Self> {
        let source = project.text(&ProjectPath::new(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if end <= start {
            return Err("There is no text to check there.".to_owned());
        }
        if end - start > MAX_GRAMMAR_CHARS {
            return Err(format!("The passage is longer than the {MAX_GRAMMAR_CHARS} characters checked at once: select a shorter one."));
        }
        let (s, e) = bytes_of(source, start, end).ok_or_else(|| format!("{path} has fewer than {end} characters."))?;
        let text = source.get(s..e).unwrap_or_default().to_owned();
        if text.trim().is_empty() {
            return Err("There is no text to check there.".to_owned());
        }
        Ok(Self { path: path.to_owned(), start, end, text, language: language.to_owned(), byte: s })
    }

    fn parts(&self) -> prompts::Parts {
        vec![("passage", self.text.clone(), Some(InputRole::Source)), ("language", self.language.clone(), None)]
    }

    /// The whole text sent to the model, with `template` (the prompt for grammar).
    pub fn prompt(&self, template: &str) -> String {
        prompts::prompt(template, &self.parts())
    }

    pub(super) fn request(&self, template: &str) -> Result<Request> {
        prompts::request(GRAMMAR, template, &self.parts())
    }

    pub(super) fn describe(&self, template: &str) -> Value {
        json!({"path": self.path, "start": self.start, "end": self.end, "text": self.text, "language": self.language, "prompt": self.prompt(template)})
    }
}

/// A problem the model reports: the words it is about, what is wrong, and what to write instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub quote: String,
    pub problem: String,
    pub replacement: Option<String>,
}

/// The problems in a model's answer: `QUOTE:`, `PROBLEM:` and `REPLACEMENT:` lines, a blank line
/// between problems (a line that follows one goes on with it).
pub fn issues(answer: &str) -> Vec<Issue> {
    let mut out: Vec<Issue> = Vec::new();
    // Which field the last line was: 0 quote, 1 problem, 2 replacement.
    let mut field: Option<u8> = None;
    for line in answer.lines() {
        let t = line.trim().trim_start_matches(['-', '*', ' ']);
        if let Some(q) = label(t, "QUOTE") {
            out.push(Issue { quote: q.to_owned(), problem: String::new(), replacement: None });
            field = Some(0);
        } else if let (Some(p), Some(last)) = (label(t, "PROBLEM"), out.last_mut()) {
            last.problem = p.to_owned();
            field = Some(1);
        } else if let (Some(r), Some(last)) = (label(t, "REPLACEMENT"), out.last_mut()) {
            last.replacement = Some(r.to_owned());
            field = Some(2);
        } else if t.is_empty() {
            field = None;
        } else if let (Some(f), Some(last)) = (field, out.last_mut()) {
            let part = match f {
                0 => &mut last.quote,
                1 => &mut last.problem,
                _ => last.replacement.get_or_insert_with(String::new),
            };
            part.push(' ');
            part.push_str(t);
        }
    }
    out.retain(|i| !i.quote.trim().is_empty() && !i.problem.trim().is_empty());
    for i in &mut out {
        i.quote = unquote(&i.quote);
        i.replacement = i.replacement.take().map(|r| unquote(&r)).filter(|r| !r.is_empty() && *r != i.quote && r != "NONE");
    }
    out
}

/// What follows `name:` on a line (perhaps in bold).
fn label<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    Some(line.strip_prefix(name)?.trim_start_matches('*').strip_prefix(':')?.trim_start_matches('*').trim())
}

/// A quote without the quotation marks around it.
fn unquote(q: &str) -> String {
    let q = q.trim();
    for (open, close) in [('"', '"'), ('“', '”'), ('«', '»'), ('`', '`')] {
        if let Some(inner) = q.strip_prefix(open).and_then(|r| r.strip_suffix(close)) {
            return inner.trim().to_owned();
        }
    }
    q.to_owned()
}

/// The last grammar check with AI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Running,
    Done {
        /// Problems placed in the text.
        found: usize,
        /// Problems whose quote is not in the passage.
        dropped: usize,
        model: String,
    },
    Failed(String),
    Cancelled,
}

impl Session {
    /// What `ai.grammar`'s (or `ai.disclose`'s) parameters name.
    pub(super) fn ai_grammar_passage(&self, p: &Value) -> Result<GrammarPassage> {
        let path = p.get("path").and_then(Value::as_str).ok_or("Name the file: `path`.")?;
        let place = |name: &str| {
            p.get(name)
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("`{name}` is a character position, from 0."))
        };
        let (language, _) = self.spelling_language();
        GrammarPassage::of(&self.project, path, place("start")?, place("end")?, &language)
    }

    /// The last grammar check with AI, if one was asked for.
    pub fn ai_grammar_checked(&self) -> Option<&Checked> {
        self.ai.grammar.as_ref()
    }

    /// `ai.grammar`: asks the model for the grammar problems of a passage; each becomes a grammar
    /// problem on the words it quotes.
    pub(crate) fn ai_grammar(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let passage = self.ai_grammar_passage(p)?;
        let opts = craft_ai::GenOpts { max_tokens: 2000, temperature: 0.1 };
        let (template, note) = self.ai_template("grammar");
        let (model, run, answer) =
            self.begin(&passage.request(&template)?, ("grammar", "Check Grammar with AI"), passage.prompt(&template), opts, Answer::Text)?;
        self.ai.prompt_note = note;
        self.ai.grammar = Some(Checked::Running);
        self.ai.task = Some(Task { work: Work::Grammar { passage: Box::new(passage), model, run }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        match self.ai.grammar.clone() {
            Some(Checked::Done { found, dropped, model }) => Ok(json!({"found": found, "dropped": dropped, "model": model})),
            Some(Checked::Failed(e)) => Err(e),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    pub(super) fn finish_grammar(
        &mut self,
        passage: &GrammarPassage,
        model: &str,
        run: String,
        answer: std::result::Result<Answer, String>,
    ) -> Event {
        let outcome = match answer {
            Ok(Answer::Text(text)) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                self.place_issues(passage, &text, model)
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                Err(e)
            }
        };
        self.ai.grammar = Some(match &outcome {
            Ok((found, dropped)) => Checked::Done { found: *found, dropped: *dropped, model: model.to_owned() },
            Err(e) => Checked::Failed(e.clone()),
        });
        Event::Grammar(outcome)
    }

    /// Each problem on the first place its quote is in the passage, if the passage is still as it
    /// was sent; how many were placed and how many dropped.
    fn place_issues(&mut self, passage: &GrammarPassage, answer: &str, model: &str) -> Result<(usize, usize)> {
        let path = ProjectPath::new(&passage.path).map_err(|e| e.to_string())?;
        let source = self.project.text(&path).map_err(|e| e.to_string())?;
        let end = passage.byte + passage.text.len();
        let at = if source.get(passage.byte..end) == Some(passage.text.as_str()) {
            passage.byte
        } else {
            source.find(&passage.text).ok_or("The passage changed while the model read it: check it again.")?
        };
        let all = issues(answer);
        let mut dropped = all.len().saturating_sub(MAX_ISSUES);
        let mut placed = Vec::new();
        for i in all.into_iter().take(MAX_ISSUES) {
            let Some(found) = passage.text.find(&i.quote) else {
                dropped += 1;
                continue;
            };
            let range = at + found..at + found + i.quote.len();
            // A replacement that would lose the quote's labels, references, citations, formulas or
            // commands is not offered: the problem is still shown.
            let replacement = i.replacement.filter(|r| texcraft_latex::keep::compare(&i.quote, r).is_empty());
            placed.push(AiIssue { range, quote: i.quote, message: i.problem, replacement, model: model.to_owned() });
        }
        let n = placed.len();
        self.set_ai_grammar(&path, at..at + passage.text.len(), placed);
        Ok((n, dropped))
    }
}
