//! GitHub's REST API: a repository, its branches, its trees and blobs, and commits made through
//! the Git Data API (blobs, a tree on the parent's, a commit, then the branch moved to it without
//! forcing), each answer checked and its errors put in words.

use std::cell::Cell;
use std::collections::BTreeMap;

use base64::Engine as _;
use serde_json::{Value, json};

use crate::forge::{Forge, Made, Target};
use crate::gitlab::is_object_id;
use crate::http::check;
use crate::{
    Address, Branch, ChangeKind, Error, FileChange, MAX_FILE, MergeRequest, PARALLEL, ProjectInfo, Request, Response, Result, Service, Token,
    Transport, TreeEntry, encode_path,
};

/// Largest answer of the API read (a tree, a commit), apart from files' contents.
const MAX_JSON: u64 = 32 * 1024 * 1024;
/// Largest answer for a blob of [`MAX_FILE`] bytes: its contents in base64, in lines of 60
/// characters, with the JSON around them.
const MAX_BLOB: u64 = MAX_FILE / 2 * 3 + 64 * 1024;
/// The API version the requests are written for.
const API_VERSION: &str = "2022-11-28";

/// A client of one repository on GitHub (or a GitHub Enterprise Server), with the token it sends.
pub struct GitHub {
    transport: Box<dyn Transport>,
    address: Address,
    token: Token,
    /// Whether the token's user can push to the repository, once the repository was read.
    push: Cell<Option<bool>>,
}

impl GitHub {
    pub fn new(transport: Box<dyn Transport>, address: Address, token: Token) -> Self {
        Self { transport, address, token, push: Cell::new(None) }
    }

    fn repo_url(&self) -> String {
        format!("{}/repos/{}", self.address.api(), self.address.path)
    }

    fn request(&self, method: &'static str, url: String, body: Option<Vec<u8>>, max_body: u64) -> Request {
        let mut headers = vec![
            ("Authorization".to_owned(), self.token.header()),
            ("Accept".to_owned(), "application/vnd.github+json".to_owned()),
            ("X-GitHub-Api-Version".to_owned(), API_VERSION.to_owned()),
        ];
        if body.is_some() {
            headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
        }
        Request { method, url, headers, body, max_body }
    }

    /// Sends `request`; an answer that is not a success is an error about `what`.
    async fn send(&self, request: Request, what: &str) -> Result<Response> {
        let response = self.transport.send(request).await.map_err(|why| Error::Network { forge: Service::GitHub, why })?;
        check(Service::GitHub, response, what)
    }

