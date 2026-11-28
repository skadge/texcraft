//! The repository's remotes and the branch's upstream, in `.git/config` as git writes them (so
//! the git command line sees them); refs; how far a branch is ahead of another and behind it; and
//! moving the checked-out branch to a commit fetched (fast-forward) or to a merge with it, the
//! work tree's files with it.
//!
//! Moving the work tree never overwrites what git does not hold: a file that differs on disk from
//! the commit checked out (a change not committed, a file git does not track in the way) stops the
//! move before anything is written.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::{EntryKind, EntryMode};
use gix::refs::transaction::PreviousValue;

use super::{Folder, History};
use crate::{Amended, ChangeKind, Person, Result};

/// A remote of the repository's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// The merge commit, and the project's files it changed (from its folder).
    Done { commit: String, changed: Vec<(String, ChangeKind)> },
    /// Files changed on both sides in ways that do not merge, with why: nothing was changed.
    Conflicts(Vec<(String, &'static str)>),
}

/// How the files of two commits compare, before they are merged (Link to a Repository's
/// preview).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compared {
    /// The two histories have a commit in common.
    pub related: bool,
    /// With none in common: the paths on both sides with other contents (a merge asks which to
    /// keep for each).
    pub clashes: Vec<String>,
    /// Files the same on both sides.
    pub same: usize,
    /// Files only on our side, and only on theirs.
    pub only_ours: usize,
    pub only_theirs: usize,
}

/// Which side's version of a file in conflict a merge keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The branch checked out: the version here.
    Ours,
    /// The commit merged in: the remote's.
    Theirs,
}

/// Which side to keep for a file in conflict, by path; none leaves it a conflict.
pub type Choose<'a> = &'a dyn Fn(&str) -> Option<Side>;

/// Every file of a tree by path.
type Flat = BTreeMap<BString, (EntryMode, ObjectId)>;

/// Merges a text file: `base` with the changes `ours` and `theirs` made to it; none when they do
/// not merge.
pub type MergeText<'a> = &'a dyn Fn(&[u8], &[u8], &[u8]) -> Option<Vec<u8>>;

/// Whether `name` can be a remote's name in git's configuration.
fn check_remote_name(name: &str) -> Result<()> {
    let fine = !name.is_empty()
        && name.len() <= 100
        && !name.starts_with(['-', '.'])
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if fine { Ok(()) } else { Err(format!("{name:?} cannot be a remote's name: use letters, digits, '-', '_' or '.'.")) }
}

/// Writes `bytes` to `path` through `<path>.lock`, as git does, which keeps two writers apart.
fn write_locked(path: &Path, bytes: &[u8]) -> Result<()> {
    let lock = path.with_extension("lock");
    let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).and_then(|mut f| {
        use std::io::Write;
        f.write_all(bytes)?;
        f.sync_all()
    });
    written.and_then(|()| std::fs::rename(&lock, path)).map_err(|e| {
        let _ = std::fs::remove_file(&lock);
        format!("{} cannot be written: {e}", path.display())
    })
}

impl History {
    fn config_path(&self) -> PathBuf {
        self.repo.git_dir().join("config")
    }

    /// The repository's own configuration file (`.git/config`), as it is on disk.
    fn config(&self) -> Result<gix::config::File> {
        git!(
            "The repository's configuration cannot be read",
            gix::config::File::from_path_no_includes(self.config_path(), gix::config::Source::Local)
        )
    }

    /// Writes `file` as the repository's configuration, and reads the repository again.
    fn write_config(&mut self, file: &gix::config::File) -> Result<()> {
        let mut bytes = Vec::new();
        git!("The repository's configuration cannot be written", file.write_to(&mut bytes))?;
        let path = self.config_path();
        write_locked(&path, &bytes)?;
        let git_dir = self.repo.git_dir().to_path_buf();
        self.repo = super::with_committer(git!("The project's git repository cannot be opened", gix::open(git_dir))?);
        Ok(())
    }

    /// The remotes of the repository's configuration that have an address.
    pub fn remotes(&self) -> Result<Vec<Remote>> {
        let file = self.config()?;
        let mut out = Vec::new();
        if let Some(sections) = file.sections_by_name("remote") {
            for section in sections {
                let Some(name) = section.header().subsection_name() else { continue };
                if let Some(url) = section.value("url") {
                    out.push(Remote { name: name.to_string(), url: url.to_string() });
                }
            }
        }
        Ok(out)
    }

