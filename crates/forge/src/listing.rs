//! The projects an account reaches on a forge, and a project's branches: what the Open dialog
//! suggests as the user types and offers as a drop-down (docs/forges.md, "Choosing a project and a
//! branch").
//!
//! On GitHub the repositories the user owns, collaborates on or reaches through an organisation
//! are listed once (most recently pushed first, up to [`MAX_LISTED`]) and filtered here as the
//! user types ([`matching`]); a search of GitHub's repositories by name ([`Lister::search`]) finds
//! the others when nothing listed matches. On GitLab the instance searches the projects the user
//! is a member of, most recently active first. A branch list marks the default branch and the
//! protected ones.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::gitlab::next_link;
use crate::http::check;
use crate::{Address, Error, Request, Response, Result, Service, Token, Transport, encode};

/// Projects listed at most from GitHub's list of the user's repositories: five pages of 100.
pub const MAX_LISTED: usize = 500;
/// Branches listed at most: five pages of 100.
pub const MAX_BRANCHES: usize = 500;
/// Projects a search returns at most.
pub const MAX_FOUND: usize = 50;
/// Largest page of a listing read.
const MAX_PAGE: u64 = 8 * 1024 * 1024;
/// Entries a page lists.
const PER_PAGE: usize = 100;
/// The GitHub API version the requests are written for.
const API_VERSION: &str = "2022-11-28";

/// Who can see a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    /// Every signed-in user of the instance (GitLab's internal, GitHub Enterprise's internal).
    Internal,
    Private,
}

impl Visibility {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Visibility::Public),
            "internal" => Some(Visibility::Internal),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }

    /// `public`, `internal` or `private`.
    pub fn id(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Internal => "internal",
            Visibility::Private => "private",
        }
    }
}

/// A project the account reaches, as a suggestion shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    /// `group/project` or `owner/repository`.
    pub path: String,
    pub visibility: Visibility,
    /// None for an empty repository.
    pub default_branch: Option<String>,
    /// The last push (GitHub) or activity (GitLab).
    pub activity: Option<DateTime<Utc>>,
    pub description: Option<String>,
}

/// A branch of a project, as the drop-down lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummary {
    pub name: String,
    pub protected: bool,
    /// The project's default branch.
    pub default: bool,
}

/// A list, and whether it stopped at its cap with more left.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listed<T> {
    pub items: Vec<T>,
    pub truncated: bool,
}

/// Lists what a token reaches on one instance.
pub struct Lister {
    transport: Box<dyn Transport>,
    service: Service,
    /// The instance's address, with no `/` at the end.
    instance: String,
    token: Token,
}

impl Lister {
    pub fn new(transport: Box<dyn Transport>, service: Service, instance: &str, token: Token) -> Self {
        Self { transport, service, instance: instance.trim_end_matches('/').to_owned(), token }
    }

    /// The instance's API (GitLab's version 4, GitHub's), as [`Address::api`] says it.
    fn api(&self) -> String {
        Address { instance: self.instance.clone(), path: String::new(), service: self.service }.api()
    }

    fn request(&self, url: String) -> Request {
        let mut headers = vec![("Authorization".to_owned(), self.token.header())];
        match self.service {
            Service::GitLab => headers.push(("Accept".to_owned(), "application/json".to_owned())),
            Service::GitHub => {
                headers.push(("Accept".to_owned(), "application/vnd.github+json".to_owned()));
                headers.push(("X-GitHub-Api-Version".to_owned(), API_VERSION.to_owned()));
            }
        }
        Request { method: "GET", url, headers, body: None, max_body: MAX_PAGE }
    }

