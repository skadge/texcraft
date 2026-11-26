//! The formulas of a file, typeset (docs/ROADMAP.md, F2): what the formatted view draws and what
//! `language.formulas` tells agents. Each formula of the file is typeset after the main document's
//! preamble by `texcraft_compile::formulas` (the desktop and the CLI); the browser has no engine
//! for this yet and says so.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use texcraft_latex::view::{Block, BlockKind, Inline, InlineKind};
use texcraft_project::ProjectPath;

/// A typeset formula: page `page` of `pdf`, as tight as its box (in TeX points).
#[derive(Debug, Clone)]
pub struct FormulaBox {
    pub pdf: Arc<Vec<u8>>,
    /// From 0.
    pub page: u32,
    pub width_pt: f32,
    /// Above the baseline.
    pub height_pt: f32,
    /// Below the baseline.
    pub depth_pt: f32,
}

#[derive(Debug, Clone)]
pub enum FormulaState {
    /// Being typeset.
    Pending,
    Typeset(FormulaBox),
    /// Not typeset: retex's messages.
    Failed(Vec<String>),
    /// This build has no engine for formulas: why.
    Unavailable(String),
}

/// A formula of a file.
#[derive(Debug, Clone)]
pub struct Formula {
    /// The file it is in.
    pub file: ProjectPath,
    /// The formula with its delimiters, in bytes of the file.
    pub range: Range<usize>,
    /// What is typeset: the math without delimiters, an environment's lines in `aligned` or
    /// `gathered`, without `\label`, `\tag` and `\nonumber`.
    pub source: String,
    pub display: bool,
    pub state: FormulaState,
}

/// The formulas of a file.
#[derive(Debug, Clone, Default)]
pub struct Formulas {
    /// The size of the document's body text, in TeX points (its class's `10pt`, `11pt`, `12pt`),
    /// to draw formulas at the size of the text around them.
    pub body_pt: f32,
    pub formulas: Vec<Formula>,
}

/// What the session keeps between frames: the file's formulas for a revision, and the engine.
#[derive(Default)]
pub(crate) struct State {
    /// The files and revision `found` and `preamble` are for.
    of: Option<(Vec<ProjectPath>, u64)>,
    found: Vec<Found>,
    preamble: String,
    body_pt: f32,
    #[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
    engine: Option<texcraft_compile::formulas::Formulas>,
}

/// A formula found in the file, before it is typeset.
#[derive(Debug, Clone)]
struct Found {
    file: ProjectPath,
    range: Range<usize>,
    source: String,
    display: bool,
    closed: bool,
}

impl crate::Session {
    /// The formulas of `path` and how far they are typeset: those not typeset yet are sent to the
    /// engine in one batch. Call it again (each frame) for the batch's results.
    pub fn formulas(&mut self, path: &ProjectPath) -> crate::Result<Formulas> {
        self.formulas_of(std::slice::from_ref(path))
    }

    /// The formulas of several files in one batch: the file shown and those it includes, which
    /// the formatted view shows in place. The first file must be readable; the others are skipped
    /// when they are not.
    pub fn formulas_of(&mut self, paths: &[ProjectPath]) -> crate::Result<Formulas> {
        let revision = self.revision;
        let Some(path) = paths.first() else { return Ok(Formulas::default()) };
        if !self.formulas.of.as_ref().is_some_and(|(p, r)| p.as_slice() == paths && *r == revision) {
            let text = self.project.text(path).map_err(|e| e.to_string())?;
            let main = self.project.main().and_then(|m| self.project.text(m).ok()).unwrap_or(text);
            // The view's blocks, as the formatted view makes them (with the main file's theorems).
            let theorems = texcraft_latex::view::theorems(main);
            let mut found = Vec::new();
            for p in paths {
                if let Ok(t) = self.project.text(p) {
                    found.extend(find(p, t, &texcraft_latex::view::view_with(t, &theorems)));
                }
            }
            let preamble = preamble_of(main);
            self.formulas.body_pt = body_size(&preamble);
            self.formulas.preamble = preamble;
            self.formulas.found = found;
            self.formulas.of = Some((paths.to_vec(), revision));
        }
        let states = self.typeset_found();
        let found = &self.formulas.found;
        let formulas = found
            .iter()
            .zip(states)
            .map(|(f, state)| Formula { file: f.file.clone(), range: f.range.clone(), source: f.source.clone(), display: f.display, state })
            .collect();
        Ok(Formulas { body_pt: self.formulas.body_pt, formulas })
    }

    /// `math` (without delimiters; in display environment `env`, or inline unless `display`)
    /// typeset after the main document's preamble, as `path`'s formulas are: for the formula
    /// editor's preview while it is typed. Call it again (each frame) for the result.
    pub fn formula_preview(&mut self, path: &ProjectPath, math: &str, display: bool, env: Option<&str>) -> FormulaState {
        // The preamble of the formulas asked for last serves when they are this file's.
        let current = self.formulas.of.as_ref().is_some_and(|(p, r)| p.first() == Some(path) && *r == self.revision);
        if !current && self.formulas(path).is_err() {
            return FormulaState::Failed(vec![format!("`{path}` cannot be read")]);
        }
        let source = if display { display_source(math, env) } else { math.trim().to_owned() };
        self.preview_one(source, display)
    }

