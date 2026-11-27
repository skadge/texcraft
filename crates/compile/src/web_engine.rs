//! The engine's side of the browser build, in a Web Worker: retex's WebAssembly module (built by
//! `cargo xtask retex-wasm`, served beside the app) over `texcraft-wasi`'s in-memory files, with
//! the TeX tree of the web build's own site (`cargo xtask tree-site`) loaded as retex reads it (docs/ROADMAP.md).
//!
//! WASI's reads are synchronous. A file of the tree not fetched yet is fetched with a
//! synchronous request where the browser serves one; where it does not (Chrome stops serving
//! them once the worker has run for a few seconds without returning to its event loop), the run
//! stops, the file is fetched, and the run starts again. Before its first build the worker
//! fetches the files a plain article reads (retex's read list), so that a first build mostly
//! runs through; and before a project's first build in a visit, the files its earlier builds
//! read (its read list, kept per project: [`Kept::read_list`]), so that a project using
//! packages beyond a plain article's does not stop again for each of them. A project's first
//! build ever fetches what its class and packages read, as the tree's lists say
//! ([`crate::prefetch`], [`Engine::prefetch`]); and a run that stops for a package's file fetches
//! what that package reads with it.
//!
//! What it fetched and what the engine made (its format, the indexes of the tree's files and
//! fonts: `/cache`) is kept in the browser's Cache Storage, so a later visit neither fetches nor
//! makes them again ([`Kept`]).
//!
//! The messages, both ways, are plain objects: see [`crate::web`].
//!
//! Missing packages (W8): retex's module asks for a TeX Live package its tree lacks by opening a
//! file of its repository, a directory the worker serves (`RETEX_REPOSITORY=/tlnet/<snapshot>`).
//! Such an open stops the run as a file of the tree does; the worker fetches the file from the
//! package server ([`crate::packages`]) and starts the run again. What retex unpacks goes to
//! `/cache/texlive/<year>/`, kept in a cache of its own, per TeX Live year, so a package is
//! downloaded once, also across TeXCraft's updates.
//!
//! Updates (W10): the build names the worker's files and retex's module after their contents
//! (`cargo xtask version-worker`): the worker finds that name in its own loader's
//! (`engine-worker-H_loader.js`) and fetches `retex-H.wasm` ([`retex_url`]). The tree's index and
//! seed lists are fetched past the browser's HTTP cache (revalidated), and the tree's files with
//! the index's hash in their URL (`?v=`), so a browser never mixes an old tree with a new one.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use crate::packages::{self, Answer, Downloads, PackageServer, Plan, Reply};
use crate::prefetch::{self, Reported};
use js_sys::{Array, Object, Reflect, Uint8Array, WebAssembly};
use texcraft_wasi::web::{Ended, Image, Instance};
use texcraft_wasi::{FileSystem, Missing, Platform, Wasi};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortSignal, Cache, DedicatedWorkerGlobalScope, MessageEvent, Request, RequestCache, RequestInit, Response, ResponseInit, XmlHttpRequest,
    XmlHttpRequestResponseType,
};

/// Where the TeX tree and its seed are: the web build's own site (`cargo xtask tree-site`), beside
/// the worker's script, so relative to it.
pub const TREE_SITE: &str = "";

/// The environment retex runs in: the tree under `/texlive`, its caches (the format) under
/// `/cache` (as retex's own page sets it).
const ENV: [(&str, &str); 7] = [
    ("HOME", "/home"),
    ("RETEX_CACHE", "/cache"),
    ("TEXMFCNF", "/texlive/texmf-dist/web2c"),
    ("SELFAUTOLOC", "/texlive/bin/wasm32-wasi"),
    ("SELFAUTODIR", "/texlive/bin"),
    ("SELFAUTOPARENT", "/texlive"),
    ("SELFAUTOGRANDPARENT", "/"),
];

/// Where the project's files are in the module's file system.
const PROJECT: &str = "/project";

/// The exit status of a resumed run whose snapshot no longer fits the files (`snapshot_wasm.rs`).
const STALE: u32 = 75;
/// The exit status of a resumed pass that calls for another.
const RERUN: u32 = 76;
/// How many times a run may stop to fetch a file before the build gives up.
const MAX_FETCH_STOPS: u32 = 2000;
/// How long into a run a missing file may still be fetched synchronously, in milliseconds.
/// Chrome stops serving synchronous requests once a worker has run a few seconds without
/// returning to its event loop: the request then hangs (observed, Chrome 129). Past this, the
/// run stops instead and the file is fetched between runs.
const SYNC_WINDOW_MS: f64 = 1500.0;
/// A synchronous request that takes longer fails (the run then stops for the file).
const SYNC_TIMEOUT_MS: u32 = 3000;
/// When a run stops for a file, the other files of its folder are fetched with it, up to this
/// many bytes in all: a scan of a folder (retex's font index) then stops once, not once per file.
const SIBLINGS_BYTES: u64 = 4 << 20;
/// Files fetched at once.
const FETCH_BATCH: usize = 24;
/// How long the package server may take to say its newest snapshot, in milliseconds.
const SNAPSHOT_TIMEOUT_MS: u32 = 15_000;
/// How long it may take for a file of its repository (an archive is up to tens of MB).
const PACKAGE_TIMEOUT_MS: u32 = 120_000;
/// The source of a repository file the run stopped for: this, then its path.
const REPOSITORY_SOURCE: &str = "tlnet:";

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

/// The URL of retex's module: `retex-H.wasm` when the worker was started by a loader named after
/// the engine's contents (`engine-worker-H_loader.js`, see `xtask/src/web_version.rs`), so that
/// a new worker never runs an old module from the browser's cache; `retex.wasm` otherwise (a
/// build without that step).
fn retex_url() -> String {
    let path = scope().location().pathname();
    let name = path.rsplit('/').next().unwrap_or_default();
    match name.strip_prefix("engine-worker-").and_then(|n| n.strip_suffix("_loader.js")) {
        Some(h) if !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric()) => format!("retex-{h}.wasm"),
        _ => "retex.wasm".to_owned(),
    }
}

fn now_ns() -> u64 {
    (js_sys::Date::now() * 1e6) as u64
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn set(obj: &Object, key: &str, v: &JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), v);
}

fn post(kind: &str, id: f64, fields: &[(&str, JsValue)], transfer: &[JsValue]) {
    let msg = Object::new();
    set(&msg, "type", &kind.into());
    set(&msg, "id", &id.into());
    for (k, v) in fields {
        set(&msg, k, v);
    }
    let list: Array = transfer.iter().collect();
    if let Err(e) = scope().post_message_with_transfer(&msg, &list) {
        log::error!("posting {kind}: {e:?}");
    }
}

