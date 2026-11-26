//! Grammar checking (docs/ROADMAP.md, E12): English prose checked offline by Harper
//! (`harper-core`), and passages in any language checked by the connected model on request
//! (`ai.grammar`, [`crate::ai`]).
//!
//! Harper reads the prose of a file as [`texcraft_latex::passage`] gives it: paragraphs,
//! headings, captions and footnotes, without commands, math, keys or verbatim text. Its findings
//! are mapped back to the source; one that touches a formula's, reference's or citation's
//! placeholder is dropped (math is not prose), and a replacement is offered only where the source
//! holds the flagged words exactly as written, so taking it keeps the LaTeX. Harper's own
//! spelling rules are off: TeXCraft's spelling ([`crate::spelling`]) does that.
//!
//! Harper is slow next to a frame (a paragraph takes milliseconds), so it runs on its own thread
//! on the desktop, a few passages at a time, those nearest the lines in view first, and only once
//! typing has paused; in the browser, which has no threads, one passage per call. A passage's
//! findings are kept by its text, so an edit rechecks only the passage it changed. Findings can be
//! ignored for the session, and rules and groups of rules switched off (Settings › Editor,
//! `grammar.rule`), which filters what was found without checking again.
//!
//! Harper is behind the `grammar` feature (on for the desktop app and the CLI, off for the
//! browser, whose bundle it would grow by megabytes): without it offline grammar is absent and
//! says so ([`NOT_IN_BUILD`]); the model's grammar check works either way.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use texcraft_latex::passage::Passage;
use texcraft_project::ProjectPath;

use crate::{Result, Session};

/// Whether this build checks grammar offline (the `grammar` feature).
pub const OFFLINE: bool = cfg!(feature = "grammar");

/// Why there is no offline grammar, in a build without it.
pub const NOT_IN_BUILD: &str = "Grammar checking is not in this build; Check Grammar with AI is available.";

/// An English dialect, as Harper knows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    American,
    British,
    Australian,
    Canadian,
    Indian,
}

/// How long typing must pause before a changed passage is checked, in milliseconds.
pub const PAUSE_MS: i64 = 500;

/// Passages sent to the checker at once: few, so the nearest to the view go first.
const IN_FLIGHT: usize = 4;

/// Passages whose findings are kept, at most, before the cache starts again.
const MAX_KEPT: usize = 50_000;

/// Characters of context around a finding that identify it for Ignore.
const IGNORE_CONTEXT: usize = 24;

/// How long `grammar.check` waits for the checker, at most, in seconds.
#[cfg(all(feature = "grammar", not(target_arch = "wasm32")))]
const WAIT_S: u64 = 120;

/// The groups of rules Settings shows, by id: their name, and the kinds of Harper's findings in
/// them.
pub const GROUPS: [(&str, &str); 6] = [
    ("agreement", "Agreement"),
    ("grammar", "Grammar and word order"),
    ("punctuation", "Punctuation and capitals"),
    ("repetition", "Repeated and redundant words"),
    ("usage", "Word choice and usage"),
    ("style", "Style and readability"),
];

/// The grammar settings (Settings › Editor), kept with the preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrammarOptions {
    /// Check English documents with Harper.
    pub on: bool,
    /// Harper's rules switched off, by key (`PronounVerbAgreement`).
    pub rules_off: BTreeSet<String>,
    /// Groups switched off ([`GROUPS`]).
    pub groups_off: BTreeSet<String>,
}

impl Default for GrammarOptions {
    fn default() -> Self {
        // Style and readability remarks (long sentences, wordy phrases) are many in technical
        // writing and are no mistakes: off until asked for.
        Self { on: true, rules_off: BTreeSet::new(), groups_off: BTreeSet::from(["style".to_owned()]) }
    }
}

/// What a finding comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum Origin {
    /// Harper, offline.
    Harper,
    /// The connected model (`ai.grammar`).
    Ai { model: String },
}

