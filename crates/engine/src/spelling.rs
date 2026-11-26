//! Spell checking (docs/ROADMAP.md, E11): the words of a file's prose
//! ([`texcraft_latex::prose`]) checked against a [`Dictionary`], the user's words and the
//! project's (`.texcraft/words.txt`).
//!
//! The dictionary is the suite's proofing library, craft-proof ([`ProofDictionary`]): English
//! always (its built-in word list, or SCOWL's when the build embeds craft-i18n's language data),
//! other languages from installed Hunspell dictionaries or language packs on the desktop. A
//! language without one is reported, never checked against another language.
//!
//! A file's words are found again only when its text changes; each word's verdict is kept, so
//! checking the lines in view as they scroll costs a lookup per new word.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use texcraft_project::ProjectPath;

use crate::{Result, Session};

/// The project's own word list, in its folder.
pub const PROJECT_WORDS: &str = ".texcraft/words.txt";

/// The longest word kept in a word list, in characters.
const MAX_WORD_CHARS: usize = 64;

/// Words whose verdict is kept, at most, before the cache starts again.
const MAX_KNOWN: usize = 200_000;

/// Where spelling words come from.
pub trait Dictionary: Send {
    /// Why words in `lang` (a BCP 47 tag) cannot be checked, if they cannot.
    fn available(&self, lang: &str) -> std::result::Result<(), String>;
    /// Whether `word` is spelled right in `lang`.
    fn check(&self, word: &str, lang: &str) -> bool;
    /// Spellings to offer for `word` in `lang`, best first, at most `max`.
    fn suggest(&self, word: &str, lang: &str, max: usize) -> Vec<String>;
}

/// The suite's proofing library (craft-proof).
pub struct ProofDictionary;

impl ProofDictionary {
    fn lang(tag: &str) -> craft_proof::ProofLang {
        craft_proof::ProofLang::from_tag(Some(tag))
    }
}

impl Dictionary for ProofDictionary {
    fn available(&self, lang: &str) -> std::result::Result<(), String> {
        let l = Self::lang(lang);
        if l != craft_proof::ProofLang::Other && l.is_proofed() {
            return Ok(());
        }
        if let Some((name, _, reason)) = craft_proof::unreadable_dictionary(Some(lang)) {
            return Err(format!("There is no usable spelling dictionary for {lang}: {name} could not be read ({reason})."));
        }
        let hint = if cfg!(target_arch = "wasm32") {
            "the browser has English only".to_owned()
        } else {
            match craft_proof::missing_dictionary(Some(lang)) {
                Some(name) => format!("install the Hunspell dictionary {name} (.aff and .dic) to check it"),
                None => "none is installed".to_owned(),
            }
        };
        Err(format!("There is no spelling dictionary for {lang}: {hint}."))
    }

    fn check(&self, word: &str, lang: &str) -> bool {
        craft_proof::is_correct_in(word, Self::lang(lang))
    }

    fn suggest(&self, word: &str, lang: &str, max: usize) -> Vec<String> {
        craft_proof::suggest_in(word, max, Self::lang(lang))
    }
}

/// The spelling settings (Settings › Editor), kept with the preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpellingOptions {
    pub on: bool,
    /// A BCP 47 tag, or `auto`: the document's (babel, polyglossia), else the interface's.
    pub language: String,
    /// Check the text of comments too.
    pub comments: bool,
}

impl Default for SpellingOptions {
    fn default() -> Self {
        Self { on: true, language: "auto".to_owned(), comments: false }
    }
}

/// A misspelled word: its bytes, and its line and columns in characters from 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Misspelling {
    pub range: Range<usize>,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub word: String,
}

/// Where the language checked comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageFrom {
    /// Chosen in the settings.
    Setting,
    /// babel's or polyglossia's, in the main file.
    Document,
    /// The interface's.
    Interface,
}

/// A word of a file's prose: its bytes, line and columns.
#[derive(Debug, Clone)]
struct Word {
    range: Range<usize>,
    line: usize,
    column: usize,
    end_column: usize,
}

/// The words of a file at a session revision, with the text's hash and the options they were
/// found with.
struct FileWords {
    revision: u64,
    hash: u64,
    comments: bool,
    words: Arc<Vec<Word>>,
}

pub(crate) struct State {
    dictionary: Box<dyn Dictionary>,
    /// The user's words (Add to Dictionary), for every project.
    user: BTreeSet<String>,
    /// Words ignored for the session (Ignore).
    ignored: HashSet<String>,
    /// The project's words: read from its folder (`None` without one), with the folder they are
    /// of; without a folder they live in the session.
    project: Option<(Option<PathBuf>, BTreeSet<String>)>,
    /// The interface's language, when the document names none.
    pub(crate) fallback: String,
    files: HashMap<ProjectPath, FileWords>,
    /// The dictionary's verdict by language and word.
    known: HashMap<(String, String), bool>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            dictionary: Box::new(ProofDictionary),
            user: BTreeSet::new(),
            ignored: HashSet::new(),
            project: None,
            fallback: "en".to_owned(),
            files: HashMap::new(),
            known: HashMap::new(),
        }
    }
}

