//! A TeXCraft project: a set of files addressed by relative paths, one of which is the main
//! document. The model is in memory and does no I/O; [`Project::load_dir`] and
//! [`Project::save_dir`] are the only functions that touch a file system, and they are not built
//! for the web, which keeps projects in browser storage.
//!
//! Paths in a project are always relative, use `/` and never leave the project: [`ProjectPath`]
//! refuses absolute paths, `..`, empty components and NUL, so a path read from a document or an
//! agent cannot name a file outside it.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::BTreeMap;
use std::fmt;

/// Largest file a project holds, in bytes. Bigger files are refused when added or loaded.
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
/// Most files a project holds.
pub const MAX_FILES: usize = 10_000;
/// Longest path, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("`{0}` is not a valid path in a project: {1}")]
    BadPath(String, &'static str),
    #[error("there is no file `{0}` in the project")]
    NoSuchFile(String),
    #[error("a file `{0}` already exists")]
    Exists(String),
    #[error("`{0}` is {1} bytes, more than the {MAX_FILE_BYTES} a project file may have")]
    TooLarge(String, usize),
    #[error("the project already has {MAX_FILES} files")]
    TooManyFiles,
    #[error("`{0}` is not a text file")]
    NotText(String),
    #[error("{0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A checked relative path inside a project, such as `chapters/intro.tex`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectPath(String);

impl ProjectPath {
    /// Checks `path` lexically. A leading `./` is dropped and `\` is not accepted as a separator,
    /// so a path means the same thing on every platform.
    pub fn new(path: &str) -> Result<Self> {
        let bad = |why| Error::BadPath(path.to_owned(), why);
        let trimmed = path.strip_prefix("./").unwrap_or(path);
        if trimmed.is_empty() {
            return Err(bad("it is empty"));
        }
        if trimmed.len() > MAX_PATH_BYTES {
            return Err(bad("it is too long"));
        }
        if trimmed.starts_with('/') || trimmed.contains(':') {
            return Err(bad("it is absolute"));
        }
        if trimmed.contains('\\') || trimmed.contains('\0') {
            return Err(bad("it contains a backslash or NUL"));
        }
        for part in trimmed.split('/') {
            match part {
                "" => return Err(bad("it has an empty component")),
                "." | ".." => return Err(bad("it has a `.` or `..` component")),
                _ => {}
            }
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last component, `intro.tex` for `chapters/intro.tex`.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The extension without the dot, lower-cased; empty when there is none.
    pub fn extension(&self) -> String {
        match self.file_name().rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
            _ => String::new(),
        }
    }
}

impl TryFrom<String> for ProjectPath {
    type Error = Error;
    fn try_from(s: String) -> Result<Self> {
        Self::new(&s)
    }
}

impl From<ProjectPath> for String {
    fn from(p: ProjectPath) -> String {
        p.0
    }
}

impl fmt::Display for ProjectPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The contents of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contents {
    /// UTF-8 text the editor can open.
    Text(String),
    /// Anything else: images, PDFs, fonts.
    Binary(Vec<u8>),
}

impl Contents {
    /// Text when `bytes` is UTF-8 and has no NUL, binary otherwise.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        if bytes.contains(&0) {
            return Contents::Binary(bytes);
        }
        match String::from_utf8(bytes) {
            Ok(s) => Contents::Text(s),
            Err(e) => Contents::Binary(e.into_bytes()),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Contents::Text(s) => s.as_bytes(),
            Contents::Binary(b) => b,
        }
    }

    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The files of a project and which one is compiled.
#[derive(Debug, Clone, Default)]
pub struct Project {
    files: BTreeMap<ProjectPath, Contents>,
    main: Option<ProjectPath>,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    /// A one-file project whose `main.tex` holds a short article, for a new blank project.
    pub fn blank() -> Self {
        let mut p = Self::new();
        // `main.tex` is a valid path, so neither call can fail; errors are ignored deliberately
        // rather than unwrapped.
        if let Ok(path) = ProjectPath::new("main.tex") {
            let _ = p.add(path.clone(), Contents::Text(BLANK_DOCUMENT.to_owned()));
            p.main = Some(path);
        }
        p
    }

    pub fn files(&self) -> impl Iterator<Item = (&ProjectPath, &Contents)> {
        self.files.iter()
    }

    pub fn paths(&self) -> impl Iterator<Item = &ProjectPath> {
        self.files.keys()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn get(&self, path: &ProjectPath) -> Option<&Contents> {
        self.files.get(path)
    }

    pub fn text(&self, path: &ProjectPath) -> Result<&str> {
        match self.files.get(path) {
            Some(Contents::Text(s)) => Ok(s),
            Some(Contents::Binary(_)) => Err(Error::NotText(path.to_string())),
            None => Err(Error::NoSuchFile(path.to_string())),
        }
    }

    /// Adds a new file; refuses to replace one.
    pub fn add(&mut self, path: ProjectPath, contents: Contents) -> Result<()> {
        if self.files.contains_key(&path) {
            return Err(Error::Exists(path.to_string()));
        }
        if self.files.len() >= MAX_FILES {
            return Err(Error::TooManyFiles);
        }
        check_size(&path, &contents)?;
        if self.main.is_none() && path.extension() == "tex" {
            self.main = Some(path.clone());
        }
        self.files.insert(path, contents);
        Ok(())
    }

    /// Replaces the contents of an existing file.
    pub fn set(&mut self, path: &ProjectPath, contents: Contents) -> Result<()> {
        check_size(path, &contents)?;
        match self.files.get_mut(path) {
            Some(slot) => {
                *slot = contents;
                Ok(())
            }
            None => Err(Error::NoSuchFile(path.to_string())),
        }
    }

    pub fn remove(&mut self, path: &ProjectPath) -> Result<Contents> {
        let removed = self.files.remove(path).ok_or_else(|| Error::NoSuchFile(path.to_string()))?;
        if self.main.as_ref() == Some(path) {
            self.main = self.files.keys().find(|p| p.extension() == "tex").cloned();
        }
        Ok(removed)
    }

    pub fn rename(&mut self, from: &ProjectPath, to: ProjectPath) -> Result<()> {
        if self.files.contains_key(&to) {
            return Err(Error::Exists(to.to_string()));
        }
        let contents = self.files.remove(from).ok_or_else(|| Error::NoSuchFile(from.to_string()))?;
        if self.main.as_ref() == Some(from) {
            self.main = Some(to.clone());
        }
        self.files.insert(to, contents);
        Ok(())
    }

    /// The document that is compiled.
    pub fn main(&self) -> Option<&ProjectPath> {
        self.main.as_ref()
    }

    /// Makes the main document `main.tex` when there is one, else the first `.tex` file with
    /// `\documentclass`, else the first `.tex` file; none when there is no `.tex` file.
    pub fn choose_main(&mut self) {
        let tex: Vec<&ProjectPath> = self.paths().filter(|p| p.extension() == "tex").collect();
        self.main = tex
            .iter()
            .find(|p| p.as_str() == "main.tex")
            .or_else(|| tex.iter().find(|p| matches!(self.get(p), Some(Contents::Text(t)) if t.contains("\\documentclass"))))
            .or_else(|| tex.first())
            .map(|p| (*p).clone());
    }

    pub fn set_main(&mut self, path: &ProjectPath) -> Result<()> {
        if !self.files.contains_key(path) {
            return Err(Error::NoSuchFile(path.to_string()));
        }
        self.main = Some(path.clone());
        Ok(())
    }
}

fn check_size(path: &ProjectPath, contents: &Contents) -> Result<()> {
    if contents.len() > MAX_FILE_BYTES {
        return Err(Error::TooLarge(path.to_string(), contents.len()));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
mod disk;

const BLANK_DOCUMENT: &str = "\\documentclass{article}\n\
\\title{Untitled}\n\
\\author{}\n\
\\begin{document}\n\
\\maketitle\n\
\n\
\\section{Introduction}\n\
\n\
\\end{document}\n";
