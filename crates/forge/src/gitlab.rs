//! GitLab's REST API (version 4): the calls a project opened from it needs, each answer checked
//! and its errors put in words.

use serde_json::{Value, json};

use std::collections::BTreeMap;

use base64::Engine as _;

use crate::forge::{Forge, Made, Target};
use crate::http::check;
use crate::{Address, ChangeKind, Error, FileChange, MAX_FILE, Request, Response, Result, Service, Token, Transport, encode};

/// Largest answer of the API read (a page of the tree, a commit), apart from files' contents.
const MAX_JSON: u64 = 16 * 1024 * 1024;
/// Entries a page of the tree lists.
const PER_PAGE: usize = 100;
/// Pages of the tree read at most: more than a project holds (`texcraft_project::MAX_FILES`).
const MAX_PAGES: usize = 200;

/// A client of one project on one GitLab instance, with the token it sends.
pub struct GitLab {
    transport: Box<dyn Transport>,
    address: Address,
    token: Token,
}

/// What the API says of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub id: u64,
    /// `group/project`, as GitLab writes it.
    pub path: String,
    /// None for a project with an empty repository.
    pub default_branch: Option<String>,
    pub web_url: String,
}

/// A branch and the commit at its head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub commit: String,
    /// Whether the token's user can commit to it (false for a protected branch, or a role that
    /// cannot push).
    pub can_push: bool,
}

/// An entry of a commit's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    /// The object's id: a file's blob id.
    pub id: String,
    /// `blob` (a file or a symbolic link), `tree` (a folder) or `commit` (a submodule).
    pub kind: String,
    /// Git's mode: `100644`, `100755`, `120000` (a symbolic link)…
    pub mode: String,
}

impl TreeEntry {
    /// A file a project loads: a blob that is not a symbolic link.
    pub fn is_file(&self) -> bool {
        self.kind == "blob" && self.mode != "120000"
    }
}

