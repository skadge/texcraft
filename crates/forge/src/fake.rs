//! A forge in memory, for tests: one project, its branches and commits, answering the API's calls
//! that [`crate::GitLab`] makes as gitlab.com does (pagination, raw blobs, commits with actions,
//! error statuses), or those [`crate::GitHub`] makes as github.com does (trees, blobs in base64,
//! the Git Data API's blobs, trees, commits and references, pull requests). No test reaches the
//! network.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine as _;
use serde_json::{Value, json};

use crate::{Address, GITHUB_COM, Request, Response, Sending, Service, Transport};

/// A file of a commit: its mode (`100644`, `120000` for a symbolic link, `160000` for a
/// submodule) and its object id.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    mode: String,
    id: String,
}

#[derive(Debug, Clone)]
struct Commit {
    parent: Option<String>,
    files: BTreeMap<String, Entry>,
    /// Its tree's id (GitHub's API names trees).
    tree: String,
}

#[derive(Debug, Default)]
struct State {
    service: Service,
    instance: String,
    path: String,
    token: String,
    default_branch: Option<String>,
    can_push: bool,
    branches: BTreeMap<String, String>,
    commits: BTreeMap<String, Commit>,
    blobs: BTreeMap<String, Vec<u8>>,
    per_page: usize,
    link_header: bool,
    fail: Option<Response>,
    requests: Vec<String>,
    last_actions: Vec<Value>,
    /// The paths the last commit request named.
    last_paths: Vec<String>,
    /// Trees made by GitHub's API, by id: their files.
    trees: BTreeMap<String, BTreeMap<String, Entry>>,
    /// Branches whose rules (GitHub's rulesets) refuse this user's commits, although the branch
    /// does not say it is protected.
    rules: std::collections::BTreeSet<String>,
    /// Whether GitHub says its tree is truncated.
    truncated: bool,
    /// A commit someone else makes just before the next commit request: branch, path, bytes.
    race: Option<(String, String, Vec<u8>)>,
    counter: u64,
    /// Branches no one can commit to.
    protected: std::collections::BTreeSet<String>,
    merge_requests: Vec<FakeMergeRequest>,
}

/// A merge request (a pull request on GitHub) opened on the fake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeMergeRequest {
    pub iid: u64,
    pub source: String,
    pub target: String,
    pub title: String,
}

/// The fake instance; clones share it.
#[derive(Clone)]
pub struct Fake {
    state: Arc<Mutex<State>>,
}

/// The fake as a GitLab ([`Fake::new`]).
pub type FakeGitLab = Fake;

fn id_of(text: &str) -> String {
    texcraft_history::store::blob_id(text.as_bytes()).unwrap_or_default()
}

impl Fake {
    /// A GitLab: project `path` (`group/project`) on `instance` (`https://gitlab.example`), whose
    /// API accepts `token`, with a branch `main` holding `files`.
    pub fn new(instance: &str, path: &str, token: &str, files: &[(&str, &[u8])]) -> Self {
        Self::with(Service::GitLab, instance, path, token, files)
    }

    /// A GitHub: repository `path` (`owner/repository`) on github.com, whose API accepts
    /// `token`, with a branch `main` holding `files`.
    pub fn github(path: &str, token: &str, files: &[(&str, &[u8])]) -> Self {
        Self::with(Service::GitHub, GITHUB_COM, path, token, files)
    }

