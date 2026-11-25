//! Bringing many files into a project at once: a folder on disk, a zip file, or the files the
//! browser read from a dropped folder (`file.import`, `file.unzip`, `project.import`).
//!
//! Whatever the source, the same rules apply: hidden entries (`.git`, `.DS_Store`, any name
//! starting with a dot, a Mac zip's `__MACOSX`) and build outputs (`.aux`, `.log`, …) are left
//! out, a link that leaves the folder is not followed, a zip's names must stay inside the project
//! (no `..`, no absolute path or drive letter), and limits on the number of files and their total
//! size bound the work. What is left out is reported with its reason, never dropped silently
//! (AGENTS.md rule 15).

use serde::Serialize;
use serde_json::{Value, json};
use texcraft_project::{Contents, MAX_FILE_BYTES, ProjectPath};

use crate::Session;

/// What one import may bring in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most files.
    pub files: usize,
    /// Most bytes, all files together.
    pub bytes: u64,
    /// Largest ratio of a zip entry's size to its compressed size, for entries bigger than
    /// [`RATIO_FLOOR`]: more is taken for a zip bomb and the zip is refused.
    pub ratio: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { files: 5_000, bytes: 500 * 1024 * 1024, ratio: 200 }
    }
}

/// Entries smaller than this are not judged by their compression ratio: a small file of one
/// repeated character compresses very well and is harmless.
pub const RATIO_FLOOR: u64 = 1024 * 1024;

/// Deepest folder nesting read from disk.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DEPTH: usize = 32;

/// Endings of files a LaTeX build writes, which a project does not need.
const BUILD_OUTPUTS: &[&str] = &[
    ".aux",
    ".log",
    ".synctex.gz",
    ".synctex",
    ".fls",
    ".fdb_latexmk",
    ".out",
    ".toc",
    ".lof",
    ".lot",
    ".blg",
    ".bcf",
    ".run.xml",
    ".nav",
    ".snm",
    ".vrb",
    ".xdv",
    ".ilg",
];

/// Why a file was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// A hidden entry (its name starts with a dot) or a system's own (`__MACOSX`, `Thumbs.db`).
    Hidden,
    /// A file a build writes.
    BuildOutput,
    /// A symbolic link: in a zip, or on disk one that leads out of the folder or to a folder.
    Link,
    /// A zip name that would leave the project (`..`, an absolute path, a drive letter).
    UnsafeName,
    /// Bigger than a project file may be.
    TooLarge,
    /// Past the limit on the number of files.
    TooMany,
    /// Past the limit on the total size.
    TotalTooLarge,
    /// Nested deeper than folders are read.
    TooDeep,
    /// A file of that path is in the project already, and the import was told to skip those.
    Exists,
    /// Its path is a folder in the project, or inside a file of it.
    Clash,
    /// It could not be read (a damaged or encrypted zip entry, a file that went away, a name
    /// that is not text).
    Unreadable,
    /// An agent session may not read it (outside its roots, or with more than one hard link).
    Refused,
}

/// A file left out, by its path in the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: Skip,
}

/// Files gathered from a source, with the ones left out.
#[derive(Debug, Default, Clone)]
pub struct Gathered {
    /// Paths (with `/`, relative) and contents.
    pub files: Vec<(String, Vec<u8>)>,
    pub skipped: Vec<Skipped>,
    /// The folder every file of a zip was in, taken off their paths.
    pub stripped: Option<String>,
    bytes: u64,
    limits: Limits,
}

/// Why `name`, one component of a path, is left out wherever it is.
fn hidden(name: &str) -> bool {
    name.starts_with('.') || name == "__MACOSX" || name.eq_ignore_ascii_case("Thumbs.db") || name.eq_ignore_ascii_case("desktop.ini")
}

