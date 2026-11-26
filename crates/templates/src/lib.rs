//! Project templates (docs/ROADMAP.md).
//!
//! The [`Catalog`] is TeXCraft's own list (`templates/catalog.toml`, compiled in): each template's
//! title, publisher, category, licence and where its files come from. The files are not bundled:
//! the [`Store`] downloads them from their source the first time they are needed, checks them, and
//! keeps them in the user's data folder, so a kept template is offered offline.
//!
//! Sources:
//! - **TeXCraft's own** (`builtin`): files in `templates/builtin/<id>/`, CC0-1.0, compiled in, so
//!   they need no download and work offline and in the browser.
//! - **TeX Live packages.** The template is part of a package's documentation. The store fetches
//!   TeX Live's package database, checks its SHA-512 against the signature made with TeX Live's
//!   keys (retex-texlive's `openpgp`), then the package's documentation archive against the
//!   database, and extracts only the template's files. The class itself is in TeX Live, which
//!   retex reads or downloads.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};

mod builtin;
#[cfg(all(feature = "download", not(target_arch = "wasm32")))]
mod download;

const CATALOG: &str = include_str!("../../../templates/catalog.toml");

/// Largest file a template may hold, in bytes.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("there is no template `{0}`")]
    Unknown(String),
    #[error("the template catalogue is damaged: {0}")]
    Catalog(String),
    #[error("{0}")]
    Io(String),
    #[error("downloading: {0}")]
    Download(String),
    #[error("the download failed its check: {0}")]
    Integrity(String),
    #[error("this build of TeXCraft cannot download templates")]
    NoDownloads,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Source {
    /// Files from the documentation of this TeX Live package.
    #[serde(rename = "texlive")]
    TexLive(String),
    /// TeXCraft's own template, compiled in (`templates/builtin/<the template's id>/`).
    #[serde(rename = "builtin")]
    Builtin(bool),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Template {
    pub id: String,
    pub title: String,
    /// What it is for, in a line (English; shown through the interface's `@msg` rows).
    #[serde(default)]
    pub description: String,
    pub publisher: String,
    /// journal, conference, thesis, report, cv, letter, slides, poster or book; `example` for the
    /// example project, which is not offered as a template.
    pub category: String,
    pub source: Source,
    /// Paths inside the source (for TeX Live, inside `doc/latex/<package>/`).
    pub files: Vec<String>,
    /// The file compiled, by its name in the project.
    pub main: String,
    /// The source's licence, as CTAN's catalogue gives it.
    pub licence: String,
    /// Other names a file has in other TeX Live years (`samples/sigconf.tex` was
    /// `samples/sample-sigconf.tex` in 2025); the file is kept under its name in `files`.
    #[serde(default)]
    pub aliases: std::collections::BTreeMap<String, Vec<String>>,
}

impl Template {
    /// Where each file goes in a project: relative to the folder of the main file's source.
    pub fn project_paths(&self) -> Vec<(String, String)> {
        let root = self
            .files
            .iter()
            .find(|f| f.rsplit('/').next() == Some(self.main.as_str()))
            .and_then(|f| f.rsplit_once('/'))
            .map(|(dir, _)| format!("{dir}/"));
        self.files
            .iter()
            .map(|f| {
                let rel = match &root {
                    Some(r) => f.strip_prefix(r.as_str()).unwrap_or_else(|| f.rsplit('/').next().unwrap_or(f)),
                    None => f.as_str(),
                };
                (f.clone(), rel.to_owned())
            })
            .collect()
    }

    /// The names `file` may have in the source: its own and its aliases.
    pub fn names_of<'a>(&'a self, file: &'a str) -> impl Iterator<Item = &'a str> {
        std::iter::once(file).chain(self.aliases.get(file).into_iter().flatten().map(String::as_str))
    }

    /// A short description of where it comes from, for the gallery.
    pub fn source_text(&self) -> String {
        match &self.source {
            Source::TexLive(package) => format!("TeX Live package {package}"),
            Source::Builtin(_) => "TeXCraft's own templates".to_owned(),
        }
    }

    /// Whether it is the example project the start page offers, not a template to start from.
    pub fn is_example(&self) -> bool {
        self.category == "example"
    }

    /// Whether its files are compiled in (no download needed).
    pub fn is_builtin(&self) -> bool {
        matches!(self.source, Source::Builtin(_))
    }

    /// A built-in template's files (path in the project, bytes); none for the others.
    pub fn builtin_files(&self) -> Vec<(String, Vec<u8>)> {
        if self.is_builtin() { builtin::files(&self.id) } else { Vec::new() }
    }
}

#[derive(Debug, serde::Deserialize)]
struct File {
    template: Vec<Template>,
}

/// TeXCraft's templates.
#[derive(Debug, Clone)]
pub struct Catalog {
    templates: Vec<Template>,
}

impl Catalog {
    /// The catalogue compiled into this build.
    pub fn builtin() -> Result<Catalog> {
        Self::parse(CATALOG)
    }

    pub fn parse(text: &str) -> Result<Catalog> {
        let file: File = toml::from_str(text).map_err(|e| Error::Catalog(e.to_string()))?;
        for t in &file.template {
            if !t.files.iter().any(|f| f.rsplit('/').next() == Some(t.main.as_str())) {
                return Err(Error::Catalog(format!("{}: its main file {} is not among its files", t.id, t.main)));
            }
            let names = t.files.iter().chain(t.aliases.values().flatten());
            if names.clone().any(|f| f.split('/').any(|part| part == ".." || part.is_empty())) || names.clone().any(|f| f.starts_with('/')) {
                return Err(Error::Catalog(format!("{}: a file path leaves its folder", t.id)));
            }
        }
        Ok(Catalog { templates: file.template })
    }

    pub fn templates(&self) -> &[Template] {
        &self.templates
    }

    pub fn find(&self, id: &str) -> Result<&Template> {
        self.templates.iter().find(|t| t.id == id).ok_or_else(|| Error::Unknown(id.to_owned()))
    }
}

/// Kept templates, in a folder of the user's data: `<dir>/<id>/` holds a template's files at their
/// project paths, and `<dir>/<id>/.complete` marks a finished download.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

const COMPLETE: &str = ".complete";

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder of `template`'s files when it has been downloaded.
    pub fn kept(&self, template: &Template) -> Option<PathBuf> {
        let dir = self.dir.join(&template.id);
        dir.join(COMPLETE).is_file().then_some(dir)
    }

    /// Whether `template` can be used without a download: kept, or built in.
    pub fn available(&self, template: &Template) -> bool {
        template.is_builtin() || self.kept(template).is_some()
    }

    /// The folder of `template`'s files, downloading them first when they are not kept.
    pub fn fetch(&self, template: &Template) -> Result<PathBuf> {
        if let Some(dir) = self.kept(template) {
            return Ok(dir);
        }
        // Built in: written from the build, nothing to download.
        if template.is_builtin() {
            return self.keep(template, &template.builtin_files());
        }
        #[cfg(all(feature = "download", not(target_arch = "wasm32")))]
        {
            download::fetch(self, template)
        }
        #[cfg(not(all(feature = "download", not(target_arch = "wasm32"))))]
        {
            Err(Error::NoDownloads)
        }
    }

    /// Writes `files` (project path, bytes) as `template`'s kept copy, with a note of its source
    /// and licence, replacing an earlier copy.
    pub fn keep(&self, template: &Template, files: &[(String, Vec<u8>)]) -> Result<PathBuf> {
        let io = |e: std::io::Error, p: &Path| Error::Io(format!("{}: {e}", p.display()));
        let dir = self.dir.join(&template.id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| io(e, &dir))?;
        }
        for (path, bytes) in files {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(e, parent))?;
            }
            std::fs::write(&target, bytes).map_err(|e| io(e, &target))?;
        }
        let whose = if template.is_builtin() { "" } else { " (CTAN's catalogue)" };
        let note = format!(
            "{}\nfrom the {}, under {}{whose}.\nKept by TeXCraft; this notice is copied into projects made from it.\n",
            template.title,
            template.source_text(),
            template.licence
        );
        std::fs::write(dir.join("TEMPLATE-LICENSE"), note).map_err(|e| io(e, &dir))?;
        std::fs::write(dir.join(COMPLETE), b"").map_err(|e| io(e, &dir))?;
        Ok(dir)
    }

    /// Forgets `template`'s kept copy.
    pub fn forget(&self, template: &Template) -> Result<()> {
        let dir = self.dir.join(&template.id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        }
        Ok(())
    }
}
