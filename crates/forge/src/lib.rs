//! Projects on a git forge (docs/forges.md; docs/ROADMAP.md, C5): GitLab, over its REST API
//! (`/api/v4`), and GitHub, over its REST API and its Git Data API, both of which answer browsers
//! too. A project's branch is read as its tree and its files' blobs, and changes go back as one
//! commit. No git client is needed, on the desktop or in the browser. The sync logic is written
//! once over a [`Forge`].
//!
//! The logic is plain async Rust over a [`Transport`] (request in, response out): the desktop's
//! blocking HTTP client on a thread of its own, the browser's `fetch`, or a fake in tests
//! ([`fake`]). [`task`] runs an operation either way and hands back its outcome when it ends.
//!
//! A project opened from a forge remembers where it came from ([`Link`]): the branch, the commit
//! its files are at (the base) and each file's git blob id there. A file has changed when its
//! blob id differs from its base's, so no copy of the base is kept.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod address;
pub mod fake;
mod forge;
mod github;
mod gitlab;
mod http;
pub mod merge;
mod sync;
pub mod task;

pub use address::{Address, GITHUB_COM, GITLAB_COM, Service, encode, encode_path};
pub use forge::{Client, Forge, Made, Target};
pub use github::GitHub;
pub use gitlab::{Branch, GitLab, ProjectInfo, TreeEntry};
pub use http::{Request, Response, Sending, Transport, transport};
pub use sync::{
    ChangeKind, Choice, Choices, Committed, Conflict, ConflictKind, FileChange, Link, Loaded, MergeRequest, Pulled, REVIEW_DIR, Skipped, Update,
    changes, check_branch_name, commit, commit_to_new_branch, is_review, open, pull,
};

/// Files larger than this are not loaded (datasets, videos): the project opens without them.
pub const MAX_FILE: u64 = 10 * 1024 * 1024;
/// Projects larger than this are refused: their files would all be in memory.
pub const MAX_PROJECT: u64 = 200 * 1024 * 1024;
/// Files fetched at once.
pub const PARALLEL: usize = 8;

