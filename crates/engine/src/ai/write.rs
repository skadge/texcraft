//! AI3 (docs/ROADMAP.md): rewriting a selection and continuing a paragraph.
//!
//! A rewrite sends the selection and a few lines around it; its answer becomes a suggested change
//! on exactly the selection, by "AI (<model>)", and only when it kept the selection's labels,
//! references, citations, formulas and commands ([`texcraft_latex::keep`]) and the selection is
//! still as it was sent. A continuation sends the paragraph before the caret, at most once every
//! [`COMPLETE_EVERY_MS`]; its answer is kept as a [`Ghost`] while the text before the caret is
//! still the text sent, for the editor to show after the caret.

use craft_ai_core::history::{EndRecord, HISTORY_SCHEMA, Outcome as Logged};
use craft_ai_core::{HistoryRecord, InputRole, Request};
use serde_json::{Value, json};
use texcraft_project::{Project, ProjectPath};

use super::{Answer, COMPLETE, Event, INSTRUCT, REWRITE, Task, Work, kind_of, now_ms, prompts};
use crate::{Result, Session};

/// Characters of text sent on each side of a selection to rewrite, at most (whole lines).
pub const REWRITE_CONTEXT_CHARS: usize = 300;
/// The longest selection rewritten, in characters.
pub const MAX_REWRITE_CHARS: usize = 6000;
/// The most of a paragraph sent to continue it, in characters (its end, up to the caret).
pub const MAX_PARAGRAPH_CHARS: usize = 1500;
/// The longest continuation shown, in characters.
const MAX_CONTINUATION_CHARS: usize = 160;
/// A continuation is asked for at most this often, in milliseconds.
pub const COMPLETE_EVERY_MS: u64 = 2000;

/// How a selection is rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    Clearer,
    Shorter,
    Formal,
    Grammar,
    /// Into this language, named in English.
    Translate(String),
    /// As the user asked, in their words ("flesh out these bullet points"): its own kind of
    /// request, and it may add text and LaTeX.
    Instruct(String),
}

impl How {
    /// From `ai.rewrite`'s `how` and `language`.
    pub fn parse(how: &str, language: Option<&str>) -> Result<Self> {
        Ok(match how {
            "clearer" => Self::Clearer,
            "shorter" => Self::Shorter,
            "formal" => Self::Formal,
            "grammar" => Self::Grammar,
            "translate" => {
                let l = language.map(str::trim).unwrap_or_default();
                let fits = l.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '(' | ')'));
                if l.is_empty() || l.chars().count() > 40 || !fits {
                    return Err("Name the `language` to translate into, in words (such as French or Brazilian Portuguese).".to_owned());
                }
                Self::Translate(l.to_owned())
            }
            _ => return Err(format!("There is no rewrite `{how}`: there are clearer, shorter, formal, grammar and translate.")),
        })
    }

    /// The name `ai.rewrite` takes.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Clearer => "clearer",
            Self::Shorter => "shorter",
            Self::Formal => "formal",
            Self::Grammar => "grammar",
            Self::Translate(_) => "translate",
            Self::Instruct(_) => "instruct",
        }
    }

    /// An instruction, checked: one line of at most [`MAX_INSTRUCTION_CHARS`].
    pub fn instruct(instruction: &str) -> Result<Self> {
        let i = instruction.trim();
        if i.is_empty() {
            return Err("Say how to change the selection: `instruction`, such as \"flesh out these bullet points\".".to_owned());
        }
        if i.chars().count() > MAX_INSTRUCTION_CHARS || i.chars().any(char::is_control) {
            return Err(format!("Say how to change it in one line of at most {MAX_INSTRUCTION_CHARS} characters."));
        }
        Ok(Self::Instruct(i.to_owned()))
    }

    /// The kind of request it is, for consent.
    fn capability(&self) -> &'static str {
        if matches!(self, Self::Instruct(_)) { INSTRUCT } else { REWRITE }
    }

    /// Its prompt's name ([`prompts`]).
    pub fn prompt_kind(&self) -> &'static str {
        if matches!(self, Self::Instruct(_)) { "instruct" } else { "rewrite" }
    }

    fn instruction(&self) -> String {
        match self {
            Self::Clearer => "Make it clearer and easier to read, keeping its meaning.".to_owned(),
            Self::Shorter => "Make it shorter, keeping its meaning.".to_owned(),
            Self::Formal => "Make it more formal, as in a scholarly text.".to_owned(),
            Self::Grammar => "Correct its grammar and spelling only, changing as little as possible.".to_owned(),
            Self::Translate(l) => format!("Translate it into {l}."),
            Self::Instruct(i) => i.clone(),
        }
    }
}

