//! Scratch repositories and commits made with gix.

use std::path::{Path, PathBuf};

use gix::ObjectId;
use gix::bstr::BString;
use gix::objs::tree::{EntryKind, EntryMode};

pub type R<T> = Result<T, String>;

/// A new empty scratch folder, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("texcraft-gitwire-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self(dir))
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A repository with a work tree at `dir`; its `.git` folder is returned.
pub fn init(dir: &Path) -> R<PathBuf> {
    gix::init(dir).map_err(|e| e.to_string())?;
    Ok(dir.join(".git"))
}

/// A bare repository at `dir`.
pub fn init_bare(dir: &Path) -> R<PathBuf> {
    gix::init_bare(dir).map_err(|e| e.to_string())?;
    Ok(dir.to_path_buf())
}

/// Writes a tree of `files` (path, text; one level of folders at most) into `git_dir`.
pub fn tree(git_dir: &Path, files: &[(&str, &str)]) -> R<ObjectId> {
    let repo = gix::open(git_dir).map_err(|e| e.to_string())?;
    let mut root = gix::objs::Tree::empty();
    let mut folders: std::collections::BTreeMap<String, gix::objs::Tree> = Default::default();
    for (path, text) in files {
        let id = repo.write_blob(text.as_bytes()).map_err(|e| e.to_string())?.detach();
        let entry = |name: &str| gix::objs::tree::Entry { mode: EntryMode::from(EntryKind::Blob), filename: BString::from(name), oid: id };
        match path.split_once('/') {
            Some((folder, name)) => folders.entry(folder.to_owned()).or_insert_with(gix::objs::Tree::empty).entries.push(entry(name)),
            None => root.entries.push(entry(path)),
        }
    }
    for (name, mut folder) in folders {
        folder.entries.sort();
        let id = repo.write_object(&folder).map_err(|e| e.to_string())?.detach();
        root.entries.push(gix::objs::tree::Entry { mode: EntryMode::from(EntryKind::Tree), filename: BString::from(name), oid: id });
    }
    root.entries.sort();
    Ok(repo.write_object(&root).map_err(|e| e.to_string())?.detach())
}

/// Makes a commit of `files` on `refs/heads/main` of `git_dir`, after its current head; its id.
pub fn commit(git_dir: &Path, files: &[(&str, &str)], message: &str) -> R<ObjectId> {
    let tree = tree(git_dir, files)?;
    let repo = gix::open(git_dir).map_err(|e| e.to_string())?;
    let parent = head(git_dir, "refs/heads/main");
    let signature = gix::actor::Signature { name: "Test".into(), email: "test@example.org".into(), time: gix::date::Time::new(1_700_000_000, 0) };
    let mut buf = gix::date::parse::TimeBuf::default();
    let sig = signature.to_ref(&mut buf);
    Ok(repo.commit_as(sig, sig, "refs/heads/main", message, tree, parent).map_err(|e| e.to_string())?.detach())
}

/// The commit `reference` points at in `git_dir`, if any.
pub fn head(git_dir: &Path, reference: &str) -> Option<ObjectId> {
    let repo = gix::open(git_dir).ok()?;
    let mut r = repo.try_find_reference(reference).ok()??;
    r.peel_to_id().ok().map(gix::Id::detach)
}

/// Whether `git_dir` has object `id` with the same bytes as `other` has.
pub fn same_object(git_dir: &Path, other: &Path, id: ObjectId) -> bool {
    let (Ok(a), Ok(b)) = (gix::open(git_dir), gix::open(other)) else { return false };
    match (a.find_object(id), b.find_object(id)) {
        (Ok(x), Ok(y)) => x.kind == y.kind && x.data == y.data,
        _ => false,
    }
}

/// Every object reachable from commit `tip` in `git_dir`.
pub fn reachable(git_dir: &Path, tip: ObjectId) -> R<Vec<ObjectId>> {
    let repo = gix::open(git_dir).map_err(|e| e.to_string())?;
    texcraft_gitwire::pack::objects_to_send(&repo, tip, &[]).map_err(|e| e.to_string())
}
