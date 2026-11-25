//! The history in a git repository on disk, through gitoxide (no network, no work-tree scans).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gix::ObjectId;
use gix::bstr::{BString, ByteSlice};
use gix::objs::tree::{EntryKind, EntryMode};

use crate::{ChangeKind, Entry, GITIGNORE, OURS_KEY, Owner, Person, Result, Snapshot, is_hidden};

/// The repository a project folder is in.
pub struct History {
    repo: gix::Repository,
    owner: Owner,
    /// The project's folder from the top of the work tree, `/`-separated; empty at the top.
    prefix: String,
}

/// A git error as a message that says what could not be done.
macro_rules! git {
    ($what:expr, $e:expr) => {
        $e.map_err(|e| format!("{}: {e}", $what))
    };
}

/// A folder of the tree being built: names to files (mode, blob) or folders.
#[derive(Default)]
struct Folder(BTreeMap<BString, Node>);

enum Node {
    File(EntryMode, ObjectId),
    Folder(Folder),
}

impl Folder {
    /// Puts a file at `path` (`/`-separated), replacing whatever is in its way.
    fn put(&mut self, path: &[u8], mode: EntryMode, id: ObjectId) {
        match path.find_byte(b'/') {
            None => {
                self.0.insert(path.into(), Node::File(mode, id));
            }
            Some(at) => {
                let node = self.0.entry(path[..at].into()).or_insert_with(|| Node::Folder(Folder::default()));
                if let Node::File(..) = node {
                    *node = Node::Folder(Folder::default());
                }
                if let Node::Folder(folder) = node {
                    folder.put(&path[at + 1..], mode, id);
                }
            }
        }
    }
}

impl History {
    /// The repository `dir` is in, if it is in one (its own, or a folder's above it).
    pub fn find(dir: &Path) -> Result<Option<Self>> {
        let dir = std::fs::canonicalize(dir).map_err(|e| format!("The folder {} cannot be read: {e}", dir.display()))?;
        if !dir.ancestors().any(|d| d.join(".git").exists()) {
            return Ok(None);
        }
        let repo = git!("The project's git repository cannot be opened", gix::discover(&dir))?;
        let top = repo.workdir().ok_or("The project's folder is inside a git repository without files (a bare one).")?;
        let top = std::fs::canonicalize(top).map_err(|e| format!("The repository's folder {} cannot be read: {e}", top.display()))?;
        let relative = dir.strip_prefix(&top).map_err(|_| "The project's folder is not inside its repository's folder.".to_owned())?;
        let parts: Option<Vec<&str>> = relative.components().map(|c| c.as_os_str().to_str()).collect();
        let prefix = parts.ok_or("The project's folder has a name git cannot store.")?.join("/");
        let ours = prefix.is_empty() && repo.config_snapshot().boolean(OURS_KEY) == Some(true);
        Ok(Some(Self { repo, owner: if ours { Owner::Ours } else { Owner::Theirs }, prefix }))
    }