/// Whether `id` is a full git object id (40 hexadecimal digits).
pub(crate) fn is_object_id(id: &str) -> bool {
    id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl GitLab {
    pub fn new(transport: Box<dyn Transport>, address: Address, token: Token) -> Self {
        Self { transport, address, token }
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    fn project_url(&self) -> String {
        format!("{}/projects/{}", self.address.api(), self.address.id())
    }

    fn request(&self, method: &'static str, url: String, body: Option<Vec<u8>>, max_body: u64) -> Request {
        let mut headers = vec![("Authorization".to_owned(), self.token.header()), ("Accept".to_owned(), "application/json".to_owned())];
        if body.is_some() {
            headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
        }
        Request { method, url, headers, body, max_body }
    }

    /// Sends `request`; an answer that is not a success is an error about `what`.
    async fn send(&self, request: Request, what: &str) -> Result<Response> {
        let response = self.transport.send(request).await.map_err(|why| Error::Network { forge: Service::GitLab, why })?;
        check(Service::GitLab, response, what)
    }

    async fn get_json(&self, url: String, what: &str) -> Result<(Value, Response)> {
        let response = self.send(self.request("GET", url, None, MAX_JSON), what).await?;
        if response.too_large {
            return Err(Error::Invalid(format!("the answer about {what} is too large")));
        }
        let value = serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))?;
        Ok((value, response))
    }

    /// The project: its id, path and default branch.
    pub async fn project(&self) -> Result<ProjectInfo> {
        let what = format!("project {}", self.address.path);
        let (v, _) = self.get_json(self.project_url(), &what).await?;
        Ok(ProjectInfo {
            id: v["id"].as_u64().ok_or_else(|| Error::Invalid("the project has no id".to_owned()))?,
            path: v["path_with_namespace"].as_str().unwrap_or(&self.address.path).to_owned(),
            default_branch: v["default_branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned),
            web_url: v["web_url"].as_str().map_or_else(|| self.address.web_url(), str::to_owned),
        })
    }

    /// Branch `name`: the commit at its head, and whether the token's user can commit to it.
    pub async fn branch(&self, name: &str) -> Result<Branch> {
        let what = format!("branch {name} in {}", self.address.path);
        let url = format!("{}/repository/branches/{}", self.project_url(), encode(name));
        let (v, _) = self.get_json(url, &what).await?;
        let commit = v["commit"]["id"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the branch has no commit".to_owned()))?;
        Ok(Branch {
            name: v["name"].as_str().unwrap_or(name).to_owned(),
            commit: commit.to_owned(),
            can_push: v["can_push"].as_bool().unwrap_or(false),
        })
    }

    /// Every entry of `commit`'s tree, its folders' too, page after page.
    pub async fn tree(&self, commit: &str) -> Result<Vec<TreeEntry>> {
        if !is_object_id(commit) {
            return Err(Error::Invalid(format!("{commit} is not a commit's id")));
        }
        let what = format!("files of {} at {commit}", self.address.path);
        let first = format!("{}/repository/tree?ref={commit}&recursive=true&per_page={PER_PAGE}", self.project_url());
        let mut url = first.clone();
        let mut entries = Vec::new();
        for _ in 0..MAX_PAGES {
            let (v, response) = self.get_json(url, &what).await?;
            let page = v.as_array().ok_or_else(|| Error::Invalid("the tree is not a list".to_owned()))?;
            for e in page {
                let (Some(path), Some(id), Some(kind)) = (e["path"].as_str(), e["id"].as_str(), e["type"].as_str()) else {
                    return Err(Error::Invalid("an entry of the tree has no path, id or type".to_owned()));
                };
                entries.push(TreeEntry {
                    path: path.to_owned(),
                    id: id.to_owned(),
                    kind: kind.to_owned(),
                    mode: e["mode"].as_str().unwrap_or_default().to_owned(),
                });
            }
            // The next page: from the `Link` header (keyset pagination), else by number. Only an
            // address of this instance's API, which the token may go to.
            let api = format!("{}/", self.address.api());
            let next = response.header("link").and_then(next_link).filter(|n| n.starts_with(&api)).or_else(|| {
                let page = response.header("x-next-page").map(str::trim).filter(|p| !p.is_empty())?;
                page.parse::<u32>().ok().map(|n| format!("{first}&page={n}"))
            });
            match next {
                Some(next) => url = next,
                None => return Ok(entries),
            }
        }
        Err(Error::Invalid(format!("the project lists more than {} files and folders", MAX_PAGES * PER_PAGE)))
    }

    /// The contents of blob `id`, checked against its id; none when it is over [`MAX_FILE`].
    pub async fn blob(&self, id: &str) -> Result<Option<Vec<u8>>> {
        if !is_object_id(id) {
            return Err(Error::Invalid(format!("{id} is not a file's id")));
        }
        let url = format!("{}/repository/blobs/{id}/raw", self.project_url());
        let response = self.send(self.request("GET", url, None, MAX_FILE), &format!("file {id} in {}", self.address.path)).await?;
        if response.too_large {
            return Ok(None);
        }
        let got = texcraft_history::store::blob_id(&response.body).map_err(Error::Invalid)?;
        if !got.eq_ignore_ascii_case(id) {
            return Err(Error::Invalid(format!("the contents of file {id} do not match its id")));
        }
        Ok(Some(response.body))
    }

    /// Makes one commit of `actions` (the API's `create`, `update`, `delete`, `move`) on `branch`,
    /// or on a new branch `branch` from commit `start`: its id and its parents' ids.
    pub async fn create_commit(&self, branch: &str, message: &str, actions: Vec<Value>, start: Option<&str>) -> Result<(String, Vec<String>)> {
        let mut body = json!({"branch": branch, "commit_message": message, "actions": actions});
        if let Some(start) = start {
            body["start_sha"] = json!(start);
        }
        let body = serde_json::to_vec(&body).map_err(|e| Error::Invalid(e.to_string()))?;
        let url = format!("{}/repository/commits", self.project_url());
        let response = self.send(self.request("POST", url, Some(body), MAX_JSON), &format!("the commit to branch {branch}")).await?;
        let v: Value = serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))?;
        let id = v["id"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the commit has no id".to_owned()))?;
        let parents = v["parent_ids"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        Ok((id.to_owned(), parents))
    }

    /// Opens a merge request of branch `source` into `target`.
    pub async fn create_merge_request(&self, source: &str, target: &str, title: &str) -> Result<crate::MergeRequest> {
        let body = json!({"source_branch": source, "target_branch": target, "title": title});
        let body = serde_json::to_vec(&body).map_err(|e| Error::Invalid(e.to_string()))?;
        let url = format!("{}/merge_requests", self.project_url());
        let what = format!("a merge request of {source} into {target}");
        let response = self.send(self.request("POST", url, Some(body), MAX_JSON), &what).await?;
        let v: Value = serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))?;
        let iid = v["iid"].as_u64().ok_or_else(|| Error::Invalid("the merge request has no number".to_owned()))?;
        // Only an address on this instance is shown as the merge request's link.
        let web_url = v["web_url"]
            .as_str()
            .filter(|u| u.starts_with(&format!("{}/", self.address.instance)))
            .map_or_else(|| format!("{}/-/merge_requests/{iid}", self.address.web_url()), str::to_owned);
        Ok(crate::MergeRequest { iid, web_url })
    }
}