    fn with(service: Service, instance: &str, path: &str, token: &str, files: &[(&str, &[u8])]) -> Self {
        let state = State {
            service,
            instance: instance.to_owned(),
            path: path.to_owned(),
            token: token.to_owned(),
            default_branch: Some("main".to_owned()),
            can_push: true,
            per_page: 100,
            link_header: true,
            ..Default::default()
        };
        let fake = Self { state: Arc::new(Mutex::new(state)) };
        let changes: Vec<(&str, Option<&[u8]>)> = files.iter().map(|(p, b)| (*p, Some(*b))).collect();
        fake.push("main", &changes);
        fake
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The project's address.
    pub fn address(&self) -> Address {
        let s = self.lock();
        Address { instance: s.instance.clone(), path: s.path.clone(), service: s.service }
    }

    /// A transport to this instance.
    pub fn transport(&self) -> Box<dyn Transport> {
        Box::new(self.clone())
    }

    /// Someone commits to `branch`: each path's new bytes, or none to delete it. The commit's id.
    pub fn push(&self, branch: &str, changes: &[(&str, Option<&[u8]>)]) -> String {
        let mut s = self.lock();
        let parent = s.branches.get(branch).cloned();
        let mut files = parent.as_ref().and_then(|p| s.commits.get(p)).map(|c| c.files.clone()).unwrap_or_default();
        for (path, bytes) in changes {
            match bytes {
                Some(bytes) => {
                    let id = texcraft_history::store::blob_id(bytes).unwrap_or_default();
                    s.blobs.insert(id.clone(), bytes.to_vec());
                    files.insert((*path).to_owned(), Entry { mode: "100644".to_owned(), id });
                }
                None => {
                    files.remove(*path);
                }
            }
        }
        new_commit(&mut s, branch, parent, files)
    }

    /// Adds an entry that is not a file to `branch` (a symbolic link `120000`, a submodule
    /// `160000`), in a commit of its own.
    pub fn push_entry(&self, branch: &str, path: &str, mode: &str) -> String {
        let mut s = self.lock();
        let parent = s.branches.get(branch).cloned();
        let mut files = parent.as_ref().and_then(|p| s.commits.get(p)).map(|c| c.files.clone()).unwrap_or_default();
        let id = id_of(&format!("{mode} {path}"));
        s.blobs.insert(id.clone(), b"target".to_vec());
        files.insert(path.to_owned(), Entry { mode: mode.to_owned(), id });
        new_commit(&mut s, branch, parent, files)
    }

    /// Gives file `path` of `branch` git's mode `mode` (`100755`: executable), in a commit of its
    /// own.
    pub fn chmod(&self, branch: &str, path: &str, mode: &str) -> String {
        let mut s = self.lock();
        let parent = s.branches.get(branch).cloned();
        let mut files = parent.as_ref().and_then(|p| s.commits.get(p)).map(|c| c.files.clone()).unwrap_or_default();
        if let Some(e) = files.get_mut(path) {
            e.mode = mode.to_owned();
        }
        new_commit(&mut s, branch, parent, files)
    }

    /// The mode of file `path` at `branch`'s head.
    pub fn mode(&self, branch: &str, path: &str) -> Option<String> {
        let s = self.lock();
        s.branches.get(branch).and_then(|c| s.commits.get(c)).and_then(|c| c.files.get(path)).map(|e| e.mode.clone())
    }

    /// The files of `branch` at its head: paths and bytes.
    pub fn files(&self, branch: &str) -> BTreeMap<String, Vec<u8>> {
        let s = self.lock();
        let Some(commit) = s.branches.get(branch).and_then(|c| s.commits.get(c)) else { return BTreeMap::new() };
        commit.files.iter().filter_map(|(p, e)| Some((p.clone(), s.blobs.get(&e.id)?.clone()))).collect()
    }

    /// The commit at `branch`'s head.
    pub fn head(&self, branch: &str) -> Option<String> {
        self.lock().branches.get(branch).cloned()
    }

    /// The parent of commit `id`.
    pub fn parent(&self, id: &str) -> Option<String> {
        self.lock().commits.get(id).and_then(|c| c.parent.clone())
    }

    /// The paths the last commit request named: its actions' (GitLab), its tree's entries'
    /// (GitHub).
    pub fn last_paths(&self) -> Vec<String> {
        self.lock().last_paths.clone()
    }

    /// Whether GitHub answers that its trees are truncated (too many entries).
    pub fn set_truncated(&self, on: bool) {
        self.lock().truncated = on;
    }

    /// The actions of the last commit requested (GitLab).
    pub fn last_actions(&self) -> Vec<Value> {
        self.lock().last_actions.clone()
    }

    /// The requests answered so far, as `METHOD url`.
    pub fn requests(&self) -> Vec<String> {
        self.lock().requests.clone()
    }

    /// Entries per page of the tree.
    pub fn set_per_page(&self, n: usize) {
        self.lock().per_page = n.max(1);
    }

    /// Whether the next page of the tree is given in a `Link` header (else only `X-Next-Page`).
    pub fn set_link_header(&self, on: bool) {
        self.lock().link_header = on;
    }

    /// Whether the token's user can commit to the branches.
    pub fn set_can_push(&self, on: bool) {
        self.lock().can_push = on;
    }

    /// Protects `branch`: the token's user cannot commit to it (other branches stay open).
    pub fn protect(&self, branch: &str) {
        self.lock().protected.insert(branch.to_owned());
    }

    /// Gives `branch` a rule that refuses the user's commits when the branch is moved, which the
    /// branch does not show (GitHub).
    pub fn rule(&self, branch: &str) {
        self.lock().rules.insert(branch.to_owned());
    }

    /// The merge requests opened so far.
    pub fn merge_requests(&self) -> Vec<FakeMergeRequest> {
        self.lock().merge_requests.clone()
    }

    /// The names of the branches.
    pub fn branches(&self) -> Vec<String> {
        self.lock().branches.keys().cloned().collect()
    }

    /// Every request is answered with `status`, these headers (names in lower case) and this body
    /// from now on.
    pub fn fail_with(&self, status: u16, headers: &[(&str, &str)], body: &str) {
        let headers = headers.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        self.lock().fail = Some(Response { status, headers, body: body.as_bytes().to_vec(), too_large: false });
    }

    /// Requests are answered normally again.
    pub fn heal(&self) {
        self.lock().fail = None;
    }

    /// Just before the next commit request (GitLab), or the next move of a branch (GitHub),
    /// someone else commits `bytes` to `path` on `branch`.
    pub fn race(&self, branch: &str, path: &str, bytes: &[u8]) {
        self.lock().race = Some((branch.to_owned(), path.to_owned(), bytes.to_vec()));
    }
}

/// A commit of `files` on `parent`, on no branch.
fn make_commit(s: &mut State, parent: Option<String>, files: BTreeMap<String, Entry>) -> String {
    s.counter += 1;
    let id = id_of(&format!("commit {}", s.counter));
    let tree = make_tree(s, files.clone());
    s.commits.insert(id.clone(), Commit { parent, files, tree });
    id
}

fn make_tree(s: &mut State, files: BTreeMap<String, Entry>) -> String {
    s.counter += 1;
    let id = id_of(&format!("tree {}", s.counter));
    s.trees.insert(id.clone(), files);
    id
}

fn new_commit(s: &mut State, branch: &str, parent: Option<String>, files: BTreeMap<String, Entry>) -> String {
    let id = make_commit(s, parent, files);
    s.branches.insert(branch.to_owned(), id.clone());
    id
}

/// Someone else's commit, made just before the request that `race` waits for.
fn run_race(s: &mut State) {
    if let Some((branch, path, bytes)) = s.race.take() {
        let parent = s.branches.get(&branch).cloned();
        let mut files = parent.as_ref().and_then(|p| s.commits.get(p)).map(|c| c.files.clone()).unwrap_or_default();
        let id = texcraft_history::store::blob_id(&bytes).unwrap_or_default();
        s.blobs.insert(id.clone(), bytes);
        files.insert(path, Entry { mode: "100644".to_owned(), id });
        new_commit(s, &branch, parent, files);
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json_response(status: u16, value: &Value) -> Response {
    Response {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: value.to_string().into_bytes(),
        too_large: false,
    }
}

fn message(status: u16, text: &str) -> Response {
    json_response(status, &json!({"message": text}))
}

fn query(url: &str) -> BTreeMap<String, String> {
    url.split_once('?')
        .map(|(_, q)| q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_owned(), decode(v))).collect())
        .unwrap_or_default()
}

impl State {
    fn answer(&mut self, request: &Request) -> Response {
        self.requests.push(format!("{} {}", request.method, request.url));
        if let Some(failure) = &self.fail {
            return failure.clone();
        }
        let auth = request.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.as_str());
        if auth != Some(format!("Bearer {}", self.token).as_str()) {
            return message(401, if self.service == Service::GitHub { "Bad credentials" } else { "401 Unauthorized" });
        }
        if self.service == Service::GitHub {
            return self.github(request);
        }
        let prefix = format!("{}/api/v4/projects/{}", self.instance, crate::encode(&self.path));
        let Some(rest) = request.url.strip_prefix(&prefix) else { return message(404, "404 Project Not Found") };
        let route = rest.split('?').next().unwrap_or_default();
        match (request.method, route) {
            ("GET", "") => json_response(
                200,
                &json!({"id": 42, "path_with_namespace": self.path, "default_branch": self.default_branch, "web_url": format!("{}/{}", self.instance, self.path)}),
            ),
            ("GET", r) if r.starts_with("/repository/branches/") => {
                let name = decode(&r["/repository/branches/".len()..]);
                match self.branches.get(&name) {
                    Some(commit) => {
                        let can_push = self.can_push && !self.protected.contains(&name);
                        json_response(
                            200,
                            &json!({"name": name, "commit": {"id": commit}, "can_push": can_push, "protected": self.protected.contains(&name)}),
                        )
                    }
                    None => message(404, "404 Branch Not Found"),
                }
            }
            ("GET", "/repository/tree") => self.tree(request),
            ("GET", r) if r.starts_with("/repository/blobs/") && r.ends_with("/raw") => {
                let id = &r["/repository/blobs/".len()..r.len() - "/raw".len()];
                match self.blobs.get(id) {
                    Some(bytes) if bytes.len() as u64 > request.max_body => Response { status: 200, too_large: true, ..Default::default() },
                    Some(bytes) => Response { status: 200, headers: Vec::new(), body: bytes.clone(), too_large: false },
                    None => message(404, "404 Not Found"),
                }
            }
            ("POST", "/repository/commits") => self.commit(request),
            ("POST", "/merge_requests") => self.merge_request(request),
            _ => message(404, "404 Not Found"),
        }
    }