/// The longest instruction, in characters.
pub const MAX_INSTRUCTION_CHARS: usize = 500;

/// What a rewrite sends: the selection and a little text on each side, in [`Passage::prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub path: String,
    /// The selection, in characters from 0.
    pub start: usize,
    pub end: usize,
    pub how: How,
    pub before: String,
    pub text: String,
    pub after: String,
}

/// Characters `start..end` of `text`, as bytes; `None` past its end.
pub(super) fn bytes_of(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let mut at = text.char_indices().map(|(b, _)| b).chain(std::iter::once(text.len()));
    let s = at.nth(start)?;
    let e = if end == start { s } else { at.nth(end - start - 1)? };
    Some((s, e))
}

impl Passage {
    /// What rewriting characters `start..end` of `path` sends.
    pub fn of(project: &Project, path: &str, start: usize, end: usize, how: How) -> Result<Self> {
        let p = ProjectPath::new(path).map_err(|e| e.to_string())?;
        let source = project.text(&p).map_err(|e| e.to_string())?;
        if end <= start {
            return Err("Select the text to rewrite first.".to_owned());
        }
        if end - start > MAX_REWRITE_CHARS {
            return Err(format!("Select at most {MAX_REWRITE_CHARS} characters to rewrite at once."));
        }
        let (s, e) = bytes_of(source, start, end).ok_or_else(|| format!("{path} has fewer than {end} characters."))?;
        let text = source.get(s..e).unwrap_or_default().to_owned();
        if text.trim().is_empty() {
            return Err("Select the text to rewrite first.".to_owned());
        }
        let before = source.get(..s).unwrap_or_default();
        let from = before.char_indices().rev().nth(REWRITE_CONTEXT_CHARS - 1).map_or(0, |(b, _)| b);
        // Whole lines, unless the text starts within reach.
        let from = if from == 0 { 0 } else { before[from..].find('\n').map_or(from, |n| from + n + 1) };
        let after = source.get(e..).unwrap_or_default();
        let to = after.char_indices().nth(REWRITE_CONTEXT_CHARS).map_or(after.len(), |(b, _)| b);
        let to = if to == after.len() { to } else { after[..to].rfind('\n').unwrap_or(to) };
        Ok(Self { path: path.to_owned(), start, end, how, before: before[from..].to_owned(), text, after: after[..to].to_owned() })
    }

    fn parts(&self) -> prompts::Parts {
        let block = |text: &str, said: &str, name: &str| {
            if text.trim().is_empty() { String::new() } else { format!("{said}:\n<<<{name}\n{text}\n{name}>>>") }
        };
        let mut parts: prompts::Parts = vec![
            ("passage", self.text.clone(), Some(InputRole::Source)),
            ("before", block(&self.before, "The text just before the passage, for context only", "BEFORE"), Some(InputRole::Context)),
            ("after", block(&self.after, "The text just after it, for context only", "AFTER"), Some(InputRole::Context)),
        ];
        match &self.how {
            How::Instruct(i) => parts.push(("instruction", i.clone(), None)),
            how => {
                parts.push(("how", how.instruction(), None));
                parts.push(("language", if let How::Translate(l) = how { l.clone() } else { String::new() }, None));
            }
        }
        parts
    }

    /// The whole text sent to the model, with `template` (its kind's prompt).
    pub fn prompt(&self, template: &str) -> String {
        prompts::prompt(template, &self.parts())
    }

    pub(super) fn request(&self, template: &str) -> Result<Request> {
        prompts::request(self.how.capability(), template, &self.parts())
    }