    #[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
    fn preview_one(&mut self, source: String, display: bool) -> FormulaState {
        use texcraft_compile::formulas::{Done, Files, State as Engine, Wanted};
        let engine = self.formulas.engine.get_or_insert_with(new_engine);
        let project = &self.project;
        let files = || {
            let main_dir = project.main().and_then(|m| m.as_str().rsplit_once('/').map(|(d, _)| d.to_owned()));
            let files = project.files().map(|(p, c)| (p.clone(), Arc::<[u8]>::from(c.as_bytes()))).collect();
            Files { files, main_dir }
        };
        match engine.request(&self.formulas.preamble, &[Wanted { source, display }], files).into_iter().next() {
            Some(Engine::Done(done)) => match &*done {
                Done::Typeset(t) => FormulaState::Typeset(FormulaBox {
                    pdf: Arc::clone(&t.pdf),
                    page: t.page,
                    width_pt: t.width_pt,
                    height_pt: t.height_pt,
                    depth_pt: t.depth_pt,
                }),
                Done::Failed(messages) => FormulaState::Failed(messages.clone()),
            },
            Some(Engine::Pending) | None => FormulaState::Pending,
        }
    }

    #[cfg(not(all(feature = "retex", not(target_arch = "wasm32"))))]
    fn preview_one(&mut self, _source: String, _display: bool) -> FormulaState {
        FormulaState::Unavailable("Formulas are typeset in the desktop app; this build shows their source.".to_owned())
    }

    /// Waits until no batch of formulas runs (the CLI and the agent tools).
    pub fn wait_for_formulas(&mut self) {
        #[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
        if let Some(engine) = self.formulas.engine.as_mut() {
            engine.wait();
        }
    }

    #[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
    fn typeset_found(&mut self) -> Vec<FormulaState> {
        use texcraft_compile::formulas::{Done, Files, State as Engine, Wanted};
        let found = &self.formulas.found;
        let wanted: Vec<Wanted> = found.iter().filter(|f| f.closed).map(|f| Wanted { source: f.source.clone(), display: f.display }).collect();
        if wanted.is_empty() {
            return found.iter().map(|_| not_closed()).collect();
        }
        let engine = self.formulas.engine.get_or_insert_with(new_engine);
        let project = &self.project;
        let files = || {
            let main_dir = project.main().and_then(|m| m.as_str().rsplit_once('/').map(|(d, _)| d.to_owned()));
            let files = project.files().map(|(p, c)| (p.clone(), Arc::<[u8]>::from(c.as_bytes()))).collect();
            Files { files, main_dir }
        };
        let mut states = engine.request(&self.formulas.preamble, &wanted, files).into_iter();
        found
            .iter()
            .map(|f| {
                if !f.closed {
                    return not_closed();
                }
                match states.next() {
                    Some(Engine::Pending) | None => FormulaState::Pending,
                    Some(Engine::Done(done)) => match &*done {
                        Done::Typeset(t) => FormulaState::Typeset(FormulaBox {
                            pdf: Arc::clone(&t.pdf),
                            page: t.page,
                            width_pt: t.width_pt,
                            height_pt: t.height_pt,
                            depth_pt: t.depth_pt,
                        }),
                        Done::Failed(messages) => FormulaState::Failed(messages.clone()),
                    },
                }
            })
            .collect()
    }

    #[cfg(not(all(feature = "retex", not(target_arch = "wasm32"))))]
    fn typeset_found(&mut self) -> Vec<FormulaState> {
        let why = "Formulas are typeset in the desktop app; this build shows their source.";
        self.formulas.found.iter().map(|f| if f.closed { FormulaState::Unavailable(why.to_owned()) } else { not_closed() }).collect()
    }
}

/// The formulas engine of a session, in a folder of its own: two sessions of one process (the
/// tests) do not typeset into the same files.
#[cfg(all(feature = "retex", not(target_arch = "wasm32")))]
fn new_engine() -> texcraft_compile::formulas::Formulas {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("texcraft-formulas-{}-{n}", std::process::id()));
    texcraft_compile::formulas::Formulas::new(dir)
}

fn not_closed() -> FormulaState {
    FormulaState::Failed(vec!["The formula is not closed.".to_owned()])
}