    fn tree(&self, request: &Request) -> Response {
        let q = query(&request.url);
        let Some(commit) = q.get("ref").and_then(|r| self.commits.get(r).or_else(|| self.branches.get(r).and_then(|c| self.commits.get(c)))) else {
            return message(404, "404 Tree Not Found");
        };
        // Folders are listed too, as GitLab does with `recursive=true`.
        let mut entries: BTreeMap<String, Value> = BTreeMap::new();
        for (path, e) in &commit.files {
            let mut at = 0;
            while let Some(i) = path[at..].find('/') {
                let dir = &path[..at + i];
                entries
                    .entry(dir.to_owned())
                    .or_insert_with(|| json!({"id": id_of(dir), "name": dir, "type": "tree", "path": dir, "mode": "040000"}));
                at += i + 1;
            }
            let kind = if e.mode == "160000" { "commit" } else { "blob" };
            entries.insert(path.clone(), json!({"id": e.id, "name": path.rsplit('/').next(), "type": kind, "path": path, "mode": e.mode}));
        }
        let all: Vec<Value> = entries.into_values().collect();
        let page: usize = q.get("page").and_then(|p| p.parse().ok()).unwrap_or(1).max(1);
        let start = (page - 1) * self.per_page;
        let items: Vec<Value> = all.iter().skip(start).take(self.per_page).cloned().collect();
        let mut response = json_response(200, &Value::Array(items));
        let more = start + self.per_page < all.len();
        let next = if more { (page + 1).to_string() } else { String::new() };
        response.headers.push(("x-next-page".to_owned(), next.clone()));
        if more && self.link_header {
            let base = request.url.split("&page=").next().unwrap_or(&request.url);
            response.headers.push(("link".to_owned(), format!("<{base}&page={next}>; rel=\"next\", <{base}&page=1>; rel=\"first\"")));
        }
        response
    }