/// A grammar problem in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GrammarProblem {
    /// Bytes of the file.
    pub range: Range<usize>,
    /// From 0; columns in characters.
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    /// Harper's rule (`PronounVerbAgreement`), or `ai`.
    pub rule: String,
    /// Its group ([`GROUPS`]), or `ai`.
    pub group: &'static str,
    pub message: String,
    /// Texts that can replace [`GrammarProblem::range`], best first; none where the source does
    /// not hold the flagged words exactly as written.
    pub replacements: Vec<String>,
    /// Identifies the finding for Ignore: its rule and the words around it.
    pub id: String,
    pub origin: Origin,
}

/// A finding in a passage: its characters, rule, group, message and replacements.
#[derive(Debug, Clone)]
struct Found {
    chars: Range<usize>,
    rule: String,
    /// Its group ([`GROUPS`]); `None` for spelling, which is left to the spell checker.
    group: Option<&'static str>,
    message: String,
    /// Texts to put in place of its characters, best first.
    replacements: Vec<String>,
}

/// Harper itself, in builds with the `grammar` feature.
#[cfg(feature = "grammar")]
mod harper {
    use harper_core::Document;
    use harper_core::linting::{LintGroup, LintKind, Suggestion};
    use harper_core::spell::FstDictionary;

    use super::{Dialect, Found};

    fn to_harper(d: Dialect) -> harper_core::Dialect {
        match d {
            Dialect::American => harper_core::Dialect::American,
            Dialect::British => harper_core::Dialect::British,
            Dialect::Australian => harper_core::Dialect::Australian,
            Dialect::Canadian => harper_core::Dialect::Canadian,
            Dialect::Indian => harper_core::Dialect::Indian,
        }
    }

    /// The group a kind of finding is in; `None` for spelling, which is left to the spell checker.
    fn group_of(kind: LintKind) -> Option<&'static str> {
        Some(match kind {
            LintKind::Spelling => return None,
            LintKind::Agreement => "agreement",
            LintKind::Grammar | LintKind::WordOrder | LintKind::BoundaryError => "grammar",
            LintKind::Punctuation | LintKind::Capitalization | LintKind::Formatting => "punctuation",
            LintKind::Repetition | LintKind::Redundancy => "repetition",
            LintKind::Style | LintKind::Enhancement | LintKind::Readability => "style",
            _ => "usage",
        })
    }

    /// Harper with its rules, for one dialect.
    pub(super) struct Harper {
        dialect: Dialect,
        group: LintGroup,
    }

    impl Harper {
        pub(super) fn new(dialect: Dialect) -> Self {
            let mut group = LintGroup::new_curated(FstDictionary::curated(), to_harper(dialect));
            // TeXCraft's own spelling checks words; Harper's would say the same twice.
            group.config.set_rule_enabled("SpellCheck", false);
            Self { dialect, group }
        }

        pub(super) fn check(&mut self, text: &str, dialect: Dialect) -> Vec<Found> {
            if self.dialect != dialect {
                *self = Self::new(dialect);
            }
            let doc = Document::new_plain_english_curated(text);
            let mut out = Vec::new();
            for (rule, lints) in self.group.organized_lints(&doc) {
                for l in lints {
                    let flagged: String = text.chars().skip(l.span.start).take(l.span.end.saturating_sub(l.span.start)).collect();
                    // TeX reads a run of spaces, or a line's end and the next line's indent, as one
                    // space: several in the source are no mistake.
                    if rule == "Spaces" && flagged.chars().count() > 1 && flagged.chars().all(char::is_whitespace) {
                        continue;
                    }
                    let replacements = l
                        .suggestions
                        .iter()
                        .map(|s| match s {
                            Suggestion::ReplaceWith(c) => c.iter().collect(),
                            Suggestion::Remove => String::new(),
                            Suggestion::InsertAfter(c) => format!("{flagged}{}", c.iter().collect::<String>()),
                        })
                        .collect();
                    out.push(Found {
                        chars: l.span.start..l.span.end,
                        rule: rule.clone(),
                        group: group_of(l.lint_kind),
                        message: l.message,
                        replacements,
                    });
                }
            }
            out.sort_by_key(|f| f.chars.start);
            out
        }
    }

    /// Whether `key` is one of Harper's rules.
    pub(super) fn is_rule(key: &str) -> bool {
        harper_core::linting::FlatConfig::new_curated().has_rule(key)
    }
}