/// Why the file at `path` (relative, with `/`) is left out by its name, if it is.
pub fn skip_reason(path: &str) -> Option<Skip> {
    if path.split('/').any(hidden) {
        return Some(Skip::Hidden);
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    BUILD_OUTPUTS.iter().any(|end| name.len() > end.len() && name.ends_with(end)).then_some(Skip::BuildOutput)
}

/// `name` from an archive or a browser as a relative path with `/`, or `None` when it would leave
/// the folder it is unpacked into: `..`, an absolute path, a drive letter, a NUL. A `\` is taken
/// as a separator (some Windows tools write them) and `.` components are dropped.
pub fn safe_path(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains('\0') || name.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl Gathered {
    pub fn new(limits: Limits) -> Self {
        Gathered { limits, ..Gathered::default() }
    }

    fn skip(&mut self, path: impl Into<String>, reason: Skip) {
        self.skipped.push(Skipped { path: path.into(), reason });
    }

    /// Whether a file of `size` bytes at `path` may be read: its name is not left out and it fits
    /// the limits. A refusal is recorded.
    fn admit(&mut self, path: &str, size: u64) -> bool {
        let reason = if let Some(reason) = skip_reason(path) {
            Some(reason)
        } else if size > MAX_FILE_BYTES as u64 {
            Some(Skip::TooLarge)
        } else if self.files.len() >= self.limits.files {
            Some(Skip::TooMany)
        } else if self.bytes.saturating_add(size) > self.limits.bytes {
            Some(Skip::TotalTooLarge)
        } else {
            None
        };
        match reason {
            Some(reason) => {
                self.skip(path, reason);
                false
            }
            None => true,
        }
    }

    /// One file the user named: it is not filtered.
    pub fn one(path: String, bytes: Vec<u8>) -> Self {
        let mut g = Gathered::new(Limits::default());
        g.push(path, bytes);
        g
    }

    fn push(&mut self, path: String, bytes: Vec<u8>) {
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
        self.files.push((path, bytes));
    }

    /// Files given by path and contents (the browser's dropped folder), with the same filter and
    /// limits; with `strip`, the one folder all of them are in is taken off their paths.
    pub fn from_files(files: Vec<(String, Vec<u8>)>, limits: Limits, strip: bool) -> Self {
        let mut g = Gathered::new(limits);
        let mut kept = Vec::new();
        for (name, bytes) in files {
            match safe_path(&name) {
                None => g.skip(name, Skip::UnsafeName),
                Some(path) => match skip_reason(&path) {
                    Some(reason) => g.skip(path, reason),
                    None => kept.push((path, bytes)),
                },
            }
        }
        if strip {
            g.stripped = strip_common_folder(kept.iter_mut().map(|(p, _)| p));
        }
        for (path, bytes) in kept {
            if g.admit(&path, bytes.len() as u64) {
                g.push(path, bytes);
            }
        }
        g
    }

    /// The files of zip `bytes`. Directory entries are not files; symbolic links and names that
    /// would leave the project are left out; the one folder every file is in, if any, is taken
    /// off their paths. A zip that declares more entries than can be imported, or one whose
    /// entries expand far more than files do (a zip bomb), is refused whole.
    pub fn from_zip(bytes: &[u8], limits: Limits) -> Result<Self, String> {
        // Folders are entries too: room for one per file.
        let max_entries = (limits.files as u64).saturating_mul(2).saturating_add(64);
        let mut zip = craft_zip::Archive::open(bytes, craft_zip::Limits { max_entries, max_total_bytes: limits.bytes }).map_err(|e| match e {
            craft_zip::Error::TooManyEntries { declared, .. } => {
                format!("The zip file has {declared} entries, more than the {} files an import may bring in.", limits.files)
            }
            e => format!("This is not a zip file TeXCraft can read: {e}"),
        })?;
        let mut g = Gathered::new(limits);
        let mut entries = Vec::new();
        for index in 0..zip.len() {
            let name = |e: craft_zip::Error| e.to_string();
            let Some(raw) = zip.entry_name(index).map_err(name)? else { continue };
            if zip.entry_is_dir(index).map_err(name)? == Some(true) {
                continue;
            }
            if zip.entry_is_symlink(index).map_err(name)? == Some(true) {
                g.skip(raw, Skip::Link);
                continue;
            }
            let Some(path) = safe_path(&raw) else {
                g.skip(raw, Skip::UnsafeName);
                continue;
            };
            if let Some(reason) = skip_reason(&path) {
                g.skip(path, reason);
                continue;
            }
            let (size, compressed) = zip.entry_sizes(index).map_err(name)?.unwrap_or((0, 0));
            if size > RATIO_FLOOR && size / compressed.max(1) > limits.ratio {
                return Err(format!(
                    "{path} in the zip file would expand {} times, more than files do: the zip file is refused as a likely zip bomb.",
                    size / compressed.max(1)
                ));
            }
            entries.push((index, path, size));
        }
        g.stripped = strip_common_folder(entries.iter_mut().map(|(_, p, _)| p));
        for (index, path, size) in entries {
            if !g.admit(&path, size) {
                continue;
            }
            match zip.read_index(index, MAX_FILE_BYTES as u64) {
                Ok(Some(data)) => g.push(path, data),
                Ok(None) => g.skip(path, Skip::Unreadable),
                Err(craft_zip::Error::EntryTooLarge { .. }) => g.skip(path, Skip::TooLarge),
                Err(craft_zip::Error::BudgetSpent { .. }) => g.skip(path, Skip::TotalTooLarge),
                Err(_) => g.skip(path, Skip::Unreadable),
            }
        }
        Ok(g)
    }

    /// In the browser there are no folders on disk.
    #[cfg(target_arch = "wasm32")]
    pub fn from_folder(_: &std::path::Path, _: Limits, _: bool, _: Option<&crate::file_access::AutomationRoots>) -> Result<Self, String> {
        Err("Folders on disk are not available in the browser.".to_owned())
    }

    /// The files under folder `dir`, their paths starting with the folder's own name when
    /// `named`. Links to files inside the folder are read; links to folders, or out of it, are
    /// not. In an agent session each file is checked against its roots.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_folder(
        dir: &std::path::Path,
        limits: Limits,
        named: bool,
        roots: Option<&crate::file_access::AutomationRoots>,
    ) -> Result<Self, String> {
        let root = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let prefix = match root.file_name().and_then(|n| n.to_str()) {
            Some(name) if named => name.to_owned(),
            Some(_) => String::new(),
            None if named => return Err(format!("{} has no name to give its folder in the project", dir.display())),
            None => String::new(),
        };
        let mut g = Gathered::new(limits);
        g.walk(&root, &root, &prefix, 0, roots);
        Ok(g)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn walk(
        &mut self,
        root: &std::path::Path,
        dir: &std::path::Path,
        prefix: &str,
        depth: usize,
        roots: Option<&crate::file_access::AutomationRoots>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            self.skip(prefix, Skip::Unreadable);
            return;
        };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                self.skip(format!("{prefix}/{}", name.to_string_lossy()), Skip::Unreadable);
                continue;
            };
            let rel = if prefix.is_empty() { name.to_owned() } else { format!("{prefix}/{name}") };
            if let Some(reason) = skip_reason(name) {
                self.skip(rel, reason);
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                self.skip(rel, Skip::Unreadable);
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth >= MAX_DEPTH {
                    self.skip(rel, Skip::TooDeep);
                } else {
                    self.walk(root, &path, &rel, depth + 1, roots);
                }
                continue;
            }
            if kind.is_symlink() {
                // A link to a file of the folder is that file; any other is not followed.
                let inside = std::fs::canonicalize(&path).ok().filter(|t| t.starts_with(root) && t.is_file());
                if inside.is_none() {
                    self.skip(rel, Skip::Link);
                    continue;
                }
            } else if !kind.is_file() {
                continue;
            }
            if let Some(roots) = roots
                && roots.check(&path, crate::file_access::Access::Read).is_err()
            {
                self.skip(rel, Skip::Refused);
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if !self.admit(&rel, size) {
                continue;
            }
            match std::fs::read(&path) {
                // The file may have grown since it was measured.
                Ok(bytes) if bytes.len() > MAX_FILE_BYTES => self.skip(rel, Skip::TooLarge),
                Ok(bytes) => self.push(rel, bytes),
                Err(_) => self.skip(rel, Skip::Unreadable),
            }
        }
    }
}