    fn commit(&mut self, request: &Request) -> Response {
        run_race(self);
        let Ok(body) = serde_json::from_slice::<Value>(request.body.as_deref().unwrap_or_default()) else { return message(400, "bad JSON") };
        let (Some(branch), Some(_message), Some(actions)) = (body["branch"].as_str(), body["commit_message"].as_str(), body["actions"].as_array())
        else {
            return message(400, "branch, commit_message and actions are required");
        };
        self.last_actions = actions.clone();
        self.last_paths = actions.iter().filter_map(|a| a["file_path"].as_str().map(str::to_owned)).collect();
        if !self.can_push || self.protected.contains(branch) {
            return message(403, "403 Forbidden - You are not allowed to push into this branch");
        }
        let parent = match (body["start_sha"].as_str(), self.branches.get(branch)) {
            (Some(_), Some(_)) => {
                return message(400, &format!("A branch called '{branch}' already exists. Switch to that branch in order to make changes"));
            }
            (Some(start), None) if self.commits.contains_key(start) => start.to_owned(),
            (Some(_), None) => return message(404, "404 Commit Not Found"),
            (None, Some(head)) => head.clone(),
            (None, None) => return message(400, "You can only create or edit files when you are on a branch"),
        };
        let mut files = self.commits.get(&parent).map(|c| c.files.clone()).unwrap_or_default();
        for a in actions {
            let path = a["file_path"].as_str().unwrap_or_default().to_owned();
            let content = || -> Option<Vec<u8>> {
                let c = a["content"].as_str()?;
                if a["encoding"].as_str() == Some("base64") {
                    base64::engine::general_purpose::STANDARD.decode(c).ok()
                } else {
                    Some(c.as_bytes().to_vec())
                }
            };
            match a["action"].as_str() {
                Some("create") | Some("update") => {
                    let exists = files.contains_key(&path);
                    if a["action"] == "create" && exists {
                        return message(400, "A file with this name already exists");
                    }
                    if a["action"] == "update" && !exists {
                        return message(400, "A file with this name doesn't exist");
                    }
                    let Some(bytes) = content() else { return message(400, "content is missing") };
                    let id = texcraft_history::store::blob_id(&bytes).unwrap_or_default();
                    self.blobs.insert(id.clone(), bytes);
                    files.insert(path, Entry { mode: "100644".to_owned(), id });
                }
                Some("delete") => {
                    if files.remove(&path).is_none() {
                        return message(400, "A file with this name doesn't exist");
                    }
                }
                Some("move") => {
                    let from = a["previous_path"].as_str().unwrap_or_default();
                    let Some(mut entry) = files.remove(from) else { return message(400, "A file with this name doesn't exist") };
                    if let Some(bytes) = content() {
                        entry.id = texcraft_history::store::blob_id(&bytes).unwrap_or_default();
                        self.blobs.insert(entry.id.clone(), bytes);
                    }
                    if files.contains_key(&path) {
                        return message(400, "A file with this name already exists");
                    }
                    files.insert(path, entry);
                }
                _ => return message(400, "unknown action"),
            }
        }
        let id = new_commit(self, branch, Some(parent.clone()), files);
        json_response(201, &json!({"id": id, "parent_ids": [parent]}))
    }
}

