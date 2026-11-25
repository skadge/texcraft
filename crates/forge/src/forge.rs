//! What the sync logic needs of a forge: a project and its default branch, a branch's head and
//! whether the user can commit to it, a commit's tree with its blob ids, a blob's contents, one
//! commit of several changes, and a request to merge a branch. GitLab ([`crate::GitLab`]) is one
//! forge.

use std::future::Future;

use crate::{Address, Branch, FileChange, MergeRequest, ProjectInfo, Result, TreeEntry};

/// Where a commit goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// Branch `name`, whose head was `head` when its new commits were taken in.
    Branch { name: &'a str, head: &'a str },
    /// A new branch `name`, started from commit `start`.
    NewBranch { name: &'a str, start: &'a str },
}

/// What a commit request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Made {
    /// The commit: its id and its parents' ids.
    Commit { id: String, parents: Vec<String> },
    /// Nothing: the branch moved past the head the commit was made on, and does not take it
    /// without undoing what was committed meanwhile. Take its new commits in and try again.
    Moved,
}

/// A forge's API, as the sync logic ([`crate::open`], [`crate::pull`], [`crate::commit`]…) uses it.
pub trait Forge {
    /// The project this client reaches.
    fn address(&self) -> &Address;

    /// The project: its id, path, default branch and page.
    fn project(&self) -> impl Future<Output = Result<ProjectInfo>>;

    /// Branch `name`: the commit at its head, and whether the token's user can commit to it.
    fn branch(&self, name: &str) -> impl Future<Output = Result<Branch>>;

    /// Every entry of `commit`'s tree, its folders' too.
    fn tree(&self, commit: &str) -> impl Future<Output = Result<Vec<TreeEntry>>>;

    /// The contents of blob `id`, checked against its id; none when it is over [`crate::MAX_FILE`].
    fn blob(&self, id: &str) -> impl Future<Output = Result<Option<Vec<u8>>>>;

    /// Makes one commit of `changes` (their contents from `files`) with `message`, to `target`.
    fn commit(&self, target: Target<'_>, message: &str, changes: &[FileChange], files: &[(String, Vec<u8>)]) -> impl Future<Output = Result<Made>>;

    /// Asks for branch `source` to be merged into `target`: a merge request on GitLab.
    fn request_merge(&self, source: &str, target: &str, title: &str) -> impl Future<Output = Result<MergeRequest>>;
}
