//! The engine's side of the browser build, in a Web Worker: retex's WebAssembly module (built by
//! `cargo xtask retex-wasm`, served beside the app) over `texcraft-wasi`'s in-memory files, with
//! the TeX tree of the web build's own site (`cargo xtask tree-site`) loaded as retex reads it (plan, phase 6).
//!
//! WASI's reads are synchronous. A file of the tree not fetched yet is fetched with a
//! synchronous request where the browser serves one; where it does not (Chrome stops serving
//! them once the worker has run for a few seconds without returning to its event loop), the run
//! stops, the file is fetched, and the run starts again. Before its first build the worker
//! fetches the files a plain article reads (retex's read list), so that a first build mostly
//! runs through.
//!
//! What it fetched and what the engine made (its format, the indexes of the tree's files and
//! fonts: `/cache`) is kept in the browser's Cache Storage, so a later visit neither fetches nor
//! makes them again ([`Kept`]).
//!
//! The messages, both ways, are plain objects: see [`crate::web`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use js_sys::{Array, Object, Reflect, Uint8Array, WebAssembly};
use texcraft_wasi::web::{Ended, Image, Instance};
use texcraft_wasi::{FileSystem, Platform, Wasi};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Cache, DedicatedWorkerGlobalScope, MessageEvent, Request, Response, ResponseInit, XmlHttpRequest, XmlHttpRequestResponseType};

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

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
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
}