impl State {
    fn merge_request(&mut self, request: &Request) -> Response {
        let Ok(body) = serde_json::from_slice::<Value>(request.body.as_deref().unwrap_or_default()) else { return message(400, "bad JSON") };
        let (Some(source), Some(target), Some(title)) = (body["source_branch"].as_str(), body["target_branch"].as_str(), body["title"].as_str())
        else {
            return message(400, "source_branch, target_branch and title are required");
        };
        if !self.branches.contains_key(source) || !self.branches.contains_key(target) {
            return message(404, "404 Branch Not Found");
        }
        if !self.can_push {
            return message(403, "403 Forbidden");
        }
        let iid = self.merge_requests.len() as u64 + 1;
        self.merge_requests.push(FakeMergeRequest { iid, source: source.to_owned(), target: target.to_owned(), title: title.to_owned() });
        json_response(201, &json!({"iid": iid, "web_url": format!("{}/{}/-/merge_requests/{iid}", self.instance, self.path)}))
    }
}

/// GitHub's answers.
impl State {
    fn github(&mut self, request: &Request) -> Response {
        let prefix = format!("https://api.github.com/repos/{}", self.path);
        let Some(rest) = request.url.strip_prefix(&prefix) else { return message(404, "Not Found") };
        let route = rest.split('?').next().unwrap_or_default().to_owned();
        let body = serde_json::from_slice::<Value>(request.body.as_deref().unwrap_or_default()).unwrap_or(Value::Null);
        let writes = request.method != "GET";
        if writes && !self.can_push {
            return message(403, "Resource not accessible by personal access token");
        }
        match (request.method, route.as_str()) {
            ("GET", "") => json_response(
                200,
                &json!({
                    "id": 4242,
                    "full_name": self.path,
                    "default_branch": self.default_branch,
                    "html_url": format!("{}/{}", self.instance, self.path),
                    "permissions": {"admin": false, "push": self.can_push, "pull": true},
                }),
            ),
            ("GET", r) if r.starts_with("/branches/") => {
                let name = decode(&r["/branches/".len()..]);
                match self.branches.get(&name) {
                    Some(commit) => {
                        json_response(200, &json!({"name": name, "commit": {"sha": commit}, "protected": self.protected.contains(&name)}))
                    }
                    None => message(404, "Branch not found"),
                }
            }
            ("GET", r) if r.starts_with("/git/trees/") => self.github_tree(&r["/git/trees/".len()..]),
            ("GET", r) if r.starts_with("/git/blobs/") => match self.blobs.get(&r["/git/blobs/".len()..]) {
                Some(bytes) => {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    // In lines of 60 characters, as GitHub sends it.
                    let lines: Vec<String> = encoded.as_bytes().chunks(60).map(|l| String::from_utf8_lossy(l).into_owned()).collect();
                    let id = &r["/git/blobs/".len()..];
                    let response = json_response(200, &json!({"sha": id, "size": bytes.len(), "content": lines.join("\n"), "encoding": "base64"}));
                    if response.body.len() as u64 > request.max_body {
                        Response { status: 200, too_large: true, ..Default::default() }
                    } else {
                        response
                    }
                }
                None => message(404, "Not Found"),
            },
            ("GET", r) if r.starts_with("/git/commits/") => match self.commits.get(&r["/git/commits/".len()..]) {
                Some(c) => json_response(
                    200,
                    &json!({"sha": &r["/git/commits/".len()..], "tree": {"sha": c.tree}, "parents": c.parent.iter().map(|p| json!({"sha": p})).collect::<Vec<_>>()}),
                ),
                None => message(404, "Not Found"),
            },
            ("POST", "/git/blobs") => {
                let Some(bytes) = body["content"].as_str().and_then(|c| base64::engine::general_purpose::STANDARD.decode(c).ok()) else {
                    return message(422, "Invalid request.");
                };
                let id = texcraft_history::store::blob_id(&bytes).unwrap_or_default();
                self.blobs.insert(id.clone(), bytes);
                json_response(201, &json!({"sha": id}))
            }
            ("POST", "/git/trees") => self.github_new_tree(&body),
            ("POST", "/git/commits") => {
                let (Some(tree), Some(parents)) = (body["tree"].as_str(), body["parents"].as_array()) else {
                    return message(422, "Invalid request.");
                };
                let Some(files) = self.trees.get(tree).cloned() else { return message(422, "Tree SHA does not exist") };
                let parent = parents.first().and_then(Value::as_str).map(str::to_owned);
                if parent.as_ref().is_some_and(|p| !self.commits.contains_key(p)) {
                    return message(422, "Parent SHA does not exist or is not a commit object");
                }
                let id = make_commit(self, parent.clone(), files);
                json_response(201, &json!({"sha": id, "parents": parent.iter().map(|p| json!({"sha": p})).collect::<Vec<_>>()}))
            }
            ("PATCH", r) if r.starts_with("/git/refs/heads/") => {
                run_race(self);
                let name = decode(&r["/git/refs/heads/".len()..]);
                let Some(sha) = body["sha"].as_str().filter(|s| self.commits.contains_key(*s)).map(str::to_owned) else {
                    return message(422, "Object does not exist");
                };
                let Some(head) = self.branches.get(&name).cloned() else { return message(422, "Reference does not exist") };
                if self.protected.contains(&name) {
                    return message(422, &format!("Protected branch update failed for refs/heads/{name}."));
                }
                if self.rules.contains(&name) {
                    return message(422, "Repository rule violations found");
                }
                if body["force"] != true && !self.descends(&sha, &head) {
                    return message(422, "Update is not a fast forward");
                }
                self.branches.insert(name.clone(), sha.clone());
                json_response(200, &json!({"ref": format!("refs/heads/{name}"), "object": {"sha": sha, "type": "commit"}}))
            }
            ("POST", "/git/refs") => {
                let (Some(name), Some(sha)) = (body["ref"].as_str().and_then(|r| r.strip_prefix("refs/heads/")), body["sha"].as_str()) else {
                    return message(422, "Invalid request.");
                };
                if self.branches.contains_key(name) {
                    return message(422, "Reference already exists");
                }
                if !self.commits.contains_key(sha) {
                    return message(422, "Object does not exist");
                }
                self.branches.insert(name.to_owned(), sha.to_owned());
                json_response(201, &json!({"ref": format!("refs/heads/{name}"), "object": {"sha": sha, "type": "commit"}}))
            }
            ("POST", "/pulls") => {
                let (Some(head), Some(base), Some(title)) = (body["head"].as_str(), body["base"].as_str(), body["title"].as_str()) else {
                    return message(422, "Validation Failed");
                };
                if !self.branches.contains_key(head) || !self.branches.contains_key(base) {
                    return message(422, "Validation Failed");
                }
                let iid = self.merge_requests.len() as u64 + 1;
                self.merge_requests.push(FakeMergeRequest { iid, source: head.to_owned(), target: base.to_owned(), title: title.to_owned() });
                json_response(201, &json!({"number": iid, "html_url": format!("{}/{}/pull/{iid}", self.instance, self.path)}))
            }
            _ => message(404, "Not Found"),
        }
    }