    /// The address of remote `name`, if the repository has it.
    pub fn remote_url(&self, name: &str) -> Result<Option<String>> {
        Ok(self.remotes()?.into_iter().find(|r| r.name == name).map(|r| r.url))
    }

    /// Sets remote `name`'s address (`remote.<name>.url`), and its refspec (`remote.<name>.fetch`,
    /// every branch into `refs/remotes/<name>/`) when it has none.
    pub fn set_remote(&mut self, name: &str, url: &str) -> Result<()> {
        check_remote_name(name)?;
        let url = url.trim();
        if url.is_empty() || url.chars().any(char::is_control) {
            return Err("A remote needs an address, on one line.".to_owned());
        }
        let mut file = self.config()?;
        git!("The remote cannot be set", file.set_raw_value_by("remote", name, "url", url))?;
        if file.string_by("remote", name, "fetch").is_none() {
            let spec = format!("+refs/heads/*:refs/remotes/{name}/*");
            git!("The remote cannot be set", file.set_raw_value_by("remote", name, "fetch", spec.as_str()))?;
        }
        self.write_config(&file)
    }

    /// The remote and the branch on it the checked-out branch follows (`branch.<b>.remote`,
    /// `branch.<b>.merge`), if set.
    pub fn upstream(&self) -> Result<Option<(String, String)>> {
        let Some(branch) = self.branch() else { return Ok(None) };
        let file = self.config()?;
        let remote = file.string_by("branch", branch.as_str(), "remote");
        let merge = file.string_by("branch", branch.as_str(), "merge");
        Ok(match (remote, merge) {
            (Some(remote), Some(merge)) => {
                let merge = merge.to_string();
                Some((remote.to_string(), merge.strip_prefix("refs/heads/").unwrap_or(&merge).to_owned()))
            }
            _ => None,
        })
    }

    /// Makes the checked-out branch follow branch `branch` of remote `remote`.
    pub fn set_upstream(&mut self, remote: &str, branch: &str) -> Result<()> {
        check_remote_name(remote)?;
        let local = self.branch().ok_or("No branch is checked out (HEAD is detached), so no branch can be set to follow a remote one.")?;
        let mut file = self.config()?;
        git!("The upstream cannot be set", file.set_raw_value_by("branch", local.as_str(), "remote", remote))?;
        let merge = format!("refs/heads/{branch}");
        git!("The upstream cannot be set", file.set_raw_value_by("branch", local.as_str(), "merge", merge.as_str()))?;
        self.write_config(&file)
    }

    /// Removes remote `name` from the configuration, the branch's upstream when it is on it, and
    /// the remote's branches as last known (`refs/remotes/<name>/…`). Nothing else changes: the
    /// commits stay, and the remote's repository is not reached.
    pub fn remove_remote(&mut self, name: &str) -> Result<()> {
        check_remote_name(name)?;
        let mut file = self.config()?;
        if file.remove_section("remote", Some(name.into())).is_none() {
            return Err(format!("The project has no remote {name}."));
        }
        if let Some(branch) = self.branch()
            && file.string_by("branch", branch.as_str(), "remote").is_some_and(|r| *r == *name.as_bytes())
            && let Ok(mut section) = file.section_mut("branch", Some(branch.as_str().into()))
        {
            section.remove("remote");
            section.remove("merge");
        }
        self.write_config(&file)?;
        let prefix = format!("refs/remotes/{name}/");
        let refs = git!("The repository's refs cannot be read", self.repo.references())?;
        let all = git!("The repository's refs cannot be read", refs.all())?;
        let tracking: Vec<gix::Reference<'_>> =
            all.filter_map(std::result::Result::ok).filter(|r| r.name().as_bstr().starts_with(prefix.as_bytes())).collect();
        for reference in tracking {
            git!("A remote's branch cannot be removed", reference.delete())?;
        }
        Ok(())
    }

    /// Makes the checked-out branch follow no remote branch.
    pub fn clear_upstream(&mut self) -> Result<()> {
        let Some(branch) = self.branch() else { return Ok(()) };
        let mut file = self.config()?;
        if let Ok(mut section) = file.section_mut("branch", Some(branch.as_str().into())) {
            section.remove("remote");
            section.remove("merge");
        }
        self.write_config(&file)
    }

    /// The commit ref `name` (`refs/remotes/origin/main`) points at, if there is one.
    pub fn ref_id(&self, name: &str) -> Result<Option<String>> {
        let found = git!("A ref cannot be read", self.repo.try_find_reference(name))?;
        let Some(mut reference) = found else { return Ok(None) };
        Ok(reference.peel_to_id().ok().map(|id| id.detach().to_string()))
    }