/// Takes the first component off every path when they all share it and none is at the top
/// level; returns it.
fn strip_common_folder<'a>(paths: impl Iterator<Item = &'a mut String>) -> Option<String> {
    let mut paths: Vec<&mut String> = paths.collect();
    let first = paths.first()?.split_once('/')?.0.to_owned();
    let inside = format!("{first}/");
    if !paths.iter().all(|p| p.starts_with(&inside)) {
        return None;
    }
    for p in &mut paths {
        p.replace_range(..inside.len(), "");
    }
    Some(first)
}

/// What to do with a file whose path is in the project already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existing {
    /// Its contents are replaced (what adding one file has always done).
    Replace,
    /// The new one gets a free name beside it (`figure-2.png`).
    KeepBoth,
    /// The new one is left out.
    Skip,
}

impl Existing {
    pub(crate) fn from_params(p: &Value) -> crate::Result<Self> {
        match p.get("existing").and_then(Value::as_str) {
            None | Some("replace") => Ok(Existing::Replace),
            Some("keep_both") => Ok(Existing::KeepBoth),
            Some("skip") => Ok(Existing::Skip),
            Some(other) => Err(format!("`existing` is `replace`, `keep_both` or `skip`, not `{other}`")),
        }
    }
}

/// The JSON schema of the parameters every import shares.
pub(crate) fn shared_properties() -> serde_json::Map<String, Value> {
    let v = json!({
        "into": {"type": "string", "description": "The folder of the project to put them in (default: the top level)."},
        "existing": {"type": "string", "enum": ["replace", "keep_both", "skip"], "description": "For a file whose path is in the project already: replace it (default), keep both (the new one gets a free name such as `figure-2.png`), or skip the new one."},
        "check": {"type": "boolean", "description": "Only say what the import would do: the files it would bring in, those already in the project, and those it would leave out. Nothing changes."}
    });
    v.as_object().cloned().unwrap_or_default()
}