/// A personal access token. It is only ever sent to the forge it was given for: it is never
/// printed ([`std::fmt::Debug`] hides it), stored or logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// `token`, trimmed; refused when empty or when it has spaces or control characters (it goes
    /// into a header).
    pub fn new(token: &str) -> Result<Self> {
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::Token("Give a personal access token: the forge's API needs one to open a project and commit to it.".to_owned()));
        }
        if token.len() > 1024 || !token.chars().all(|c| c.is_ascii_graphic()) {
            return Err(Error::Token("This is not an access token: it has spaces or characters a token does not have.".to_owned()));
        }
        Ok(Self(token.to_owned()))
    }

    pub(crate) fn header(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

/// What went wrong, in words a user can act on ([`std::fmt::Display`]). `forge` is the service
/// that answered, named in the words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The project's address cannot be read.
    Address(String),
    /// The token cannot be one.
    Token(String),
    /// 401: the token was not accepted.
    Unauthorized { forge: Service },
    /// 403: the token or the user's role does not allow `what`.
    Forbidden { forge: Service, what: String },
    /// 404: the forge has no `what`, or the token cannot see it.
    NotFound { forge: Service, what: String },
    /// 429, or GitHub's 403 with no requests left: too many requests; try again after so many
    /// seconds, when the forge says.
    RateLimited { forge: Service, retry_after: Option<u64> },
    /// Another refusal (400, 409, 422…), with the forge's own message.
    Refused { forge: Service, status: u16, message: String },
    /// 5xx, or a status the API does not answer with.
    Server { forge: Service, status: u16 },
    /// No answer: the network, the instance's name, its certificate.
    Network { forge: Service, why: String },
    /// An answer that cannot be read as the API's.
    Invalid(String),
    /// The project's files add up to more than [`MAX_PROJECT`].
    TooLarge { bytes: u64 },
    /// The forge lists only part of the project's files (GitHub's tree of a huge repository).
    TooManyFiles { forge: Service },
    /// The branch has commits since the base that change files changed here too, in ways that do
    /// not merge.
    Conflict { forge: Service, branch: String, files: Vec<Conflict> },
    /// The branch does not take commits from this user: it is protected, or their role cannot push.
    CannotPush { forge: Service, branch: String },
    /// The branch kept moving while the commit was made: others are committing to it.
    Busy { branch: String },
    /// A new branch's name git does not accept.
    BranchName(String),
    /// There is nothing to commit.
    Nothing,
    /// The operation's thread ended without an outcome.
    Stopped,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Address(why) => write!(f, "This is not a project's address: {why}"),
            Error::Token(why) => f.write_str(why),
            Error::Unauthorized { forge } => write!(
                f,
                "{} did not accept the access token: check that it is the whole token, and that it has not expired or been revoked.",
                forge.name()
            ),
            Error::Forbidden { forge: Service::GitLab, what } => write!(
                f,
                "GitLab refused {what}: the access token needs the read_api scope to open a project and the api scope to commit, and your role in the project must allow it."
            ),
            Error::Forbidden { forge: Service::GitHub, what } => write!(
                f,
                "GitHub refused {what}: a fine-grained access token needs access to this repository with the Contents permission (read-only to open it, read and write to commit) and Pull requests (read and write) to open a pull request; a classic token needs the repo scope. Your access to the repository must allow it too."
            ),
            Error::NotFound { forge: Service::GitHub, what } => write!(
                f,
                "GitHub has no {what}, or the access token cannot see it (a fine-grained token sees only the repositories it was given): check the address."
            ),
            Error::NotFound { forge, what } => write!(f, "{} has no {what}, or the access token cannot see it: check the address.", forge.name()),
            Error::RateLimited { forge, retry_after: Some(s) } if *s >= 120 => {
                write!(f, "{} is refusing requests for now (too many were made): try again in {} minutes.", forge.name(), s.div_ceil(60))
            }
            Error::RateLimited { forge, retry_after: Some(s) } => {
                write!(f, "{} is refusing requests for now (too many were made): try again in {s} seconds.", forge.name())
            }
            Error::RateLimited { forge, retry_after: None } => {
                write!(f, "{} is refusing requests for now (too many were made): try again later.", forge.name())
            }
            Error::Refused { forge, status, message } => write!(f, "{} refused the request ({status}): {message}", forge.name()),
            Error::Server { forge, status } => write!(f, "{} answered with an error ({status}): try again later.", forge.name()),
            Error::Network { forge, why } => write!(f, "{} could not be reached: {why}", forge.name()),
            Error::Invalid(why) => write!(f, "The forge's answer could not be read: {why}"),
            Error::TooLarge { bytes } => write!(
                f,
                "The project is too large to open: its files add up to more than {} MB (at least {} MB).",
                MAX_PROJECT / (1024 * 1024),
                bytes / (1024 * 1024)
            ),
            Error::TooManyFiles { forge } => {
                write!(f, "The project has too many files and folders to open: {} lists only part of them.", forge.name())
            }
            Error::Conflict { forge, branch, files } => write!(
                f,
                "The branch {branch} has new commits that change the same parts of files you changed ({}): nothing was taken in or committed. Choose for each file whether to keep yours or take the branch's, or commit to a new branch and open a {}.",
                files.iter().map(|c| format!("{}: {}", c.path, c.kind.as_str())).collect::<Vec<_>>().join(", "),
                forge.merge_request()
            ),
            Error::CannotPush { forge: Service::GitLab, branch } => write!(
                f,
                "Nothing was committed: the branch {branch} is protected, or your role in the project cannot push to it. Commit to a new branch and open a merge request instead."
            ),
            Error::CannotPush { forge: Service::GitHub, branch } => write!(
                f,
                "Nothing was committed: the branch {branch} is protected, or your access to the repository cannot push to it. Commit to a new branch and open a pull request instead."
            ),
            Error::Busy { branch } => {
                write!(f, "Nothing was committed: the branch {branch} kept getting new commits while yours was made. Try again in a moment.")
            }
            Error::BranchName(why) => write!(f, "This cannot be a branch's name: {why}."),
            Error::Nothing => f.write_str("Nothing to commit: the files are as on the branch."),
            Error::Stopped => f.write_str("The operation on the forge stopped unexpectedly."),
        }
    }
}

impl std::error::Error for Error {}

/// What a call returns.
pub type Result<T> = std::result::Result<T, Error>;