    /// Points ref `name` at commit `id` (a remote's branch, as last fetched or pushed).
    pub fn set_ref(&self, name: &str, id: &str, message: &str) -> Result<()> {
        let id = self.commit_id(id)?;
        git!("A ref cannot be written", self.repo.reference(name, id, PreviousValue::Any, message))?;
        Ok(())
    }

    fn commit_id(&self, id: &str) -> Result<ObjectId> {
        let oid = ObjectId::from_hex(id.trim().as_bytes()).map_err(|_| format!("`{id}` is not a commit id (40 hexadecimal digits)."))?;
        match self.repo.find_object(oid) {
            Ok(o) if o.kind == gix::objs::Kind::Commit => Ok(oid),
            _ => Err(format!("There is no commit {id} in the repository.")),
        }
    }

    fn parents(&self, id: ObjectId) -> Result<Vec<ObjectId>> {
        let object = git!("A commit cannot be read", self.repo.find_object(id))?;
        let commit = git!("A commit cannot be read", object.try_into_commit())?;
        Ok(commit.parent_ids().map(gix::Id::detach).collect())
    }

    /// Commit `id` and every commit before it.
    fn ancestors(&self, id: ObjectId) -> Result<HashSet<ObjectId>> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([id]);
        while let Some(id) = queue.pop_front() {
            if seen.insert(id) {
                queue.extend(self.parents(id)?);
            }
        }
        Ok(seen)
    }

    /// How many commits `ours` has that `theirs` does not, and the other way (git's ahead and
    /// behind).
    pub fn ahead_behind(&self, ours: &str, theirs: &str) -> Result<(usize, usize)> {
        let (a, b) = (self.ancestors(self.commit_id(ours)?)?, self.ancestors(self.commit_id(theirs)?)?);
        Ok((a.difference(&b).count(), b.difference(&a).count()))
    }

    /// Whether commit `older` is `newer` or one before it.
    pub fn is_ancestor(&self, older: &str, newer: &str) -> Result<bool> {
        Ok(self.ancestors(self.commit_id(newer)?)?.contains(&self.commit_id(older)?))
    }

    /// The newest commit both `a` and `b` come from: of `b`'s ancestors, the first met walking
    /// back from it that is also one of `a`'s.
    fn merge_base(&self, a: ObjectId, b: ObjectId) -> Result<Option<ObjectId>> {
        let of_a = self.ancestors(a)?;
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([b]);
        while let Some(id) = queue.pop_front() {
            if of_a.contains(&id) {
                return Ok(Some(id));
            }
            if seen.insert(id) {
                queue.extend(self.parents(id)?);
            }
        }
        Ok(None)
    }

    /// How the files of commit `theirs` compare with those of commit `ours` (every file of the
    /// repository, hidden ones too: a merge takes them all).
    pub fn compare(&self, ours: &str, theirs: &str) -> Result<Compared> {
        let (a, b) = (self.commit_id(ours)?, self.commit_id(theirs)?);
        let related = self.merge_base(a, b)?.is_some();
        let (mine, other) = (self.flat(Some(self.commit_tree(a)?))?, self.flat(Some(self.commit_tree(b)?))?);
        let mut out = Compared { related, ..Compared::default() };
        for (path, entry) in &mine {
            match other.get(path) {
                Some(o) if o == entry => out.same += 1,
                Some(_) => {
                    if !related {
                        out.clashes.push(path.to_str_lossy().into_owned());
                    }
                }
                None => out.only_ours += 1,
            }
        }
        out.only_theirs = other.keys().filter(|p| !mine.contains_key(*p)).count();
        Ok(out)
    }

    fn flat(&self, tree: Option<ObjectId>) -> Result<Flat> {
        let mut out = BTreeMap::new();
        self.flatten(tree, "", &mut out)?;
        Ok(out)
    }

    /// Moves the checked-out branch to commit `to`, which must come after HEAD (a fast-forward),
    /// and the work tree's files with it. Returns the project's files that changed.
    pub fn fast_forward(&self, to: &str) -> Result<Vec<(String, ChangeKind)>> {
        let to = self.commit_id(to)?;
        let head = self.head_id()?;
        if let Some(head) = head
            && !self.ancestors(to)?.contains(&head)
        {
            return Err("The commit is not after the one checked out: the branches have diverged, and need a merge.".to_owned());
        }
        if head == Some(to) {
            return Ok(Vec::new());
        }
        let old = self.flat(head.map(|h| self.commit_tree(h)).transpose()?)?;
        let tree = self.commit_tree(to)?;
        let new = self.flat(Some(tree))?;
        self.check_work_tree(&old, &new)?;
        self.move_branch(head, to, "fast-forward")?;
        self.write_work_tree(&old, &new)?;
        self.write_index(tree)?;
        Ok(self.project_changes(&old, &new))
    }

    /// Merges commit `theirs` into the checked-out branch: each file changed on one side only
    /// takes that side's version; a text file changed on both is merged by `merge_text`; anything
    /// else changed on both sides is a conflict, and then nothing is changed. The merge commit has
    /// HEAD and `theirs` as its parents.
    pub fn merge(
        &self,
        theirs: &str,
        message: &str,
        author: Option<&Person>,
        time: chrono::DateTime<chrono::Utc>,
        merge_text: MergeText<'_>,
    ) -> Result<Merged> {
        self.merge_choosing(theirs, message, author, time, merge_text, &|_| None)
    }

    /// [`History::merge`], keeping for each file in conflict the side `choose` names (a file it
    /// names none for stays a conflict).
    pub fn merge_choosing(
        &self,
        theirs: &str,
        message: &str,
        author: Option<&Person>,
        time: chrono::DateTime<chrono::Utc>,
        merge_text: MergeText<'_>,
        choose: Choose<'_>,
    ) -> Result<Merged> {
        let theirs = self.commit_id(theirs)?;
        let head = self.head_id()?.ok_or("The project has no commit yet to merge into.")?;
        let base = self.merge_base(head, theirs)?;
        let base_files = self.flat(base.map(|b| self.commit_tree(b)).transpose()?)?;
        let ours = self.flat(Some(self.commit_tree(head)?))?;
        let their_files = self.flat(Some(self.commit_tree(theirs)?))?;
        let paths: BTreeSet<&BString> = base_files.keys().chain(ours.keys()).chain(their_files.keys()).collect();
        let mut merged = Folder::default();
        let mut result: Flat = BTreeMap::new();
        let mut conflicts = Vec::new();
        for path in paths {
            let (b, o, t) = (base_files.get(path), ours.get(path), their_files.get(path));
            let chosen = choose(&path.to_str_lossy());
            let entry = if o == t || t == b {
                o.copied()
            } else if o == b {
                t.copied()
            } else if let Some(side) = chosen {
                match side {
                    Side::Ours => o.copied(),
                    Side::Theirs => t.copied(),
                }
            } else {
                match (b, o, t) {
                    (Some(b), Some(o), Some(t)) if o.0.is_blob() && t.0.is_blob() && b.0.is_blob() => {
                        let text = merge_text(&self.blob(b.1)?, &self.blob(o.1)?, &self.blob(t.1)?);
                        match text {
                            Some(bytes) => Some((o.0, git!("A file cannot be written into the repository", self.repo.write_blob(bytes))?.detach())),
                            None => {
                                conflicts.push((path.to_str_lossy().into_owned(), "changed on both sides"));
                                continue;
                            }
                        }
                    }
                    (_, None, _) => {
                        conflicts.push((path.to_str_lossy().into_owned(), "deleted here, changed on the remote"));
                        continue;
                    }
                    (_, _, None) => {
                        conflicts.push((path.to_str_lossy().into_owned(), "changed here, deleted on the remote"));
                        continue;
                    }
                    (None, ..) => {
                        conflicts.push((path.to_str_lossy().into_owned(), "added on both sides"));
                        continue;
                    }
                    _ => {
                        conflicts.push((path.to_str_lossy().into_owned(), "changed on both sides, and not text"));
                        continue;
                    }
                }
            };
            if let Some((mode, id)) = entry {
                merged.put(path, mode, id);
                result.insert(path.clone(), (mode, id));
            }
        }
        if !conflicts.is_empty() {
            return Ok(Merged::Conflicts(conflicts));
        }
        self.check_work_tree(&ours, &result)?;
        let root = match self.write_folder(&merged)? {
            Some(r) => r,
            None => git!("A tree cannot be written", self.repo.write_object(gix::objs::Tree::empty()))?.detach(),
        };
        let person = match author {
            Some(p) => p.clone(),
            None => self.configured_person().unwrap_or_else(|| Person::new("TeXCraft", "")),
        };
        let signature =
            gix::actor::Signature { name: person.name.into(), email: person.email.into(), time: gix::date::Time::new(time.timestamp(), 0) };
        let mut buf = gix::date::parse::TimeBuf::default();
        let sig = signature.to_ref(&mut buf);
        let id = git!("The merge cannot be committed", self.repo.commit_as(sig, sig, "HEAD", message, root, [head, theirs]))?.detach();
        self.write_work_tree(&ours, &result)?;
        self.write_index(root)?;
        Ok(Merged::Done { commit: id.to_string(), changed: self.project_changes(&ours, &result) })
    }

    /// Whether commit `id` is on a ref other than the branch checked out: a remote's branch as
    /// last fetched or pushed (`refs/remotes/…`), a tag, another branch, a stash. Such a commit is
    /// others' too, and is never rewritten.
    pub fn shared(&self, id: &str) -> Result<bool> {
        let id = self.commit_id(id)?;
        let branch = git!("The repository's HEAD cannot be read", self.repo.head_name())?;
        let refs = git!("The repository's refs cannot be read", self.repo.references())?;
        let all = git!("The repository's refs cannot be read", refs.all())?;
        for reference in all {
            let Ok(mut reference) = reference else { return Ok(true) };
            if branch.as_ref().is_some_and(|b| b.as_ref() == reference.name()) {
                continue;
            }
            let Ok(tip) = reference.peel_to_id() else { continue };
            let tip = tip.detach();
            if tip == id || self.ancestors(tip)?.contains(&id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Makes `snapshot` with `message` the commit in place of HEAD `replaced` (a commit amended:
    /// the same parents, the changes since them combined), the branch moved to it. `author`
    /// none: the repository's configured identity, else "TeXCraft". Refuses when HEAD is no
    /// longer `replaced`; a commit whose changes all went is dropped, the branch back on its
    /// parent.
    pub fn amend(
        &self,
        replaced: &str,
        snapshot: &crate::Snapshot,
        message: &str,
        author: Option<&Person>,
        time: chrono::DateTime<chrono::Utc>,
    ) -> Result<Amended> {
        let old = self.commit_id(replaced)?;
        let built = self.build(snapshot)?;
        if built.head != Some(old) {
            return Err("The branch moved since TeXCraft's last version: a new version is made instead.".to_owned());
        }
        let object = git!("A commit cannot be read", self.repo.find_object(old))?;
        let commit = git!("A commit cannot be read", object.try_into_commit())?;
        let same_message = commit.message_raw_sloppy().to_str_lossy().trim() == message.trim();
        if built.head_tree == Some(built.root) && same_message {
            return Ok(Amended::Same);
        }
        let parents = self.parents(old)?;
        let parent_tree = parents.first().map(|p| self.commit_tree(*p)).transpose()?;
        if let Some(parent) = parents.first().copied()
            && parent_tree == Some(built.root)
        {
            self.move_branch(Some(old), parent, "reset: TeXCraft's version undone")?;
            self.write_index(built.root)?;
            return Ok(Amended::Undone(parent.to_string()));
        }
        let person = match author {
            Some(p) => p.clone(),
            None => self.configured_person().unwrap_or_else(|| Person::new("TeXCraft", "")),
        };
        let signature =
            gix::actor::Signature { name: person.name.into(), email: person.email.into(), time: gix::date::Time::new(time.timestamp(), 0) };
        let new = gix::objs::Commit {
            message: message.into(),
            tree: built.root,
            author: signature.clone(),
            committer: signature,
            encoding: None,
            parents: parents.into_iter().collect(),
            extra_headers: Vec::new(),
        };
        let id = git!("The commit cannot be made", self.repo.write_object(&new))?.detach();
        self.move_branch(Some(old), id, "commit (amend): TeXCraft")?;
        self.write_index(built.root)?;
        Ok(Amended::Replaced(id.to_string()))
    }

    /// Moves the checked-out branch (or makes it, before the first commit) from `from` to `to`.
    fn move_branch(&self, from: Option<ObjectId>, to: ObjectId, why: &str) -> Result<()> {
        let name = git!("The repository's HEAD cannot be read", self.repo.head_name())?
            .ok_or("No branch is checked out (HEAD is detached): check out a branch with git first.")?;
        let previous = match from {
            Some(id) => PreviousValue::MustExistAndMatch(gix::refs::Target::Object(id)),
            None => PreviousValue::MustNotExist,
        };
        git!("The branch cannot be moved", self.repo.reference(name, to, previous, why))?;
        Ok(())
    }

    /// The path of a repository file in the work tree.
    fn on_disk(&self, path: &BString) -> Result<PathBuf> {
        let path = path.to_str().map_err(|_| format!("{} is not a name this computer can store.", path.to_str_lossy()))?;
        if path.split('/').any(|p| p.is_empty() || p == ".." || p == "." || p == ".git") {
            return Err(format!("The repository has a file at {path}, which cannot be written safely."));
        }
        Ok(self.work_dir().join(path))
    }

    /// Checks that each file that changes from `old` to `new` is on disk as in `old`, so that
    /// writing `new` loses nothing.
    fn check_work_tree(&self, old: &Flat, new: &Flat) -> Result<()> {
        let mut changed = Vec::new();
        for path in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
            let (was, will) = (old.get(path), new.get(path));
            if was == will {
                continue;
            }
            let file = self.on_disk(path)?;
            let same = match (was, std::fs::symlink_metadata(&file)) {
                (None, Err(_)) => true,
                (None, Ok(_)) => new.get(path).is_some_and(|(_, id)| self.disk_matches(&file, *id)),
                (Some((mode, _)), Err(_)) => !mode.is_blob(),
                (Some((mode, id)), Ok(_)) => !mode.is_blob() || mode.kind() == EntryKind::Link || self.disk_matches(&file, *id),
            };
            if !same {
                changed.push(path.to_str_lossy().into_owned());
            }
        }
        if changed.is_empty() {
            return Ok(());
        }
        Err(format!(
            "These files are changed on disk and not committed, or are in the way: {}. Commit them (or move them away), then try again.",
            changed.join(", ")
        ))
    }

    /// Whether the file at `file` has blob id `id`.
    fn disk_matches(&self, file: &Path, id: ObjectId) -> bool {
        std::fs::read(file).ok().and_then(|bytes| gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, &bytes).ok()) == Some(id)
    }

    /// Writes the files that change from `old` to `new` into the work tree, and removes those
    /// `new` does not have. Symbolic links and submodules are left to git.
    fn write_work_tree(&self, old: &Flat, new: &Flat) -> Result<()> {
        for path in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
            let (was, will) = (old.get(path), new.get(path));
            if was == will {
                continue;
            }
            let file = self.on_disk(path)?;
            match will {
                Some((mode, id)) if mode.is_blob() && mode.kind() != EntryKind::Link => {
                    if let Some(parent) = file.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| format!("The folder {} cannot be made: {e}", parent.display()))?;
                    }
                    std::fs::write(&file, self.blob(*id)?).map_err(|e| format!("{} cannot be written: {e}", file.display()))?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let executable = mode.kind() == EntryKind::BlobExecutable;
                        if let Ok(meta) = std::fs::metadata(&file) {
                            let mut perms = meta.permissions();
                            let bits = if executable { perms.mode() | 0o111 } else { perms.mode() & !0o111 };
                            perms.set_mode(bits);
                            let _ = std::fs::set_permissions(&file, perms);
                        }
                    }
                }
                Some(_) => {}
                None => {
                    if was.is_some_and(|(m, _)| m.is_blob()) && file.is_file() {
                        std::fs::remove_file(&file).map_err(|e| format!("{} cannot be removed: {e}", file.display()))?;
                        // Folders left empty go, as git does.
                        let mut dir = file.parent();
                        while let Some(d) = dir {
                            if d == self.work_dir() || std::fs::remove_dir(d).is_err() {
                                break;
                            }
                            dir = d.parent();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The files of the project (in its folder, from it) that differ from `old` to `new`.
    fn project_changes(&self, old: &Flat, new: &Flat) -> Vec<(String, ChangeKind)> {
        let mut out = Vec::new();
        for path in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
            let (was, will) = (old.get(path), new.get(path));
            if was == will {
                continue;
            }
            let Ok(full) = path.to_str() else { continue };
            let relative =
                if self.prefix.is_empty() { Some(full) } else { full.strip_prefix(self.prefix.as_str()).and_then(|r| r.strip_prefix('/')) };
            let Some(relative) = relative else { continue };
            let kind = match (was, will) {
                (None, _) => ChangeKind::Added,
                (_, None) => ChangeKind::Deleted,
                _ => ChangeKind::Modified,
            };
            out.push((relative.to_owned(), kind));
        }
        out
    }
}