/// Whether `key` is one of Harper's rules (none in a build without them).
pub fn is_rule(key: &str) -> bool {
    #[cfg(feature = "grammar")]
    return harper::is_rule(key);
    #[cfg(not(feature = "grammar"))]
    {
        let _ = key;
        false
    }
}

/// The dialect of an English language tag, `None` for another language.
pub fn dialect(tag: &str) -> Option<Dialect> {
    let t = tag.to_ascii_lowercase();
    let (primary, region) = t.split_once(['-', '_']).unwrap_or((t.as_str(), ""));
    (primary == "en").then_some(match region {
        "gb" | "uk" | "ie" | "nz" => Dialect::British,
        "au" => Dialect::Australian,
        "ca" => Dialect::Canadian,
        "in" => Dialect::Indian,
        _ => Dialect::American,
    })
}

/// A passage to check: the key of its findings, and its text.
#[cfg(feature = "grammar")]
struct Job {
    key: u64,
    text: String,
    dialect: Dialect,
}

/// A passage's key and what was found in it.
type Checked = (u64, Vec<Found>);

/// A file's passages, its line starts, and when it last changed.
type Passages = (Arc<Vec<Passage>>, Arc<Vec<usize>>, i64);

/// Where passages are checked: a thread on the desktop, this thread in the browser.
#[derive(Default)]
struct Runner {
    #[cfg(all(feature = "grammar", not(target_arch = "wasm32")))]
    thread: Option<(std::sync::mpsc::Sender<Job>, std::sync::mpsc::Receiver<Checked>)>,
    #[cfg(all(feature = "grammar", target_arch = "wasm32"))]
    harper: Option<harper::Harper>,
    #[cfg(all(feature = "grammar", target_arch = "wasm32"))]
    queue: std::collections::VecDeque<Job>,
}

#[cfg(not(feature = "grammar"))]
impl Runner {
    fn send(&mut self, _: u64, _: &str, _: Dialect) -> bool {
        false
    }

    fn take(&mut self, _: bool) -> Vec<Checked> {
        Vec::new()
    }
}

#[cfg(feature = "grammar")]
impl Runner {
    #[cfg(not(target_arch = "wasm32"))]
    fn send(&mut self, key: u64, text: &str, dialect: Dialect) -> bool {
        let job = Job { key, text: text.to_owned(), dialect };
        if self.thread.is_none() {
            let (jobs, rx) = std::sync::mpsc::channel::<Job>();
            let (tx, done) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new().name("grammar".to_owned()).spawn(move || {
                let mut harper: Option<harper::Harper> = None;
                while let Ok(job) = rx.recv() {
                    let h = harper.get_or_insert_with(|| harper::Harper::new(job.dialect));
                    let found = h.check(&job.text, job.dialect);
                    if tx.send((job.key, found)).is_err() {
                        break;
                    }
                }
            });
            if spawned.is_err() {
                return false;
            }
            self.thread = Some((jobs, done));
        }
        self.thread.as_ref().is_some_and(|(jobs, _)| jobs.send(job).is_ok())
    }

    /// Findings that came back since the last call (`wait`: at least one, unless none is out).
    #[cfg(not(target_arch = "wasm32"))]
    fn take(&mut self, wait: bool) -> Vec<Checked> {
        let Some((_, done)) = &self.thread else { return Vec::new() };
        let mut out = Vec::new();
        if wait {
            match done.recv_timeout(std::time::Duration::from_secs(WAIT_S)) {
                Ok(d) => out.push(d),
                Err(_) => {
                    // The thread stopped (or hangs): a new one is started for the next passage.
                    self.thread = None;
                    return out;
                }
            }
        }
        out.extend(done.try_iter());
        out
    }