/// Where the bytes of a source come from: a file of the site, or a slice of one of its packs.
fn locate(source: &str) -> Option<(String, Option<usize>)> {
    if let Some(rest) = source.strip_prefix("pack:") {
        let (n, offset) = rest.split_once(':')?;
        return Some((format!("tree/packs/{n}"), Some(offset.parse().ok()?)));
    }
    Some((source.strip_prefix("file:")?.to_owned(), None))
}

#[derive(Default)]
struct Fetched {
    /// The site's files fetched so far, by their path on the site.
    urls: HashMap<String, Rc<Vec<u8>>>,
    /// Whether synchronous requests still work here.
    sync_refused: bool,
    /// When the current run started (`Date.now()`).
    run_started: f64,
    /// The site's files the module opened since the current job started.
    opened: BTreeSet<String>,
    /// The query the tree's files are fetched with: the index's hash (`?v=…`), so that the
    /// browser's HTTP cache never gives a file of an older tree.
    version: String,
}

impl Fetched {
    /// The URL of the site's file at `path`.
    fn url(&self, path: &str) -> String {
        format!("{TREE_SITE}{path}{}", self.version)
    }
}

/// What retex writes during a run, and the draft PDFs it is read for.
#[derive(Default)]
struct Out {
    text: String,
    partial: String,
    job: f64,
    /// The PDF's path in the module's files.
    pdf: String,
    drafts: u32,
    /// The progress lines passed on in this build, over its runs.
    reported: Reported,
}

#[derive(Clone)]
struct Web {
    fetched: Rc<RefCell<Fetched>>,
    out: Rc<RefCell<Out>>,
    fs: FileSystem,
    downloads: Rc<RefCell<Downloads>>,
}

fn fetch_sync(url: &str) -> Option<Vec<u8>> {
    let req = XmlHttpRequest::new().ok()?;
    req.open_with_async("GET", url, false).ok()?;
    req.set_response_type(XmlHttpRequestResponseType::Arraybuffer);
    req.set_timeout(SYNC_TIMEOUT_MS);
    req.send().ok()?;
    if req.status().ok()? != 200 {
        return None;
    }
    Some(Uint8Array::new(&req.response().ok()?).to_vec())
}

impl Platform for Web {
    fn realtime_ns(&self) -> u64 {
        now_ns()
    }

    fn monotonic_ns(&self) -> u64 {
        let ms = scope().performance().map_or_else(js_sys::Date::now, |p| p.now());
        (ms * 1e6) as u64
    }

    fn random(&mut self, buf: &mut [u8]) {
        if let Ok(crypto) = scope().crypto() {
            for chunk in buf.chunks_mut(65536) {
                let _ = crypto.get_random_values_with_u8_array(chunk);
            }
        }
    }

    fn load(&mut self, source: &str, size: u64) -> Option<Vec<u8>> {
        let (path, offset) = locate(source)?;
        let have = self.fetched.borrow().urls.get(&path).cloned();
        let bytes = match have {
            Some(b) => b,
            None => {
                let late = js_sys::Date::now() - self.fetched.borrow().run_started > SYNC_WINDOW_MS;
                if self.fetched.borrow().sync_refused || late {
                    return None;
                }
                let url = self.fetched.borrow().url(&path);
                match fetch_sync(&url) {
                    Some(b) => {
                        let b = Rc::new(b);
                        self.fetched.borrow_mut().urls.insert(path, b.clone());
                        b
                    }
                    None => {
                        log::warn!("a synchronous request for {path} failed: fetching between runs from now on");
                        self.fetched.borrow_mut().sync_refused = true;
                        return None;
                    }
                }
            }
        };
        let start = offset.unwrap_or(0);
        let end = if offset.is_some() { start + size as usize } else { bytes.len() };
        bytes.get(start..end).map(<[u8]>::to_vec)
    }

    fn stdout(&mut self, bytes: &[u8]) {
        let mut out = self.out.borrow_mut();
        let text = String::from_utf8_lossy(bytes).into_owned();
        out.text.push_str(&text);
        let joined = std::mem::take(&mut out.partial) + &text;
        let mut lines: Vec<&str> = joined.split('\n').collect();
        out.partial = lines.pop().unwrap_or_default().to_owned();
        for line in lines {
            if (line.starts_with("{\"type\":\"progress\"")
                || line.starts_with("{\"type\":\"pass\"")
                || line.starts_with("{\"type\":\"package-download\""))
                && out.reported.fresh(line)
            {
                post("line", out.job, &[("text", line.into())], &[]);
            }
            if line.starts_with("{\"type\":\"pass\"")
                && let Some(pdf) = self.fs.read(&out.pdf).filter(|p| !p.is_empty())
            {
                out.drafts += 1;
                let bytes = Uint8Array::from(pdf.as_slice());
                post("draft", out.job, &[("pdf", bytes.clone().into()), ("pass", out.drafts.into())], &[bytes.buffer().into()]);
            }
        }
    }

    fn stderr(&mut self, bytes: &[u8]) {
        self.out.borrow_mut().text.push_str(&String::from_utf8_lossy(bytes));
    }

    fn opened(&mut self, source: &str) {
        if let Some((path, _)) = locate(source) {
            self.fetched.borrow_mut().opened.insert(path);
        }
    }

    /// A file of the package repository: fetched between runs ([`Engine::fetch_repository`]),
    /// unless the server lacks it or holds requests back.
    fn missing(&mut self, path: &str) -> Missing {
        if !path.starts_with(&format!("{}/", packages::REPOSITORY_DIR)) {
            return Missing::Absent;
        }
        match self.downloads.borrow().plan(path, js_sys::Date::now()) {
            Plan::Fetch(_) => Missing::Fetch(format!("{REPOSITORY_SOURCE}{path}").into()),
            Plan::Answer(Answer::Unavailable) => Missing::Unavailable,
            Plan::Answer(_) => Missing::Absent,
        }
    }
}

/// The error for a run that crashed: `why`, then the last lines the engine wrote that are not
/// its records (its panic message, if it panicked), which say what happened.
fn crashed(why: &str, output: &str) -> String {
    let last: Vec<&str> = output.lines().filter(|l| !l.starts_with('{') && !l.trim().is_empty()).collect();
    let tail = last[last.len().saturating_sub(5)..].join("\n");
    if tail.is_empty() { format!("the typesetting engine stopped: {why}") } else { format!("the typesetting engine stopped: {why}\n{tail}") }
}

/// A 64-bit FNV-1a hash, to name the caches after what they depend on.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

/// Where the engine's own files are kept in the cache: a URL of this page's site that is never
/// fetched, with the file's path.
const ENGINE_FILES: &str = "retex-cache";

/// Where the projects' read lists are kept in the cache: a URL of this page's site that is never
/// fetched, with a hash of the project's key.
const READ_LISTS: &str = "read-lists";

