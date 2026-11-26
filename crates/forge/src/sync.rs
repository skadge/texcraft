//! A project's files and its branch: opening it (the tree and the files' blobs at the branch's
//! head), what changed since (by git blob id), and committing the changes back as one commit.

use std::collections::{BTreeMap, BTreeSet};

use crate::forge::{Forge, Made, Target};
use crate::gitlab::is_object_id;
use crate::{Address, Error, MAX_PROJECT, PARALLEL, Result, TreeEntry};

/// Where a project opened from a forge came from, and the state of its files there.
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

/// A file's contents to put into the project (none: to remove it from the project).
pub type Update = (String, Option<Vec<u8>>);

/// What a commit did.
#[derive(Debug, Clone)]
pub struct Committed {
    /// The new commit's id; none when there was nothing to commit.
    pub commit: Option<String>,
    /// The link after it: the new commit as the base (on the new branch, for a merge request).
    pub link: Link,
    /// The changes committed.
    pub changes: Vec<FileChange>,
    /// What the branch's new commits changed, merged with the changes here where both changed a
    /// file: each file's contents to put into the project, sorted by path.
    pub updates: Vec<Update>,
    /// The files whose changes here and on the branch were merged.
    pub merged: Vec<String>,
    /// Someone committed to the branch between the check and the commit; the commit went on top
    /// of theirs, and what they changed was taken in.
    pub raced: bool,
    /// The merge request opened for a commit to a new branch.
    pub merge_request: Option<MergeRequest>,
    /// Why the merge request could not be opened (the commit is on the new branch).
    pub merge_request_error: Option<String>,
}

/// What taking in the branch's new commits did.
#[derive(Debug, Clone)]
pub struct Pulled {
    /// The link after it: the branch's head as the base.
    pub link: Link,
    /// Whether the branch had new commits.
    pub moved: bool,
    /// Each file's contents to put into the project, sorted by path.
    pub updates: Vec<Update>,
    /// The files whose changes here and on the branch were merged.
    pub merged: Vec<String>,
}

/// A merge request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    pub iid: u64,
    pub web_url: String,
}

/// What to do with a file in conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Keep the project's version: it is committed over the branch's.
    Mine,
    /// Take the branch's version, dropping the changes made here.
    Theirs,
}

impl Choice {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mine" => Some(Choice::Mine),
            "theirs" => Some(Choice::Theirs),
            _ => None,
        }
    }
}

/// The choice made for each file in conflict, by path.
pub type Choices = BTreeMap<String, Choice>;

/// A file changed both here and on the branch, in ways that do not merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub kind: ConflictKind,
}

/// Why a file does not merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both changed the same lines, or lines next to each other.
    Overlap,
    /// Both changed it, and it is not text.
    Binary,
    /// Changed here, deleted on the branch.
    DeletedThere,
    /// Deleted here, changed on the branch.
    DeletedHere,
    /// Added on both sides, differently.
    AddedBoth,
    /// Changed on both sides, and too large to merge.
    TooLarge,
}

impl ConflictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictKind::Overlap => "overlap",
            ConflictKind::Binary => "binary",
            ConflictKind::DeletedThere => "deleted on the branch",
            ConflictKind::DeletedHere => "deleted here",
            ConflictKind::AddedBoth => "added on both sides",
            ConflictKind::TooLarge => "too large to merge",
        }
    }
}

/// The folder of the review threads (`texcraft_review::format::DIR`), committed with the project
/// although it is hidden.
pub const REVIEW_DIR: &str = ".texcraft/review";

/// Whether `path` is a review thread's file: in [`REVIEW_DIR`] itself, not hidden.
pub fn is_review(path: &str) -> bool {
    path.strip_prefix(REVIEW_DIR)
        .and_then(|r| r.strip_prefix('/'))
        .is_some_and(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
}

/// Whether a project loads this entry of a tree: a file, not hidden (`.gitignore`, `.gitlab-ci.yml`
/// stay on the branch as they are, as a folder's hidden files stay on disk), apart from the
/// review threads.
fn loads(entry: &TreeEntry) -> bool {
    entry.is_file() && (!texcraft_history::is_hidden(&entry.path) || is_review(&entry.path)) && is_object_id(&entry.id)
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
async fn fetch(client: &impl Forge, wanted: &[(String, String)]) -> Result<(Vec<(String, Vec<u8>)>, Vec<Skipped>)> {
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
                None => {
                    let mb = crate::MAX_FILE / (1024 * 1024);
                    skipped.push(Skipped { path: path.clone(), why: format!("larger than the {mb} MB a project file may have") });
                }
            }
        }
    }
    Ok((files, skipped))
}

