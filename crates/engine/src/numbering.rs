//! The numbers the formatted view shows (docs/ROADMAP.md, F5): what the last build's `.aux`
//! says of labels and citations, and the document's headings, formulas, theorems and floats
//! numbered from its main file ([`texcraft_latex::numbers`]), made again when the project or the
//! `.aux` changes.

use std::sync::Arc;

use texcraft_latex::auxfile::{self, Aux, CiteStyle, Missing};
use texcraft_latex::document::{self, Entry};
use texcraft_latex::numbers::{self, Numbers};
use texcraft_latex::view::Declared;
use texcraft_project::ProjectPath;

#[derive(Default)]
pub(crate) struct State {
    aux: Option<Arc<Aux>>,
    /// Bumped by each build that brings an `.aux`.
    generation: u64,
    /// The project revision and `.aux` generation `numbered` was made from.
    of: Option<(u64, u64)>,
    numbered: Arc<Numbered>,
    /// The project revision and `.aux` generation `outline` was made from.
    outline_of: Option<(u64, u64)>,
    outline: Arc<Vec<Entry>>,
}

impl State {
    pub(crate) fn built(&mut self, aux: &str) {
        self.aux = Some(Arc::new(auxfile::parse(aux)));
        self.generation += 1;
    }
}

/// The document's numbers and how it cites.
#[derive(Debug, Clone, Default)]
pub struct Numbered {
    pub numbers: Numbers,
    /// What the main file declares for the view of each file: its theorem-like environments, its
    /// macros, the names its class and language print.
    pub declared: Arc<Declared>,
    pub cite_style: Option<CiteStyle>,
    /// The main file, from which includes are found.
    pub main: Option<ProjectPath>,
}

impl crate::Session {
    /// What the last build's `.aux` says, when a build has written one.
    pub fn aux(&self) -> Option<&Arc<Aux>> {
        self.numbering.aux.as_ref()
    }

    /// The document's numbers, counted from its main file and the last build's `.aux`.
    pub fn numbered(&mut self) -> Arc<Numbered> {
        let key = (self.revision, self.numbering.generation);
        if self.numbering.of == Some(key) {
            return Arc::clone(&self.numbering.numbered);
        }
        let project = &self.project;
        let main = project.main().cloned();
        let read = |p: &str| -> Option<Arc<str>> { ProjectPath::new(p).ok().and_then(|p| project.text(&p).ok()).map(Arc::from) };
        let exists = |p: &str| ProjectPath::new(p).ok().is_some_and(|p| project.get(&p).is_some());
        let aux = self.numbering.aux.as_deref();
        let main_text = main.as_ref().and_then(|m| project.text(m).ok()).unwrap_or_default();
        let numbered = Numbered {
            numbers: main.as_ref().map(|m| numbers::number(m.as_str(), &read, &exists, aux)).unwrap_or_default(),
            declared: Arc::new(Declared::of(main_text)),
            cite_style: main.as_ref().map(|_| CiteStyle::of(main_text)),
            main,
        };
        self.numbering.numbered = Arc::new(numbered);
        self.numbering.of = Some(key);
        Arc::clone(&self.numbering.numbered)
    }

    /// The whole document's outline (docs/ROADMAP.md, E14): the headings of the main file and of
    /// the files it includes, each in its place, numbered as the PDF numbers them; empty without
    /// a main file.
    pub fn document_outline(&mut self) -> Arc<Vec<Entry>> {
        let key = (self.revision, self.numbering.generation);
        if self.numbering.outline_of == Some(key) {
            return Arc::clone(&self.numbering.outline);
        }
        let numbered = self.numbered();
        let project = &self.project;
        let read = |p: &str| -> Option<Arc<str>> { ProjectPath::new(p).ok().and_then(|p| project.text(&p).ok()).map(Arc::from) };
        let exists = |p: &str| ProjectPath::new(p).ok().is_some_and(|p| project.get(&p).is_some());
        let entries = match &numbered.main {
            Some(main) => document::outline(main.as_str(), &read, &exists, Some(&numbered.numbers)),
            None => Vec::new(),
        };
        self.numbering.outline = Arc::new(entries);
        self.numbering.outline_of = Some(key);
        Arc::clone(&self.numbering.outline)
    }