    #[cfg(target_arch = "wasm32")]
    fn send(&mut self, key: u64, text: &str, dialect: Dialect) -> bool {
        self.queue.push_back(Job { key, text: text.to_owned(), dialect });
        true
    }

    /// In the browser: checks one passage queued (all of them with `wait`).
    #[cfg(target_arch = "wasm32")]
    fn take(&mut self, wait: bool) -> Vec<Checked> {
        let mut out = Vec::new();
        while let Some(job) = self.queue.pop_front() {
            let h = self.harper.get_or_insert_with(|| harper::Harper::new(job.dialect));
            out.push((job.key, h.check(&job.text, job.dialect)));
            if !wait {
                break;
            }
        }
        out
    }
}

/// A file's passages at a session revision, with the text's hash, its line starts and when it
/// last changed (unix milliseconds; 0 when first seen, so it is checked at once).
struct FilePassages {
    revision: u64,
    hash: u64,
    passages: Arc<Vec<Passage>>,
    lines: Arc<Vec<usize>>,
    changed_ms: i64,
    /// The key of each passage's findings, for a dialect.
    keys: Option<(Dialect, Arc<Vec<u64>>)>,
}

/// A problem the model found: the words it quoted, where they are, what it said and its
/// replacement.
#[derive(Debug, Clone)]
pub(crate) struct AiIssue {
    pub range: Range<usize>,
    pub quote: String,
    pub message: String,
    pub replacement: Option<String>,
    pub model: String,
}

#[derive(Default)]
pub(crate) struct State {
    runner: Runner,
    /// Findings by passage key (its dialect and text).
    found: HashMap<u64, Arc<Vec<Found>>>,
    /// Passages sent and not back yet.
    out: HashSet<u64>,
    files: HashMap<ProjectPath, FilePassages>,
    /// Findings ignored for the session, by id.
    ignored: HashSet<String>,
    /// What the model found, by file.
    pub(crate) ai: HashMap<ProjectPath, Vec<AiIssue>>,
    /// The last file asked about has passages not checked yet (waiting for typing to pause).
    waiting: bool,
}