    /// Sends `method` to `url` with `body` (none for a GET) and reads the JSON answer.
    async fn json(&self, method: &'static str, url: String, body: Option<Value>, what: &str) -> Result<Value> {
        let body = body.map(|b| serde_json::to_vec(&b)).transpose().map_err(|e| Error::Invalid(e.to_string()))?;
        let response = self.send(self.request(method, url, body, MAX_JSON), what).await?;
        if response.too_large {
            return Err(Error::Invalid(format!("the answer about {what} is too large")));
        }
        serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))
    }

    /// The repository: its id, path, default branch and page; and whether the token's user can
    /// push to it, kept for [`GitHub::branch`].
    pub async fn project(&self) -> Result<ProjectInfo> {
        let what = format!("repository {}", self.address.path);
        let v = self.json("GET", self.repo_url(), None, &what).await?;
        self.push.set(Some(v["permissions"]["push"].as_bool().unwrap_or(false)));
        Ok(ProjectInfo {
            id: v["id"].as_u64().ok_or_else(|| Error::Invalid("the repository has no id".to_owned()))?,
            path: v["full_name"].as_str().unwrap_or(&self.address.path).to_owned(),
            default_branch: v["default_branch"].as_str().filter(|b| !b.is_empty()).map(str::to_owned),
            web_url: self.on_instance(v["html_url"].as_str()).unwrap_or_else(|| self.address.web_url()),
        })
    }

    /// `url` when it is a page of this instance: only those are shown as the project's links.
    fn on_instance(&self, url: Option<&str>) -> Option<String> {
        url.filter(|u| u.starts_with(&format!("{}/", self.address.instance))).map(str::to_owned)
    }

    /// The commit at branch `name`'s head, and whether the branch is protected.
    async fn head(&self, name: &str) -> Result<(String, String, bool)> {
        let what = format!("branch {name} in {}", self.address.path);
        let v = self.json("GET", format!("{}/branches/{}", self.repo_url(), encode_path(name)), None, &what).await?;
        let commit =
            v["commit"]["sha"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the branch has no commit".to_owned()))?;
        Ok((v["name"].as_str().unwrap_or(name).to_owned(), commit.to_owned(), v["protected"].as_bool().unwrap_or(false)))
    }

    /// Branch `name`: the commit at its head, and whether the token's user can commit to it. A
    /// protected branch counts as one they cannot: its rules (reviews, checks) are visible only
    /// to the repository's administrators, so a pull request is the way that always works.
    pub async fn branch(&self, name: &str) -> Result<Branch> {
        if self.push.get().is_none() {
            self.project().await?;
        }
        let (name, commit, protected) = self.head(name).await?;
        Ok(Branch { name, commit, can_push: self.push.get().unwrap_or(false) && !protected })
    }

    /// Every entry of `commit`'s tree, its folders' too, in one answer; refused when GitHub lists
    /// only part of it (a tree of more than 100 000 entries or 7 MB).
    pub async fn tree(&self, commit: &str) -> Result<Vec<TreeEntry>> {
        if !is_object_id(commit) {
            return Err(Error::Invalid(format!("{commit} is not a commit's id")));
        }
        let what = format!("files of {} at {commit}", self.address.path);
        let v = self.json("GET", format!("{}/git/trees/{commit}?recursive=1", self.repo_url()), None, &what).await?;
        if v["truncated"].as_bool().unwrap_or(false) {
            return Err(Error::TooManyFiles { forge: Service::GitHub });
        }
        let list = v["tree"].as_array().ok_or_else(|| Error::Invalid("the tree is not a list".to_owned()))?;
        list.iter()
            .map(|e| {
                let (Some(path), Some(id), Some(kind)) = (e["path"].as_str(), e["sha"].as_str(), e["type"].as_str()) else {
                    return Err(Error::Invalid("an entry of the tree has no path, id or type".to_owned()));
                };
                Ok(TreeEntry {
                    path: path.to_owned(),
                    id: id.to_owned(),
                    kind: kind.to_owned(),
                    mode: e["mode"].as_str().unwrap_or_default().to_owned(),
                })
            })
            .collect()
    }

    /// The contents of blob `id`, checked against its id; none when it is over [`MAX_FILE`].
    pub async fn blob(&self, id: &str) -> Result<Option<Vec<u8>>> {
        if !is_object_id(id) {
            return Err(Error::Invalid(format!("{id} is not a file's id")));
        }
        let url = format!("{}/git/blobs/{id}", self.repo_url());
        let response = self.send(self.request("GET", url, None, MAX_BLOB), &format!("file {id} in {}", self.address.path)).await?;
        if response.too_large {
            return Ok(None);
        }
        let v: Value = serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))?;
        if v["size"].as_u64().is_some_and(|n| n > MAX_FILE) {
            return Ok(None);
        }
        let content = v["content"].as_str().ok_or_else(|| Error::Invalid(format!("file {id} has no contents")))?;
        let bytes = match v["encoding"].as_str() {
            Some("base64") => {
                let packed: String = content.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                base64::engine::general_purpose::STANDARD.decode(packed).map_err(|e| Error::Invalid(format!("file {id}: {e}")))?
            }
            Some("utf-8") | None => content.as_bytes().to_vec(),
            Some(other) => return Err(Error::Invalid(format!("file {id} comes in an encoding not known: {other}"))),
        };
        if bytes.len() as u64 > MAX_FILE {
            return Ok(None);
        }
        let got = texcraft_history::store::blob_id(&bytes).map_err(Error::Invalid)?;
        if !got.eq_ignore_ascii_case(id) {
            return Err(Error::Invalid(format!("the contents of file {id} do not match its id")));
        }
        Ok(Some(bytes))
    }

    /// Stores `bytes` as a blob; its id, checked against theirs.
    async fn create_blob(&self, bytes: &[u8]) -> Result<String> {
        let body = json!({"content": base64::engine::general_purpose::STANDARD.encode(bytes), "encoding": "base64"});
        let v = self.json("POST", format!("{}/git/blobs", self.repo_url()), Some(body), "a file's contents").await?;
        let id = v["sha"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the file has no id".to_owned()))?;
        let ours = texcraft_history::store::blob_id(bytes).map_err(Error::Invalid)?;
        if !ours.eq_ignore_ascii_case(id) {
            return Err(Error::Invalid("a file was stored with other contents than those sent".to_owned()));
        }
        Ok(id.to_ascii_lowercase())
    }

    /// The commit on `parent` with `changes` (their contents from `files`) and `message`, made
    /// but on no branch: its blobs, its tree on the parent's, then the commit. Its id.
    async fn make_commit(&self, parent: &str, message: &str, changes: &[FileChange], files: &[(String, Vec<u8>)]) -> Result<String> {
        if !is_object_id(parent) {
            return Err(Error::Invalid(format!("{parent} is not a commit's id")));
        }
        let what = format!("commit {parent} in {}", self.address.path);
        let base = self.json("GET", format!("{}/git/commits/{parent}", self.repo_url()), None, &what).await?;
        let base_tree =
            base["tree"]["sha"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the commit has no tree".to_owned()))?;
        // The files' new contents, as blobs.
        let bytes: BTreeMap<&str, &[u8]> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
        let contents = |path: &str| bytes.get(path).copied().unwrap_or_default();
        let written: Vec<&str> =
            changes.iter().filter(|c| matches!(c.kind, ChangeKind::Added | ChangeKind::Modified)).map(|c| c.path.as_str()).collect();
        let mut ids: BTreeMap<&str, String> = BTreeMap::new();
        for chunk in written.chunks(PARALLEL) {
            let made = crate::task::join_all(chunk.iter().map(|p| self.create_blob(contents(p))).collect()).await;
            for (path, id) in chunk.iter().zip(made) {
                ids.insert(path, id?);
            }
        }
        // A changed or moved file keeps its mode (an executable stays one): the parent's tree
        // says it, read only when there is such a file.
        let keeps = changes.iter().any(|c| matches!(c.kind, ChangeKind::Modified | ChangeKind::Moved));
        let modes: BTreeMap<String, String> =
            if keeps { self.tree(parent).await?.into_iter().filter(TreeEntry::is_file).map(|e| (e.path, e.mode)).collect() } else { BTreeMap::new() };
        let mode = |path: &str| modes.get(path).map_or("100644", String::as_str).to_owned();
        // The tree: the parent's with these entries changed; a moved file's blob is already there.
        let file = |path: &str, mode: String, sha: Value| json!({"path": path, "mode": mode, "type": "blob", "sha": sha});
        let mut entries = Vec::new();
        for c in changes {
            match c.kind {
                ChangeKind::Added | ChangeKind::Modified => entries.push(file(&c.path, mode(&c.path), json!(ids.get(c.path.as_str())))),
                ChangeKind::Deleted => entries.push(file(&c.path, mode(&c.path), Value::Null)),
                ChangeKind::Moved => {
                    let id = texcraft_history::store::blob_id(contents(&c.path)).map_err(Error::Invalid)?;
                    let from = c.from.as_deref().unwrap_or_default();
                    if !from.is_empty() {
                        entries.push(file(from, mode(from), Value::Null));
                    }
                    entries.push(file(&c.path, mode(from), json!(id)));
                }
            }
        }
        let tree = self
            .json("POST", format!("{}/git/trees", self.repo_url()), Some(json!({"base_tree": base_tree, "tree": entries})), "the commit's files")
            .await?;
        let tree = tree["sha"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the tree has no id".to_owned()))?;
        let body = json!({"message": message, "tree": tree, "parents": [parent]});
        let commit = self.json("POST", format!("{}/git/commits", self.repo_url()), Some(body), "the commit").await?;
        let id = commit["sha"].as_str().filter(|id| is_object_id(id)).ok_or_else(|| Error::Invalid("the commit has no id".to_owned()))?;
        Ok(id.to_owned())
    }

    /// Opens a pull request of branch `source` into `target`.
    pub async fn create_pull_request(&self, source: &str, target: &str, title: &str) -> Result<MergeRequest> {
        let body = json!({"title": title, "head": source, "base": target});
        let what = format!("a pull request of {source} into {target}");
        let v = self.json("POST", format!("{}/pulls", self.repo_url()), Some(body), &what).await?;
        let number = v["number"].as_u64().ok_or_else(|| Error::Invalid("the pull request has no number".to_owned()))?;
        let web_url = self.on_instance(v["html_url"].as_str()).unwrap_or_else(|| format!("{}/pull/{number}", self.address.web_url()));
        Ok(MergeRequest { iid: number, web_url })
    }
}

impl Forge for GitHub {
    fn address(&self) -> &Address {
        &self.address
    }

    async fn project(&self) -> Result<ProjectInfo> {
        GitHub::project(self).await
    }

    async fn branch(&self, name: &str) -> Result<Branch> {
        GitHub::branch(self, name).await
    }

    async fn tree(&self, commit: &str) -> Result<Vec<TreeEntry>> {
        GitHub::tree(self, commit).await
    }

    async fn blob(&self, id: &str) -> Result<Option<Vec<u8>>> {
        GitHub::blob(self, id).await
    }

    /// The commit is made on the head the branch had, then the branch is moved to it without
    /// forcing. When GitHub refuses (422) because the branch moved meanwhile, nothing is lost:
    /// [`Made::Moved`] says to take its new commits in and try again; when it refuses with the
    /// head unchanged, the branch's rules do not take this user's commits.
    async fn commit(&self, target: Target<'_>, message: &str, changes: &[FileChange], files: &[(String, Vec<u8>)]) -> Result<Made> {
        match target {
            Target::Branch { name, head } => {
                let id = self.make_commit(head, message, changes, files).await?;
                let url = format!("{}/git/refs/heads/{}", self.repo_url(), encode_path(name));
                let body = json!({"sha": id, "force": false});
                match self.json("PATCH", url, Some(body), &format!("the commit to branch {name}")).await {
                    Ok(_) => Ok(Made::Commit { id, parents: vec![head.to_owned()] }),
                    Err(Error::Refused { status: 422, .. }) => {
                        let (_, now, _) = self.head(name).await?;
                        if now == head { Err(Error::CannotPush { forge: Service::GitHub, branch: name.to_owned() }) } else { Ok(Made::Moved) }
                    }
                    Err(e) => Err(e),
                }
            }
            Target::NewBranch { name, start } => {
                let id = self.make_commit(start, message, changes, files).await?;
                let body = json!({"ref": format!("refs/heads/{name}"), "sha": id});
                self.json("POST", format!("{}/git/refs", self.repo_url()), Some(body), &format!("the new branch {name}")).await?;
                Ok(Made::Commit { id, parents: vec![start.to_owned()] })
            }
        }
    }

    async fn request_merge(&self, source: &str, target: &str, title: &str) -> Result<MergeRequest> {
        self.create_pull_request(source, target, title).await
    }
}