/// A word as word lists keep it: trimmed, lower case.
fn key(word: &str) -> String {
    word.trim().to_lowercase()
}

/// A word that can go in a word list: letters (and `'`, `-`), no space, not too long.
fn valid(word: &str) -> std::result::Result<String, String> {
    let w = word.trim();
    if w.is_empty()
        || w.chars().count() > MAX_WORD_CHARS
        || w.chars().any(|c| c.is_whitespace() || c.is_control())
        || !w.chars().any(char::is_alphabetic)
    {
        return Err(format!("“{w}” is not a word that can go in a word list: one word of letters, at most {MAX_WORD_CHARS}."));
    }
    Ok(w.to_owned())
}

/// The words of a word list file: one per line, `#` starting a comment line.
fn parse_words(text: &str) -> BTreeSet<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(|l| valid(l).ok()).map(|w| key(&w)).collect()
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The words of `text`'s prose, with their lines and columns.
fn find_words(text: &str, comments: bool) -> Vec<Word> {
    let mut out = Vec::new();
    // Lines and columns are counted forward once, as the words come in order.
    let (mut at, mut line, mut column) = (0usize, 0usize, 0usize);
    for r in texcraft_latex::prose::ranges(text, texcraft_latex::prose::Options { comments }) {
        let Some(piece) = text.get(r.clone()) else { continue };
        for (a, b) in craft_proof::words(piece) {
            let (start, end) = (r.start + a, r.start + b);
            let Some(before) = text.get(at..start) else { continue };
            for c in before.chars() {
                if c == '\n' {
                    line += 1;
                    column = 0;
                } else {
                    column += 1;
                }
            }
            let len = text.get(start..end).map_or(0, |w| w.chars().count());
            out.push(Word { range: start..end, line, column, end_column: column + len });
            at = start;
        }
    }
    out
}

impl Session {
    /// Uses `dictionary` for spelling (a fake one in tests, another library elsewhere).
    pub fn set_dictionary(&mut self, dictionary: Box<dyn Dictionary>) {
        self.spelling.dictionary = dictionary;
        self.spelling.known.clear();
    }

    /// The interface's language, checked when the document names none (`en` until set).
    pub fn set_interface_language(&mut self, tag: &str) {
        if self.spelling.fallback != tag {
            self.spelling.fallback = tag.to_owned();
        }
    }

    /// The user's words, sorted (the shell keeps them with the preferences).
    pub fn user_words(&self) -> Vec<String> {
        self.spelling.user.iter().cloned().collect()
    }

    /// Restores the user's words kept with the preferences.
    pub fn set_user_words(&mut self, words: &[String]) {
        self.spelling.user = words.iter().filter_map(|w| valid(w).ok()).map(|w| key(&w)).collect();
    }

    /// The language checked and where it comes from.
    pub fn spelling_language(&self) -> (String, LanguageFrom) {
        let chosen = self.settings.spelling.language.trim();
        if !chosen.is_empty() && chosen != "auto" {
            return (chosen.to_owned(), LanguageFrom::Setting);
        }
        self.automatic_spelling_language()
    }

    /// The language checked when none is chosen (`auto`): the document's, else the interface's.
    pub fn automatic_spelling_language(&self) -> (String, LanguageFrom) {
        let main = self.project.main().and_then(|m| self.project.text(m).ok());
        match main.and_then(texcraft_latex::prose::document_language) {
            Some(tag) => (tag.to_owned(), LanguageFrom::Document),
            None => (self.spelling.fallback.clone(), LanguageFrom::Interface),
        }
    }

    /// Why spelling is not checked now, if it is not: it is off, or the language has no
    /// dictionary.
    pub fn spelling_ready(&self) -> Result<String> {
        if !self.settings.spelling.on {
            return Err("Spelling is off (Settings › Editor).".to_owned());
        }
        let (lang, _) = self.spelling_language();
        self.spelling.dictionary.available(&lang)?;
        Ok(lang)
    }

    /// Why words in `lang` (a BCP 47 tag) cannot be checked, if they cannot: no dictionary.
    pub fn spelling_available(&self, lang: &str) -> Result<()> {
        self.spelling.dictionary.available(lang)
    }