fn hash_of(value: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The byte where each line of `text` starts.
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

/// Line and column (in characters) of byte `at` of `text`.
fn place(text: &str, lines: &[usize], at: usize) -> (usize, usize) {
    let line = lines.partition_point(|s| *s <= at).saturating_sub(1);
    let start = lines.get(line).copied().unwrap_or(0);
    (line, text.get(start..at).map_or(0, |s| s.chars().count()))
}

/// The id of a finding: its rule and the words around it in the passage.
fn id_of(rule: &str, passage: &str, chars: &Range<usize>) -> String {
    let around: String = passage.chars().skip(chars.start.saturating_sub(IGNORE_CONTEXT)).take(chars.len() + 2 * IGNORE_CONTEXT).collect();
    format!("{rule}:{:016x}", hash_of((rule, around)))
}

impl Session {
    /// Why grammar is not checked now, if it is not: it is off, or the document is not in
    /// English. Gives the dialect checked.
    pub fn grammar_ready(&self) -> Result<Dialect> {
        if !OFFLINE {
            return Err(NOT_IN_BUILD.to_owned());
        }
        if !self.settings.grammar.on {
            return Err("Grammar checking is off (Settings › Editor).".to_owned());
        }
        let (lang, _) = self.spelling_language();
        dialect(&lang).ok_or_else(|| {
            format!("Grammar is checked offline in English only, and this document is in {lang}: Check Grammar with AI works in any language.")
        })
    }

    /// Whether passages are being checked in the background (the window repaints until done).
    pub fn grammar_busy(&self) -> bool {
        !self.grammar.out.is_empty() || self.grammar.waiting
    }

    /// The passages of `path`, found again when its text changed.
    fn grammar_passages(&mut self, path: &ProjectPath) -> Result<Passages> {
        let comments = self.settings.spelling.comments;
        let revision = self.revision;
        if let Some(f) = self.grammar.files.get(path)
            && f.revision == revision
        {
            return Ok((Arc::clone(&f.passages), Arc::clone(&f.lines), f.changed_ms));
        }
        let text = self.project.text(path).map_err(|e| e.to_string())?;
        let h = hash_of((text, comments));
        let g = &mut self.grammar;
        if let Some(f) = g.files.get_mut(path)
            && f.hash == h
        {
            f.revision = revision;
            return Ok((Arc::clone(&f.passages), Arc::clone(&f.lines), f.changed_ms));
        }
        let passages = Arc::new(texcraft_latex::passage::passages(text, texcraft_latex::prose::Options { comments }));
        let lines = Arc::new(line_starts(text));
        let changed_ms = if g.files.contains_key(path) { now_ms() } else { 0 };
        g.files.insert(
            path.clone(),
            FilePassages { revision, hash: h, passages: Arc::clone(&passages), lines: Arc::clone(&lines), changed_ms, keys: None },
        );
        Ok((passages, lines, changed_ms))
    }

    /// The key of each passage of `path` (its findings' in [`State::found`]) for `dialect`.
    fn grammar_keys(&mut self, path: &ProjectPath, dialect: Dialect) -> Arc<Vec<u64>> {
        let Some(f) = self.grammar.files.get_mut(path) else { return Arc::default() };
        match &f.keys {
            Some((d, keys)) if *d == dialect => Arc::clone(keys),
            _ => {
                let keys = Arc::new(f.passages.iter().map(|p| hash_of((dialect, &p.text))).collect::<Vec<_>>());
                f.keys = Some((dialect, Arc::clone(&keys)));
                keys
            }
        }
    }

    /// Findings back from the checker kept (`wait`: waits for one).
    fn take_grammar(&mut self, wait: bool) {
        let g = &mut self.grammar;
        if g.found.len() > MAX_KEPT {
            g.found.clear();
        }
        for (key, found) in g.runner.take(wait) {
            g.out.remove(&key);
            g.found.insert(key, Arc::new(found));
        }
    }

    /// Sends the passages of `path` not checked yet, nearest to bytes `near` first, keeping
    /// [`IN_FLIGHT`] out (`all`: every one, for `grammar.check`).
    fn send_grammar(&mut self, (passages, keys): (&[Passage], &[u64]), dialect: Dialect, near: Option<Range<usize>>, all: bool) {
        let g = &mut self.grammar;
        let mut todo: Vec<(usize, u64, &Passage)> = passages
            .iter()
            .zip(keys.iter().copied())
            .filter(|(_, key)| !g.found.contains_key(key) && !g.out.contains(key))
            .map(|(p, key)| {
                let span = p.span();
                let distance =
                    near.as_ref().map_or(span.start, |n| if span.end < n.start { n.start - span.end } else { span.start.saturating_sub(n.end) });
                (distance, key, p)
            })
            .collect();
        todo.sort_by_key(|(d, ..)| *d);
        let room = if all { usize::MAX } else { IN_FLIGHT.saturating_sub(g.out.len()) };
        let mut sent = HashSet::new();
        for (_, key, p) in todo.into_iter().take(room) {
            // The same text twice in a file is checked once.
            if sent.insert(key) && g.runner.send(key, &p.text, dialect) {
                g.out.insert(key);
            }
        }
    }

    /// The grammar problems of `path` found so far, on lines `lines` (from 0, end excluded; all
    /// without), in order, and passages not checked yet sent to the checker in the background: it
    /// never waits for them. An error says why nothing is checked: it is off, or the document is
    /// not in English (the model's findings are still given then).
    pub fn grammar_problems(&mut self, path: &ProjectPath, lines: Option<Range<usize>>) -> Result<Vec<GrammarProblem>> {
        let ready = self.grammar_ready();
        let mut out = match &ready {
            Ok(dialect) => self.harper_problems(path, lines.clone(), *dialect, false)?,
            Err(_) => {
                self.grammar.waiting = false;
                Vec::new()
            }
        };
        out.extend(self.ai_problems(path, lines)?);
        out.sort_by_key(|p| p.range.start);
        if out.is_empty()
            && let Err(why) = ready
        {
            return Err(why);
        }
        Ok(out)
    }

    /// Every grammar problem of `path`, after waiting for the checker (`grammar.check`).
    pub fn check_grammar(&mut self, path: &ProjectPath) -> Result<Vec<GrammarProblem>> {
        let mut out = match self.grammar_ready() {
            Ok(dialect) => self.harper_problems(path, None, dialect, true)?,
            Err(why) if !self.grammar.ai.contains_key(path) => return Err(why),
            Err(_) => Vec::new(),
        };
        out.extend(self.ai_problems(path, None)?);
        out.sort_by_key(|p| p.range.start);
        Ok(out)
    }

    fn harper_problems(&mut self, path: &ProjectPath, lines: Option<Range<usize>>, dialect: Dialect, wait: bool) -> Result<Vec<GrammarProblem>> {
        let (passages, starts, changed_ms) = self.grammar_passages(path)?;
        let len = self.project.text(path).map_err(|e| e.to_string())?.len();
        let near = lines.as_ref().map(|l| {
            let at = |line: usize| starts.get(line).copied().unwrap_or(len);
            at(l.start)..at(l.end)
        });
        let keys = self.grammar_keys(path, dialect);
        self.take_grammar(false);
        if wait {
            self.send_grammar((&passages, &keys), dialect, None, true);
            while !self.grammar.out.is_empty() {
                let before = self.grammar.out.len();
                self.take_grammar(true);
                if self.grammar.out.len() == before {
                    return Err("The grammar checker stopped before it was done.".to_owned());
                }
            }
        } else if now_ms() - changed_ms >= PAUSE_MS {
            self.send_grammar((&passages, &keys), dialect, near.clone(), false);
            #[cfg(target_arch = "wasm32")]
            self.take_grammar(false);
        }
        let text = self.project.text(path).map_err(|e| e.to_string())?;
        let options = &self.settings.grammar;
        let mut out = Vec::new();
        self.grammar.waiting = keys.iter().any(|k| !self.grammar.found.contains_key(k));
        let g = &self.grammar;
        for (p, key) in passages.iter().zip(keys.iter()) {
            let span = p.span();
            if near.as_ref().is_some_and(|n| span.end < n.start || span.start > n.end) {
                continue;
            }
            let Some(found) = g.found.get(key) else { continue };
            for f in found.iter() {
                let Some(group) = f.group else { continue };
                if options.groups_off.contains(group) || options.rules_off.contains(&f.rule) {
                    continue;
                }
                let id = id_of(&f.rule, &p.text, &f.chars);
                if g.ignored.contains(&id) {
                    continue;
                }
                let Some(spot) = p.source(f.chars.clone()) else { continue };
                // Math, references and citations are not prose: what touches them is no finding.
                if spot.placeholder || spot.range.is_empty() {
                    continue;
                }
                let flagged = text.get(spot.range.clone()).unwrap_or_default();
                let replacements = if spot.exact { f.replacements.iter().filter(|r| *r != flagged).cloned().collect() } else { Vec::new() };
                let (line, column) = place(text, &starts, spot.range.start);
                let (end_line, end_column) = place(text, &starts, spot.range.end);
                out.push(GrammarProblem {
                    range: spot.range,
                    line,
                    column,
                    end_line,
                    end_column,
                    rule: f.rule.clone(),
                    group,
                    message: f.message.clone(),
                    replacements,
                    id,
                    origin: Origin::Harper,
                });
            }
        }
        Ok(out)
    }

    /// What the model found in `path` that is still in its text: a quote whose words moved is
    /// found again near where it was, one whose words are gone is dropped.
    fn ai_problems(&mut self, path: &ProjectPath, lines: Option<Range<usize>>) -> Result<Vec<GrammarProblem>> {
        let Some(issues) = self.grammar.ai.get(path) else { return Ok(Vec::new()) };
        let text = self.project.text(path).map_err(|e| e.to_string())?;
        let starts = line_starts(text);
        let mut kept = Vec::new();
        for mut issue in issues.iter().cloned() {
            if text.get(issue.range.clone()) != Some(issue.quote.as_str()) {
                let from = issue.range.start.saturating_sub(200);
                let Some(found) = text.get(from..).and_then(|rest| rest.find(&issue.quote)).map(|at| from + at) else { continue };
                issue.range = found..found + issue.quote.len();
            }
            kept.push(issue);
        }
        let mut out = Vec::new();
        for issue in &kept {
            let (line, column) = place(text, &starts, issue.range.start);
            let (end_line, end_column) = place(text, &starts, issue.range.end);
            if lines.as_ref().is_some_and(|l| end_line < l.start || line >= l.end) {
                continue;
            }
            let id = format!("ai:{:016x}", hash_of((&issue.quote, &issue.message)));
            if self.grammar.ignored.contains(&id) {
                continue;
            }
            out.push(GrammarProblem {
                range: issue.range.clone(),
                line,
                column,
                end_line,
                end_column,
                rule: "ai".to_owned(),
                group: "ai",
                message: issue.message.clone(),
                replacements: issue.replacement.iter().cloned().collect(),
                id,
                origin: Origin::Ai { model: issue.model.clone() },
            });
        }
        self.grammar.ai.insert(path.clone(), kept);
        Ok(out)
    }

    /// Ignores the finding `id` until the session ends.
    pub fn ignore_grammar(&mut self, id: &str) -> Result<()> {
        let id = id.trim();
        if id.is_empty() || id.len() > 200 {
            return Err("`id` is a grammar problem's id, as grammar.check gives it.".to_owned());
        }
        self.grammar.ignored.insert(id.to_owned());
        // A model's finding ignored is gone for good.
        if let Some(hash) = id.strip_prefix("ai:") {
            for issues in self.grammar.ai.values_mut() {
                issues.retain(|i| format!("{:016x}", hash_of((&i.quote, &i.message))) != hash);
            }
        }
        Ok(())
    }

    /// Switches Harper's rule `rule` (its key) or the group `group` on or off.
    pub fn set_grammar_rule(&mut self, rule: Option<&str>, group: Option<&str>, on: bool) -> Result<()> {
        if !OFFLINE {
            return Err(NOT_IN_BUILD.to_owned());
        }
        let o = &mut self.settings.grammar;
        match (rule, group) {
            (Some(rule), None) => {
                if !is_rule(rule) {
                    return Err(format!("“{rule}” is not one of Harper's rules: give its key, as grammar.check gives it (`rule`)."));
                }
                if on {
                    o.rules_off.remove(rule)
                } else {
                    o.rules_off.insert(rule.to_owned())
                };
            }
            (None, Some(group)) => {
                if !GROUPS.iter().any(|(g, _)| *g == group) {
                    let ids: Vec<&str> = GROUPS.iter().map(|(g, _)| *g).collect();
                    return Err(format!("There is no group “{group}”: there are {}.", ids.join(", ")));
                }
                if on {
                    o.groups_off.remove(group)
                } else {
                    o.groups_off.insert(group.to_owned())
                };
            }
            _ => return Err("Give `rule` (a Harper rule's key) or `group`, not both.".to_owned()),
        }
        Ok(())
    }

    /// Keeps what the model found in `path` (the AI request replaces its earlier findings there).
    pub(crate) fn set_ai_grammar(&mut self, path: &ProjectPath, within: Range<usize>, issues: Vec<AiIssue>) {
        let list = self.grammar.ai.entry(path.clone()).or_default();
        list.retain(|i| i.range.end <= within.start || i.range.start >= within.end);
        list.extend(issues);
        list.sort_by_key(|i| i.range.start);
    }
}
