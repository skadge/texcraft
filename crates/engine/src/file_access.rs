//! Confined file access for agent sessions (RFC-0015, AGENTS.md rule 18).
//!
//! An agent entry point refuses to start without at least one read root and one write root. Each
//! path is resolved first (symbolic links followed, `..` removed by the file system) and then
//! compared component by component with the roots; a file with more than one hard link is refused,
//! since another name of it may lie outside the roots. Reads are allowed under read and write
//! roots, writes only under write roots. Relative paths resolve against the first write root.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub struct AutomationRoots {
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
}

impl AutomationRoots {
    /// Both lists must be non-empty and every root an existing folder.
    pub fn new(read: &[PathBuf], write: &[PathBuf]) -> Result<Self, String> {
        if read.is_empty() || write.is_empty() {
            return Err("both --automation-read-root and --automation-write-root are required".to_owned());
        }
        let resolve = |list: &[PathBuf]| -> Result<Vec<PathBuf>, String> {
            let mut out: Vec<PathBuf> = Vec::new();
            for root in list {
                let canonical = std::fs::canonicalize(root).map_err(|e| format!("automation root {}: {e}", root.display()))?;
                if !canonical.is_dir() {
                    return Err(format!("automation root {} is not a folder", root.display()));
                }
                if !out.contains(&canonical) {
                    out.push(canonical);
                }
            }
            Ok(out)
        };
        Ok(Self { read: resolve(read)?, write: resolve(write)? })
    }

    pub fn describe(&self) -> String {
        let list = |v: &[PathBuf]| v.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
        format!("read: {read}; write: {write}", read = list(&self.read), write = list(&self.write))
    }

    /// The resolved form of `path` when `access` to it is allowed; a message naming the path, the
    /// access and the roots otherwise.
    pub fn check(&self, path: &Path, access: Access) -> Result<PathBuf, String> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            match self.write.first() {
                Some(base) => base.join(path),
                None => return Err(self.refusal(path, access, "there is no write root")),
            }
        };
        let resolved = resolve(&absolute).map_err(|e| self.refusal(path, access, &e))?;
        if hard_linked(&resolved) {
            return Err(self.refusal(path, access, "it has more than one hard link"));
        }
        let allowed = match access {
            Access::Read => self.read.iter().chain(&self.write).any(|r| resolved.starts_with(r)),
            Access::Write => self.write.iter().any(|r| resolved.starts_with(r)),
        };
        if allowed { Ok(resolved) } else { Err(self.refusal(path, access, "it is outside the roots")) }
    }

    fn refusal(&self, path: &Path, access: Access, why: &str) -> String {
        let verb = match access {
            Access::Read => "reading",
            Access::Write => "writing",
        };
        format!("automation path refused: {verb} {} ({why}; {})", path.display(), self.describe())
    }
}

/// The canonical form of `path`; for a path that does not exist yet, its deepest existing ancestor
/// canonicalised plus the remaining plain components.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = std::fs::canonicalize(path) {
        return Ok(p);
    }
    let mut missing = Vec::new();
    let mut cursor = path;
    loop {
        let name = cursor.file_name().ok_or_else(|| format!("{} cannot be resolved", path.display()))?;
        missing.push(name.to_owned());
        cursor = cursor.parent().ok_or_else(|| format!("{} cannot be resolved", path.display()))?;
        if let Ok(base) = std::fs::canonicalize(cursor) {
            let mut out = base;
            for part in missing.iter().rev() {
                if matches!(Path::new(part).components().next(), Some(Component::ParentDir | Component::CurDir)) {
                    return Err(format!("{} has `..` after a missing folder", path.display()));
                }
                out.push(part);
            }
            return Ok(out);
        }
    }
}

#[cfg(unix)]
fn hard_linked(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.nlink() > 1)
}

#[cfg(not(unix))]
fn hard_linked(_path: &Path) -> bool {
    // Windows link counts need a handle query (docs/ROADMAP.md).
    false
}