/// The browser's Cache Storage: the site's files fetched (cache `texcraft-tree-<index>`, by their
/// URL), the files the engine made in `/cache` (cache `texcraft-engine-<module>-<index>`), and
/// each project's read list, the site's files its builds read (cache `texcraft-reads-<index>`).
/// A cache's name holds a hash of what its files depend on: the tree's index (a read list names
/// the index's files and packs), and for the engine's files the module too. Caches with other
/// names are from earlier versions, and are deleted. The TeX Live packages retex downloaded
/// (`/cache/texlive/`) are kept apart, by TeX Live year only (cache `texcraft-packages-<year>`):
/// retex reads what an older module installed, so a new version of TeXCraft does not download
/// them again. Keeping is best effort: where the browser has no Cache Storage (or refuses it),
/// each visit fetches again.
#[derive(Default)]
struct Kept {
    tree: Option<Cache>,
    engine: Option<Cache>,
    reads: Option<Cache>,
    packages: Option<Cache>,
    /// The site's files in the cache.
    files: HashSet<String>,
    /// The engine's files in the cache, with their time.
    made: HashMap<String, u64>,
}

async fn promise(p: Result<js_sys::Promise, JsValue>) -> Result<JsValue, String> {
    JsFuture::from(p.map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))
}

impl Kept {
    /// Opens the caches for this module, tree and TeX Live year, and deletes the others.
    async fn open(module: u64, index: u64, year: Option<u16>) -> Kept {
        let Ok(storage) = scope().caches() else { return Kept::default() };
        let tree = format!("texcraft-tree-{index:016x}");
        let engine = format!("texcraft-engine-{module:016x}-{index:016x}");
        let reads = format!("texcraft-reads-{index:016x}");
        let packages = year.map(|y| format!("texcraft-packages-{y}"));
        if let Ok(names) = promise(Ok(storage.keys())).await {
            for name in Array::from(&names).iter().filter_map(|n| n.as_string()) {
                if name.starts_with("texcraft-") && name != tree && name != engine && name != reads && Some(&name) != packages.as_ref() {
                    log::info!("deleting the old cache {name}");
                    let _ = promise(Ok(storage.delete(&name))).await;
                }
            }
        }
        let open = |name: String| {
            let storage = storage.clone();
            async move { promise(Ok(storage.open(&name))).await.ok().and_then(|c| c.dyn_into::<Cache>().ok()) }
        };
        let packages = match packages {
            Some(name) => open(name).await,
            None => None,
        };
        Kept { tree: open(tree).await, engine: open(engine).await, reads: open(reads).await, packages, ..Kept::default() }
    }

    /// The bytes and the time (header `x-mtime`) of a cached response.
    async fn body(resp: JsValue) -> Option<(Vec<u8>, u64)> {
        let resp: Response = resp.dyn_into().ok()?;
        let mtime = resp.headers().get("x-mtime").ok().flatten().and_then(|t| t.parse().ok()).unwrap_or(0);
        let buf = promise(resp.array_buffer()).await.ok()?;
        Some((Uint8Array::new(&buf).to_vec(), mtime))
    }

    async fn put(cache: &Cache, url: &str, bytes: &[u8], mtime: u64) -> Result<(), String> {
        let init = ResponseInit::new();
        let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
        let _ = headers.set("x-mtime", &mtime.to_string());
        init.set_headers(&headers);
        let mut body = bytes.to_vec();
        let resp = Response::new_with_opt_u8_array_and_init(Some(&mut body), &init).map_err(|e| format!("{e:?}"))?;
        promise(Ok(cache.put_with_str(url, &resp))).await.map(|_| ())
    }

    /// The site's file at `path`, if it is kept.
    async fn file(&self, path: &str) -> Option<Vec<u8>> {
        let resp = promise(Ok(self.tree.as_ref()?.match_with_str(&format!("{TREE_SITE}{path}")))).await.ok()?;
        if resp.is_undefined() {
            return None;
        }
        Kept::body(resp).await.map(|(b, _)| b)
    }

    async fn keep_file(&mut self, path: &str, bytes: &[u8]) {
        let Some(tree) = &self.tree else { return };
        match Kept::put(tree, &format!("{TREE_SITE}{path}"), bytes, 0).await {
            Ok(()) => {
                self.files.insert(path.to_owned());
            }
            Err(e) => {
                log::warn!("the browser did not keep {path} ({e}): not keeping the TeX files from now on");
                self.tree = None;
            }
        }
    }

    fn read_list_url(project: &str) -> String {
        format!("{READ_LISTS}/{:016x}", fnv(project.as_bytes()))
    }

    /// The site's files the builds of `project` read, as kept by an earlier visit (empty when
    /// none is kept).
    async fn read_list(&self, project: &str) -> BTreeSet<String> {
        let Some(reads) = &self.reads else { return BTreeSet::new() };
        let Ok(resp) = promise(Ok(reads.match_with_str(&Kept::read_list_url(project)))).await else { return BTreeSet::new() };
        if resp.is_undefined() {
            return BTreeSet::new();
        }
        let Some((bytes, _)) = Kept::body(resp).await else { return BTreeSet::new() };
        String::from_utf8_lossy(&bytes).lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
    }

    async fn keep_read_list(&self, project: &str, list: &BTreeSet<String>) {
        let Some(reads) = &self.reads else { return };
        let text: String = list.iter().map(|p| format!("{p}\n")).collect();
        if let Err(e) = Kept::put(reads, &Kept::read_list_url(project), text.as_bytes(), 0).await {
            log::warn!("the browser did not keep the project's read list ({e})");
        }
    }

    /// Puts the engine's kept files in `fs`: what it made, and the packages it downloaded.
    async fn restore(&mut self, fs: &FileSystem) {
        let prefix = format!("/{ENGINE_FILES}/");
        for cache in [self.engine.clone(), self.packages.clone()].into_iter().flatten() {
            let Ok(requests) = promise(Ok(cache.keys())).await else { continue };
            for request in Array::from(&requests).iter().filter_map(|r| r.dyn_into::<Request>().ok()) {
                let url = request.url();
                let Some(path) = url.find(&prefix).map(|at| url[at + prefix.len() - 1..].to_owned()) else { continue };
                let Ok(resp) = promise(Ok(cache.match_with_request(&request))).await else { continue };
                if let Some((bytes, mtime)) = Kept::body(resp).await {
                    fs.add(&path, bytes, mtime);
                    self.made.insert(path, mtime);
                }
            }
        }
        log::info!("{} of the engine's files restored from an earlier visit", self.made.len());
    }

    /// The cache an engine's file at `path` is kept in: the packages' for retex's TeX Live trees.
    fn cache_of(&self, path: &str) -> Option<Cache> {
        if path.starts_with("/cache/texlive/") { self.packages.clone() } else { self.engine.clone() }
    }

