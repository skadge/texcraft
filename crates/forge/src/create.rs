//! A new repository on a forge, for a project that has none yet (Save Project to GitHub or
//! GitLab): where the account can make one (itself, its organisations or groups), the name
//! checked, the repository made empty (the desktop then pushes the project's history into it with
//! git), and in the browser the project's files committed to it as its first commit.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::http::check;
use crate::{Address, ChangeKind, Error, FileChange, GitHub, GitLab, Link, Request, Result, Service, Token, Transport, encode};

/// Largest answer read about owners and new repositories.
const MAX_JSON: u64 = 8 * 1024 * 1024;
/// The GitHub API version the requests are written for.
const API_VERSION: &str = "2022-11-28";

/// Where a repository can be made: the account itself, or an organisation (GitHub) or a group
/// (GitLab) it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// The user's login, the organisation's login, or the group's full path.
    pub path: String,
    /// The account itself.
    pub personal: bool,
    /// The group's id (GitLab's `namespace_id`); none for the account and on GitHub.
    pub namespace_id: Option<u64>,
}

/// A repository to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRepository {
    /// Where: an organisation or group's path; none for the account itself.
    pub owner: Option<Owner>,
    pub name: String,
    pub private: bool,
    pub description: Option<String>,
    /// GitHub only: start it with a first commit (a README) so that its API can commit to it,
    /// which it cannot do in an empty repository (the browser's first commit replaces it).
    pub initialize: bool,
}

/// A repository made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub address: Address,
    pub project_id: u64,
    pub web_url: String,
    /// Its address for git over HTTPS.
    pub clone_url: String,
    /// The branch the forge made it with, or will take as the default.
    pub default_branch: String,
}

/// Whether `name` can be a repository's name on both forges: letters, digits, `.`, `-` and `_`,
/// starting with a letter, a digit or `_`, not ending in `.git` or `.atom`, at most 100 long.
pub fn check_name(name: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::Name(why.to_owned()));
    if name.is_empty() {
        return bad("Give the repository a name.");
    }
    if name.len() > 100 {
        return bad("A repository's name is at most 100 characters long.");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return bad("A repository's name has only letters (without accents), digits, '.', '-' and '_'.");
    }
    if !name.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return bad("A repository's name starts with a letter, a digit or '_'.");
    }
    if name.ends_with(".git") || name.ends_with(".atom") {
        return bad("A repository's name cannot end in .git or .atom.");
    }
    Ok(())
}

/// A repository name made from a project folder's name: spaces and other characters a name cannot
/// have become `-`, accents and the rest are left out.
pub fn suggest_name(folder: &str) -> String {
    let mut out = String::new();
    for c in folder.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_') {
            out.push(c);
        } else if (c == '-' || c.is_whitespace() || c.is_ascii_punctuation()) && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_start_matches(['-', '.']).trim_end_matches(['-', '.']);
    let out = out.strip_suffix(".git").unwrap_or(out);
    let out: String = out.chars().take(100).collect();
    if check_name(&out).is_ok() { out } else { "paper".to_owned() }
}

/// Makes repositories on one instance with a token.
pub struct Creator {
    transport: Box<dyn Transport>,
    service: Service,
    /// The instance's address, with no `/` at the end.
    instance: String,
    token: Token,
}

impl Creator {
    pub fn new(transport: Box<dyn Transport>, service: Service, instance: &str, token: Token) -> Self {
        Self { transport, service, instance: instance.trim_end_matches('/').to_owned(), token }
    }

    fn api(&self) -> String {
        Address { instance: self.instance.clone(), path: String::new(), service: self.service }.api()
    }

    async fn send(&self, method: &'static str, url: String, body: Option<Value>, what: &str) -> Result<Value> {
        let response = self.send_raw(method, url, body).await?;
        self.read(response, what)
    }

