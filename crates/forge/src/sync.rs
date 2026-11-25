//! A project's files and its branch: opening it (the tree and the files' blobs at the branch's
//! head), what changed since (by git blob id), and committing the changes back as one commit.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use serde_json::{Value, json};

use crate::gitlab::is_object_id;
use crate::{Address, Error, GitLab, MAX_PROJECT, PARALLEL, Result, TreeEntry};

/// Where a project opened from GitLab came from, and the state of its files there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Link {
    pub address: Address,
    pub project_id: u64,
    pub web_url: String,
    pub branch: String,
    /// The commit the files are at: the branch's head when they were loaded or last committed.
    pub base: String,
    /// The files loaded from the base, by path, with their blob ids there.
    pub files: BTreeMap<String, String>,
    /// Files of the branch that were not loaded: they stay on the branch as they are.
    #[serde(default)]
    pub skipped: Vec<Skipped>,
}

/// A file of the branch the project does not have, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Skipped {
    pub path: String,
    pub why: String,
}

/// A project's files as loaded from its branch.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub link: Link,
    /// Paths and bytes, sorted by path.
    pub files: Vec<(String, Vec<u8>)>,
}

/// How a file differs from its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    /// Renamed or moved, its contents the same (a deleted and an added file with one blob id).
    Moved,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Moved => "moved",
        }
    }
}

/// A file changed since the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    /// A moved file's path at the base.
    pub from: Option<String>,
}

/// What a commit did.
#[derive(Debug, Clone)]
pub struct Committed {
    /// The new commit's id; none when there was nothing to commit.
    pub commit: Option<String>,
    /// The link after it: the new commit as the base.
    pub link: Link,
    /// The changes committed.
    pub changes: Vec<FileChange>,
    /// Files the branch changed meanwhile (others' commits), to take into the project: their
    /// paths and new bytes.
    pub taken: Vec<(String, Vec<u8>)>,
    /// Files the branch deleted meanwhile, to remove from the project.
    pub removed: Vec<String>,
    /// Someone committed to the branch between the check and the commit; the commit went on top
    /// of theirs, and what they changed was taken in.
    pub raced: bool,
}

/// Whether a project loads this entry of a tree: a file, not hidden (`.gitignore`, `.gitlab-ci.yml`
/// stay on the branch as they are, as a folder's hidden files stay on disk).
fn loads(entry: &TreeEntry) -> bool {
    entry.is_file() && !texcraft_history::is_hidden(&entry.path) && is_object_id(&entry.id)
}

/// The files of `tree` a project loads, with their blob ids, leaving out the paths in `skip`; and
/// hidden files the project has (`local`), which it committed.
fn tracked(tree: &[TreeEntry], skip: &BTreeSet<&str>, local: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    tree.iter()
        .filter(|e| (loads(e) || (e.is_file() && local.contains_key(&e.path))) && !skip.contains(e.path.as_str()))
        .map(|e| (e.path.clone(), e.id.to_ascii_lowercase()))
        .collect()
}

/// Fetches the blobs `wanted` (paths and ids), [`PARALLEL`] at a time: the files loaded, and those
/// over [`crate::MAX_FILE`], skipped. Refuses when the files add up to more than [`MAX_PROJECT`].
async fn fetch(client: &GitLab, wanted: &[(String, String)]) -> Result<(Vec<(String, Vec<u8>)>, Vec<Skipped>)> {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut total: u64 = 0;
    for chunk in wanted.chunks(PARALLEL) {
        let results = crate::task::join_all(chunk.iter().map(|(_, id)| client.blob(id)).collect()).await;
        for ((path, _), result) in chunk.iter().zip(results) {
            match result? {
                Some(bytes) => {
                    total += bytes.len() as u64;
                    if total > MAX_PROJECT {
                        return Err(Error::TooLarge { bytes: total });
                    }
                    files.push((path.clone(), bytes));
                }
                None => skipped.push(Skipped { path: path.clone(), why: format!("larger than {} MB", crate::MAX_FILE / (1024 * 1024)) }),
            }
        }
    }
    Ok((files, skipped))
}