impl Forge for GitLab {
    fn address(&self) -> &Address {
        &self.address
    }

    async fn project(&self) -> Result<ProjectInfo> {
        GitLab::project(self).await
    }

    async fn branch(&self, name: &str) -> Result<Branch> {
        GitLab::branch(self, name).await
    }

    async fn tree(&self, commit: &str) -> Result<Vec<TreeEntry>> {
        GitLab::tree(self, commit).await
    }

    async fn blob(&self, id: &str) -> Result<Option<Vec<u8>>> {
        GitLab::blob(self, id).await
    }

    /// GitLab puts a commit to an existing branch on top of its head whatever it is: the branch
    /// never [`Made::Moved`], and a commit made meanwhile shows as the parent.
    async fn commit(&self, target: Target<'_>, message: &str, changes: &[FileChange], files: &[(String, Vec<u8>)]) -> Result<Made> {
        let (branch, start) = match target {
            Target::Branch { name, .. } => (name, None),
            Target::NewBranch { name, start } => (name, Some(start)),
        };
        let (id, parents) = self.create_commit(branch, message, actions(changes, files), start).await?;
        Ok(Made::Commit { id, parents })
    }

    async fn request_merge(&self, source: &str, target: &str, title: &str) -> Result<crate::MergeRequest> {
        self.create_merge_request(source, target, title).await
    }
}

/// The commit API's actions for `changes`, the contents from `files`, in base64.
fn actions(changes: &[FileChange], files: &[(String, Vec<u8>)]) -> Vec<Value> {
    let bytes: BTreeMap<&str, &[u8]> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let content = |path: &str| base64::engine::general_purpose::STANDARD.encode(bytes.get(path).copied().unwrap_or_default());
    changes
        .iter()
        .map(|c| match c.kind {
            ChangeKind::Added => json!({"action": "create", "file_path": c.path, "content": content(&c.path), "encoding": "base64"}),
            ChangeKind::Modified => json!({"action": "update", "file_path": c.path, "content": content(&c.path), "encoding": "base64"}),
            ChangeKind::Deleted => json!({"action": "delete", "file_path": c.path}),
            ChangeKind::Moved => json!({"action": "move", "file_path": c.path, "previous_path": c.from}),
        })
        .collect()
}

/// The `rel="next"` address of a `Link` header: `<url>; rel="next", <url>; rel="first"`. Each
/// address is read whole between its brackets, so a comma in it (GitHub's
/// `affiliation=owner,collaborator`) does not split it.
pub(crate) fn next_link(header: &str) -> Option<String> {
    let mut rest = header;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let close = after.find('>')?;
        let url = &after[..close];
        let params = &after[close + 1..];
        let params = params.split('<').next().unwrap_or_default();
        if params.split([';', ',']).any(|p| matches!(p.trim(), "rel=\"next\"" | "rel=next")) {
            return Some(url.trim().to_owned());
        }
        rest = &after[close + 1..];
    }
    None
}