    pub(super) fn describe(&self, template: &str) -> Value {
        json!({
            "path": self.path,
            "start": self.start,
            "end": self.end,
            "how": self.how.id(),
            "language": match &self.how { How::Translate(l) => Some(l), _ => None },
            "instruction": match &self.how { How::Instruct(i) => Some(i), _ => None },
            "before": self.before,
            "text": self.text,
            "after": self.after,
            "prompt": self.prompt(template),
        })
    }
}

/// The text between a line `<<<NAME` and a line `NAME>>>` (the last such block), a code fence
/// around it dropped.
pub(super) fn between(text: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let open = lines.iter().rposition(|l| l.trim() == format!("<<<{name}"))?;
    let close = open + 1 + lines.iter().skip(open + 1).position(|l| l.trim() == format!("{name}>>>"))?;
    Some(unfenced(&lines.get(open + 1..close).unwrap_or_default().join("\n")))
}

pub(super) fn unfenced(text: &str) -> String {
    let lines: Vec<&str> = text.trim_matches('\n').lines().collect();
    if lines.len() >= 2 && lines.first().is_some_and(|l| l.trim_start().starts_with("```")) && lines.last().is_some_and(|l| l.trim() == "```") {
        return lines.get(1..lines.len() - 1).unwrap_or_default().join("\n");
    }
    text.to_owned()
}

/// The rewritten passage in a model's answer, with the selection's own leading and trailing
/// spaces and line breaks.
pub fn rewritten(answer: &str, original: &str) -> String {
    let body = between(answer, "TEXT").unwrap_or_else(|| unfenced(answer.trim()));
    let lead = &original[..original.len() - original.trim_start().len()];
    let trail = &original[original.trim_end().len()..];
    format!("{lead}{}{trail}", body.trim())
}

/// The rewrite of `passage` refused, with what it changed besides words; `None` when it kept
/// the passage's LaTeX. A change asked for in the user's words may add text and LaTeX, and change
/// markup, but not drop or alter a label, reference, citation or formula.
fn refusal(how: &How, original: &str, new: &str) -> Option<String> {
    let instructed = matches!(how, How::Instruct(_));
    let changes = if instructed { texcraft_latex::keep::dropped(original, new) } else { texcraft_latex::keep::compare(original, new) };
    if changes.is_empty() {
        return None;
    }
    let mut said: Vec<String> = changes.iter().take(4).map(ToString::to_string).collect();
    if changes.len() > 4 {
        said.push(format!("{} more", changes.len() - 4));
    }
    Some(if instructed {
        format!("The change is not offered: it {}. Ask again, or select less.", said.join(", "))
    } else {
        format!("The rewrite is not offered: it {}. Rewrite a smaller passage, or try again.", said.join(", "))
    })
}

/// The last rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rewritten {
    Running,
    /// It is the suggested change `suggestion`.
    Done {
        suggestion: String,
        model: String,
    },
    Failed(String),
    Cancelled,
}

/// What a continuation sends: the paragraph up to the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    pub path: String,
    /// The caret, in characters from 0, and in bytes.
    pub at: usize,
    byte_at: usize,
    /// The paragraph's text up to the caret.
    pub text: String,
    /// The rest of the caret's line (not sent): typing at the caret changes it.
    rest: String,
}