    /// Forgets the engine's files at `paths` (in the cache and in `fs`).
    async fn forget(&mut self, fs: &FileSystem, paths: &[String]) {
        for path in paths {
            fs.remove(path);
            self.made.remove(path);
            if let Some(cache) = self.cache_of(path) {
                let _ = promise(Ok(cache.delete_with_str(&format!("{ENGINE_FILES}{path}")))).await;
            }
        }
    }

    /// Keeps the engine's files in `/cache` that are new or changed.
    async fn keep_made(&mut self, fs: &FileSystem) {
        for (path, mtime) in fs.files("/cache") {
            if self.made.get(&path) == Some(&mtime) {
                continue;
            }
            let Some(cache) = self.cache_of(&path) else { continue };
            let Some(bytes) = fs.read(&path) else { continue };
            match Kept::put(&cache, &format!("{ENGINE_FILES}{path}"), &bytes, mtime).await {
                Ok(()) => {
                    self.made.insert(path, mtime);
                }
                Err(e) => {
                    log::warn!("the browser did not keep {path} ({e})");
                    return;
                }
            }
        }
    }
}

/// The worker's state from build to build.
struct Engine {
    module: Option<WebAssembly::Module>,
    fs: FileSystem,
    web: Web,
    /// The project files the last build wrote, to remove those a later job no longer has.
    written: HashSet<String>,
    /// The memory image of the last build's preamble, for the arguments it was taken with.
    snapshot: Option<(String, Image)>,
    /// A run resumed from a snapshot crashed in this visit (#56): builds start from the start,
    /// without snapshots, until the page is reloaded.
    no_resume: bool,
    /// The site's files of the tree by folder (site paths and sizes), for fetching a file's
    /// neighbours with it.
    folders: HashMap<String, Vec<(String, u64)>>,
    kept: Kept,
    /// The read lists of the projects built in this visit, by project key: fetched before the
    /// project's first build, then updated after each.
    lists: HashMap<String, BTreeSet<String>>,
    /// The tree's files by name (`natbib.sty`): where they are (a source, as [`locate`] reads
    /// it) and their size. The first of a name, as the index lists them.
    names: HashMap<String, (Rc<str>, u64)>,
    /// The classes, packages and styles whose files were fetched ([`prefetch::loads`]'s entries).
    expanded: HashSet<String>,
    /// The TeX Live year of the tree (`seed/version.txt`), which packages are downloaded for.
    year: Option<u16>,
    /// A snapshot an archive was missing from in the last build: retex's database may be stale.
    stale: Option<String>,
}

/// The TeX Live year of `seed/version.txt` (`texlive 2026`).
fn tree_year(text: &str) -> Option<u16> {
    text.lines().find_map(|l| l.strip_prefix("texlive ")?.trim().parse().ok())
}

/// Fetches `url` from the package server within `timeout_ms`: its body, its status (with
/// `Retry-After`, when the server lets the browser show it) or why there is no answer.
async fn fetch_reply(url: &str, timeout_ms: u32) -> Reply {
    let init = RequestInit::new();
    init.set_signal(Some(&AbortSignal::timeout_with_u32(timeout_ms)));
    let why = |e: JsValue| match e.dyn_ref::<js_sys::Error>() {
        Some(e) if e.name() == "TimeoutError" || e.name() == "AbortError" => format!("no answer within {} s", timeout_ms / 1000),
        // A network error, or a server that does not let this site read it (CORS): the browser
        // does not say which.
        Some(e) => String::from(e.message()),
        None => format!("{e:?}"),
    };
    let resp: Response = match JsFuture::from(scope().fetch_with_str_and_init(url, &init)).await {
        Ok(r) => match r.dyn_into() {
            Ok(r) => r,
            Err(_) => return Reply::Failed("not a response".to_owned()),
        },
        Err(e) => return Reply::Failed(why(e)),
    };
    if resp.status() != 200 {
        return Reply::Status { code: resp.status(), retry_after: resp.headers().get("retry-after").ok().flatten() };
    }
    match resp.array_buffer() {
        Ok(p) => match JsFuture::from(p).await {
            Ok(buf) => Reply::Ok(Uint8Array::new(&buf).to_vec()),
            Err(e) => Reply::Failed(why(e)),
        },
        Err(e) => Reply::Failed(why(e)),
    }
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    response_bytes(url, scope().fetch_with_str(url)).await
}

/// Fetches `url` past the browser's HTTP cache (revalidated with the server): for the lists that
/// say which files the site has, which keep their names from one update to the next.
async fn fetch_fresh(url: &str) -> Result<Vec<u8>, String> {
    let init = RequestInit::new();
    init.set_cache(RequestCache::NoCache);
    response_bytes(url, scope().fetch_with_str_and_init(url, &init)).await
}

/// The body of the response `request` gives (a `fetch` of `url`).
async fn response_bytes(url: &str, request: js_sys::Promise) -> Result<Vec<u8>, String> {
    let resp: Response =
        JsFuture::from(request).await.map_err(|e| format!("{url}: {e:?}"))?.dyn_into().map_err(|_| format!("{url}: not a response"))?;
    if !resp.ok() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(|e| format!("{url}: {e:?}"))?).await.map_err(|e| format!("{url}: {e:?}"))?;
    Ok(Uint8Array::new(&buf).to_vec())
}

impl Engine {
    fn new() -> Engine {
        let fs = FileSystem::new();
        let web = Web { fetched: Rc::default(), out: Rc::default(), fs: fs.clone(), downloads: Rc::default() };
        Engine {
            module: None,
            fs,
            web,
            written: HashSet::new(),
            snapshot: None,
            no_resume: false,
            folders: HashMap::new(),
            kept: Kept::default(),
            lists: HashMap::new(),
            names: HashMap::new(),
            expanded: HashSet::new(),
            year: None,
            stale: None,
        }
    }

    /// Fetches the site's files at `paths` not fetched yet, a batch at a time, saying so: from
    /// the browser's cache when they are kept there.
    async fn fetch_all(&mut self, id: f64, paths: Vec<String>) -> Result<(), String> {
        let mut todo = Vec::new();
        let mut seen = HashSet::new();
        for p in paths {
            if !seen.insert(p.clone()) || self.web.fetched.borrow().urls.contains_key(&p) {
                continue;
            }
            match self.kept.file(&p).await {
                Some(bytes) => {
                    self.kept.files.insert(p.clone());
                    self.web.fetched.borrow_mut().urls.insert(p, Rc::new(bytes));
                }
                None => todo.push(p),
            }
        }
        let total = todo.len();
        for (n, batch) in todo.chunks(FETCH_BATCH).enumerate() {
            post("line", id, &[("text", prefetch::fetching_record(n * FETCH_BATCH, total).into())], &[]);
            // The requests are all made before any is awaited, so they download together.
            let requests: Vec<js_sys::Promise> = batch.iter().map(|p| scope().fetch_with_str(&self.web.fetched.borrow().url(p))).collect();
            for (p, request) in batch.iter().zip(requests) {
                let bytes = response_bytes(p, request).await?;
                self.kept.keep_file(p, &bytes).await;
                self.web.fetched.borrow_mut().urls.insert(p.clone(), Rc::new(bytes));
            }
        }
        if total > 0 {
            post("line", id, &[("text", prefetch::fetching_record(total, total).into())], &[]);
        }
        Ok(())
    }