    /// The project's words: from `.texcraft/words.txt` in its folder (read once per folder), or
    /// the session's when it has none.
    fn project_words(&mut self) -> &BTreeSet<String> {
        let dir = self.dir.clone().filter(|_| self.library.is_none());
        let stale = self.spelling.project.as_ref().is_none_or(|(d, _)| *d != dir);
        if stale {
            let words = dir
                .as_ref()
                .map(|d| d.join(PROJECT_WORDS))
                .filter(|f| self.roots.as_ref().is_none_or(|r| r.check(f, crate::file_access::Access::Read).is_ok()))
                .and_then(|f| std::fs::read_to_string(f).ok())
                .map(|t| parse_words(&t))
                .unwrap_or_default();
            self.spelling.project = Some((dir, words));
        }
        static EMPTY: BTreeSet<String> = BTreeSet::new();
        self.spelling.project.as_ref().map_or(&EMPTY, |(_, w)| w)
    }

    /// The words of `path`'s prose, found again when its text changed.
    fn prose_words(&mut self, path: &ProjectPath) -> Result<Arc<Vec<Word>>> {
        let comments = self.settings.spelling.comments;
        let revision = self.revision;
        if let Some(f) = self.spelling.files.get(path)
            && f.revision == revision
            && f.comments == comments
        {
            return Ok(Arc::clone(&f.words));
        }
        let text = self.project.text(path).map_err(|e| e.to_string())?;
        let h = hash(text);
        if let Some(f) = self.spelling.files.get_mut(path)
            && f.hash == h
            && f.comments == comments
        {
            f.revision = revision;
            return Ok(Arc::clone(&f.words));
        }
        let words = Arc::new(find_words(text, comments));
        self.spelling.files.insert(path.clone(), FileWords { revision, hash: h, comments, words: Arc::clone(&words) });
        Ok(words)
    }

    /// The misspelled words of `path` on lines `lines` (from 0, end excluded; all without), in
    /// order. An error says why nothing is checked: spelling is off, or the language has no
    /// dictionary.
    pub fn misspellings(&mut self, path: &ProjectPath, lines: Option<Range<usize>>) -> Result<Vec<Misspelling>> {
        let lang = self.spelling_ready()?;
        let words = self.prose_words(path)?;
        let from = lines.as_ref().map_or(0, |l| words.partition_point(|w| w.line < l.start));
        let project: BTreeSet<String> = self.project_words().clone();
        let text = self.project.text(path).map_err(|e| e.to_string())?;
        let s = &mut self.spelling;
        if s.known.len() > MAX_KNOWN {
            s.known.clear();
        }
        let mut out = Vec::new();
        for w in words[from..].iter().take_while(|w| lines.as_ref().is_none_or(|l| w.line < l.end)) {
            let Some(word) = text.get(w.range.clone()) else { continue };
            let k = key(word);
            if s.user.contains(&k) || project.contains(&k) || s.ignored.contains(&k) {
                continue;
            }
            let dictionary = &s.dictionary;
            let right = *s.known.entry((lang.clone(), word.to_owned())).or_insert_with(|| dictionary.check(word, &lang));
            if !right {
                out.push(Misspelling { range: w.range.clone(), line: w.line, column: w.column, end_column: w.end_column, word: word.to_owned() });
            }
        }
        Ok(out)
    }

    /// Spellings to offer for `word`, best first, at most `max`.
    pub fn spelling_suggestions(&mut self, word: &str, max: usize) -> Vec<String> {
        let (lang, _) = self.spelling_language();
        self.spelling.dictionary.suggest(word, &lang, max)
    }

    /// Adds `word` to the user's words, or to the project's (`to_project`: in
    /// `.texcraft/words.txt` of its folder, or for the session without one). Says where it went.
    pub fn add_word(&mut self, word: &str, to_project: bool) -> Result<&'static str> {
        let word = valid(word)?;
        if !to_project {
            self.spelling.user.insert(key(&word));
            return Ok("user");
        }
        let mut words = self.project_words().clone();
        words.insert(key(&word));
        let dir = self.dir.clone().filter(|_| self.library.is_none());
        let kept = match &dir {
            Some(d) => {
                let file = d.join(PROJECT_WORDS);
                if let Some(roots) = &self.roots {
                    roots.check(&file, crate::file_access::Access::Write)?;
                }
                // Lines that are not words (comments) are kept as they were.
                let old = std::fs::read_to_string(&file).unwrap_or_default();
                let comments: Vec<&str> = old.lines().filter(|l| l.trim().starts_with('#')).collect();
                let mut text = comments.join("\n");
                if !text.is_empty() {
                    text.push('\n');
                }
                for w in &words {
                    text.push_str(w);
                    text.push('\n');
                }
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("Could not make {}: {e}", parent.display()))?;
                }
                std::fs::write(&file, text).map_err(|e| format!("Could not write {}: {e}", file.display()))?;
                "project"
            }
            None => "session",
        };
        self.spelling.project = Some((dir, words));
        Ok(kept)
    }

    /// Ignores `word` everywhere until the session ends.
    pub fn ignore_word(&mut self, word: &str) -> Result<()> {
        let word = valid(word)?;
        self.spelling.ignored.insert(key(&word));
        Ok(())
    }
}