#[derive(Clone)]
struct Web {
    fetched: Rc<RefCell<Fetched>>,
    out: Rc<RefCell<Out>>,
    fs: FileSystem,
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
                match fetch_sync(&format!("{TREE_SITE}{path}")) {
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
            if line.starts_with("{\"type\":\"progress\"") || line.starts_with("{\"type\":\"pass\"") {
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
}

/// A 64-bit FNV-1a hash, to name the caches after what they depend on.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

/// Where the engine's own files are kept in the cache: a URL of this page's site that is never
/// fetched, with the file's path.
const ENGINE_FILES: &str = "retex-cache";

/// The browser's Cache Storage: the site's files fetched (cache `texcraft-tree-<index>`, by their
/// URL), and the files the engine made in `/cache` (cache `texcraft-engine-<module>-<index>`).
/// A cache's name holds a hash of what its files depend on: the tree's index, and for the
/// engine's files the module too. Caches with other names are from earlier versions, and are
/// deleted. Keeping is best effort: where the browser has no Cache Storage (or refuses it), each
/// visit fetches again.
#[derive(Default)]
struct Kept {
    tree: Option<Cache>,
    engine: Option<Cache>,
    /// The site's files in the cache.
    files: HashSet<String>,
    /// The engine's files in the cache, with their time.
    made: HashMap<String, u64>,
}

async fn promise(p: Result<js_sys::Promise, JsValue>) -> Result<JsValue, String> {
    JsFuture::from(p.map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))
}

impl Kept {
    /// Opens the caches for this module and tree, and deletes the others.
    async fn open(module: u64, index: u64) -> Kept {
        let Ok(storage) = scope().caches() else { return Kept::default() };
        let tree = format!("texcraft-tree-{index:016x}");
        let engine = format!("texcraft-engine-{module:016x}-{index:016x}");
        if let Ok(names) = promise(Ok(storage.keys())).await {
            for name in Array::from(&names).iter().filter_map(|n| n.as_string()) {
                if name.starts_with("texcraft-") && name != tree && name != engine {
                    log::info!("deleting the old cache {name}");
                    let _ = promise(Ok(storage.delete(&name))).await;
                }
            }
        }
        let open = |name: String| {
            let storage = storage.clone();
            async move { promise(Ok(storage.open(&name))).await.ok().and_then(|c| c.dyn_into::<Cache>().ok()) }
        };
        Kept { tree: open(tree).await, engine: open(engine).await, ..Kept::default() }
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

    /// Puts the engine's kept files in `fs`.
    async fn restore(&mut self, fs: &FileSystem) {
        let Some(engine) = &self.engine else { return };
        let Ok(requests) = promise(Ok(engine.keys())).await else { return };
        let prefix = format!("/{ENGINE_FILES}/");
        for request in Array::from(&requests).iter().filter_map(|r| r.dyn_into::<Request>().ok()) {
            let url = request.url();
            let Some(path) = url.find(&prefix).map(|at| url[at + prefix.len() - 1..].to_owned()) else { continue };
            let Ok(resp) = promise(Ok(engine.match_with_request(&request))).await else { continue };
            if let Some((bytes, mtime)) = Kept::body(resp).await {
                fs.add(&path, bytes, mtime);
                self.made.insert(path, mtime);
            }
        }
        log::info!("{} of the engine's files restored from an earlier visit", self.made.len());
    }

    /// Keeps the engine's files in `/cache` that are new or changed.
    async fn keep_made(&mut self, fs: &FileSystem) {
        let Some(engine) = self.engine.clone() else { return };
        for (path, mtime) in fs.files("/cache") {
            if self.made.get(&path) == Some(&mtime) {
                continue;
            }
            let Some(bytes) = fs.read(&path) else { continue };
            match Kept::put(&engine, &format!("{ENGINE_FILES}{path}"), &bytes, mtime).await {
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
    /// The site's files of the tree by folder (site paths and sizes), for fetching a file's
    /// neighbours with it.
    folders: HashMap<String, Vec<(String, u64)>>,
    kept: Kept,
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    response_bytes(url, scope().fetch_with_str(url)).await
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
        let web = Web { fetched: Rc::default(), out: Rc::default(), fs: fs.clone() };
        Engine { module: None, fs, web, written: HashSet::new(), snapshot: None, folders: HashMap::new(), kept: Kept::default() }
    }

    /// Fetches the site's files at `paths` not fetched yet, a batch at a time, saying so: from
    /// the browser's cache when they are kept there.
    async fn fetch_all(&mut self, id: f64, paths: Vec<String>) -> Result<(), String> {
        let mut todo = Vec::new();
        for p in paths {
            if self.web.fetched.borrow().urls.contains_key(&p) {
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
            post(
                "line",
                id,
                &[("text", format!("{{\"type\":\"progress\",\"note\":\"downloading TeX files ({} of {total})\"}}", n * FETCH_BATCH).into())],
                &[],
            );
            // The requests are all made before any is awaited, so they download together.
            let requests: Vec<js_sys::Promise> = batch.iter().map(|p| scope().fetch_with_str(&format!("{TREE_SITE}{p}"))).collect();
            for (p, request) in batch.iter().zip(requests) {
                let bytes = response_bytes(p, request).await?;
                self.kept.keep_file(p, &bytes).await;
                self.web.fetched.borrow_mut().urls.insert(p.clone(), Rc::new(bytes));
            }
        }
        Ok(())
    }

    /// The module and the tree's list of files, the first time.
    async fn init(&mut self, id: f64) -> Result<(), String> {
        if self.module.is_some() {
            return Ok(());
        }
        post("line", id, &[("text", "{\"type\":\"progress\",\"note\":\"loading the typesetting engine\"}".into())], &[]);
        log::info!("loading retex.wasm and the tree's index");
        let wasm = fetch_bytes("retex.wasm").await?;
        let module =
            JsFuture::from(WebAssembly::compile(&Uint8Array::from(wasm.as_slice()).into())).await.map_err(|e| format!("retex.wasm: {e:?}"))?;
        let index = String::from_utf8(fetch_bytes(&format!("{TREE_SITE}tree/index.txt")).await?).map_err(|_| "tree/index.txt is not UTF-8")?;
        self.kept = Kept::open(fnv(&wasm), fnv(index.as_bytes())).await;
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
        if let Ok(list) = fetch_bytes(&format!("{TREE_SITE}seed/index.txt")).await {
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
        if let Ok(list) = fetch_bytes(&format!("{TREE_SITE}seed/reads/blank.txt")).await {
            let mut paths: Vec<String> = String::from_utf8_lossy(&list).lines().filter(|l| l.starts_with("tree/")).map(str::to_owned).collect();
            paths.extend(seeds);
            self.fetch_all(id, paths).await?;
        }
        log::info!("engine ready: {} files fetched", self.web.fetched.borrow().urls.len());
        self.module = Some(module.unchecked_into());
        Ok(())
    }

    async fn build(&mut self, msg: &JsValue) -> Result<(), String> {
        let id = get(msg, "id").as_f64().unwrap_or(0.0);
        self.init(id).await?;
        let Some(module) = self.module.clone() else { return Err("the engine did not load".into()) };
        let main = get(msg, "main").as_string().ok_or("no main document")?;
        let names: Array = get(msg, "names").unchecked_into();
        let contents: Array = get(msg, "contents").unchecked_into();
        let flags: Array = get(msg, "args").unchecked_into();
        let mut now = HashSet::new();
        for (name, bytes) in names.iter().zip(contents.iter()) {
            let name = format!("/{}", name.as_string().unwrap_or_default());
            self.fs.add(&name, Uint8Array::new(&bytes).to_vec(), now_ns());
            now.insert(name);
        }
        for old in self.written.difference(&now) {
            self.fs.remove(old);
        }
        self.written = now;
        let job = main.strip_suffix(".tex").unwrap_or(&main).to_owned();
        let pdf = format!("/{job}.pdf");
        let synctex = format!("/{job}.synctex");
        self.fs.remove(&pdf);
        self.fs.remove(&synctex);
        let mut args: Vec<String> =
            vec!["retex".into(), "--color=never".into(), "--message-format=json".into(), "--symbols".into(), "-synctex=-1".into()];
        args.extend(flags.iter().filter_map(|a| a.as_string()));
        args.push(main.clone());
        let key = args.join("\u{0}");
        let env: Vec<(String, String)> = ENV.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        let started = js_sys::Date::now();
        let mut stops = 0;
        let mut reruns = 0;
        let code = loop {
            *self.web.out.borrow_mut() = Out { job: id, pdf: pdf.clone(), ..Out::default() };
            self.web.fetched.borrow_mut().run_started = js_sys::Date::now();
            let wasi = Wasi::new(self.fs.clone(), self.web.clone(), args.clone(), env.clone());
            let instance = Instance::new(&module, wasi, true).map_err(|e| format!("instantiating retex: {e:?}"))?;
            let resumed = matches!(&self.snapshot, Some((k, _)) if *k == key);
            let (ended, instance) = match (&self.snapshot, resumed) {
                (Some((_, image)), true) => instance.resume(image),
                _ => instance.start(),
            };
            let image = instance.image();
            match ended {
                Ended::Exit(STALE) if resumed => {
                    // The preamble's files changed: from the start, with a new snapshot.
                    self.snapshot = None;
                }
                Ended::Exit(RERUN) if resumed && reruns < 10 => reruns += 1,
                Ended::Exit(code) => {
                    if let Some(image) = image {
                        self.snapshot = Some((key.clone(), image));
                    }
                    break code;
                }
                Ended::Fetch(source) => {
                    stops += 1;
                    if stops > MAX_FETCH_STOPS {
                        return Err("the build needed more TeX files than it could fetch".into());
                    }
                    let Some((path, _)) = locate(&source) else { return Err(format!("unknown source {source}")) };
                    log::info!("build {id}: run {stops} stopped to fetch {path}");
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
                    self.fetch_all(id, paths).await?;
                }
                Ended::Crashed(why) => return Err(format!("the typesetting engine stopped: {why}")),
            }
        };
        self.fs.remove("/.retex-resume");
        let out = std::mem::take(&mut *self.web.out.borrow_mut());
        let seconds = (js_sys::Date::now() - started) / 1000.0;
        let mut fields = vec![("code", JsValue::from(code)), ("output", out.text.into()), ("seconds", seconds.into()), ("stops", stops.into())];
        let mut transfer = Vec::new();
        for (key, path) in [("pdf", &pdf), ("synctex", &synctex)] {
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
                    post("done", id, &[("error", e.into())], &[]);
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