impl Paragraph {
    /// What continuing the paragraph at character `at` of `path` sends.
    pub fn of(project: &Project, path: &str, at: usize) -> Result<Self> {
        let p = ProjectPath::new(path).map_err(|e| e.to_string())?;
        let source = project.text(&p).map_err(|e| e.to_string())?;
        let (byte_at, _) = bytes_of(source, at, at).ok_or_else(|| format!("{path} has fewer than {at} characters."))?;
        let before = &source[..byte_at];
        if source.find("\\begin{document}").is_some_and(|d| byte_at <= d) {
            return Err("The caret is in the preamble: nothing is suggested there.".to_owned());
        }
        // Back to a blank line or the start of the document's body.
        let mut start = 0;
        let mut line_end = before.len();
        while let Some(n) = before[..line_end].rfind('\n') {
            let line = &before[n + 1..line_end];
            if line_end != before.len() && (line.trim().is_empty() || line.trim_start().starts_with("\\begin{document}")) {
                start = line_end + 1;
                break;
            }
            line_end = n;
        }
        let text = before.get(start.min(before.len())..).unwrap_or_default();
        let from = text.char_indices().rev().nth(MAX_PARAGRAPH_CHARS - 1).map_or(0, |(b, _)| b);
        let text = text[from..].to_owned();
        if text.trim().is_empty() {
            return Err("There is nothing before the caret to continue.".to_owned());
        }
        let after = &source[byte_at..];
        let rest = after[..after.find('\n').map_or(after.len(), |n| n + 1)].to_owned();
        Ok(Self { path: path.to_owned(), at, byte_at, text, rest })
    }

    /// An example, for showing what continuations send before there is a paragraph.
    pub fn example() -> Self {
        Self { path: String::new(), at: 0, byte_at: 0, text: "(the paragraph you are typing, up to the caret)".to_owned(), rest: String::new() }
    }

    fn parts(&self) -> prompts::Parts {
        vec![("text", self.text.clone(), Some(InputRole::Source))]
    }

    /// The whole text sent to the model, with `template` (the continuation's prompt).
    pub fn prompt(&self, template: &str) -> String {
        prompts::prompt(template, &self.parts())
    }

    pub(super) fn request(&self, template: &str) -> Result<Request> {
        prompts::request(COMPLETE, template, &self.parts())
    }

    pub(super) fn describe(&self, template: &str) -> Value {
        json!({"path": self.path, "at": self.at, "paragraph": self.text, "prompt": self.prompt(template)})
    }

    /// Whether `source` is still as it was around the caret: the text sent before it, the rest
    /// of its line after it.
    fn still_in(&self, source: &str) -> bool {
        source.get(..self.byte_at).is_some_and(|b| b.ends_with(&self.text)) && source.get(self.byte_at..).is_some_and(|a| a.starts_with(&self.rest))
    }
}

/// The continuation in a model's answer: its first line, at most [`MAX_CONTINUATION_CHARS`]
/// (cut at a space), without a space the text before already ends with; `None` when empty.
pub fn continuation(answer: &str, before: &str) -> Option<String> {
    let body = between(answer, "TEXT").unwrap_or_else(|| unfenced(answer.trim_matches('\n')));
    let line = body.lines().find(|l| !l.trim().is_empty())?.trim_end();
    let line = if before.ends_with(char::is_whitespace) { line.trim_start() } else { line };
    let mut out: String = line.chars().take(MAX_CONTINUATION_CHARS).collect();
    if line.chars().count() > MAX_CONTINUATION_CHARS
        && let Some(cut) = out.rfind(' ')
    {
        out.truncate(cut);
    }
    (!out.trim().is_empty()).then_some(out)
}

/// A continuation shown after the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ghost {
    pub paragraph: Paragraph,
    pub text: String,
}