    /// What `\command{keys}` (a cross-reference or a citation) shows in the PDF, or why it is not
    /// known.
    pub fn shown(&mut self, command: &str, keys: &str) -> Result<String, Missing> {
        let style = self.numbered().cite_style.unwrap_or(CiteStyle::Plain);
        let aux = self.numbering.aux.as_deref();
        if texcraft_latex::view::is_citation(command) { Aux::citation(aux, style, command, keys) } else { Aux::reference(aux, command, keys) }
    }

    /// The project file `\input{name}` or `\include{name}` reads.
    pub fn included(&self, name: &str) -> Option<ProjectPath> {
        let main = self.project.main()?;
        let exists = |p: &str| ProjectPath::new(p).ok().is_some_and(|p| self.project.get(&p).is_some());
        numbers::resolve(main.as_str(), name, &exists).and_then(|p| ProjectPath::new(&p).ok())
    }
}

/// What the formatted view shows for a file's cross-references, citations and numbered parts
/// (`language.numbers`): each with its line (from 1).
pub(crate) fn report(s: &mut crate::Session, path: &ProjectPath) -> crate::Result<serde_json::Value> {
    use serde_json::json;
    use texcraft_latex::view::{self, BlockKind, ChipKind, InlineKind};
    let numbered = s.numbered();
    let text: Arc<str> = Arc::from(s.project().text(path).map_err(|e| e.to_string())?);
    let line_of = |byte: usize| text.get(..byte).map_or(0, |t| t.matches('\n').count()) + 1;
    let blocks = view::view_with(&text, &numbered.declared.theorems);
    let mut chips = Vec::new();
    let mut parts = Vec::new();
    view::walk(&blocks, &mut |b| {
        for i in view::inlines_of(b) {
            if let InlineKind::Chip { kind: ChipKind::Reference | ChipKind::Citation, command, key } = &i.kind {
                chips.push((i.range.start, command.clone(), key.clone()));
            }
        }
        let what = match &b.kind {
            BlockKind::Heading { .. } => "heading",
            BlockKind::Formula { .. } => "formula",
            BlockKind::Theorem { .. } => "theorem",
            BlockKind::Figure { .. } => "figure",
            BlockKind::Table(_) => "table",
            _ => return,
        };
        let number = match numbered.numbers.of(path.as_str(), b.range.start) {
            Some(texcraft_latex::numbers::Number::One(n)) => json!(n),
            Some(texcraft_latex::numbers::Number::Rows(rows)) => json!(rows),
            None => return,
        };
        parts.push(json!({"line": line_of(b.range.start), "kind": what, "number": number}));
    });
    let references: Vec<serde_json::Value> = chips
        .into_iter()
        .map(|(at, command, key)| {
            let mut v = json!({"line": line_of(at), "command": command, "key": key});
            match s.shown(&command, &key) {
                Ok(shown) => v["shown"] = json!(shown),
                Err(why) => v["missing"] = json!(missing_words(&why)),
            }
            v
        })
        .collect();
    Ok(json!({"built": s.aux().is_some(), "references": references, "numbered": parts}))
}

/// Why a reference shows no number, in words (English: the agent interface's language).
pub fn missing_words(why: &Missing) -> String {
    match why {
        Missing::NotBuilt => "not built yet".to_owned(),
        Missing::Unknown(key) => format!("no label or entry named {key} in the last build"),
        Missing::Biblatex => "biblatex's labels are not read yet".to_owned(),
        Missing::Silent => "\\nocite shows nothing in the text".to_owned(),
    }
}