    /// The module and the tree's list of files, the first time, with the files a plain article
    /// reads.
    async fn init(&mut self, id: f64) -> Result<(), String> {
        if self.module.is_some() {
            return Ok(());
        }
        post("line", id, &[("text", "{\"type\":\"progress\",\"note\":\"loading the typesetting engine\"}".into())], &[]);
        log::info!("loading retex.wasm and the tree's index");
        let retex = retex_url();
        let wasm = fetch_bytes(&retex).await?;
        let module = JsFuture::from(WebAssembly::compile(&Uint8Array::from(wasm.as_slice()).into())).await.map_err(|e| format!("{retex}: {e:?}"))?;
        let index = String::from_utf8(fetch_fresh(&format!("{TREE_SITE}tree/index.txt")).await?).map_err(|_| "tree/index.txt is not UTF-8")?;
        self.web.fetched.borrow_mut().version = format!("?v={:016x}", fnv(index.as_bytes()));
        // The tree's TeX Live year: retex reads it from the tree's `release-texlive.txt` (else
        // from the LaTeX kernel's date, which can be the year before's), and downloads packages
        // of that year only.
        self.year = match fetch_fresh(&format!("{TREE_SITE}seed/version.txt")).await {
            Ok(text) => tree_year(&String::from_utf8_lossy(&text)),
            Err(e) => {
                log::warn!("the tree says no TeX Live year ({e}): no package downloads");
                None
            }
        };
        if let Some(year) = self.year {
            self.fs.add("/texlive/release-texlive.txt", format!("TeX Live (https://tug.org/texlive) version {year}\n").into_bytes(), 0);
        }
        self.fs.mkdirs(packages::REPOSITORY_DIR, 0);
        *self.web.downloads.borrow_mut() = Downloads::new(PackageServer::Default, self.year);
        self.kept = Kept::open(fnv(&wasm), fnv(index.as_bytes()), self.year).await;
        for line in index.lines() {
            let mut parts = line.split(' ');
            let (Some(size), Some(path)) = (parts.next().and_then(|s| s.parse::<u64>().ok()), parts.next()) else { continue };
            let source: Rc<str> = match (parts.next(), parts.next()) {
                (Some(pack), Some(offset)) => format!("pack:{pack}:{offset}").into(),
                _ => {
                    let site = format!("tree{path}");
                    if let Some((folder, _)) = site.rsplit_once('/') {
                        self.folders.entry(folder.to_owned()).or_default().push((site.clone(), size));
                    }
                    format!("file:{site}").into()
                }
            };
            if let Some(name) = path.rsplit('/').next() {
                self.names.entry(name.to_owned()).or_insert_with(|| (source.clone(), size));
            }
            self.fs.add_lazy(path, size, source);
        }
        self.fs.mkdirs("/cache", now_ns());
        self.fs.mkdirs("/home", now_ns());
        // The format and indexes this engine made on an earlier visit, before the site's indexes.
        self.kept.restore(&self.fs).await;
        // The site's indexes of the tree (ls-R, font names), made by a native retex over the same
        // tree: retex checks each against what it indexes (sizes and times, the same here as
        // there), so it uses them only where they fit and remakes them otherwise. Without them,
        // the first build reads every font of the tree. Not a format: that depends on the
        // engine's build, and this one makes its own.
        let mut seeds = Vec::new();
        if let Ok(list) = fetch_fresh(&format!("{TREE_SITE}seed/index.txt")).await {
            for line in String::from_utf8_lossy(&list).lines() {
                let Some((size, path)) = line.split_once(' ') else { continue };
                let Ok(size) = size.parse::<u64>() else { continue };
                if path.starts_with("/cache/kpse/") && !self.fs.exists(path) {
                    self.fs.add_lazy(path, size, format!("file:seed{path}").into());
                    seeds.push(format!("seed{path}"));
                }
            }
        }
        // What the blank project reads, fetched together before the first build.
        let mut paths = seeds;
        if let Ok(list) = fetch_fresh(&format!("{TREE_SITE}seed/reads/blank.txt")).await {
            paths.extend(String::from_utf8_lossy(&list).lines().filter(|l| l.starts_with("tree/")).map(str::to_owned));
        }
        self.fetch_all(id, paths).await?;
        log::info!("engine ready: {} files fetched", self.web.fetched.borrow().urls.len());
        self.module = Some(module.unchecked_into());
        Ok(())
    }