    /// Whether commit `id` is `ancestor` or has it among its parents' parents.
    fn descends(&self, id: &str, ancestor: &str) -> bool {
        let mut at = Some(id.to_owned());
        while let Some(c) = at {
            if c == ancestor {
                return true;
            }
            at = self.commits.get(&c).and_then(|c| c.parent.clone());
        }
        false
    }

    /// A commit's or a tree's entries, folders too, as `?recursive=1` lists them.
    fn github_tree(&self, id: &str) -> Response {
        let Some(files) = self.commits.get(id).map(|c| &c.files).or_else(|| self.trees.get(id)) else { return message(404, "Not Found") };
        let mut entries: BTreeMap<String, Value> = BTreeMap::new();
        for (path, e) in files {
            let mut at = 0;
            while let Some(i) = path[at..].find('/') {
                let dir = &path[..at + i];
                entries.entry(dir.to_owned()).or_insert_with(|| json!({"path": dir, "mode": "040000", "type": "tree", "sha": id_of(dir)}));
                at += i + 1;
            }
            let kind = if e.mode == "160000" { "commit" } else { "blob" };
            entries.insert(path.clone(), json!({"path": path, "mode": e.mode, "type": kind, "sha": e.id}));
        }
        let tree: Vec<Value> = entries.into_values().collect();
        json_response(200, &json!({"sha": id, "tree": tree, "truncated": self.truncated}))
    }

