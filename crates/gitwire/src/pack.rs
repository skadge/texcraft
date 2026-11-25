//! The objects a server lacks, and a pack of them.
//!
//! The server has every object reachable from the commits it advertises; those of them this
//! repository has are left out of the walk from the commit to send. The objects of the commits
//! at the edge (the server's commits the new ones build on) are left out too, so an unchanged
//! file or folder is not sent again. Anything else reachable from the new commits is sent: a few
//! objects too many at worst, never one too few.
//!
//! The pack holds whole objects, zlib-compressed, with no deltas (git's version 2 pack: `PACK`,
//! the version, the count, the objects, the SHA-1 of it all). Text projects are small; a server
//! stores the pack as it likes.

use std::collections::{HashSet, VecDeque};
use std::io::Write as _;

use gix::ObjectId;
use gix::objs::Kind;

use crate::{Error, Result, local};

/// Every commit reachable from `tips` (those of them this repository has).
pub fn ancestors(repo: &gix::Repository, tips: &[ObjectId]) -> Result<HashSet<ObjectId>> {
    let mut seen = HashSet::new();
    let mut queue: VecDeque<ObjectId> = tips.iter().copied().filter(|id| has_commit(repo, *id)).collect();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        for parent in parents(repo, id)? {
            if !seen.contains(&parent) && has_commit(repo, parent) {
                queue.push_back(parent);
            }
        }
    }
    Ok(seen)
}

/// Whether this repository has commit `id`.
pub fn has_commit(repo: &gix::Repository, id: ObjectId) -> bool {
    repo.find_object(id).is_ok_and(|o| o.kind == Kind::Commit)
}

/// Commit `id`'s parents.
pub fn parents(repo: &gix::Repository, id: ObjectId) -> Result<Vec<ObjectId>> {
    let commit = repo.find_object(id).map_err(|e| local("A commit cannot be read", e))?;
    let commit = commit.try_into_commit().map_err(|e| local("A commit cannot be read", e))?;
    Ok(commit.parent_ids().map(gix::Id::detach).collect())
}

fn tree_of(repo: &gix::Repository, commit: ObjectId) -> Result<ObjectId> {
    let commit = repo.find_object(commit).map_err(|e| local("A commit cannot be read", e))?;
    let commit = commit.try_into_commit().map_err(|e| local("A commit cannot be read", e))?;
    Ok(commit.tree_id().map_err(|e| local("A commit cannot be read", e))?.detach())
}

/// Adds tree `id` and everything in it (not submodules' commits) to `out`, but what `skip` has.
fn tree_objects(repo: &gix::Repository, id: ObjectId, skip: &HashSet<ObjectId>, out: &mut Vec<ObjectId>, seen: &mut HashSet<ObjectId>) -> Result<()> {
    if skip.contains(&id) || !seen.insert(id) {
        return Ok(());
    }
    out.push(id);
    let tree = repo.find_object(id).map_err(|e| local("A folder cannot be read", e))?;
    let tree = tree.try_into_tree().map_err(|e| local("A folder cannot be read", e))?;
    let decoded = tree.decode().map_err(|e| local("A folder cannot be read", e))?;
    let entries: Vec<(bool, bool, ObjectId)> = decoded.entries.iter().map(|e| (e.mode.is_tree(), e.mode.is_commit(), e.oid.to_owned())).collect();
    for (is_tree, is_commit, oid) in entries {
        if is_commit {
            continue;
        }
        if is_tree {
            tree_objects(repo, oid, skip, out, seen)?;
        } else if !skip.contains(&oid) && seen.insert(oid) {
            out.push(oid);
        }
    }
    Ok(())
}

/// The objects to send so that a server that has `theirs` (commits, those this repository does
/// not have ignored) has `tip` and all it needs: commits first, newest first, then their trees
/// and files.
pub fn objects_to_send(repo: &gix::Repository, tip: ObjectId, theirs: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let have = ancestors(repo, theirs)?;
    let mut commits = Vec::new();
    let mut edge = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([tip]);
    while let Some(id) = queue.pop_front() {
        if have.contains(&id) {
            edge.push(id);
            continue;
        }
        if !seen.insert(id) {
            continue;
        }
        if !has_commit(repo, id) {
            return Err(Error::Local(format!("The repository lacks commit {id}, which the branch needs: it cannot be sent.")));
        }
        commits.push(id);
        queue.extend(parents(repo, id)?);
    }
    // What the server has at the edge is not sent again.
    let mut skip = HashSet::new();
    let mut skipped = Vec::new();
    for id in edge {
        tree_objects(repo, tree_of(repo, id)?, &HashSet::new(), &mut skipped, &mut skip)?;
    }
    let mut out = commits.clone();
    let mut seen = HashSet::new();
    for id in commits {
        tree_objects(repo, tree_of(repo, id)?, &skip, &mut out, &mut seen)?;
    }
    Ok(out)
}

/// A pack's type number for an object kind.
fn type_number(kind: Kind) -> u8 {
    match kind {
        Kind::Commit => 1,
        Kind::Tree => 2,
        Kind::Blob => 3,
        Kind::Tag => 4,
    }
}

/// Appends an entry's header: its type and size, the size in 7-bit groups after the first four
/// bits, low ones first.
fn entry_header(out: &mut Vec<u8>, kind: Kind, size: usize) {
    let mut size = size as u64;
    let mut byte = (type_number(kind) << 4) | (size & 0x0f) as u8;
    size >>= 4;
    while size > 0 {
        out.push(byte | 0x80);
        byte = (size & 0x7f) as u8;
        size >>= 7;
    }
    out.push(byte);
}

/// `data` zlib-compressed.
fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut writer = gix::zlib::stream::deflate::Write::new(Vec::with_capacity(data.len() / 2 + 64), gix::zlib::Compression::DEFAULT);
    writer.write_all(data).map_err(|e| local("A file cannot be compressed", e))?;
    writer.flush().map_err(|e| local("A file cannot be compressed", e))?;
    Ok(writer.into_inner())
}

/// A pack of objects `ids` of the repository.
pub fn write(repo: &gix::Repository, ids: &[ObjectId]) -> Result<Vec<u8>> {
    let count = u32::try_from(ids.len()).map_err(|_| Error::Local("Too many objects to send in one pack.".to_owned()))?;
    let mut out = Vec::new();
    out.extend_from_slice(b"PACK");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    for id in ids {
        let object = repo.find_object(*id).map_err(|e| local("An object cannot be read from the repository", e))?;
        entry_header(&mut out, object.kind, object.data.len());
        out.extend_from_slice(&deflate(&object.data)?);
    }
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(&out);
    let sum = hasher.try_finalize().map_err(|e| local("The pack's checksum cannot be made", e))?;
    out.extend_from_slice(sum.as_bytes());
    Ok(out)
}