    async fn build(&mut self, msg: &JsValue) -> Result<(), String> {
        let id = get(msg, "id").as_f64().unwrap_or(0.0);
        let project = get(msg, "project").as_string();
        self.init(id).await?;
        let Some(module) = self.module.clone() else { return Err("the engine did not load".into()) };
        if let Some(project) = &project {
            self.fetch_read_list(id, project).await;
        }
        let main = get(msg, "main").as_string().ok_or("no main document")?;
        // The project's files are under `/project`, at their paths in the project; retex starts in
        // the main document's folder (`PWD`, retex 2.5).
        let dir = get(msg, "dir").as_string().filter(|d| !d.is_empty());
        let cwd = match &dir {
            Some(d) => format!("{PROJECT}/{d}"),
            None => PROJECT.to_owned(),
        };
        let names: Array = get(msg, "names").unchecked_into();
        let contents: Array = get(msg, "contents").unchecked_into();
        let flags: Array = get(msg, "args").unchecked_into();
        self.prefetch_sources(id, &names, &contents).await;
        // An export (`{type: "export", format}`): retex's `-output-format`, run once from the start
        // and apart from the preview's snapshot.
        let export = get(msg, "format").as_string().filter(|f| f != "pdf");
        let mut now = HashSet::new();
        for (name, bytes) in names.iter().zip(contents.iter()) {
            let name = format!("{PROJECT}/{}", name.as_string().unwrap_or_default());
            self.fs.add(&name, Uint8Array::new(&bytes).to_vec(), now_ns());
            now.insert(name);
        }
        for old in self.written.difference(&now) {
            self.fs.remove(old);
        }
        self.written = now;
        let job = main.strip_suffix(".tex").unwrap_or(&main).to_owned();
        let pdf = format!("{cwd}/{job}.pdf");
        let synctex = format!("{cwd}/{job}.synctex");
        // A print build's PDF/X, beside the PDF (`-output-format=pdf,pdf-x4`).
        let pdfx: Vec<String> = ["pdf-x4", "pdf-x1a"].iter().map(|v| format!("{cwd}/{job}.{v}.pdf")).collect();
        self.fs.remove(&pdf);
        self.fs.remove(&synctex);
        for p in &pdfx {
            self.fs.remove(p);
        }
        let mut args: Vec<String> =
            vec!["retex".into(), "--color=never".into(), "--message-format=json".into(), "--symbols".into(), "-synctex=-1".into()];
        args.extend(flags.iter().filter_map(|a| a.as_string()));
        let output = export.as_ref().map(|f| format!("{cwd}/{job}.{f}"));
        if let (Some(f), Some(o)) = (&export, &output) {
            args.push(format!("-output-format={f}"));
            self.fs.remove(o);
        }
        args.push(main.clone());
        let (env, key) = self.environment(msg, &args, &cwd).await;
        let started = js_sys::Date::now();
        let mut stops = 0;
        let mut reruns = 0;
        self.web.fetched.borrow_mut().opened.clear();
        let mut reported = Reported::default();
        // `whole`: whether the run that ended was started from the start (not resumed from the
        // preamble's snapshot), so it opened every file the build needs.
        let (code, whole) = loop {
            *self.web.out.borrow_mut() = Out { job: id, pdf: pdf.clone(), reported: std::mem::take(&mut reported), ..Out::default() };
            self.web.fetched.borrow_mut().run_started = js_sys::Date::now();
            let wasi = Wasi::new(self.fs.clone(), self.web.clone(), args.clone(), env.clone());
            let instance = Instance::new(&module, wasi, !self.no_resume).map_err(|e| format!("instantiating retex: {e:?}"))?;
            let resumed = export.is_none() && !self.no_resume && matches!(&self.snapshot, Some((k, _)) if *k == key);
            let (ended, instance) = match (&self.snapshot, resumed) {
                (Some((_, image)), true) => instance.resume(image),
                _ => instance.start(),
            };
            let image = instance.image();
            reported = std::mem::take(&mut self.web.out.borrow_mut().reported);
            match ended {
                Ended::Exit(STALE) if resumed => {
                    // The preamble's files changed: from the start, with a new snapshot.
                    self.snapshot = None;
                }
                Ended::Exit(RERUN) if resumed && reruns < 10 => reruns += 1,
                Ended::Exit(code) => {
                    if let Some(image) = image.filter(|_| export.is_none()) {
                        self.snapshot = Some((key.clone(), image));
                    }
                    break (code, !resumed);
                }
                Ended::Fetch(source) => {
                    stops += 1;
                    if stops > MAX_FETCH_STOPS {
                        return Err("the build needed more TeX files than it could fetch".into());
                    }
                    log::info!("build {id}: run {stops} stopped to fetch {source}");
                    self.fetch_for(id, &source).await?;
                }
                Ended::Crashed(why) if resumed => {
                    // Resuming from the preamble's snapshot failed (#56): from the start, as a
                    // reload would, and without snapshots for the rest of the visit.
                    let why = crashed(&why, &std::mem::take(&mut self.web.out.borrow_mut().text));
                    log::warn!("build {id}: the run resumed from the preamble's snapshot failed, running from the start: {why}");
                    self.snapshot = None;
                    self.no_resume = true;
                }
                Ended::Crashed(why) => return Err(crashed(&why, &std::mem::take(&mut self.web.out.borrow_mut().text))),
            }
        };
        self.fs.remove("/.retex-resume");
        let mut out = std::mem::take(&mut *self.web.out.borrow_mut());
        self.after_downloads(&mut out.text);
        let seconds = (js_sys::Date::now() - started) / 1000.0;
        if let Some(output) = output {
            self.exported(id, &cwd, &job, &output, &self.written, &out.text);
            self.keep().await;
            return Ok(());
        }
        if let Some(project) = &project {
            self.keep_read_list(project, whole && code == 0).await;
        }
        let mut fields = vec![("code", JsValue::from(code)), ("output", out.text.into()), ("seconds", seconds.into()), ("stops", stops.into())];
        // TeX's log, which says which files were open when a problem was noticed.
        if let Some(log) = self.fs.read(&format!("{cwd}/{job}.log")) {
            fields.push(("log", String::from_utf8_lossy(&log).into_owned().into()));
        }
        // The `.aux` files, for the numbers the formatted view shows: the job's own (the newest,
        // wherever retex keeps it), then those beside it (`\include`'s).
        let files = self.fs.files(&cwd);
        let own = files.iter().filter(|(p, _)| p.ends_with(&format!("/{job}.aux"))).max_by_key(|(_, t)| *t).map(|(p, _)| p.clone());
        if let Some(own) = own {
            let folder = own.rsplit_once('/').map_or(String::new(), |(f, _)| format!("{f}/"));
            let mut text = self.fs.read(&own).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            for (p, _) in files.iter().filter(|(p, _)| p.ends_with(".aux") && p.starts_with(&folder) && *p != own) {
                if let Some(b) = self.fs.read(p) {
                    text.push('\n');
                    text.push_str(&String::from_utf8_lossy(&b));
                }
            }
            fields.push(("aux", text.into()));
        }
        let mut transfer = Vec::new();
        let pdfx = pdfx.iter().find(|p| self.fs.read(p).is_some()).cloned().unwrap_or_default();
        for (key, path) in [("pdf", &pdf), ("synctex", &synctex), ("pdfx", &pdfx)] {
            if let Some(bytes) = self.fs.read(path) {
                let arr = Uint8Array::from(bytes.as_slice());
                transfer.push(arr.buffer().into());
                fields.push((key, arr.into()));
            }
        }
        post("done", id, &fields, &transfer);
        self.keep().await;
        Ok(())
    }

    /// The module's environment for a build's runs in `cwd`, and the key of its preamble's
    /// snapshot, which holds the arguments and the environment it was taken with.
    async fn environment(&mut self, msg: &JsValue, args: &[String], cwd: &str) -> (Vec<(String, String)>, String) {
        let mut env: Vec<(String, String)> = ENV.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        env.push(("PWD".to_owned(), cwd.to_owned()));
        let server = get(msg, "packages").as_string().and_then(|s| PackageServer::parse(&s).ok()).unwrap_or_default();
        let repository = self.package_repository(server).await;
        if let Some(r) = &repository {
            env.push(("RETEX_REPOSITORY".to_owned(), r.clone()));
        }
        let key = format!("{}\u{0}{cwd}\u{0}{}", args.join("\u{0}"), repository.unwrap_or_default());
        (env, key)
    }