    async fn get(&self, url: String, what: &str) -> Result<(Value, Response)> {
        let response = self.transport.send(self.request(url)).await.map_err(|why| Error::Network { forge: self.service, why })?;
        let response = check(self.service, response, what)?;
        if response.too_large {
            return Err(Error::Invalid(format!("the answer about {what} is too large")));
        }
        let value = serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))?;
        Ok((value, response))
    }

    /// The pages from `first` on, each a list read by `read`, until `cap` entries: the next page
    /// is the `Link` header's (only an address of this instance's API, which the token may go
    /// to), else GitLab's `X-Next-Page`.
    async fn pages<T>(&self, first: String, what: &str, cap: usize, read: impl Fn(&Value) -> Option<T>) -> Result<Listed<T>> {
        let api = format!("{}/", self.api());
        let mut url = first.clone();
        let mut items = Vec::new();
        loop {
            let (v, response) = self.get(url, what).await?;
            let page = v.as_array().ok_or_else(|| Error::Invalid(format!("the {what} are not a list")))?;
            items.extend(page.iter().filter_map(&read));
            let next = response.header("link").and_then(next_link).filter(|n| n.starts_with(&api)).or_else(|| {
                let page = response.header("x-next-page").map(str::trim).filter(|p| !p.is_empty())?;
                page.parse::<u32>().ok().map(|n| format!("{first}&page={n}"))
            });
            match next {
                Some(_) if items.len() >= cap => {
                    items.truncate(cap);
                    return Ok(Listed { items, truncated: true });
                }
                Some(next) => url = next,
                None => {
                    let truncated = items.len() > cap;
                    items.truncate(cap);
                    return Ok(Listed { items, truncated });
                }
            }
        }
    }

    /// GitHub: the repositories the user owns, collaborates on or reaches through an
    /// organisation, most recently pushed first, up to [`MAX_LISTED`]. GitLab: the projects the
    /// user is a member of, most recently active first, up to [`MAX_FOUND`].
    pub async fn reachable(&self) -> Result<Listed<ProjectSummary>> {
        match self.service {
            Service::GitHub => {
                let url = format!("{}/user/repos?per_page={PER_PAGE}&sort=pushed&affiliation=owner,collaborator,organization_member", self.api());
                self.pages(url, "repositories", MAX_LISTED, github_repo).await
            }
            Service::GitLab => self.search("").await,
        }
    }

    /// The projects whose name holds `text`: on GitHub its search of repositories (public ones,
    /// and the private ones the token reaches; `owner/name` searches the owner's); on GitLab the
    /// projects the user is a member of (`group/name` also matches the group's path).
    pub async fn search(&self, text: &str) -> Result<Listed<ProjectSummary>> {
        let text = text.trim();
        match self.service {
            Service::GitHub => {
                let q = match text.split_once('/') {
                    Some((owner, name)) if !owner.trim().is_empty() => {
                        let name = name.trim();
                        if name.is_empty() { format!("user:{}", owner.trim()) } else { format!("{name} in:name user:{}", owner.trim()) }
                    }
                    _ => format!("{text} in:name"),
                };
                let url = format!("{}/search/repositories?q={}&per_page={MAX_FOUND}", self.api(), encode(&q));
                let (v, _) = self.get(url, "repositories found").await?;
                let list = v["items"].as_array().ok_or_else(|| Error::Invalid("the repositories found are not a list".to_owned()))?;
                let items: Vec<ProjectSummary> = list.iter().filter_map(github_repo).take(MAX_FOUND).collect();
                let truncated = v["total_count"].as_u64().is_some_and(|n| n > items.len() as u64);
                Ok(Listed { items, truncated })
            }
            Service::GitLab => {
                let mut url = format!("{}/projects?membership=true&order_by=last_activity_at&per_page={MAX_FOUND}", self.api());
                if !text.is_empty() {
                    url.push_str(&format!("&search={}", encode(text)));
                    if text.contains('/') {
                        url.push_str("&search_namespaces=true");
                    }
                }
                let (v, response) = self.get(url, "projects").await?;
                let list = v.as_array().ok_or_else(|| Error::Invalid("the projects are not a list".to_owned()))?;
                let items: Vec<ProjectSummary> = list.iter().filter_map(gitlab_project).collect();
                let more =
                    response.header("x-next-page").is_some_and(|p| !p.trim().is_empty()) || response.header("link").and_then(next_link).is_some();
                Ok(Listed { items, truncated: more })
            }
        }
    }

    /// The branches of project `address` (on this instance), up to [`MAX_BRANCHES`], the default
    /// one marked (and listed even past the cap), and the default branch's name.
    pub async fn branches(&self, address: &Address) -> Result<(Listed<BranchSummary>, Option<String>)> {
        let what = format!("branches of {}", address.path);
        let (url, default) = match self.service {
            Service::GitHub => {
                let (v, _) = self.get(format!("{}/repos/{}", self.api(), address.path), &format!("repository {}", address.path)).await?;
                let default = v["default_branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned);
                (format!("{}/repos/{}/branches?per_page={PER_PAGE}", self.api(), address.path), default)
            }
            Service::GitLab => {
                let (v, _) = self.get(format!("{}/projects/{}", self.api(), address.id()), &format!("project {}", address.path)).await?;
                let default = v["default_branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned);
                (format!("{}/projects/{}/repository/branches?per_page={PER_PAGE}", self.api(), address.id()), default)
            }
        };
        let mut listed = self
            .pages(url, &what, MAX_BRANCHES, |b| {
                let name = b["name"].as_str().filter(|n| !n.is_empty())?;
                Some(BranchSummary { name: name.to_owned(), protected: b["protected"].as_bool().unwrap_or(false), default: false })
            })
            .await?;
        if let Some(default) = &default {
            match listed.items.iter_mut().find(|b| &b.name == default) {
                Some(b) => b.default = true,
                None => listed.items.insert(0, BranchSummary { name: default.clone(), protected: false, default: true }),
            }
        }
        Ok((listed, default))
    }
}

fn time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

fn description(v: &Value) -> Option<String> {
    let d = v.as_str()?.trim();
    (!d.is_empty()).then(|| d.chars().take(200).collect())
}

/// A repository of GitHub's lists.
fn github_repo(r: &Value) -> Option<ProjectSummary> {
    let path = r["full_name"].as_str().filter(|p| p.contains('/'))?;
    let visibility = r["visibility"].as_str().and_then(Visibility::parse).unwrap_or(if r["private"].as_bool().unwrap_or(false) {
        Visibility::Private
    } else {
        Visibility::Public
    });
    Some(ProjectSummary {
        path: path.to_owned(),
        visibility,
        default_branch: r["default_branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned),
        activity: time(&r["pushed_at"]).or_else(|| time(&r["updated_at"])),
        description: description(&r["description"]),
    })
}

/// A project of GitLab's list. Its visibility is not in GitLab's `simple` answer, so the full one
/// is read; a project that says none counts as private.
fn gitlab_project(p: &Value) -> Option<ProjectSummary> {
    let path = p["path_with_namespace"].as_str().filter(|p| p.contains('/'))?;
    Some(ProjectSummary {
        path: path.to_owned(),
        visibility: p["visibility"].as_str().and_then(Visibility::parse).unwrap_or(Visibility::Private),
        default_branch: p["default_branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned),
        activity: time(&p["last_activity_at"]),
        description: description(&p["description"]),
    })
}

/// The projects of `list` whose path holds every word of `query` (ignoring case), those whose
/// name starts with the query first, in the list's order otherwise (most recent first). All of
/// them for an empty query.
pub fn matching<'a>(list: &'a [ProjectSummary], query: &str) -> Vec<&'a ProjectSummary> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<(u8, &ProjectSummary)> = list
        .iter()
        .filter_map(|p| {
            let path = p.path.to_lowercase();
            if !words.iter().all(|w| path.contains(w)) {
                return None;
            }
            let name = path.rsplit('/').next().unwrap_or_default();
            let rank = if path == query || name == query {
                0
            } else if name.starts_with(&query) || path.starts_with(&query) {
                1
            } else {
                2
            };
            Some((rank, p))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, p)| p).collect()
}

/// Whether `query`, which nothing listed matches, is worth a search of GitHub's repositories: it
/// names an owner (`owner/…`) or has three characters or more.
pub fn worth_searching(query: &str) -> bool {
    let query = query.trim();
    query.contains('/') || query.chars().count() >= 3
}

/// What [`find`] found: the projects matching, whether there are more than it shows, whether
/// GitHub's search was asked, and the listing it read (to keep for the next query).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    pub projects: Vec<ProjectSummary>,
    pub truncated: bool,
    pub searched: bool,
    pub listing: Option<Listed<ProjectSummary>>,
}

/// The projects matching `query` the account reaches. GitHub: those of `listing` (the list of the
/// user's repositories, read when none is given), and a search when none of them matches and the
/// query is [`worth_searching`]. GitLab: the instance's search of the user's projects.
pub async fn find(lister: &Lister, listing: Option<Listed<ProjectSummary>>, query: &str) -> Result<Found> {
    match lister.service {
        Service::GitLab => {
            let found = lister.search(query).await?;
            Ok(Found { projects: found.items, truncated: found.truncated, searched: true, listing: None })
        }
        Service::GitHub => {
            let (listing, fresh) = match listing {
                Some(l) => (l, false),
                None => (lister.reachable().await?, true),
            };
            let matched: Vec<ProjectSummary> = matching(&listing.items, query).into_iter().cloned().collect();
            let keep = fresh.then(|| listing.clone());
            if matched.is_empty() && worth_searching(query) {
                let found = lister.search(query).await?;
                return Ok(Found { projects: found.items, truncated: found.truncated, searched: true, listing: keep });
            }
            // A listing cut at its cap may not hold what is typed: say there may be more.
            Ok(Found { projects: matched, truncated: listing.truncated, searched: false, listing: keep })
        }
    }
}
