//! A project's history kept as values, for projects that have no folder (the browser's library,
//! which keeps them in IndexedDB): the same rules as a repository TeXCraft makes, a version per
//! save that changed something, with no git on disk.
//!
//! Content-addressed as git is, so versions share the files they did not change: each file's
//! bytes are kept once, under their git blob id (the SHA-1 of `blob <length>\0<bytes>`), and a
//! version lists its files' paths and blob ids. A version's id is the SHA-1 of what it holds
//! ([`Version::manifest`]). The blob ids are git's, so the history can later be written into a
//! git repository as it is (a forge, C5); the versions' ids are not git commit ids, and no tree
//! or commit object is made here.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ChangeKind, Entry, Result, Snapshot, Versioned, is_hidden};

/// One saved version of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// The SHA-1 of its manifest, 40 hexadecimal digits.
    pub id: String,
    /// The version before it; none for the first.
    pub parent: Option<String>,
    /// When it was saved, in seconds since 1970 (UTC).
    pub time: i64,
    pub author: String,
    pub message: String,
    /// The project's files, by path (`/`-separated, sorted), with their blob ids.
    pub files: Vec<(String, String)>,
}

impl Version {
    /// What the version's id is the hash of: its parent, time, author, files and message.
    pub fn manifest(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(parent) = &self.parent {
            out.extend_from_slice(format!("parent {parent}\n").as_bytes());
        }
        out.extend_from_slice(format!("time {}\nauthor {}\n", self.time, self.author).as_bytes());
        for (path, blob) in &self.files {
            out.extend_from_slice(format!("file {blob} {path}").as_bytes());
            out.push(0);
        }
        out.push(b'\n');
        out.extend_from_slice(self.message.as_bytes());
        out
    }
}

/// Files' bytes by blob id.
type Blobs = BTreeMap<String, Vec<u8>>;

/// A version just recorded, with the files' bytes the history did not hold yet: what a shell
/// adds to what it stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub version: Version,
    pub blobs: Vec<(String, Vec<u8>)>,
}

/// A project's versions, oldest first, and the bytes of their files by blob id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Versions {
    versions: Vec<Version>,
    blobs: BTreeMap<String, Vec<u8>>,
}

/// The SHA-1 of `parts` one after the other, in hexadecimal.
fn sha1(parts: &[&[u8]]) -> Result<String> {
    let mut hasher = gix_hash::hasher(gix_hash::Kind::Sha1);
    for part in parts {
        hasher.update(part);
    }
    let id = hasher.try_finalize().map_err(|e| format!("A file's id cannot be computed: {e}"))?;
    Ok(id.to_string())
}

/// The id git gives a file of these bytes (its blob id).
pub fn blob_id(bytes: &[u8]) -> Result<String> {
    sha1(&[format!("blob {}\0", bytes.len()).as_bytes(), bytes])
}

impl Versions {
    /// A history as a shell stored it: versions in any order (they are put in order by their
    /// parents, the latest last) and the files' bytes by blob id. A version whose files are not
    /// all there, or whose parent is not, is left out, with the versions after it.
    pub fn from_parts(versions: Vec<Version>, blobs: BTreeMap<String, Vec<u8>>) -> Self {
        let mut by_id: BTreeMap<String, Version> = versions.into_iter().map(|v| (v.id.clone(), v)).collect();
        let mut ordered = Vec::new();
        let mut parent: Option<String> = None;
        loop {
            // The child of the last one taken (versions are a line: each has one parent).
            let next = by_id.values().filter(|v| v.parent == parent).max_by_key(|v| v.time).map(|v| v.id.clone());
            let Some(id) = next else { break };
            let Some(version) = by_id.remove(&id) else { break };
            if !version.files.iter().all(|(_, b)| blobs.contains_key(b)) {
                break;
            }
            parent = Some(version.id.clone());
            ordered.push(version);
        }
        Self { versions: ordered, blobs }
    }

    /// The versions, oldest first.
    pub fn versions(&self) -> &[Version] {
        &self.versions
    }

    /// The files' bytes by blob id.
    pub fn blobs(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.blobs
    }

    /// The latest version's id; none before the first.
    pub fn head(&self) -> Option<&str> {
        self.versions.last().map(|v| v.id.as_str())
    }

    /// The files of `snapshot` by path with their blob ids, and the bytes the history does not
    /// hold yet.
    fn files_of(&self, snapshot: &Snapshot) -> Result<(Vec<(String, String)>, Blobs)> {
        let mut files = Vec::new();
        let mut new_blobs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (path, bytes) in &snapshot.files {
            let id = blob_id(bytes)?;
            if !self.blobs.contains_key(&id) {
                new_blobs.insert(id.clone(), bytes.clone());
            }
            files.push((path.clone(), id));
        }
        files.sort();
        files.dedup_by(|a, b| a.0 == b.0);
        Ok((files, new_blobs))
    }