    /// After a run stopped for `source`: a file of the package repository, or of the tree.
    async fn fetch_for(&mut self, id: f64, source: &str) -> Result<(), String> {
        match source.strip_prefix(REPOSITORY_SOURCE) {
            Some(path) => {
                self.fetch_repository(path).await;
                Ok(())
            }
            None => self.fetch_stopped_for(id, source).await,
        }
    }

    /// Before a build: `RETEX_REPOSITORY` for its runs, `None` when packages are not downloaded
    /// (downloads off, no TeX Live year, or the server's newest snapshot unknown: asked here when
    /// retex keeps no package database yet).
    async fn package_repository(&mut self, server: PackageServer) -> Option<String> {
        let year = self.year?;
        self.web.downloads.borrow_mut().set_server(server);
        let kept_path = packages::kept_repository_path(year);
        let mut kept = self.fs.read(&kept_path).map(|b| String::from_utf8_lossy(&b).into_owned());
        // An archive was missing from the snapshot of retex's database: when the server has a
        // newer one, the database goes, and the next run fetches the newer one's.
        if let Some(stale) = self.stale.take()
            && kept.as_deref().and_then(packages::kept_snapshot) == Some(stale.as_str())
        {
            let url = self.web.downloads.borrow_mut().recheck_url(js_sys::Date::now());
            if let Some(url) = url {
                let reply = fetch_reply(&url, SNAPSHOT_TIMEOUT_MS).await;
                self.web.downloads.borrow_mut().snapshot_reply(reply, js_sys::Date::now());
                let newer = self.web.downloads.borrow().current().is_some_and(|c| c != stale);
                if newer {
                    log::info!("the package server no longer has snapshot {stale}: TeX Live's package list is fetched again");
                    let dir = format!("/cache/texlive/{year}");
                    let gone: Vec<String> =
                        self.fs.files(&format!("{dir}/tlpdb")).into_iter().map(|(p, _)| p).chain([format!("{dir}/index.bin")]).collect();
                    self.kept.forget(&self.fs, &gone).await;
                    kept = None;
                }
            }
        }
        let url = self.web.downloads.borrow().snapshot_url(kept.as_deref(), js_sys::Date::now());
        if let Some(url) = url {
            let reply = fetch_reply(&url, SNAPSHOT_TIMEOUT_MS).await;
            self.web.downloads.borrow_mut().snapshot_reply(reply, js_sys::Date::now());
        }
        self.web.downloads.borrow().repository(kept.as_deref())
    }

    /// After a run stopped for the repository file at `path`: fetches it (with the database's
    /// other files, for one of them) and adds what the server gave to the module's files.
    async fn fetch_repository(&mut self, path: &str) {
        for file in Downloads::with(path) {
            let plan = self.web.downloads.borrow().plan(&file, js_sys::Date::now());
            let Plan::Fetch(url) = plan else { continue };
            log::info!("fetching {url}");
            let reply = fetch_reply(&url, PACKAGE_TIMEOUT_MS).await;
            let answer = self.web.downloads.borrow_mut().reply(&file, &url, reply, js_sys::Date::now());
            if let Answer::Bytes(bytes) = answer {
                self.fs.add(&file, bytes, now_ns());
            }
        }
    }

