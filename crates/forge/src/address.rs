//! Where a project is on a forge: the instance (`https://gitlab.com`) and the project's path on it
//! (`group/subgroup/project`), from what a user pastes.

use crate::Error;

/// The instance a bare `group/project` is on.
pub const GITLAB_COM: &str = "https://gitlab.com";

/// A project on a GitLab instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Address {
    /// The instance's address, scheme and host (and port), with no `/` at the end.
    pub instance: String,
    /// The project's path on it: its groups and its name, `/`-separated.
    pub path: String,
}

impl Address {
    /// Reads a project's address as a user gives it: its page (`https://gitlab.com/group/project`,
    /// or any page under it), its clone address (`https://…/project.git`, `git@host:group/project.git`),
    /// `host/group/project`, or `group/project` on gitlab.com.
    ///
    /// The token travels with every request, so the instance must be reached over HTTPS; plain HTTP
    /// is accepted only on this computer (`localhost`), for a test server.
    pub fn parse(input: &str) -> crate::Result<Self> {
        let input = input.trim();
        let bad = |why: &str| Error::Address(format!("{input}: {why}"));
        if input.is_empty() {
            return Err(Error::Address("Give the project's address, such as https://gitlab.com/group/project.".to_owned()));
        }
        let (instance, rest) = if let Some(rest) = input.strip_prefix("https://") {
            split_host(rest, "https").ok_or_else(|| bad("it names no project after the host."))?
        } else if let Some(rest) = input.strip_prefix("http://") {
            let (instance, rest) = split_host(rest, "http").ok_or_else(|| bad("it names no project after the host."))?;
            let host = instance.trim_start_matches("http://").split(':').next().unwrap_or_default();
            if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
                return Err(bad("the address must start with https://, so that the access token is not sent in the clear."));
            }
            (instance, rest)
        } else if let Some(rest) = input.strip_prefix("git@") {
            let (host, path) = rest.split_once(':').ok_or_else(|| bad("a git@ address has a colon after the host."))?;
            (format!("https://{host}"), path.to_owned())
        } else if input.split('/').next().is_some_and(|first| first.contains('.')) {
            // `gitlab.example.org/group/project`
            split_host(input, "https").ok_or_else(|| bad("it names no project after the host."))?
        } else {
            (GITLAB_COM.to_owned(), input.to_owned())
        };
        let host = instance.split("://").nth(1).unwrap_or_default();
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']')) {
            return Err(bad("the host is not a valid name."));
        }
        let path = project_path(&rest).ok_or_else(|| bad("the project is its group and its name, such as group/project."))?;
        Ok(Self { instance, path })
    }

    /// The instance's REST API (version 4).
    pub fn api(&self) -> String {
        format!("{}/api/v4", self.instance)
    }

    /// The project's id in the API's paths: its path, URL-encoded.
    pub fn id(&self) -> String {
        encode(&self.path)
    }

    /// The project's page.
    pub fn web_url(&self) -> String {
        format!("{}/{}", self.instance, self.path)
    }

    /// The project's own name: the last part of its path.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// `host/rest` as the instance (`scheme://host`) and the rest.
fn split_host(s: &str, scheme: &str) -> Option<(String, String)> {
    let (host, rest) = s.split_once('/')?;
    (!host.is_empty()).then(|| (format!("{scheme}://{host}"), rest.to_owned()))
}

/// The project's path from what follows the host: up to a page's `/-/` part, without a trailing
/// `/` or `.git`. At least a group and a name, each of the characters GitLab allows in paths.
fn project_path(rest: &str) -> Option<String> {
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let rest = rest.split("/-/").next().unwrap_or_default().trim_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let parts: Vec<&str> = rest.split('/').collect();
    let valid =
        |p: &&str| !p.is_empty() && !p.starts_with(['.', '-']) && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    (parts.len() >= 2 && parts.iter().all(valid)).then(|| parts.join("/"))
}

/// `s` with every byte but the unreserved ones (`A-Z a-z 0-9 - . _ ~`) percent-encoded, for a
/// part of a URL's path or query.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