/// Opens the project at `client`'s address: branch `branch`, or the default one. Its files are
/// those at the branch's head (the base); hidden files, symbolic links and submodules are left
/// on the branch, and files over [`crate::MAX_FILE`] skipped.
pub async fn open(client: &GitLab, branch: Option<&str>) -> Result<Loaded> {
    let info = client.project().await?;
    let name = match branch.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => b.to_owned(),
        None => info.default_branch.clone().ok_or_else(|| Error::NotFound { what: format!("branch in {} (its repository is empty)", info.path) })?,
    };
    let head = client.branch(&name).await?;
    let tree = client.tree(&head.commit).await?;
    let mut skipped: Vec<Skipped> = tree
        .iter()
        .filter(|e| e.kind == "blob" && e.mode == "120000")
        .map(|e| Skipped { path: e.path.clone(), why: "a symbolic link".to_owned() })
        .collect();
    let mut base = tracked(&tree, &BTreeSet::new(), &BTreeMap::new());
    let wanted: Vec<(String, String)> = base.iter().map(|(p, b)| (p.clone(), b.clone())).collect();
    let (files, too_large) = fetch(client, &wanted).await?;
    for s in &too_large {
        base.remove(&s.path);
    }
    skipped.extend(too_large);
    let link = Link {
        address: client.address().clone(),
        project_id: info.id,
        web_url: info.web_url,
        branch: head.name,
        base: head.commit,
        files: base,
        skipped,
    };
    Ok(Loaded { link, files })
}

