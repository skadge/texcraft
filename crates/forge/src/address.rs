//! Where a project is on a forge: the service (GitLab or GitHub), the instance
//! (`https://gitlab.com`, `https://github.com`) and the project's path on it
//! (`group/subgroup/project`, `owner/repository`), from what a user pastes.

use crate::Error;

/// The instance a bare `group/project` is on.
pub const GITLAB_COM: &str = "https://gitlab.com";
/// GitHub's own instance, which a bare `owner/repository` is on when GitHub is chosen.
pub const GITHUB_COM: &str = "https://github.com";

/// The kind of forge a project is on: which API it speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    /// GitLab (gitlab.com, or an instance of its own), over its REST API version 4.
    #[default]
    GitLab,
    /// GitHub (github.com, or GitHub Enterprise Server), over its REST API.
    GitHub,
}

impl Service {
    /// Its name, as users know it.
    pub fn name(self) -> &'static str {
        match self {
            Service::GitLab => "GitLab",
            Service::GitHub => "GitHub",
        }
    }

    /// `gitlab` or `github`, as commands take it.
    pub fn id(self) -> &'static str {
        match self {
            Service::GitLab => "gitlab",
            Service::GitHub => "github",
        }
    }

    /// `gitlab` or `github` (any case).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gitlab" => Some(Service::GitLab),
            "github" => Some(Service::GitHub),
            _ => None,
        }
    }

    /// What it calls asking for a branch to be merged: a merge request, a pull request.
    pub fn merge_request(self) -> &'static str {
        match self {
            Service::GitLab => "merge request",
            Service::GitHub => "pull request",
        }
    }
}

/// A project on a forge.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Address {
    /// The instance's address, scheme and host (and port), with no `/` at the end.
    pub instance: String,
    /// The project's path on it: its groups and its name (GitLab), its owner and its name
    /// (GitHub), `/`-separated.
    pub path: String,
    /// The API it speaks; GitLab for a link saved before GitHub was supported.
    #[serde(default)]
    pub service: Service,
}

impl Address {
    /// Reads a project's address as a user gives it: its page (`https://gitlab.com/group/project`,
    /// or any page under it), its clone address (`https://…/project.git`, `git@host:group/project.git`),
    /// `host/group/project`, or `group/project` on gitlab.com.
    ///
    /// The token travels with every request, so the instance must be reached over HTTPS; plain HTTP
    /// is accepted only on this computer (`localhost`), for a test server.
    ///
    /// github.com's addresses are GitHub's (`https://github.com/owner/repository`, its pages and
    /// clone addresses); any other is GitLab's, unless [`Address::parse_as`] says otherwise.
    pub fn parse(input: &str) -> crate::Result<Self> {
        Self::parse_as(input, None)
    }

    /// Reads a project's address on `service` when it is given: then a bare `owner/repository`
    /// is on github.com for GitHub, and another host is a GitHub Enterprise Server. An address
    /// of gitlab.com or github.com is refused for the other service.
    pub fn parse_as(input: &str, service: Option<Service>) -> crate::Result<Self> {
        let input = input.trim();
        let bad = |why: &str| Error::Address(format!("{input}: {why}"));
        if input.is_empty() {
            return Err(Error::Address(match service {
                Some(Service::GitHub) => "Give the repository's address, such as https://github.com/owner/repository.".to_owned(),
                _ => "Give the project's address, such as https://gitlab.com/group/project.".to_owned(),
            }));
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
        } else if service == Some(Service::GitHub) {
            (GITHUB_COM.to_owned(), input.to_owned())
        } else {
            (GITLAB_COM.to_owned(), input.to_owned())
        };
        let instance = if instance == "https://www.github.com" { GITHUB_COM.to_owned() } else { instance };
        let host = instance.split("://").nth(1).unwrap_or_default();
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']')) {
            return Err(bad("the host is not a valid name."));
        }
        let known = match instance.as_str() {
            GITHUB_COM => Some(Service::GitHub),
            GITLAB_COM => Some(Service::GitLab),
            _ => None,
        };
        let service = match (known, service) {
            (Some(known), Some(chosen)) if known != chosen => {
                return Err(bad(&format!("this is an address on {known}, not {chosen}.", known = known.name(), chosen = chosen.name())));
            }
            (Some(known), _) => known,
            (None, chosen) => chosen.unwrap_or_default(),
        };
        let path = match service {
            Service::GitLab => project_path(&rest).ok_or_else(|| bad("the project is its group and its name, such as group/project."))?,
            Service::GitHub => repository_path(&rest).ok_or_else(|| bad("the repository is its owner and its name, such as owner/repository."))?,
        };
        Ok(Self { instance, path, service })
    }

    /// The instance's REST API: GitLab's version 4; GitHub's at api.github.com, or under
    /// `/api/v3` on a GitHub Enterprise Server.
    pub fn api(&self) -> String {
        match self.service {
            Service::GitLab => format!("{}/api/v4", self.instance),
            Service::GitHub if self.instance == GITHUB_COM => "https://api.github.com".to_owned(),
            Service::GitHub => format!("{}/api/v3", self.instance),
        }
    }

    /// The page of commit `id`.
    pub fn commit_url(&self, web_url: &str, id: &str) -> String {
        match self.service {
            Service::GitLab => format!("{web_url}/-/commit/{id}"),
            Service::GitHub => format!("{web_url}/commit/{id}"),
        }
    }

    /// The project's id in the API's paths: its path, URL-encoded.
    pub fn id(&self) -> String {
        encode(&self.path)
    }

    /// The project's address for git over HTTPS.
    pub fn clone_url(&self) -> String {
        format!("{}/{}.git", self.instance, self.path)
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

/// A GitHub repository's path from what follows the host: its owner and its name, the first two
/// parts (a page under it has more), without `.git`. An owner has letters, digits and `-`; a
/// repository also `.` and `_`.
fn repository_path(rest: &str) -> Option<String> {
    let rest = rest.split(['?', '#']).next().unwrap_or_default().trim_matches('/');
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_ok = !owner.is_empty() && !owner.starts_with('-') && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let name_ok = !name.is_empty() && name != "." && name != ".." && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    (owner_ok && name_ok).then(|| format!("{owner}/{name}"))
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

/// `s`, a branch's name, with each of its `/`-separated parts encoded ([`encode`]): for the
/// paths of GitHub's API, which name a branch as a path.
pub fn encode_path(s: &str) -> String {
    s.split('/').map(encode).collect::<Vec<_>>().join("/")
}