/// The folder of the project files go into, from `into`.
pub(crate) fn into(p: &Value) -> crate::Result<Option<ProjectPath>> {
    match p.get("into").and_then(Value::as_str).map(str::trim).map(|s| s.trim_matches('/')) {
        None | Some("") => Ok(None),
        Some(folder) => ProjectPath::new(folder).map(Some).map_err(|e| e.to_string()),
    }
}

fn target(into: Option<&ProjectPath>, path: &str) -> Result<ProjectPath, texcraft_project::Error> {
    match into {
        Some(folder) => ProjectPath::new(&format!("{folder}/{path}")),
        None => ProjectPath::new(path),
    }
}

/// Whether `path` is a folder of the project or inside one of its files.
fn clashes(s: &Session, path: &ProjectPath) -> bool {
    let inside = format!("{path}/");
    s.folders.contains(path) || s.project.paths().any(|f| f.as_str().starts_with(&inside) || path.as_str().starts_with(&format!("{f}/")))
}

/// A free name beside `path`: `stem-2.ext`, `stem-3.ext`, …
fn free_name(s: &Session, path: &ProjectPath, taken: &std::collections::BTreeSet<ProjectPath>) -> Option<ProjectPath> {
    let (dir, name) = match path.as_str().rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name),
        None => (String::new(), path.as_str()),
    };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..10_000)
        .filter_map(|n| ProjectPath::new(&format!("{dir}{stem}-{n}{ext}")).ok())
        .find(|p| s.project.get(p).is_none() && !taken.contains(p) && !clashes(s, p))
}

/// What [`apply`] did, for the command's result.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// Paths in the project of the files added or replaced.
    pub files: Vec<String>,
    pub added: usize,
    pub replaced: usize,
    /// Files kept beside one of the same name: their path in the source and in the project.
    pub renamed: Vec<Value>,
    pub skipped: Vec<Skipped>,
    /// The folder of a zip taken off its files' paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// The paths of the project `g`'s files would replace.
pub(crate) fn existing(s: &Session, into: Option<&ProjectPath>, g: &Gathered) -> Vec<String> {
    g.files.iter().filter_map(|(path, _)| target(into, path).ok()).filter(|p| s.project.get(p).is_some()).map(String::from).collect()
}

/// What an import with `check` answers: the files it would bring in, those already in the project
/// and those it would leave out.
pub(crate) fn check(s: &Session, into: Option<&ProjectPath>, g: &Gathered) -> Value {
    let files: Vec<String> = g.files.iter().filter_map(|(path, _)| target(into, path).ok()).map(String::from).collect();
    json!({"files": files, "existing": existing(s, into, g), "skipped": g.skipped, "folder": g.stripped})
}

/// Adds `g`'s files to the project under `into`, deciding by `existing` for paths already there.
pub(crate) fn apply(s: &mut Session, into: Option<&ProjectPath>, g: Gathered, existing: Existing) -> Report {
    let mut report = Report { skipped: g.skipped, folder: g.stripped, ..Report::default() };
    let mut taken = std::collections::BTreeSet::new();
    for (source, bytes) in g.files {
        let Ok(mut path) = target(into, &source) else {
            report.skipped.push(Skipped { path: source, reason: Skip::UnsafeName });
            continue;
        };
        let contents = Contents::from_bytes(bytes);
        let exists = s.project.get(&path).is_some();
        if exists && existing == Existing::Skip {
            report.skipped.push(Skipped { path: source, reason: Skip::Exists });
            continue;
        }
        if exists && existing == Existing::KeepBoth {
            match free_name(s, &path, &taken) {
                Some(free) => {
                    report.renamed.push(json!({"from": source, "to": free.as_str()}));
                    path = free;
                }
                None => {
                    report.skipped.push(Skipped { path: source, reason: Skip::Exists });
                    continue;
                }
            }
        }
        let result = if s.project.get(&path).is_some() {
            s.project.set(&path, contents).map(|()| report.replaced += 1)
        } else if clashes(s, &path) {
            report.skipped.push(Skipped { path: source, reason: Skip::Clash });
            continue;
        } else {
            s.project.add(path.clone(), contents).map(|()| report.added += 1)
        };
        match result {
            Ok(()) => {
                taken.insert(path.clone());
                report.files.push(path.into());
            }
            Err(texcraft_project::Error::TooManyFiles) => report.skipped.push(Skipped { path: source, reason: Skip::TooMany }),
            Err(texcraft_project::Error::TooLarge(..)) => report.skipped.push(Skipped { path: source, reason: Skip::TooLarge }),
            Err(_) => report.skipped.push(Skipped { path: source, reason: Skip::Unreadable }),
        }
    }
    if !report.files.is_empty() {
        s.touch();
    }
    report
}
