//! What the sync logic needs of a forge: a project and its default branch, a branch's head and
//! whether the user can commit to it, a commit's tree with its blob ids, a blob's contents, one
//! commit of several changes, and a request to merge a branch. GitLab ([`GitLab`]) and GitHub
//! ([`GitHub`]) are forges; [`Client`] is either, as an address says.

use std::future::Future;

use crate::{Address, Branch, FileChange, GitHub, GitLab, MergeRequest, ProjectInfo, Result, Service, Token, Transport, TreeEntry};

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

    /// Asks for branch `source` to be merged into `target`: a merge request on GitLab, a pull
    /// request on GitHub.
    fn request_merge(&self, source: &str, target: &str, title: &str) -> impl Future<Output = Result<MergeRequest>>;
}

/// A client of the forge an [`Address`] is on: GitLab or GitHub.
pub enum Client {
    GitLab(GitLab),
    GitHub(GitHub),
}

impl Client {
    /// A client of `address`'s project, through `transport`, sending `token`.
    pub fn new(transport: Box<dyn Transport>, address: Address, token: Token) -> Self {
        match address.service {
            Service::GitLab => Client::GitLab(GitLab::new(transport, address, token)),
            Service::GitHub => Client::GitHub(GitHub::new(transport, address, token)),
        }
    }
}

impl Forge for Client {
    fn address(&self) -> &Address {
        match self {
            Client::GitLab(c) => Forge::address(c),
            Client::GitHub(c) => Forge::address(c),
        }
    }

    async fn project(&self) -> Result<ProjectInfo> {
        match self {
            Client::GitLab(c) => Forge::project(c).await,
            Client::GitHub(c) => Forge::project(c).await,
        }
    }

    async fn branch(&self, name: &str) -> Result<Branch> {
        match self {
            Client::GitLab(c) => Forge::branch(c, name).await,
            Client::GitHub(c) => Forge::branch(c, name).await,
        }
    }

    async fn tree(&self, commit: &str) -> Result<Vec<TreeEntry>> {
        match self {
            Client::GitLab(c) => Forge::tree(c, commit).await,
            Client::GitHub(c) => Forge::tree(c, commit).await,
        }
    }

    async fn blob(&self, id: &str) -> Result<Option<Vec<u8>>> {
        match self {
            Client::GitLab(c) => Forge::blob(c, id).await,
            Client::GitHub(c) => Forge::blob(c, id).await,
        }
    }

    async fn commit(&self, target: Target<'_>, message: &str, changes: &[FileChange], files: &[(String, Vec<u8>)]) -> Result<Made> {
        match self {
            Client::GitLab(c) => Forge::commit(c, target, message, changes, files).await,
            Client::GitHub(c) => Forge::commit(c, target, message, changes, files).await,
        }
    }

    async fn request_merge(&self, source: &str, target: &str, title: &str) -> Result<MergeRequest> {
        match self {
            Client::GitLab(c) => Forge::request_merge(c, source, target, title).await,
            Client::GitHub(c) => Forge::request_merge(c, source, target, title).await,
        }
    }
}