/// Opens the project at `client`'s address: branch `branch`, or the default one. Its files are
/// those at the branch's head (the base); hidden files, symbolic links and submodules are left
/// on the branch (the review threads in [`REVIEW_DIR`] apart), and files over [`crate::MAX_FILE`]
/// skipped.
pub async fn open(client: &impl Forge, branch: Option<&str>) -> Result<Loaded> {
    let info = client.project().await?;
    let name = match branch.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => b.to_owned(),
        None => info
            .default_branch
            .clone()
            .ok_or_else(|| Error::NotFound { forge: client.address().service, what: format!("branch in {} (its repository is empty)", info.path) })?,
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

/// The branch's state at `head` against the project's `files`, with what the branch changed since
/// `link`'s base taken in: files changed only there are taken, files changed on both sides merged
/// ([`crate::merge::merge3`]), or decided by `choices`. Any file that does not merge is a conflict,
/// and then nothing is taken in.
struct CaughtUp {
    /// The link at `head`.
    link: Link,
    /// The project's files with the branch's changes in them.
    files: BTreeMap<String, Vec<u8>>,
    merged: Vec<String>,
}

async fn catch_up(client: &impl Forge, link: &Link, head: &str, files: &[(String, Vec<u8>)], choices: &Choices) -> Result<CaughtUp> {
    let mut now: BTreeMap<String, Vec<u8>> = files.iter().cloned().collect();
    if head == link.base {
        return Ok(CaughtUp { link: link.clone(), files: now, merged: Vec::new() });
    }
    let local = blob_ids(files)?;
    let skip: BTreeSet<&str> = link.skipped.iter().map(|s| s.path.as_str()).collect();
    let tree = client.tree(head).await?;
    let mut theirs = tracked(&tree, &skip, &local);
    let paths: BTreeSet<&String> = link.files.keys().chain(theirs.keys()).collect();
    let (mut take, mut merge, mut conflicts) = (Vec::new(), Vec::new(), Vec::new());
    for path in paths {
        let (base, there, here) = (link.files.get(path), theirs.get(path), local.get(path));
        if there == base || there == here {
            // Not changed there, or changed the same way on both sides.
            continue;
        }
        if here == base {
            take.push((path.clone(), there.cloned()));
            continue;
        }
        match (choices.get(path), base, there, here) {
            (Some(Choice::Mine), ..) => {}
            (Some(Choice::Theirs), ..) => take.push((path.clone(), there.cloned())),
            (None, Some(b), Some(t), Some(_)) => merge.push((path.clone(), b.clone(), t.clone())),
            (None, _, None, _) => conflicts.push(Conflict { path: path.clone(), kind: ConflictKind::DeletedThere }),
            (None, _, _, None) => conflicts.push(Conflict { path: path.clone(), kind: ConflictKind::DeletedHere }),
            (None, None, ..) => conflicts.push(Conflict { path: path.clone(), kind: ConflictKind::AddedBoth }),
        }
    }
    // The files changed on both sides, merged.
    let ids: Vec<String> = merge.iter().flat_map(|(_, b, t)| [b.clone(), t.clone()]).collect::<BTreeSet<_>>().into_iter().collect();
    let blobs = blobs(client, &ids).await?;
    let mut merged = Vec::new();
    for (path, b, t) in merge {
        let (Some(Some(base)), Some(Some(there)), Some(here)) = (blobs.get(&b), blobs.get(&t), now.get(&path)) else {
            conflicts.push(Conflict { path, kind: ConflictKind::TooLarge });
            continue;
        };
        if ![base, there, here].iter().all(|b| crate::merge::is_text(b)) {
            conflicts.push(Conflict { path, kind: ConflictKind::Binary });
            continue;
        }
        match crate::merge::merge3(base, here, there) {
            Some(bytes) => {
                now.insert(path.clone(), bytes);
                merged.push(path);
            }
            None => conflicts.push(Conflict { path, kind: ConflictKind::Overlap }),
        }
    }
    if !conflicts.is_empty() {
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
        return Err(Error::Conflict { forge: client.address().service, branch: link.branch.clone(), files: conflicts });
    }
    // The files changed there only, taken.
    let wanted: Vec<(String, String)> = take.iter().filter_map(|(p, id)| Some((p.clone(), id.clone()?))).collect();
    for (path, id) in &take {
        if id.is_none() {
            now.remove(path);
        }
    }
    let (fetched, too_large) = fetch(client, &wanted).await?;
    now.extend(fetched);
    let mut skipped: Vec<Skipped> = link.skipped.iter().filter(|s| tree.iter().any(|e| e.path == s.path)).cloned().collect();
    for s in &too_large {
        // Its new version is not loaded: the project's older one goes, so it is not committed back.
        theirs.remove(&s.path);
        now.remove(&s.path);
    }
    skipped.extend(too_large);
    let link = Link { base: head.to_owned(), files: theirs, skipped, ..link.clone() };
    Ok(CaughtUp { link, files: now, merged })
}

/// The blobs `ids`, [`PARALLEL`] at a time, by id; none for one over [`crate::MAX_FILE`].
async fn blobs(client: &impl Forge, ids: &[String]) -> Result<BTreeMap<String, Option<Vec<u8>>>> {
    let mut out = BTreeMap::new();
    for chunk in ids.chunks(PARALLEL) {
        let results = crate::task::join_all(chunk.iter().map(|id| client.blob(id)).collect()).await;
        for (id, result) in chunk.iter().zip(results) {
            out.insert(id.clone(), result?);
        }
    }
    Ok(out)
}

/// What turns `before` into `after`: each file's new contents, or none to remove it.
fn updates(before: &[(String, Vec<u8>)], after: &BTreeMap<String, Vec<u8>>) -> Vec<Update> {
    let old: BTreeMap<&str, &Vec<u8>> = before.iter().map(|(p, b)| (p.as_str(), b)).collect();
    let mut out: Vec<Update> = after.iter().filter(|(p, b)| old.get(p.as_str()) != Some(b)).map(|(p, b)| (p.clone(), Some(b.clone()))).collect();
    out.extend(old.keys().filter(|p| !after.contains_key(**p)).map(|p| ((*p).to_owned(), None)));
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Takes the branch's new commits into the project's `files`: files changed only on the branch
/// are taken, files changed on both sides merged line by line. When a file does not merge
/// ([`Error::Conflict`]) nothing is taken, unless `choices` says for it whether to keep the
/// project's version or take the branch's.
pub async fn pull(client: &impl Forge, link: &Link, files: &[(String, Vec<u8>)], choices: &Choices) -> Result<Pulled> {
    let head = client.branch(&link.branch).await?;
    let up = catch_up(client, link, &head.commit, files, choices).await?;
    Ok(Pulled { moved: up.link.base != link.base, updates: updates(files, &up.files), merged: up.merged, link: up.link })
}

/// Commits the project's `files` to `link`'s branch, as one commit with `message`, then brings
/// the link to that commit.
///
/// A protected branch, or one the user's role cannot push to, is refused before anything is sent
/// ([`Error::CannotPush`]): [`commit_to_new_branch`] is the way then. When the branch has moved
/// since the base, its new commits are taken in first as [`pull`] does (conflicts refuse the
/// commit unless `choices` decides them), and the commit goes on top of them; what they changed
/// comes back in [`Committed::updates`].
pub async fn commit(client: &impl Forge, link: &Link, files: &[(String, Vec<u8>)], message: &str, choices: &Choices) -> Result<Committed> {
    let ours = changes(link, files)?;
    if ours.is_empty() {
        return Ok(Committed {
            commit: None,
            link: link.clone(),
            changes: ours,
            updates: Vec::new(),
            merged: Vec::new(),
            raced: false,
            merge_request: None,
            merge_request_error: None,
        });
    }
    // A forge that refuses a commit on a head that moved meanwhile ([`Made::Moved`]) gets the
    // branch's new commits taken in again, a few times.
    let mut attempts = 0;
    let (up, integrated, to_commit, id, raced) = loop {
        attempts += 1;
        let head = client.branch(&link.branch).await?;
        if !head.can_push {
            return Err(Error::CannotPush { forge: client.address().service, branch: link.branch.clone() });
        }
        let up = catch_up(client, link, &head.commit, files, choices).await?;
        let integrated: Vec<(String, Vec<u8>)> = up.files.iter().map(|(p, b)| (p.clone(), b.clone())).collect();
        let to_commit = changes(&up.link, &integrated)?;
        if to_commit.is_empty() {
            let id = head.commit.clone();
            break (up, integrated, to_commit, id, false);
        }
        let target = Target::Branch { name: &link.branch, head: &head.commit };
        let made = client.commit(target, message, &to_commit, &integrated).await.map_err(|e| match e {
            Error::Forbidden { .. } => Error::CannotPush { forge: client.address().service, branch: link.branch.clone() },
            e => e,
        })?;
        match made {
            Made::Commit { id, parents } => {
                let raced = attempts > 1 || parents.first() != Some(&head.commit);
                break (up, integrated, to_commit, id, raced);
            }
            Made::Moved if attempts < MAX_ATTEMPTS => continue,
            Made::Moved => return Err(Error::Busy { branch: link.branch.clone() }),
        }
    };
    // The project as the new commit has it: what others committed meanwhile is taken in.
    let local = blob_ids(&integrated)?;
    let skip: BTreeSet<&str> = up.link.skipped.iter().map(|s| s.path.as_str()).collect();
    let tree = client.tree(&id).await?;
    let mut new_files = tracked(&tree, &skip, &local);
    let wanted: Vec<(String, String)> = new_files.iter().filter(|(p, b)| local.get(*p) != Some(*b)).map(|(p, b)| (p.clone(), b.clone())).collect();
    let mut after = up.files;
    after.retain(|p, _| new_files.contains_key(p));
    let (fetched, too_large) = fetch(client, &wanted).await?;
    after.extend(fetched);
    let mut skipped: Vec<Skipped> = up.link.skipped.iter().filter(|s| tree.iter().any(|e| e.path == s.path)).cloned().collect();
    for s in &too_large {
        // Its new version is not loaded: the project's older one goes, so it is not committed back.
        new_files.remove(&s.path);
        after.remove(&s.path);
    }
    skipped.extend(too_large);
    let new_link = Link { base: id.clone(), files: new_files, skipped, ..up.link };
    Ok(Committed {
        commit: (!to_commit.is_empty()).then_some(id),
        link: new_link,
        changes: to_commit,
        updates: updates(files, &after),
        merged: up.merged,
        raced,
        merge_request: None,
        merge_request_error: None,
    })
}

/// Commits tried on a branch that keeps moving before giving up ([`Error::Busy`]).
const MAX_ATTEMPTS: usize = 3;

/// Whether `name` can be a new branch's name (git's rules, simplified).
pub fn check_branch_name(name: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::BranchName(format!("{name}: {why}")));
    if name.is_empty() || name.len() > 200 {
        return bad("a branch's name has 1 to 200 characters");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c)) {
        return bad("it has spaces or one of ~ ^ : ? * [ \\");
    }
    if name.starts_with(['-', '/', '.'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
        || name.contains("/.")
    {
        return bad("it is not a name git accepts");
    }
    Ok(())
}

/// Commits the project's changes since `link`'s base to a new branch `branch` started from the
/// base (so they need no merge), opens a merge request of it into `link`'s branch, and moves the
/// link to the new branch: the way when the branch cannot take the commit (protected, a role
/// that cannot push, or a conflict), where the forge shows the conflict. GitHub calls it a pull
/// request.
pub async fn commit_to_new_branch(client: &impl Forge, link: &Link, files: &[(String, Vec<u8>)], message: &str, branch: &str) -> Result<Committed> {
    let branch = branch.trim();
    check_branch_name(branch)?;
    if branch == link.branch {
        return Err(Error::BranchName(format!("{branch}: this is the project's branch; give a new one")));
    }
    let ours = changes(link, files)?;
    if ours.is_empty() {
        return Err(Error::Nothing);
    }
    let made = client.commit(Target::NewBranch { name: branch, start: &link.base }, message, &ours, files).await?;
    let Made::Commit { id, .. } = made else { return Err(Error::Invalid(format!("the new branch {branch} was not made"))) };
    let title = message.lines().find(|l| !l.trim().is_empty()).unwrap_or(message).trim();
    let (merge_request, merge_request_error) = match client.request_merge(branch, &link.branch, title).await {
        Ok(mr) => (Some(mr), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let local = blob_ids(files)?;
    let skip: BTreeSet<&str> = link.skipped.iter().map(|s| s.path.as_str()).collect();
    let new_files = tracked(&client.tree(&id).await?, &skip, &local);
    let new_link = Link { branch: branch.to_owned(), base: id.clone(), files: new_files, ..link.clone() };
    Ok(Committed {
        commit: Some(id),
        link: new_link,
        changes: ours,
        updates: Vec::new(),
        merged: Vec::new(),
        raced: false,
        merge_request,
        merge_request_error,
    })
}