    /// After a build: the downloads' notes at the end of its output (a line of the log for each
    /// package downloaded, a warning for each refusal), and the repository's files dropped (retex
    /// unpacked what it needed into `/cache`).
    fn after_downloads(&mut self, text: &mut String) {
        let notes = self.web.downloads.borrow_mut().take_notes();
        for note in notes {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&note.record().unwrap_or_else(|| note.text()));
            text.push('\n');
        }
        self.stale = self.web.downloads.borrow_mut().take_missing_from();
        self.fs.remove(packages::REPOSITORY_DIR);
        self.fs.mkdirs(packages::REPOSITORY_DIR, 0);
    }

    /// Sends an export's files (`{type: "exported", id, names, contents}`: the document, then the
    /// pictures Markdown writes beside it), or why there is none (`{..., error}`: retex's first
    /// error).
    fn exported(&self, id: f64, cwd: &str, job: &str, output: &str, project: &HashSet<String>, text: &str) {
        let Some(data) = self.fs.read(output) else {
            let why = text
                .lines()
                .filter_map(|l| crate::records::parse(l, None))
                .find_map(|r| match r {
                    crate::records::Record::Diagnostic(d) if d.severity == crate::Severity::Error => Some(d.message),
                    _ => None,
                })
                .unwrap_or_else(|| "retex wrote no file".to_owned());
            post("exported", id, &[("error", why.into())], &[]);
            return;
        };
        let names = Array::new();
        let contents = Array::new();
        let mut transfer = Vec::new();
        let mut add = |name: &str, bytes: &[u8]| {
            let arr = Uint8Array::from(bytes);
            transfer.push(JsValue::from(arr.buffer()));
            names.push(&name.into());
            contents.push(&arr.into());
        };
        add(output.rsplit('/').next().unwrap_or(output), &data);
        // Markdown's pictures: the files retex wrote beside it, under a name of the job's.
        const BUILD_FILES: &[&str] = &["aux", "log", "fls", "toc", "out", "synctex", "pdf", "nav", "snm", "bbl", "blg", "bcf", "xml"];
        for (path, _) in self.fs.files(cwd) {
            let Some(rel) = path.strip_prefix(&format!("{cwd}/")) else { continue };
            let ext = rel.rsplit('.').next().unwrap_or_default();
            if path == output || project.contains(&path) || !rel.starts_with(job) || rel.starts_with(".retex") || BUILD_FILES.contains(&ext) {
                continue;
            }
            if let Some(bytes) = self.fs.read(&path) {
                add(rel, &bytes);
            }
        }
        post("exported", id, &[("names", names.into()), ("contents", contents.into())], &transfer);
    }

    /// What the project's sources (`names`, `contents`: the build message's) load, fetched
    /// together before the first run that needs it.
    async fn prefetch_sources(&mut self, id: f64, names: &Array, contents: &Array) {
        let mut wanted = Vec::new();
        for (name, bytes) in names.iter().zip(contents.iter()) {
            let name = name.as_string().unwrap_or_default();
            if [".tex", ".sty", ".cls", ".ltx"].iter().any(|e| name.ends_with(e)) {
                wanted.extend(prefetch::loads(&String::from_utf8_lossy(&Uint8Array::new(&bytes).to_vec())));
            }
        }
        let paths = self.expand(id, wanted).await;
        if let Err(e) = self.fetch_all(id, paths).await {
            log::warn!("fetching what the document loads: {e}");
        }
    }

    /// After a run stopped for `source`: fetches its file, the others of its folder (up to
    /// [`SIBLINGS_BYTES`]), and for a package's file what the package reads.
    async fn fetch_stopped_for(&mut self, id: f64, source: &str) -> Result<(), String> {
        let Some((path, _)) = locate(source) else { return Err(format!("unknown source {source}")) };
        let mut paths = vec![path.clone()];
        if let Some((folder, _)) = path.rsplit_once('/') {
            let mut budget = SIBLINGS_BYTES;
            for (p, size) in self.folders.get(folder).into_iter().flatten() {
                if *p != path && *size <= budget && !self.web.fetched.borrow().urls.contains_key(p) {
                    budget -= size;
                    paths.push(p.clone());
                }
            }
        }
        // A package's file: what the package reads comes with it.
        paths.extend(self.expand(id, prefetch::entry_of(&path).into_iter().collect()).await);
        self.fetch_all(id, paths).await
    }

    /// The site's files the classes, packages and styles `wanted` ([`prefetch::loads`]'s
    /// entries) read, for [`Self::fetch_all`] to fetch together: for each, the tree's list of what
    /// a document loading it reads (`seed/reads/package/NAME.txt`, …) or, without one, its file
    /// (fetched here) and what that file loads in turn. Entries expanded before are left out.
    async fn expand(&mut self, id: f64, mut wanted: Vec<String>) -> Vec<String> {
        let mut paths = Vec::new();
        while !wanted.is_empty() {
            wanted.retain(|e| self.expanded.insert(e.clone()));
            // The lists of this round, requested together.
            let requests: Vec<(String, js_sys::Promise)> =
                wanted.iter().map(|e| (e.clone(), scope().fetch_with_str(&self.web.fetched.borrow().url(&format!("seed/reads/{e}.txt"))))).collect();
            let mut next = Vec::new();
            for (entry, request) in requests {
                match response_bytes(&entry, request).await {
                    Ok(list) => paths.extend(String::from_utf8_lossy(&list).lines().filter(|l| l.starts_with("tree/")).map(str::to_owned)),
                    Err(_) => {
                        // No list: the file itself, and what it loads.
                        let Some((source, size)) = prefetch::file_name(&entry).and_then(|n| self.names.get(&n).cloned()) else { continue };
                        let Some((path, offset)) = locate(&source) else { continue };
                        if self.fetch_all(id, vec![path.clone()]).await.is_err() {
                            continue;
                        }
                        let bytes = self.web.fetched.borrow().urls.get(&path).cloned();
                        let start = offset.unwrap_or(0);
                        let end = if offset.is_some() { start + size as usize } else { usize::MAX };
                        if let Some(text) = bytes.as_ref().and_then(|b| b.get(start..end.min(b.len()))) {
                            next.extend(prefetch::loads(&String::from_utf8_lossy(text)));
                        }
                    }
                }
            }
            wanted = next;
        }
        paths.sort();
        paths.dedup();
        paths
    }

    /// Before `project`'s first build in this visit, fetches its read list. A file that cannot be
    /// fetched is left for the run to stop for, as without a list.
    async fn fetch_read_list(&mut self, id: f64, project: &str) {
        if self.lists.contains_key(project) {
            return;
        }
        let list = self.kept.read_list(project).await;
        log::info!("the project's read list: {} files", list.len());
        if let Err(e) = self.fetch_all(id, list.iter().cloned().collect()).await {
            log::warn!("fetching the project's read list: {e}");
        }
        self.lists.insert(project.to_owned(), list);
    }

    /// Updates `project`'s read list with the site's files this build opened, and keeps it when
    /// it changed. After a run from the start that succeeded (`whole`), what it opened is the
    /// whole list, so a package the project no longer uses leaves it; after a run resumed from
    /// the preamble's snapshot, which did not open the preamble's files again, or one that failed
    /// and may have stopped early, they are added to it. Only
    /// builds update it: an export reads files a preview does not need.
    async fn keep_read_list(&mut self, project: &str, whole: bool) {
        let opened = std::mem::take(&mut self.web.fetched.borrow_mut().opened);
        let list = self.lists.entry(project.to_owned()).or_default();
        let new = if whole { opened } else { list.union(&opened).cloned().collect() };
        if new != *list {
            log::info!("the project's read list: {} files, was {}", new.len(), list.len());
            *list = new;
            self.kept.keep_read_list(project, list).await;
        }
    }

    /// Keeps what this build fetched while it ran and what the engine made, for later visits.
    async fn keep(&mut self) {
        let new: Vec<(String, Rc<Vec<u8>>)> =
            self.web.fetched.borrow().urls.iter().filter(|(p, _)| !self.kept.files.contains(*p)).map(|(p, b)| (p.clone(), b.clone())).collect();
        for (path, bytes) in new {
            self.kept.keep_file(&path, &bytes).await;
            if self.kept.tree.is_none() {
                break;
            }
        }
        self.kept.keep_made(&self.fs).await;
    }
}

/// The worker's log, sent to the editor (`{type: "log", level, text}`), whose console shows it.
struct Relay;

impl log::Log for Relay {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            post("log", -1.0, &[("level", r.level().as_str().into()), ("text", r.args().to_string().into())], &[]);
        }
    }

    fn flush(&self) {}
}

static RELAY: Relay = Relay;

/// Runs the engine's side: call it from the worker's entry point.
pub fn worker_main() {
    if log::set_logger(&RELAY).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    // A panic ends the worker: the editor is told, so the build it waits for fails with the
    // reason instead of never ending.
    std::panic::set_hook(Box::new(|info| post("crash", -1.0, &[("error", info.to_string().into())], &[])));
    // The engine, taken out while a build runs (only one runs at a time).
    let engine = Rc::new(RefCell::new(Some(Engine::new())));
    let queue: Rc<RefCell<Vec<JsValue>>> = Rc::default();
    let busy = Rc::new(RefCell::new(false));
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        queue.borrow_mut().push(e.data());
        if *busy.borrow() {
            return;
        }
        *busy.borrow_mut() = true;
        let (engine, queue, busy) = (engine.clone(), queue.clone(), busy.clone());
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let next = {
                    let mut q = queue.borrow_mut();
                    if q.is_empty() { None } else { Some(q.remove(0)) }
                };
                let Some(msg) = next else { break };
                let id = get(&msg, "id").as_f64().unwrap_or(0.0);
                let Some(mut e) = engine.borrow_mut().take() else { break };
                let result = e.build(&msg).await;
                *engine.borrow_mut() = Some(e);
                if let Err(e) = result {
                    let kind = if get(&msg, "type").as_string().as_deref() == Some("export") { "exported" } else { "done" };
                    post(kind, id, &[("error", e.into())], &[]);
                }
            }
            *busy.borrow_mut() = false;
        });
    });
    scope().set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
    // Messages sent before the handler was set are lost: the editor waits for this.
    post("ready", -1.0, &[], &[]);
}
