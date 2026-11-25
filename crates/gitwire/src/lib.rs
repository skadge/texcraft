//! Git's smart HTTP protocol, the client side (protocol version 0, stateless): fetching a branch
//! from a server (`git-upload-pack`) and pushing one to it (`git-receive-pack`), between a
//! repository on disk and the server, over an HTTP client the caller gives ([`Http`]).
//!
//! gix (gitoxide) has no push, and its fetch needs its own HTTP stack (curl, or reqwest); this
//! crate speaks the protocol over any blocking HTTP client and uses gix for the repository: the
//! objects to send read from it, a received pack indexed into it.
//!
//! - [`pktline`]: git's packet lines and side-band.
//! - [`advertise`]: the refs a server advertises (`GET …/info/refs?service=…`).
//! - [`pack`]: which objects the server lacks, and a pack of them (whole objects, no deltas).
//! - [`push`]: one ref updated on the server, fast-forward only unless forced.
//! - [`fetch`]: a ref's commits received into the repository (no thin packs).
//!
//! Credentials go in HTTP basic authentication and nowhere else: they are never printed
//! ([`Credentials`]'s `Debug` hides the password), logged or put in an error.
//!
//! Written to move to storytold/craft-libs as it is: it knows nothing of TeXCraft.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod pktline;

#[cfg(not(target_arch = "wasm32"))]
pub mod advertise;
#[cfg(not(target_arch = "wasm32"))]
pub mod fetch;
#[cfg(not(target_arch = "wasm32"))]
pub mod pack;
#[cfg(not(target_arch = "wasm32"))]
pub mod push;

#[cfg(not(target_arch = "wasm32"))]
pub use advertise::{Advertised, Service, advertised};
#[cfg(not(target_arch = "wasm32"))]
pub use fetch::{Fetched, fetch};
#[cfg(not(target_arch = "wasm32"))]
pub use push::{Pushed, push};

/// The id git writes for "no object": a ref being created, or one that does not exist.
pub const ZERO_ID: &str = "0000000000000000000000000000000000000000";

/// The largest answer read from a server: a pack of a project's history.
pub const MAX_ANSWER: u64 = 1024 * 1024 * 1024;

/// What this client calls itself to servers (git's `agent` capability, and its `User-Agent`,
/// which some servers want to start with `git/` before they answer with git's protocol).
pub const AGENT: &str = "git/texcraft-gitwire";

/// A user name and a password (an access token) for HTTP basic authentication.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    password: String,
}

impl Credentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self { username: username.into(), password: password.into() }
    }

    /// The `Authorization` header's value.
    fn header(&self) -> String {
        use base64::Engine as _;
        let pair = format!("{}:{}", self.username, self.password);
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(pair))
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("username", &self.username).finish_non_exhaustive()
    }
}

/// A server's repository, and how to sign in to it.
#[derive(Debug, Clone)]
pub struct Remote {
    /// Its address, as `git clone` takes it over HTTPS (`https://host/owner/name.git`).
    pub url: String,
    pub credentials: Option<Credentials>,
}

impl Remote {
    pub fn new(url: &str, credentials: Option<Credentials>) -> Self {
        Self { url: url.trim().trim_end_matches('/').to_owned(), credentials }
    }

    /// The headers of every request: the agent, and the credentials when there are some.
    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("User-Agent".to_owned(), AGENT.to_owned())];
        if let Some(c) = &self.credentials {
            headers.push(("Authorization".to_owned(), c.header()));
        }
        headers
    }
}

/// An HTTP answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// The `Content-Type` header, if any.
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// A blocking HTTP client. Any answer, an error status included, is a [`Reply`]; only a failure
/// to get one is an error (the network, in words). Headers are sent as given; the client may add
/// its own.
pub trait Http {
    fn get(&self, url: &str, headers: &[(String, String)]) -> std::result::Result<Reply, String>;
    fn post(&self, url: &str, headers: &[(String, String)], body: Vec<u8>) -> std::result::Result<Reply, String>;
}

/// What went wrong, in words ([`std::fmt::Display`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 401: no credentials, or the server did not accept them.
    Unauthorized,
    /// 403: the credentials do not allow it.
    Forbidden,
    /// 404: no such repository, or the credentials cannot see it.
    NotFound,
    /// Another error status.
    Status(u16),
    /// No answer: the network, the server's name, its certificate.
    Network(String),
    /// An answer that is not git's protocol.
    Protocol(String),
    /// The server's own message (`ERR …`, or side-band 3).
    Server(String),
    /// The server's branch has commits the local one does not: pull first.
    NotFastForward { branch: String },
    /// The server did not take the pack (`unpack …`).
    Unpack(String),
    /// The server refused to update the ref, and why (`ng …`).
    Rejected { reference: String, why: String },
    /// The local repository: reading or writing it.
    Local(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unauthorized => f.write_str("The server did not accept the credentials: sign in again, or check the access token."),
            Error::Forbidden => {
                f.write_str("The server refused: the credentials do not allow this (pushing needs permission to write to the repository).")
            }
            Error::NotFound => f.write_str("The server has no such repository, or the credentials cannot see it: check the address."),
            Error::Status(status) => write!(f, "The server answered with an error ({status}): try again later."),
            Error::Network(why) => write!(f, "The server could not be reached: {why}"),
            Error::Protocol(why) => write!(f, "The server's answer is not git's: {why}"),
            Error::Server(message) => write!(f, "The server said: {message}"),
            Error::NotFastForward { branch } => {
                write!(f, "The remote's branch {branch} has commits this project does not have: pull first, then push.")
            }
            Error::Unpack(why) => write!(f, "The server could not take the commits sent: {why}"),
            Error::Rejected { reference, why } => write!(f, "The server refused to update {reference}: {why}"),
            Error::Local(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// `reply` when its status is a success, else the error the status means.
fn checked(reply: Reply) -> Result<Reply> {
    match reply.status {
        200..=299 => Ok(reply),
        401 => Err(Error::Unauthorized),
        403 => Err(Error::Forbidden),
        404 => Err(Error::NotFound),
        status => Err(Error::Status(status)),
    }
}

/// A local repository error in words.
fn local(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Local(format!("{what}: {e}"))
}
