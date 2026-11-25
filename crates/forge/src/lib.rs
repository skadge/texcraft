//! Projects on a git forge (docs/forges.md; docs/ROADMAP.md, C5). GitLab first, over its REST API
//! (`/api/v4`), which answers browsers too: a project's branch is read as its tree and its files'
//! blobs, and changes go back as one commit with several actions. No git client is needed, on the
//! desktop or in the browser.
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
mod gitlab;
mod http;
mod sync;
pub mod task;

pub use address::{Address, GITLAB_COM, encode};
pub use gitlab::{Branch, GitLab, ProjectInfo, TreeEntry};
pub use http::{Request, Response, Sending, Transport, transport};
pub use sync::{ChangeKind, Committed, FileChange, Link, Loaded, Skipped, changes, commit, open};

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
            return Err(Error::Token("Give a personal access token: GitLab's API needs one to open a project and commit to it.".to_owned()));
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

/// What went wrong, in words a user can act on ([`std::fmt::Display`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The project's address cannot be read.
    Address(String),
    /// The token cannot be one.
    Token(String),
    /// 401: the token was not accepted.
    Unauthorized,
    /// 403: the token or the user's role does not allow `what`.
    Forbidden { what: String },
    /// 404: GitLab has no `what`, or the token cannot see it.
    NotFound { what: String },
    /// 429: too many requests; try again after so many seconds, when GitLab says.
    RateLimited { retry_after: Option<u64> },
    /// Another refusal (400, 409, 422…), with GitLab's own message.
    Refused { status: u16, message: String },
    /// 5xx, or a status the API does not answer with.
    Server { status: u16 },
    /// No answer: the network, the instance's name, its certificate.
    Network(String),
    /// An answer that cannot be read as the API's.
    Invalid(String),
    /// The project's files add up to more than [`MAX_PROJECT`].
    TooLarge { bytes: u64 },
    /// The branch has commits since the base that change files changed here too.
    Conflict { branch: String, files: Vec<String> },
    /// The operation's thread ended without an outcome.
    Stopped,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Address(why) => write!(f, "This is not a project's address: {why}"),
            Error::Token(why) => f.write_str(why),
            Error::Unauthorized => {
                f.write_str("GitLab did not accept the access token: check that it is the whole token, and that it has not expired or been revoked.")
            }
            Error::Forbidden { what } => write!(
                f,
                "GitLab refused {what}: the access token needs the read_api scope to open a project and the api scope to commit, and your role in the project must allow it."
            ),
            Error::NotFound { what } => write!(f, "GitLab has no {what}, or the access token cannot see it: check the address."),
            Error::RateLimited { retry_after: Some(s) } if *s >= 120 => {
                write!(f, "GitLab is refusing requests for now (too many were made): try again in {} minutes.", s.div_ceil(60))
            }
            Error::RateLimited { retry_after: Some(s) } => {
                write!(f, "GitLab is refusing requests for now (too many were made): try again in {s} seconds.")
            }
            Error::RateLimited { retry_after: None } => f.write_str("GitLab is refusing requests for now (too many were made): try again later."),
            Error::Refused { status, message } => write!(f, "GitLab refused the request ({status}): {message}"),
            Error::Server { status } => write!(f, "GitLab answered with an error ({status}): try again later."),
            Error::Network(why) => write!(f, "GitLab could not be reached: {why}"),
            Error::Invalid(why) => write!(f, "GitLab's answer could not be read: {why}"),
            Error::TooLarge { bytes } => write!(
                f,
                "The project is too large to open: its files add up to more than {} MB (at least {} MB).",
                MAX_PROJECT / (1024 * 1024),
                bytes / (1024 * 1024)
            ),
            Error::Conflict { branch, files } => write!(
                f,
                "Nothing was committed: the branch {branch} has new commits that change files you changed too ({}). Merging them is not available yet: copy your changes aside, open the branch again from GitLab and make them there.",
                files.join(", ")
            ),
            Error::Stopped => f.write_str("The GitLab operation stopped unexpectedly."),
        }
    }
}

impl std::error::Error for Error {}

/// What a call returns.
pub type Result<T> = std::result::Result<T, Error>;