    /// The repository `dir` is in, or a new one made in `dir` (branch `main`, the build files
    /// ignored, marked as TeXCraft's) when it is in none.
    pub fn open_or_init(dir: &Path) -> Result<Self> {
        if let Some(history) = Self::find(dir)? {
            return Ok(history);
        }
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, GITIGNORE).map_err(|e| format!("writing {}: {e}", ignore.display()))?;
        }
        git!("The project's git repository cannot be made", gix::init(dir))?;
        let git_dir = dir.join(".git");
        // `main`, whatever the user's `init.defaultBranch` says.
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").map_err(|e| format!("writing {}: {e}", git_dir.display()))?;
        let config = git_dir.join("config");
        let mut text = std::fs::read_to_string(&config).map_err(|e| format!("reading {}: {e}", config.display()))?;
        let (section, key) = OURS_KEY.split_once('.').unwrap_or((OURS_KEY, "on"));
        text.push_str(&format!("[{section}]\n\t{key} = true\n"));
        std::fs::write(&config, text).map_err(|e| format!("writing {}: {e}", config.display()))?;
        Self::find(dir)?.ok_or_else(|| "The project's new git repository cannot be found.".to_owned())
    }

    pub fn owner(&self) -> Owner {
        self.owner
    }

    /// The repository's `.git` folder.
    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    /// The top of the repository's work tree.
    pub fn work_dir(&self) -> PathBuf {
        self.repo.workdir().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The project's folder from the top of the work tree (`/`-separated, empty at the top).
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The checked-out branch (`main`), none when detached.
    pub fn branch(&self) -> Option<String> {
        let head = self.repo.head_name().ok()??;
        Some(head.shorten().to_string())
    }

    /// The commit checked out, none before the first.
    pub fn head(&self) -> Result<Option<String>> {
        Ok(self.head_id()?.map(|id| id.to_string()))
    }

    fn head_id(&self) -> Result<Option<ObjectId>> {
        let mut head = git!("The repository's HEAD cannot be read", self.repo.head())?;
        Ok(head.id().map(gix::Id::detach).or_else(|| head.peel_to_commit().ok().map(|c| c.id)))
    }

    /// Commits `snapshot` with `message`. `author` none: the repository's configured identity,
    /// else "TeXCraft". Returns the new commit's id, or none when the project is as in HEAD.
    ///
    /// In someone else's repository, refuses while changes are staged in git: the commit rewrites
    /// the index from its tree, which would unstage them.
    pub fn commit(&self, snapshot: &Snapshot, message: &str, author: Option<&Person>, time: chrono::DateTime<chrono::Utc>) -> Result<Option<String>> {
        let head = self.head_id()?;
        let head_tree = head.map(|c| self.commit_tree(c)).transpose()?;
        if self.owner == Owner::Theirs && self.staged(head_tree)? {
            return Err("Some changes are staged in git: commit them with git first.".to_owned());
        }
        let old = match head_tree {
            Some(t) => self.subtree(t, &self.prefix)?,
            None => None,
        };
        let mut before = BTreeMap::new();
        self.flatten(old, "", &mut before)?;

        let owned = |p: &str| snapshot.owned_dirs.iter().any(|d| p == d || p.strip_prefix(d.as_str()).is_some_and(|r| r.starts_with('/')));
        let mut tree = Folder::default();
        // What the project does not hold keeps its version: hidden files, links, submodules, names
        // that are not UTF-8.
        for (path, (mode, id)) in &before {
            let kept = match path.to_str() {
                Ok(p) => (is_hidden(p) && !owned(p)) || !mode.is_blob(),
                Err(_) => true,
            };
            if kept {
                tree.put(path, *mode, *id);
            }
        }
        let index = git!("The repository's index cannot be read", self.repo.index_or_empty())?;
        let mut excludes = git!(
            "The repository's ignore rules cannot be read",
            self.repo.excludes(&index, None, gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped)
        )?;
        for (path, bytes) in &snapshot.files {
            let tracked = before.get(path.as_bytes().as_bstr());
            if tracked.is_none() {
                // A file git does not track yet and the ignore rules leave out stays out.
                let full = self.full_path(path);
                let ignored = git!("The ignore rules cannot be checked", excludes.at_entry(full.as_str(), None))?.is_excluded();
                if ignored {
                    continue;
                }
            }
            let mode = match tracked {
                Some((m, _)) if m.kind() == EntryKind::BlobExecutable => *m,
                _ => EntryKind::Blob.into(),
            };
            let id = git!("A file cannot be written into the repository", self.repo.write_blob(bytes))?.detach();
            tree.put(path.as_bytes(), mode, id);
        }
        // In our repository, the ignore file is the one on disk (written when it was made; the
        // user may change it), unless the snapshot has its own.
        if self.owner == Owner::Ours && !snapshot.files.iter().any(|(p, _)| p == ".gitignore") {
            let on_disk = self.work_dir().join(".gitignore");
            if let Ok(bytes) = std::fs::read(&on_disk) {
                let id = git!("A file cannot be written into the repository", self.repo.write_blob(bytes))?.detach();
                tree.put(b".gitignore", EntryKind::Blob.into(), id);
            }
        }
        let new = self.write_folder(&tree)?;
        let components: Vec<&str> = self.prefix.split('/').filter(|c| !c.is_empty()).collect();
        let root = self.splice(head_tree, &components, new)?;
        let root = match root {
            Some(r) => r,
            None => git!("A tree cannot be written", self.repo.write_object(gix::objs::Tree::empty()))?.detach(),
        };
        if Some(root) == head_tree || (head.is_none() && new.is_none()) {
            return Ok(None);
        }
        let person = match author {
            Some(p) => p.clone(),
            None => self.configured_person().unwrap_or_else(|| Person::new("TeXCraft", "")),
        };
        let signature =
            gix::actor::Signature { name: person.name.into(), email: person.email.into(), time: gix::date::Time::new(time.timestamp(), 0) };
        let mut buf = gix::date::parse::TimeBuf::default();
        let sig = signature.to_ref(&mut buf);
        let id = git!("The commit cannot be made", self.repo.commit_as(sig, sig, "HEAD", message, root, head))?.detach();
        self.write_index(root)?;
        Ok(Some(id.to_string()))
    }

    /// Whether the index differs from tree `head` (what `git diff --cached` would show).
    fn staged(&self, head: Option<ObjectId>) -> Result<bool> {
        if !self.repo.index_path().exists() {
            return Ok(false);
        }
        let index = git!("The repository's index cannot be read", self.repo.open_index())?;
        let expected = match head {
            Some(tree) => git!("The repository's index cannot be compared", self.repo.index_from_tree(&tree))?,
            None => return Ok(!index.entries().is_empty()),
        };
        let entries = |state: &gix::index::State| -> Vec<(BString, ObjectId, gix::index::entry::Mode, gix::index::entry::Stage)> {
            state.entries().iter().map(|e| (e.path(state).to_owned(), e.id, e.mode, e.stage())).collect()
        };
        Ok(entries(&index) != entries(&expected))
    }

    /// The identity of the repository's configuration (`user.name`, `user.email`), if any.
    fn configured_person(&self) -> Option<Person> {
        let sig = self.repo.committer()?.ok()?;
        Some(Person::new(sig.name.to_string(), sig.email.to_string()))
    }

    /// The index made to match commit tree `root`, so `git status` shows what changed since.
    /// Entries whose file did not change keep what git knew of them on disk.
    fn write_index(&self, root: ObjectId) -> Result<()> {
        let mut index = git!("The repository's index cannot be made", self.repo.index_from_tree(&root))?;
        if let Ok(old) = self.repo.open_index() {
            for (entry, path) in index.entries_mut_with_paths() {
                if let Some(was) = old.entry_by_path(path)
                    && was.id == entry.id
                    && was.mode == entry.mode
                {
                    entry.stat = was.stat;
                }
            }
        }
        let mut bytes = Vec::new();
        git!("The repository's index cannot be written", index.write_to(&mut bytes, gix::index::write::Options::default()))?;
        // As git does: through `index.lock`, which also keeps two writers apart.
        let path = index.path().to_path_buf();
        let lock = path.with_extension("lock");
        let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).and_then(|mut f| {
            use std::io::Write;
            f.write_all(&bytes)?;
            f.sync_all()
        });
        let renamed = written.and_then(|()| std::fs::rename(&lock, &path));
        renamed.map_err(|e| {
            let _ = std::fs::remove_file(&lock);
            format!("The repository's index {} cannot be written: {e}", path.display())
        })
    }

    /// The commits that changed the project, newest first, at most `limit`.
    pub fn log(&self, limit: usize) -> Result<Vec<Entry>> {
        let Some(head) = self.head_id()? else { return Ok(Vec::new()) };
        let walk = git!(
            "The history cannot be read",
            self.repo.rev_walk([head]).sorting(gix::revision::walk::Sorting::ByCommitTime(Default::default())).all()
        )?;
        let mut out = Vec::new();
        for info in walk {
            if out.len() >= limit {
                break;
            }
            let info = git!("The history cannot be read", info)?;
            let commit = git!("A commit cannot be read", info.object())?;
            let tree = git!("A commit cannot be read", commit.tree_id())?.detach();
            let parent = commit.parent_ids().next().map(gix::Id::detach);
            let new = self.subtree(tree, &self.prefix)?;
            let old = match parent {
                Some(p) => self.subtree(self.commit_tree(p)?, &self.prefix)?,
                None => None,
            };
            // In someone's repository, only the commits that touched the project.
            if !self.prefix.is_empty() && old == new {
                continue;
            }
            let mut changes = Vec::new();
            self.diff(old, new, "", &mut changes)?;
            changes.sort();
            let author = git!("A commit cannot be read", commit.author())?;
            let seconds = git!("A commit's time cannot be read", author.time())?.seconds;
            out.push(Entry {
                id: commit.id.to_string(),
                parent: parent.map(|p| p.to_string()),
                time: chrono::DateTime::from_timestamp(seconds, 0).unwrap_or_default(),
                author: author.name.to_string(),
                message: commit.message_raw_sloppy().to_str_lossy().trim().to_owned(),
                changes,
            });
        }
        Ok(out)
    }

    /// File `path` of the project at commit `commit` (a full id); none when it had no such file.
    pub fn file_at(&self, commit: &str, path: &str) -> Result<Option<Vec<u8>>> {
        let tree = self.commit_tree(self.parse(commit)?)?;
        let full = self.full_path(path);
        let mut at = tree;
        let parts: Vec<&str> = full.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            let Some((mode, id)) = self.entries(at)?.remove(part.as_bytes().as_bstr()) else { return Ok(None) };
            let last = i + 1 == parts.len();
            match (last, mode.is_blob(), mode.is_tree()) {
                (true, true, _) => return Ok(Some(self.blob(id)?)),
                (false, _, true) => at = id,
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    /// The project's files at commit `commit` (a full id): those a project loads, not hidden ones.
    pub fn files_at(&self, commit: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let tree = self.commit_tree(self.parse(commit)?)?;
        let mut flat = BTreeMap::new();
        self.flatten(self.subtree(tree, &self.prefix)?, "", &mut flat)?;
        let mut out = Vec::new();
        for (path, (mode, id)) in flat {
            let Ok(path) = path.to_str() else { continue };
            if mode.is_blob() && !is_hidden(path) {
                out.push((path.to_owned(), self.blob(id)?));
            }
        }
        Ok(out)
    }

    /// The first parent of commit `commit` (a full id); none for a first commit.
    pub fn parent_of(&self, commit: &str) -> Result<Option<String>> {
        let object = git!("A commit cannot be read", self.repo.find_object(self.parse(commit)?))?;
        let commit = git!("A commit cannot be read", object.try_into_commit())?;
        Ok(commit.parent_ids().next().map(|id| id.detach().to_string()))
    }

    fn full_path(&self, path: &str) -> String {
        if self.prefix.is_empty() { path.to_owned() } else { format!("{}/{path}", self.prefix) }
    }

    fn parse(&self, commit: &str) -> Result<ObjectId> {
        let id = ObjectId::from_hex(commit.trim().as_bytes()).map_err(|_| format!("`{commit}` is not a commit id (40 hexadecimal digits)."))?;
        let object = self.repo.find_object(id).map_err(|_| format!("There is no commit {commit} in the project's history."))?;
        if object.kind != gix::objs::Kind::Commit {
            return Err(format!("{commit} is not a commit."));
        }
        Ok(id)
    }

    fn commit_tree(&self, commit: ObjectId) -> Result<ObjectId> {
        let object = git!("A commit cannot be read", self.repo.find_object(commit))?;
        let commit = git!("A commit cannot be read", object.try_into_commit())?;
        Ok(git!("A commit cannot be read", commit.tree_id())?.detach())
    }

    fn blob(&self, id: ObjectId) -> Result<Vec<u8>> {
        let object = git!("A file cannot be read from the repository", self.repo.find_object(id))?;
        Ok(object.detach().data)
    }

    /// The entries of tree `id` by name.
    fn entries(&self, id: ObjectId) -> Result<BTreeMap<BString, (EntryMode, ObjectId)>> {
        let object = git!("A folder cannot be read from the repository", self.repo.find_object(id))?;
        let tree = git!("A folder cannot be read from the repository", object.try_into_tree())?;
        let decoded = git!("A folder cannot be read from the repository", tree.decode())?;
        Ok(decoded.entries.iter().map(|e| (e.filename.to_owned(), (e.mode, e.oid.to_owned()))).collect())
    }

    /// The tree at `prefix` (`/`-separated) under `root`, if there is one.
    fn subtree(&self, root: ObjectId, prefix: &str) -> Result<Option<ObjectId>> {
        let mut at = root;
        for part in prefix.split('/').filter(|p| !p.is_empty()) {
            match self.entries(at)?.remove(part.as_bytes().as_bstr()) {
                Some((mode, id)) if mode.is_tree() => at = id,
                _ => return Ok(None),
            }
        }
        Ok(Some(at))
    }

    /// Every entry that is not a folder under tree `id`, by path below `base`.
    fn flatten(&self, id: Option<ObjectId>, base: &str, out: &mut BTreeMap<BString, (EntryMode, ObjectId)>) -> Result<()> {
        let Some(id) = id else { return Ok(()) };
        for (name, (mode, oid)) in self.entries(id)? {
            let path = join(base, &name);
            if mode.is_tree() {
                self.flatten(Some(oid), &path.to_str_lossy(), out)?;
            } else {
                out.insert(path, (mode, oid));
            }
        }
        Ok(())
    }

    /// The files that differ between trees `old` and `new`, by path below `base`.
    fn diff(&self, old: Option<ObjectId>, new: Option<ObjectId>, base: &str, out: &mut Vec<(String, ChangeKind)>) -> Result<()> {
        if old == new {
            return Ok(());
        }
        let read = |id: Option<ObjectId>| id.map(|i| self.entries(i)).transpose().map(Option::unwrap_or_default);
        let (old, new) = (read(old)?, read(new)?);
        let names: std::collections::BTreeSet<&BString> = old.keys().chain(new.keys()).collect();
        for name in names {
            let path = join(base, name).to_str_lossy().into_owned();
            let a = old.get(name).copied();
            let b = new.get(name).copied();
            if a == b {
                continue;
            }
            let tree = |e: Option<(EntryMode, ObjectId)>| e.filter(|(m, _)| m.is_tree()).map(|(_, id)| id);
            let file = |e: Option<(EntryMode, ObjectId)>| e.filter(|(m, _)| !m.is_tree());
            self.diff(tree(a), tree(b), &path, out)?;
            match (file(a), file(b)) {
                (Some(_), Some(_)) => out.push((path, ChangeKind::Modified)),
                (None, Some(_)) => out.push((path, ChangeKind::Added)),
                (Some(_), None) => out.push((path, ChangeKind::Deleted)),
                (None, None) => {}
            }
        }
        Ok(())
    }

    /// Writes `folder` as trees; none when it holds no file.
    fn write_folder(&self, folder: &Folder) -> Result<Option<ObjectId>> {
        let mut tree = gix::objs::Tree::empty();
        for (name, node) in &folder.0 {
            let entry = match node {
                Node::File(mode, id) => Some((*mode, *id)),
                Node::Folder(sub) => self.write_folder(sub)?.map(|id| (EntryKind::Tree.into(), id)),
            };
            if let Some((mode, oid)) = entry {
                tree.entries.push(gix::objs::tree::Entry { mode, filename: name.clone(), oid });
            }
        }
        if tree.entries.is_empty() {
            return Ok(None);
        }
        tree.entries.sort();
        Ok(Some(git!("A folder cannot be written into the repository", self.repo.write_object(&tree))?.detach()))
    }

    /// Tree `root` with the tree at `path` replaced by `new` (removed when none); none when
    /// nothing is left.
    fn splice(&self, root: Option<ObjectId>, path: &[&str], new: Option<ObjectId>) -> Result<Option<ObjectId>> {
        let Some((first, rest)) = path.split_first() else { return Ok(new) };
        let mut entries = match root {
            Some(r) => self.entries(r)?,
            None => BTreeMap::new(),
        };
        let name: BString = first.as_bytes().into();
        let child = entries.remove(&name).filter(|(m, _)| m.is_tree()).map(|(_, id)| id);
        if let Some(id) = self.splice(child, rest, new)? {
            entries.insert(name, (EntryKind::Tree.into(), id));
        }
        if entries.is_empty() {
            return Ok(None);
        }
        let mut tree = gix::objs::Tree::empty();
        tree.entries = entries.into_iter().map(|(filename, (mode, oid))| gix::objs::tree::Entry { mode, filename, oid }).collect();
        tree.entries.sort();
        Ok(Some(git!("A folder cannot be written into the repository", self.repo.write_object(&tree))?.detach()))
    }
}

fn join(base: &str, name: &BString) -> BString {
    if base.is_empty() {
        name.clone()
    } else {
        let mut path = BString::from(base);
        path.push(b'/');
        path.extend_from_slice(name);
        path
    }
}