impl Session {
    /// The passage `ai.rewrite`'s (or `ai.disclose`'s) parameters name.
    pub(super) fn ai_passage(&self, p: &Value) -> Result<Passage> {
        let path = p.get("path").and_then(Value::as_str).ok_or("Name the file: `path`.")?;
        let place = |name: &str| {
            p.get(name)
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("`{name}` is a character position, from 0."))
        };
        let how = match p.get("instruction") {
            Some(i) => How::instruct(i.as_str().ok_or("`instruction` is text: how to change the selection.")?)?,
            None => How::parse(p.get("how").and_then(Value::as_str).unwrap_or_default(), p.get("language").and_then(Value::as_str))?,
        };
        Passage::of(&self.project, path, place("start")?, place("end")?, how)
    }

    /// The paragraph `ai.complete`'s (or `ai.disclose`'s) parameters name.
    pub(super) fn ai_paragraph(&self, p: &Value) -> Result<Paragraph> {
        let path = p.get("path").and_then(Value::as_str).ok_or("Name the file: `path`.")?;
        let at =
            p.get("at").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or("`at` is the caret's position, in characters from 0.")?;
        Paragraph::of(&self.project, path, at)
    }

    /// The last rewrite, if one was asked for.
    pub fn ai_rewritten(&self) -> Option<&Rewritten> {
        self.ai.rewrite.as_ref()
    }

    /// The continuation to show after the caret in `path`: its place (characters from 0) and
    /// text, while the text before it is still what was sent.
    pub fn ai_ghost(&self, path: &ProjectPath) -> Option<(usize, &str)> {
        let g = self.ai.ghost.as_ref().filter(|g| g.paragraph.path == path.as_str())?;
        let source = self.project.text(path).ok()?;
        g.paragraph.still_in(source).then_some((g.paragraph.at, g.text.as_str()))
    }

    /// `ai.rewrite`: asks the model to rewrite a selection; its rewrite becomes a suggested
    /// change on that selection. Refused before anything is sent while its kind is switched off.
    pub(crate) fn ai_rewrite(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        let passage = self.ai_passage(p)?;
        let instructed = matches!(passage.how, How::Instruct(_));
        // A change in the user's words may make the passage much longer.
        let tokens = if instructed { 4000 } else { u32::try_from(passage.text.len() / 2 + 400).unwrap_or(4000).min(4000) };
        let opts = craft_ai::GenOpts { max_tokens: tokens, temperature: 0.3 };
        let role = if instructed { ("instruct", "Change as Asked") } else { ("rewrite", "Rewrite") };
        let (template, note) = self.ai_template(passage.how.prompt_kind());
        let (model, run, answer) = self.begin(&passage.request(&template)?, role, passage.prompt(&template), opts, Answer::Text)?;
        self.ai.prompt_note = note;
        self.ai.rewrite = Some(Rewritten::Running);
        self.ai.task = Some(Task { work: Work::Rewrite { passage: Box::new(passage), model, run }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        match self.ai.rewrite.clone() {
            Some(Rewritten::Done { suggestion, model }) => Ok(json!({"suggestion": suggestion, "model": model})),
            Some(Rewritten::Failed(e)) => Err(e),
            _ => Err("The request was cancelled.".to_owned()),
        }
    }

    /// `ai.complete`: asks the model how the paragraph at the caret goes on. Refused while
    /// switched off or not agreed to, while another request is in flight, and more often than every
    /// [`COMPLETE_EVERY_MS`].
    pub(crate) fn ai_complete(&mut self, p: &Value, background: bool) -> Result<Value> {
        self.ai_ready()?;
        if self.ai.task.is_some() {
            return Err("The model is still answering the last request.".to_owned());
        }
        let paragraph = self.ai_paragraph(p)?;
        // Refused when switched off before the rate limit counts it.
        self.ai_permitted("complete")?;
        if now_ms().saturating_sub(self.ai.last_complete) < COMPLETE_EVERY_MS {
            return Err(format!("The next words are suggested at most once every {} seconds.", COMPLETE_EVERY_MS / 1000));
        }
        self.ai.ghost = None;
        let opts = craft_ai::GenOpts { max_tokens: 60, temperature: 0.3 };
        let (template, note) = self.ai_template("complete");
        let request = paragraph.request(&template)?;
        let (_, run, answer) = self.begin(&request, ("complete", "Suggest the next words"), paragraph.prompt(&template), opts, Answer::Text)?;
        self.ai.prompt_note = note;
        self.ai.last_complete = now_ms();
        self.ai.task = Some(Task { work: Work::Complete { paragraph: Box::new(paragraph), run }, answer, cancelled: Default::default() });
        if background {
            return Ok(json!({"started": true}));
        }
        self.wait_ai()?;
        Ok(json!({"text": self.ai.ghost.as_ref().map(|g| g.text.clone())}))
    }

    pub(super) fn finish_rewrite(&mut self, passage: &Passage, model: &str, run: String, answer: std::result::Result<Answer, String>) -> Event {
        let outcome = match answer {
            Ok(Answer::Text(text)) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                self.offer_rewrite(passage, &text, model)
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                Err(e)
            }
        };
        self.ai.rewrite = Some(match &outcome {
            Ok(id) => Rewritten::Done { suggestion: id.clone(), model: model.to_owned() },
            Err(e) => Rewritten::Failed(e.clone()),
        });
        Event::Rewritten(outcome)
    }

    /// The model's rewrite as a suggested change on the passage, if it kept its LaTeX and the
    /// passage is as it was sent.
    fn offer_rewrite(&mut self, passage: &Passage, answer: &str, model: &str) -> Result<String> {
        let new = rewritten(answer, &passage.text);
        if new.trim().is_empty() {
            return Err("The model gave no rewrite.".to_owned());
        }
        if new == passage.text {
            return Err("The model left the passage as it was.".to_owned());
        }
        if let Some(why) = refusal(&passage.how, &passage.text, &new) {
            return Err(why);
        }
        let path = ProjectPath::new(&passage.path).map_err(|e| e.to_string())?;
        let source = self.project.text(&path).map_err(|e| e.to_string())?.to_owned();
        let now = bytes_of(&source, passage.start, passage.end).and_then(|(s, e)| source.get(s..e));
        if now != Some(passage.text.as_str()) {
            return Err("The passage changed while the model answered, so its rewrite is not offered.".to_owned());
        }
        self.suggest_change(&path, &source, passage.start, passage.end, &new, model)
    }

    pub(super) fn finish_complete(&mut self, paragraph: &Paragraph, run: String, answer: std::result::Result<Answer, String>) -> Event {
        match answer {
            Ok(Answer::Text(text)) => {
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Done, usage: None }));
                let current = ProjectPath::new(&paragraph.path).ok().and_then(|p| self.project.text(&p).ok());
                // Typed on meanwhile: the continuation is of a text that is no longer there.
                let shown = match continuation(&text, &paragraph.text) {
                    Some(text) if current.is_some_and(|s| paragraph.still_in(s)) => {
                        self.ai.ghost = Some(Ghost { paragraph: paragraph.clone(), text });
                        true
                    }
                    _ => false,
                };
                Event::Completed(Ok(shown))
            }
            other => {
                let e = other.err().unwrap_or_else(|| "The model's answer could not be read.".to_owned());
                let kind = kind_of(&e);
                self.log(HistoryRecord::End(EndRecord { v: HISTORY_SCHEMA, run, at_ms: now_ms(), outcome: Logged::Failed { kind }, usage: None }));
                Event::Completed(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rewrite_keeps_the_selections_own_spacing_and_drops_markers_and_fences() {
        let answer = "Here it is:\n<<<TEXT\n```latex\nA shorter line.\n```\nTEXT>>>\n";
        assert_eq!(rewritten(answer, "  A long line, which is long.\n"), "  A shorter line.\n");
        assert_eq!(rewritten("Plain answer.", "x"), "Plain answer.");
    }

    #[test]
    fn a_change_as_asked_may_grow_the_passage_but_not_lose_a_citation() {
        let how = How::Instruct("flesh it out".to_owned());
        let bullets = "\\item fast \\cite{a}\n\\item small\n";
        assert_eq!(refusal(&how, bullets, "It is fast \\cite{a}, and \\emph{small} too \\cite{b}.\n"), None);
        assert!(refusal(&how, bullets, "It is fast and small.\n").is_some_and(|why| why.contains("\\cite{a}")));
        // A rewrite may not add a citation.
        assert!(refusal(&How::Shorter, bullets, "Fast \\cite{a} \\cite{b}; small.\\item").is_some());
        assert!(How::instruct("  ").is_err() && How::instruct("two\nlines").is_err());
    }

    #[test]
    fn a_continuation_is_one_line_without_a_doubled_space() {
        assert_eq!(continuation(" the results.\nMore.", "We show ").as_deref(), Some("the results."));
        assert_eq!(continuation(" the results.", "We show").as_deref(), Some(" the results."));
        assert_eq!(continuation("\n\n", "We"), None);
        let long = "word ".repeat(100);
        assert!(continuation(&long, "x ").is_some_and(|c| c.chars().count() <= MAX_CONTINUATION_CHARS && c.ends_with("word")));
    }
}