/// The project's `files` (paths and bytes) against `link`'s base: what was added, modified,
/// deleted or moved, sorted by path.
pub fn changes(link: &Link, files: &[(String, Vec<u8>)]) -> Result<Vec<FileChange>> {
    let local = blob_ids(files)?;
    let mut added: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for (path, id) in &local {
        match link.files.get(path) {
            None => added.push(path),
            Some(base) if base != id => out.push(FileChange { path: path.clone(), kind: ChangeKind::Modified, from: None }),
            Some(_) => {}
        }
    }
    let mut deleted: Vec<&str> = link.files.keys().filter(|p| !local.contains_key(*p)).map(String::as_str).collect();
    for path in added {
        // An added file with the blob id of a deleted one: that file, moved.
        let id = &local[path];
        match deleted.iter().position(|d| link.files.get(*d) == Some(id)) {
            Some(i) => {
                let from = deleted.remove(i);
                out.push(FileChange { path: path.to_owned(), kind: ChangeKind::Moved, from: Some(from.to_owned()) });
            }
            None => out.push(FileChange { path: path.to_owned(), kind: ChangeKind::Added, from: None }),
        }
    }
    out.extend(deleted.into_iter().map(|p| FileChange { path: p.to_owned(), kind: ChangeKind::Deleted, from: None }));
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The blob id of each file, by path.
fn blob_ids(files: &[(String, Vec<u8>)]) -> Result<BTreeMap<String, String>> {
    files.iter().map(|(p, b)| Ok((p.clone(), texcraft_history::store::blob_id(b).map_err(Error::Invalid)?))).collect()
}

/// The commit API's actions for `changes`, the contents from `files`, in base64.
pub(crate) fn actions(changes: &[FileChange], files: &[(String, Vec<u8>)]) -> Vec<Value> {
    let bytes: BTreeMap<&str, &[u8]> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let content = |path: &str| base64::engine::general_purpose::STANDARD.encode(bytes.get(path).copied().unwrap_or_default());
    changes
        .iter()
        .map(|c| match c.kind {
            ChangeKind::Added => json!({"action": "create", "file_path": c.path, "content": content(&c.path), "encoding": "base64"}),
            ChangeKind::Modified => json!({"action": "update", "file_path": c.path, "content": content(&c.path), "encoding": "base64"}),
            ChangeKind::Deleted => json!({"action": "delete", "file_path": c.path}),
            ChangeKind::Moved => json!({"action": "move", "file_path": c.path, "previous_path": c.from}),
        })
        .collect()
}

/// Commits the project's `files` to `link`'s branch, as one commit with `message`, then brings
/// the link to that commit.
///
/// When the branch has moved since the base, its new commits are compared with the base: if they
/// changed files changed here too (differently), nothing is committed ([`Error::Conflict`]: the
/// three-way merge comes later); otherwise the commit goes on top of them, and the files they
/// changed come back in [`Committed::taken`] and [`Committed::removed`].
pub async fn commit(client: &GitLab, link: &Link, files: &[(String, Vec<u8>)], message: &str) -> Result<Committed> {
    let ours = changes(link, files)?;
    if ours.is_empty() {
        return Ok(Committed { commit: None, link: link.clone(), changes: ours, taken: Vec::new(), removed: Vec::new(), raced: false });
    }
    let head = client.branch(&link.branch).await?;
    if !head.can_push {
        return Err(Error::Forbidden { what: format!("commits to the branch {} (it is protected, or your role cannot push to it)", link.branch) });
    }
    let local = blob_ids(files)?;
    let skip: BTreeSet<&str> = link.skipped.iter().map(|s| s.path.as_str()).collect();
    let mut to_commit = ours.clone();
    if head.commit != link.base {
        let theirs = tracked(&client.tree(&head.commit).await?, &skip, &local);
        let changed_there = |p: &str| link.files.get(p) != theirs.get(p);
        // Each path a change touches, and what it holds after it here.
        let touched = |c: &FileChange| -> Vec<(String, Option<String>)> {
            let mut v = vec![(c.path.clone(), local.get(&c.path).cloned())];
            if let Some(from) = &c.from {
                v.push((from.clone(), None));
            }
            v
        };
        let mut conflicts = BTreeSet::new();
        to_commit.retain(|c| {
            let paths = touched(c);
            let same = paths.iter().all(|(p, mine)| theirs.get(p) == mine.as_ref());
            if same {
                // The branch made this very change: nothing to commit for it.
                return false;
            }
            for (p, _) in &paths {
                if changed_there(p) {
                    conflicts.insert(p.clone());
                }
            }
            true
        });
        if !conflicts.is_empty() {
            return Err(Error::Conflict { branch: link.branch.clone(), files: conflicts.into_iter().collect() });
        }
    }
    let (id, raced) = if to_commit.is_empty() {
        (head.commit.clone(), false)
    } else {
        let (id, parents) = client.create_commit(&link.branch, message, actions(&to_commit, files)).await?;
        let raced = parents.first() != Some(&head.commit);
        (id, raced)
    };
    // The project as the new commit has it: what others changed is taken in.
    let tree = client.tree(&id).await?;
    let mut new_files = tracked(&tree, &skip, &local);
    let wanted: Vec<(String, String)> = new_files.iter().filter(|(p, b)| local.get(*p) != Some(*b)).map(|(p, b)| (p.clone(), b.clone())).collect();
    let mut removed: Vec<String> = local.keys().filter(|p| !new_files.contains_key(*p)).cloned().collect();
    let (taken, too_large) = fetch(client, &wanted).await?;
    let mut skipped: Vec<Skipped> = link.skipped.iter().filter(|s| tree.iter().any(|e| e.path == s.path)).cloned().collect();
    for s in &too_large {
        // Its new version is not loaded: the project's older one goes, so it is not committed back.
        new_files.remove(&s.path);
        if local.contains_key(&s.path) {
            removed.push(s.path.clone());
        }
    }
    skipped.extend(too_large);
    let new_link = Link { base: id.clone(), files: new_files, skipped, ..link.clone() };
    Ok(Committed { commit: (!to_commit.is_empty()).then_some(id), link: new_link, changes: ours, taken, removed, raced })
}