    /// Adds a version of `files` after the latest one.
    fn push(
        &mut self,
        files: Vec<(String, String)>,
        new_blobs: BTreeMap<String, Vec<u8>>,
        message: &str,
        author: &str,
        time: i64,
    ) -> Result<Recorded> {
        let mut version = Version {
            id: String::new(),
            parent: self.head().map(str::to_owned),
            time,
            author: author.to_owned(),
            message: message.to_owned(),
            files,
        };
        version.id = sha1(&[&version.manifest()])?;
        self.blobs.extend(new_blobs.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.versions.push(version.clone());
        Ok(Recorded { version, blobs: new_blobs.into_iter().collect() })
    }

    /// Records `snapshot` (all its files) as a new version; none when its files are those of the
    /// latest version.
    pub fn record(&mut self, snapshot: &Snapshot, message: &str, author: &str, time: chrono::DateTime<chrono::Utc>) -> Result<Option<Recorded>> {
        let (files, new_blobs) = self.files_of(snapshot)?;
        if self.versions.last().is_some_and(|v| v.files == files) {
            return Ok(None);
        }
        self.push(files, new_blobs, message, author, time.timestamp()).map(Some)
    }

    /// Records `snapshot` with `message` in place of the latest version, `replaced` (as git
    /// amends a commit): after the same version, with the changes since it combined. A version
    /// whose changes all went is dropped. Refuses when `replaced` is no longer the latest.
    pub fn amend(
        &mut self,
        replaced: &str,
        snapshot: &Snapshot,
        message: &str,
        author: &str,
        time: chrono::DateTime<chrono::Utc>,
    ) -> Result<(crate::Amended, Option<Recorded>)> {
        if self.head() != Some(replaced) {
            return Err("The history moved since TeXCraft's last version: a new version is made instead.".to_owned());
        }
        let (files, new_blobs) = self.files_of(snapshot)?;
        if self.versions.last().is_some_and(|v| v.files == files && v.message == message) {
            return Ok((crate::Amended::Same, None));
        }
        self.versions.pop();
        if let Some(before) = self.versions.last()
            && before.files == files
        {
            return Ok((crate::Amended::Undone(before.id.clone()), None));
        }
        let recorded = self.push(files, new_blobs, message, author, time.timestamp())?;
        Ok((crate::Amended::Replaced(recorded.version.id.clone()), Some(recorded)))
    }

    fn find(&self, id: &str) -> Result<&Version> {
        let id = id.trim();
        self.versions.iter().find(|v| v.id == id).ok_or_else(|| format!("There is no version {id} in the project's history."))
    }

    fn blob(&self, id: &str) -> Result<Vec<u8>> {
        self.blobs.get(id).cloned().ok_or_else(|| format!("A file of the project's history is missing ({id})."))
    }
}

impl Versioned for Versions {
    fn log(&self, limit: usize) -> Result<Vec<Entry>> {
        let mut out = Vec::new();
        for (i, v) in self.versions.iter().enumerate().rev().take(limit) {
            let before: BTreeMap<&str, &str> =
                if i == 0 { BTreeMap::new() } else { self.versions[i - 1].files.iter().map(|(p, b)| (p.as_str(), b.as_str())).collect() };
            let after: BTreeMap<&str, &str> = v.files.iter().map(|(p, b)| (p.as_str(), b.as_str())).collect();
            let paths: BTreeSet<&str> = before.keys().chain(after.keys()).copied().collect();
            let changes = paths
                .into_iter()
                .filter_map(|p| match (before.get(p), after.get(p)) {
                    (None, Some(_)) => Some((p.to_owned(), ChangeKind::Added)),
                    (Some(_), None) => Some((p.to_owned(), ChangeKind::Deleted)),
                    (Some(a), Some(b)) if a != b => Some((p.to_owned(), ChangeKind::Modified)),
                    _ => None,
                })
                .collect();
            out.push(Entry {
                id: v.id.clone(),
                parent: v.parent.clone(),
                time: chrono::DateTime::from_timestamp(v.time, 0).unwrap_or_default(),
                author: v.author.clone(),
                message: v.message.clone(),
                changes,
            });
        }
        Ok(out)
    }

    fn file_at(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>> {
        let version = self.find(commit)?;
        version.files.iter().find(|(p, _)| p == path).map(|(_, b)| self.blob(b)).transpose()
    }

    fn files_at(&self, commit: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let version = self.find(commit)?;
        version.files.iter().filter(|(p, _)| !is_hidden(p)).map(|(p, b)| Ok((p.clone(), self.blob(b)?))).collect()
    }

    fn ids_at(&self, commit: &str) -> Result<Vec<(String, String)>> {
        Ok(self.find(commit)?.files.iter().filter(|(p, _)| !is_hidden(p)).cloned().collect())
    }

    fn latest(&self) -> Result<Option<String>> {
        Ok(self.head().map(str::to_owned))
    }

    fn parent_of(&self, commit: &str) -> Result<Option<String>> {
        Ok(self.find(commit)?.parent.clone())
    }
}