/// The text of `main` before `\begin{document}` (all of it when there is none: the formulas then
/// get the default class).
fn preamble_of(main: &str) -> String {
    match texcraft_latex::view::view(main).first() {
        Some(Block { kind: BlockKind::Preamble, range }) => main.get(range.clone()).unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// The body size a preamble's `\documentclass[…12pt…]` asks for; 10pt otherwise.
fn body_size(preamble: &str) -> f32 {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\\documentclass\s*\[([^\]]*)\]").ok());
    re.as_ref()
        .and_then(|re| re.captures(preamble))
        .and_then(|c| c.get(1))
        .and_then(|options| options.as_str().split(',').find_map(|o| o.trim().strip_suffix("pt")?.parse::<f32>().ok()))
        .filter(|pt| (5.0..=30.0).contains(pt))
        .unwrap_or(10.0)
}

/// The formulas of the view `blocks` of `text`, in reading order.
fn find(file: &ProjectPath, text: &str, blocks: &[Block]) -> Vec<Found> {
    let mut out = Vec::new();
    for b in blocks {
        find_block(file, text, b, &mut out);
    }
    out
}

fn find_block(file: &ProjectPath, text: &str, block: &Block, out: &mut Vec<Found>) {
    match &block.kind {
        BlockKind::Formula { source, env, closed } => {
            let math = text.get(source.clone()).unwrap_or_default();
            out.push(Found {
                file: file.clone(),
                range: block.range.clone(),
                source: display_source(math, env.as_deref()),
                display: true,
                closed: *closed,
            });
        }
        BlockKind::Title { title, author, date } => [title, author, date].into_iter().for_each(|i| find_inlines(file, text, i, out)),
        BlockKind::Heading { content, .. } => find_inlines(file, text, content, out),
        BlockKind::Paragraph(inlines) => find_inlines(file, text, inlines, out),
        BlockKind::List { items, .. } => {
            for item in items {
                if let Some(label) = &item.label {
                    find_inlines(file, text, label, out);
                }
                item.blocks.iter().for_each(|b| find_block(file, text, b, out));
            }
        }
        BlockKind::Container { blocks, .. } => blocks.iter().for_each(|b| find_block(file, text, b, out)),
        BlockKind::FrontMatter(fm) => {
            find_inlines(file, text, &fm.title, out);
            for a in &fm.authors {
                find_inlines(file, text, &a.name, out);
            }
            fm.affiliations.iter().for_each(|a| find_inlines(file, text, &a.text, out));
            fm.notes.iter().for_each(|n| find_inlines(file, text, &n.text, out));
            fm.blocks.iter().for_each(|b| find_block(file, text, b, out));
        }
        BlockKind::Theorem { title, blocks, .. } => {
            find_inlines(file, text, title, out);
            blocks.iter().for_each(|b| find_block(file, text, b, out));
        }
        BlockKind::Figure { caption, .. } => find_inlines(file, text, caption, out),
        BlockKind::Table(t) => {
            t.cells.iter().flatten().for_each(|c| find_inlines(file, text, c, out));
            if let Some(f) = &t.float {
                find_inlines(file, text, &f.caption, out);
            }
        }
        BlockKind::Preamble | BlockKind::Source | BlockKind::UnsupportedTable(_) | BlockKind::Include { .. } => {}
    }
}

fn find_inlines(file: &ProjectPath, text: &str, inlines: &[Inline], out: &mut Vec<Found>) {
    for i in inlines {
        match &i.kind {
            InlineKind::Formula { source, display, closed } => {
                let math = text.get(source.clone()).unwrap_or_default();
                let source = if *display { display_source(math, None) } else { math.trim().to_owned() };
                out.push(Found { file: file.clone(), range: i.range.clone(), source, display: *display, closed: *closed });
            }
            InlineKind::Footnote(inner) | InlineKind::Link { text: inner, .. } => find_inlines(file, text, inner, out),
            InlineKind::Text { .. } | InlineKind::LineBreak | InlineKind::Chip { .. } | InlineKind::Source => {}
        }
    }
}

/// The math of display environment `env` as one formula in display style: lines aligned at `&`
/// in `aligned`, others centred in `gathered`; numbering commands (`\label`, `\tag`, `\nonumber`)
/// are left out, as they are not allowed there.
fn display_source(math: &str, env: Option<&str>) -> String {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\\(label|tag\*?)\s*\{[^{}]*\}|\\(nonumber|notag)\b").ok());
    let math = match re {
        Some(re) => re.replace_all(math, ""),
        None => math.into(),
    };
    let math = math.trim();
    let base = env.map(|e| e.trim_end_matches('*'));
    match base {
        Some("align" | "flalign" | "alignat" | "eqnarray") => format!("\\begin{{aligned}}{math}\\end{{aligned}}"),
        Some("gather" | "multline") => format!("\\begin{{gathered}}{math}\\end{{gathered}}"),
        _ => math.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_of_several_lines_becomes_one_aligned_formula_without_its_numbering() {
        let s = display_source("a &= b \\label{eq:x} \\\\\n c &= d \\nonumber", Some("align*"));
        assert!(s.starts_with("\\begin{aligned}") && s.contains("&= d"), "{s}");
        assert!(!s.contains("label") && !s.contains("nonumber"), "{s}");
    }

    #[test]
    fn the_body_size_comes_from_the_class_options() {
        assert!(body_size("\\documentclass[a4paper, 12pt]{article}") > body_size("\\documentclass{article}"));
    }
}
