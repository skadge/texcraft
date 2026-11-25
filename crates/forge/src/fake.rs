//! A GitLab in memory, for tests: one project, its branches and commits, answering the API's calls
//! that [`crate::GitLab`] makes as gitlab.com does (pagination, raw blobs, commits with actions,
//! error statuses). No test reaches the network.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use base64::Engine as _;
use serde_json::{Value, json};

use crate::{Request, Response, Sending, Transport};

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
}

#[derive(Debug, Default)]
struct State {
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
    /// A commit someone else makes just before the next commit request: branch, path, bytes.
    race: Option<(String, String, Vec<u8>)>,
    counter: u64,
}

/// The fake instance; clones share it.
#[derive(Clone)]
pub struct FakeGitLab {
    state: Arc<Mutex<State>>,
}

fn id_of(text: &str) -> String {
    texcraft_history::store::blob_id(text.as_bytes()).unwrap_or_default()
}

impl FakeGitLab {
    /// Project `path` (`group/project`) on `instance` (`https://gitlab.example`), whose API
    /// accepts `token`, with a branch `main` holding `files`.
    pub fn new(instance: &str, path: &str, token: &str, files: &[(&str, &[u8])]) -> Self {
        let state = State {
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

    /// The actions of the last commit requested.
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

    /// Just before the next commit request, someone else commits `bytes` to `path` on `branch`.
    pub fn race(&self, branch: &str, path: &str, bytes: &[u8]) {
        self.lock().race = Some((branch.to_owned(), path.to_owned(), bytes.to_vec()));
    }
}

fn new_commit(s: &mut State, branch: &str, parent: Option<String>, files: BTreeMap<String, Entry>) -> String {
    s.counter += 1;
    let id = id_of(&format!("commit {}", s.counter));
    s.commits.insert(id.clone(), Commit { parent, files });
    s.branches.insert(branch.to_owned(), id.clone());
    id
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
            return message(401, "401 Unauthorized");
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
                    Some(commit) => json_response(200, &json!({"name": name, "commit": {"id": commit}, "can_push": self.can_push})),
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
        if let Some((branch, path, bytes)) = self.race.take() {
            let parent = self.branches.get(&branch).cloned();
            let mut files = parent.as_ref().and_then(|p| self.commits.get(p)).map(|c| c.files.clone()).unwrap_or_default();
            let id = texcraft_history::store::blob_id(&bytes).unwrap_or_default();
            self.blobs.insert(id.clone(), bytes);
            files.insert(path, Entry { mode: "100644".to_owned(), id });
            new_commit(self, &branch, parent, files);
        }
        let Ok(body) = serde_json::from_slice::<Value>(request.body.as_deref().unwrap_or_default()) else { return message(400, "bad JSON") };
        let (Some(branch), Some(_message), Some(actions)) = (body["branch"].as_str(), body["commit_message"].as_str(), body["actions"].as_array())
        else {
            return message(400, "branch, commit_message and actions are required");
        };
        self.last_actions = actions.clone();
        if !self.can_push {
            return message(403, "403 Forbidden - You are not allowed to push into this branch");
        }
        let Some(parent) = self.branches.get(branch).cloned() else {
            return message(400, "You can only create or edit files when you are on a branch");
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

impl Transport for FakeGitLab {
    fn send(&self, request: Request) -> Sending<'_> {
        let response = self.lock().answer(&request);
        Box::pin(std::future::ready(Ok(response)))
    }
}