    fn read(&self, response: crate::Response, what: &str) -> Result<Value> {
        let response = check(self.service, response, what)?;
        if response.too_large {
            return Err(Error::Invalid(format!("the answer about {what} is too large")));
        }
        serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))
    }

    async fn send_raw(&self, method: &'static str, url: String, body: Option<Value>) -> Result<crate::Response> {
        let mut headers = vec![("Authorization".to_owned(), self.token.header())];
        match self.service {
            Service::GitLab => headers.push(("Accept".to_owned(), "application/json".to_owned())),
            Service::GitHub => {
                headers.push(("Accept".to_owned(), "application/vnd.github+json".to_owned()));
                headers.push(("X-GitHub-Api-Version".to_owned(), API_VERSION.to_owned()));
            }
        }
        let body = match body {
            Some(v) => {
                headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
                Some(serde_json::to_vec(&v).map_err(|e| Error::Invalid(e.to_string()))?)
            }
            None => None,
        };
        let request = Request { method, url, headers, body, max_body: MAX_JSON };
        self.transport.send(request).await.map_err(|why| Error::Network { forge: self.service, why })
    }

    /// Where the account can make a repository: itself first, then its organisations (GitHub)
    /// or the groups in which it is at least a developer (GitLab), by path.
    pub async fn owners(&self) -> Result<Vec<Owner>> {
        let api = self.api();
        let user = self.send("GET", format!("{api}/user"), None, "the signed-in account").await?;
        let login = match self.service {
            Service::GitHub => user["login"].as_str(),
            Service::GitLab => user["username"].as_str(),
        };
        let login = login.ok_or_else(|| Error::Invalid("the account has no name".to_owned()))?;
        let mut out = vec![Owner { path: login.to_owned(), personal: true, namespace_id: None }];
        let mut others = Vec::new();
        match self.service {
            Service::GitHub => {
                let orgs = self.send("GET", format!("{api}/user/orgs?per_page=100"), None, "the account's organisations").await?;
                for org in orgs.as_array().into_iter().flatten() {
                    if let Some(name) = org["login"].as_str() {
                        others.push(Owner { path: name.to_owned(), personal: false, namespace_id: None });
                    }
                }
            }
            Service::GitLab => {
                let url = format!("{api}/groups?min_access_level=30&per_page=100&order_by=path");
                let groups = self.send("GET", url, None, "the account's groups").await?;
                for group in groups.as_array().into_iter().flatten() {
                    if let (Some(path), Some(id)) = (group["full_path"].as_str(), group["id"].as_u64()) {
                        others.push(Owner { path: path.to_owned(), personal: false, namespace_id: Some(id) });
                    }
                }
            }
        }
        others.sort_by_key(|o| o.path.to_lowercase());
        out.extend(others);
        Ok(out)
    }

    /// Makes repository `new`. A name the owner has already is [`Error::NameTaken`].
    pub async fn create(&self, new: &NewRepository) -> Result<Created> {
        check_name(&new.name)?;
        let api = self.api();
        let owner = new.owner.as_ref().filter(|o| !o.personal);
        let what = format!("making the repository {}", new.name);
        let made = match self.service {
            Service::GitHub => {
                let url = match owner {
                    Some(o) => format!("{api}/orgs/{}/repos", encode(&o.path)),
                    None => format!("{api}/user/repos"),
                };
                let mut body = json!({"name": new.name, "private": new.private, "auto_init": new.initialize});
                if let Some(d) = new.description.as_deref().filter(|d| !d.trim().is_empty()) {
                    body["description"] = json!(d.trim());
                }
                self.send_raw("POST", url, Some(body)).await?
            }
            Service::GitLab => {
                let mut body = json!({
                    "name": new.name,
                    "path": new.name,
                    "visibility": if new.private { "private" } else { "public" },
                    "initialize_with_readme": false,
                });
                if let Some(id) = owner.and_then(|o| o.namespace_id) {
                    body["namespace_id"] = json!(id);
                }
                if let Some(d) = new.description.as_deref().filter(|d| !d.trim().is_empty()) {
                    body["description"] = json!(d.trim());
                }
                self.send_raw("POST", format!("{api}/projects"), Some(body)).await?
            }
        };
        if matches!(made.status, 400 | 422) && taken(&made.body) {
            let owner = new.owner.as_ref().filter(|o| !o.personal).map(|o| o.path.clone());
            return Err(Error::NameTaken { forge: self.service, name: new.name.clone(), owner });
        }
        let v = self.read(made, &what)?;
        let (path, web, clone) = match self.service {
            Service::GitHub => (v["full_name"].as_str(), v["html_url"].as_str(), v["clone_url"].as_str()),
            Service::GitLab => (v["path_with_namespace"].as_str(), v["web_url"].as_str(), v["http_url_to_repo"].as_str()),
        };
        let path = path.ok_or_else(|| Error::Invalid("the new repository has no path".to_owned()))?;
        let address = Address { instance: self.instance.clone(), path: path.to_owned(), service: self.service };
        // Only addresses on this instance are taken from the answer.
        let on_instance = |u: Option<&str>| u.filter(|u| u.starts_with(&format!("{}/", self.instance))).map(str::to_owned);
        Ok(Created {
            web_url: on_instance(web).unwrap_or_else(|| address.web_url()),
            clone_url: on_instance(clone).unwrap_or_else(|| address.clone_url()),
            project_id: v["id"].as_u64().unwrap_or_default(),
            default_branch: v["default_branch"].as_str().filter(|b| !b.is_empty()).unwrap_or("main").to_owned(),
            address,
        })
    }

    /// Commits `files` (paths and bytes) as the first commit of `created`'s default branch, and
    /// links the project to it (consuming the creator: its transport goes to the new
    /// repository's client). On GitHub the repository was made with a first commit (its
    /// README), which this one replaces: GitHub's API cannot commit to an empty repository.
    pub async fn first_commit(self, created: &Created, files: &[(String, Vec<u8>)], message: &str) -> Result<Link> {
        let changes: Vec<FileChange> = files.iter().map(|(p, _)| FileChange { path: p.clone(), kind: ChangeKind::Added, from: None }).collect();
        let branch = created.default_branch.clone();
        let commit = match self.service {
            Service::GitLab => {
                let client = GitLab::new(self.transport, created.address.clone(), self.token.clone());
                let actions = crate::gitlab::actions(&changes, files);
                client.create_commit(&branch, message, actions, None).await?.0
            }
            Service::GitHub => {
                let client = GitHub::new(self.transport, created.address.clone(), self.token.clone());
                client.replace_branch(&branch, message, files).await?
            }
        };
        let ids: BTreeMap<String, String> =
            files.iter().map(|(p, b)| Ok((p.clone(), texcraft_history::store::blob_id(b).map_err(Error::Invalid)?))).collect::<Result<_>>()?;
        Ok(Link {
            address: created.address.clone(),
            project_id: created.project_id,
            web_url: created.web_url.clone(),
            branch,
            base: commit,
            files: ids,
            skipped: Vec::new(),
        })
    }
}

/// Whether a refusal's body says the name is taken: GitHub's "name already exists on this
/// account", GitLab's "has already been taken".
fn taken(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_lowercase();
    text.contains("already exists") || text.contains("already been taken")
}