    /// A tree made from `base_tree` with the entries given: a blob's id, or none to remove it.
    fn github_new_tree(&mut self, body: &Value) -> Response {
        let Some(entries) = body["tree"].as_array() else { return message(422, "Invalid tree info") };
        let mut files = match body["base_tree"].as_str() {
            Some(base) => match self.trees.get(base) {
                Some(files) => files.clone(),
                None => return message(422, "base_tree is not a valid tree oid"),
            },
            None => BTreeMap::new(),
        };
        self.last_paths.clear();
        for e in entries {
            let Some(path) = e["path"].as_str() else { return message(422, "Invalid tree info") };
            self.last_paths.push(path.to_owned());
            match e["sha"].as_str() {
                Some(sha) if self.blobs.contains_key(sha) => {
                    files.insert(path.to_owned(), Entry { mode: e["mode"].as_str().unwrap_or("100644").to_owned(), id: sha.to_owned() });
                }
                Some(_) => return message(422, "GitRPC::BadObjectState"),
                None => {
                    files.remove(path);
                }
            }
        }
        let id = make_tree(self, files);
        json_response(201, &json!({"sha": id}))
    }
}

impl Transport for FakeGitLab {
    fn send(&self, request: Request) -> Sending<'_> {
        let response = self.lock().answer(&request);
        Box::pin(std::future::ready(Ok(response)))
    }
}
