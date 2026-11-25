//! Formulas typeset on their own (docs/ROADMAP.md, F2): each formula of a file after the main
//! document's preamble, on a page of its own as tight as its box, with retex's fragments
//! (`retex::Worker::fragments`, `retex::fragments::Batch`).
//!
//! [`Formulas`] keeps what it typeset by (preamble, source, mode), so asking again for a file
//! after one formula changed typesets that formula only: the formulas it does not have go in one
//! batch, on a thread of its own. In the desktop app the batch goes to a `retex worker` process of
//! its own (not the build's: a batch would stop a running build), which keeps the preamble's
//! snapshot, so a batch after the first starts at `\begin{document}`. Elsewhere (the CLI, tests)
//! it runs in process.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use texcraft_project::ProjectPath;

/// A formula to typeset: its math without delimiters (an environment's lines inside amsmath's
/// `aligned` or `gathered`), in text or display style.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Wanted {
    pub source: String,
    pub display: bool,
}

/// A typeset formula: its page of a batch's PDF and its box, in TeX points.
#[derive(Debug, Clone)]
pub struct Typeset {
    pub pdf: Arc<Vec<u8>>,
    /// From 0.
    pub page: u32,
    pub width_pt: f32,
    /// Above the baseline.
    pub height_pt: f32,
    /// Below the baseline.
    pub depth_pt: f32,
}

/// What became of a formula.
#[derive(Debug, Clone)]
pub enum Done {
    Typeset(Typeset),
    /// Not typeset: retex's messages, for the user.
    Failed(Vec<String>),
}

/// A formula's state.
#[derive(Debug, Clone)]
pub enum State {
    /// In a batch that runs or waits.
    Pending,
    Done(Arc<Done>),
}

/// The files of the project and the main document's folder in it, for a batch's preamble to find
/// its packages and inputs.
pub struct Files {
    pub files: Vec<(ProjectPath, Arc<[u8]>)>,
    /// The main document's folder (`None` at the project's root).
    pub main_dir: Option<String>,
}

type Key = (u64, Wanted);

struct Request {
    files: Files,
    preamble: String,
    preamble_hash: u64,
    wanted: Vec<Wanted>,
}

struct Reply {
    preamble_hash: u64,
    results: Vec<(Wanted, Done)>,
}

/// Formulas typeset on a thread of their own, cached by (preamble, source, mode).
pub struct Formulas {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
    /// The preamble of the cache; another one empties it.
    preamble_hash: Option<u64>,
    cache: HashMap<Key, Arc<Done>>,
    /// Sent and not back yet.
    running: HashSet<Key>,
    /// The batch to send when the running one is back.
    waiting: Option<Request>,
    /// Batches run so far, for the tests and the log.
    batches: u64,
    gone: bool,
}

impl Formulas {
    /// Typesets in `dir` (the project's files are copied to `dir/src`).
    pub fn new(dir: PathBuf) -> Formulas {
        let (requests, request_rx) = channel::<Request>();
        let (reply_tx, replies) = channel();
        let program = crate::WORKER_PROGRAM.get().cloned();
        let spawned = std::thread::Builder::new().name("texcraft-formulas".to_owned()).spawn(move || {
            let mut engine = Engine { dir, program, worker: None, written: HashSet::new() };
            for request in request_rx {
                let hash = request.preamble_hash;
                let wanted = request.wanted.clone();
                let results = catch_unwind(AssertUnwindSafe(|| engine.run(request))).unwrap_or_else(|_| {
                    let why = "The typesetting engine stopped unexpectedly.".to_owned();
                    wanted.into_iter().map(|w| (w, Done::Failed(vec![why.clone()]))).collect()
                });
                if reply_tx.send(Reply { preamble_hash: hash, results }).is_err() {
                    break;
                }
            }
        });
        let gone = match spawned {
            Ok(_) => false,
            Err(e) => {
                log::error!("cannot start the formulas thread: {e}");
                true
            }
        };
        Formulas { requests, replies, preamble_hash: None, cache: HashMap::new(), running: HashSet::new(), waiting: None, batches: 0, gone }
    }

    /// The states of `wanted` after `preamble`, sending those not typeset yet in one batch (after
    /// the running one, replacing one that waits). `files` is called only when a batch is sent.
    pub fn request(&mut self, preamble: &str, wanted: &[Wanted], files: impl FnOnce() -> Files) -> Vec<State> {
        self.poll();
        let hash = hash_of(preamble);
        if self.preamble_hash != Some(hash) {
            self.preamble_hash = Some(hash);
            self.cache.clear();
            self.waiting = None;
        }
        let mut missing = Vec::new();
        let mut seen = HashSet::new();
        let states = wanted
            .iter()
            .map(|w| {
                let key = (hash, w.clone());
                match self.cache.get(&key) {
                    Some(done) => State::Done(Arc::clone(done)),
                    None if self.gone => State::Done(Arc::new(Done::Failed(vec!["The typesetting engine is not running.".to_owned()]))),
                    None => {
                        if !self.running.contains(&key) && seen.insert(w.clone()) {
                            missing.push(w.clone());
                        }
                        State::Pending
                    }
                }
            })
            .collect();
        if !missing.is_empty() && !self.gone {
            let waits_for_same = self.waiting.as_ref().is_some_and(|r| r.preamble_hash == hash && r.wanted == missing);
            if !waits_for_same {
                let request = Request { files: files(), preamble: preamble.to_owned(), preamble_hash: hash, wanted: missing };
                if self.running.is_empty() {
                    self.send(request);
                } else {
                    self.waiting = Some(request);
                }
            }
        }
        states
    }

    fn send(&mut self, request: Request) {
        let keys: Vec<Key> = request.wanted.iter().map(|w| (request.preamble_hash, w.clone())).collect();
        if self.requests.send(request).is_err() {
            log::error!("the formulas thread is gone");
            self.gone = true;
            return;
        }
        self.batches += 1;
        self.running.extend(keys);
    }

    /// Takes the results that arrived; true when some did.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    self.take(reply);
                    any = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.gone = !self.running.is_empty() || self.gone;
                    self.running.clear();
                    break;
                }
            }
        }
        any
    }

    fn take(&mut self, reply: Reply) {
        for (wanted, done) in reply.results {
            let key = (reply.preamble_hash, wanted);
            self.running.remove(&key);
            if Some(reply.preamble_hash) == self.preamble_hash {
                self.cache.insert(key, Arc::new(done));
            }
        }
        if self.running.is_empty()
            && let Some(next) = self.waiting.take()
        {
            // Formulas typeset meanwhile are not sent again.
            let wanted: Vec<Wanted> = next.wanted.iter().filter(|w| !self.cache.contains_key(&(next.preamble_hash, (*w).clone()))).cloned().collect();
            if !wanted.is_empty() {
                self.send(Request { wanted, ..next });
            }
        }
    }

    /// Whether a batch runs or waits.
    pub fn busy(&self) -> bool {
        !self.running.is_empty() || self.waiting.is_some()
    }

    /// Waits until no batch runs or waits (the CLI and the agent tools).
    pub fn wait(&mut self) {
        while self.busy() && !self.gone {
            match self.replies.recv() {
                Ok(reply) => self.take(reply),
                Err(_) => {
                    self.gone = true;
                    self.running.clear();
                    self.waiting = None;
                }
            }
        }
    }

    /// How many batches were sent.
    pub fn batches(&self) -> u64 {
        self.batches
    }
}

fn hash_of(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The thread's side: the copy of the project, and the worker process when there is one.
struct Engine {
    dir: PathBuf,
    program: Option<PathBuf>,
    worker: Option<retex::Worker>,
    written: HashSet<String>,
}

impl Engine {
    fn run(&mut self, request: Request) -> Vec<(Wanted, Done)> {
        let fail = |wanted: Vec<Wanted>, why: String| wanted.into_iter().map(|w| (w, Done::Failed(vec![why.clone()]))).collect();
        let dir = match self.write(&request.files) {
            Ok(d) => d,
            Err(e) => return fail(request.wanted, e),
        };
        let fragments: Vec<retex::Fragment> = request
            .wanted
            .iter()
            .enumerate()
            .map(|(i, w)| retex::Fragment {
                id: i as u64,
                source: w.source.clone(),
                mode: if w.display { retex::FragmentMode::DisplayMath } else { retex::FragmentMode::InlineMath },
            })
            .collect();
        let typeset = self.through_worker(&dir, &request.preamble, &fragments).unwrap_or_else(|| in_process(&dir, &request.preamble, &fragments));
        let (pdf, results) = match typeset {
            Ok(r) => r,
            Err(e) => return fail(request.wanted, e),
        };
        let mut by_id: HashMap<u64, Result<Typeset, Vec<String>>> = results.into_iter().collect();
        request
            .wanted
            .into_iter()
            .enumerate()
            .map(|(i, w)| {
                let done = match by_id.remove(&(i as u64)) {
                    Some(Ok(mut t)) => {
                        t.pdf = Arc::clone(&pdf);
                        Done::Typeset(t)
                    }
                    Some(Err(why)) => Done::Failed(why),
                    None => Done::Failed(vec!["retex did not typeset this formula.".to_owned()]),
                };
                (w, done)
            })
            .collect()
    }

    /// Writes the project's files that changed to `dir/src`; returns the main document's folder
    /// there.
    fn write(&mut self, files: &Files) -> Result<PathBuf, String> {
        let src = self.dir.join("src");
        let mut now = HashSet::new();
        for (path, bytes) in &files.files {
            let target = src.join(path.as_str());
            now.insert(path.as_str().to_owned());
            if std::fs::read(&target).is_ok_and(|old| old.as_slice() == &bytes[..]) {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
            }
            std::fs::write(&target, bytes).map_err(|e| format!("writing {}: {e}", target.display()))?;
        }
        for gone in self.written.difference(&now) {
            let _ = std::fs::remove_file(src.join(gone));
        }
        self.written = now;
        let dir = match &files.main_dir {
            Some(d) => src.join(d),
            None => src,
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        Ok(dir)
    }

    /// Through the worker process; `None` when there is none or it failed (the caller typesets in
    /// process).
    fn through_worker(&mut self, dir: &Path, preamble: &str, fragments: &[retex::Fragment]) -> Option<Batch> {
        let program = self.program.clone()?;
        if self.worker.is_none() {
            let mut config = retex::WorkerConfig::new();
            config.program = Some(program);
            match retex::Worker::spawn(config) {
                Ok(w) => self.worker = Some(w),
                Err(e) => {
                    log::warn!("the formulas worker did not start ({e}); typesetting in process");
                    self.program = None;
                    return None;
                }
            }
        }
        let worker = self.worker.as_ref()?;
        match worker.fragments(dir, preamble, fragments) {
            Ok((doc, typeset)) => {
                let results = typeset
                    .into_iter()
                    .map(|t| {
                        let messages: Vec<String> = t.diagnostics.iter().map(message).collect();
                        let r = match t.page {
                            Some(page) => {
                                Ok(Typeset { pdf: Arc::default(), page, width_pt: t.width_pt, height_pt: t.height_pt, depth_pt: t.depth_pt })
                            }
                            None => Err(or_not_typeset(messages)),
                        };
                        (t.id, r)
                    })
                    .collect();
                Some(Ok((Arc::new(doc.pdf().to_vec()), results)))
            }
            Err(retex::Error::Failed { diagnostics, .. }) => Some(Err(failed(&diagnostics))),
            Err(e) => {
                log::warn!("the formulas worker failed ({e}); typesetting in process");
                self.worker = None;
                None
            }
        }
    }
}

/// A batch's PDF and each fragment's box or messages, by its id.
type Batch = Result<(Arc<Vec<u8>>, Vec<(u64, Result<Typeset, Vec<String>>)>), String>;

/// In process, as `retex::Worker::fragments` does it in the worker.
fn in_process(dir: &Path, preamble: &str, fragments: &[retex::Fragment]) -> Batch {
    let batch = retex::fragments::Batch::new(preamble, fragments);
    let path = dir.join(".retex/fragments").join(batch.file_name());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, batch.text()).map_err(|e| format!("writing {}: {e}", path.display()))?;
    let doc = match retex::Retex::new().passes(1).working_dir(dir).compile_file(&path) {
        Ok(d) => d,
        Err(retex::Error::Failed { diagnostics, .. }) => return Err(failed(&diagnostics)),
        Err(e) => return Err(e.to_string()),
    };
    let all = doc.diagnostics();
    let places: Vec<retex::fragments::DiagnosticRef<'_>> =
        all.iter().map(|d| retex::fragments::DiagnosticRef::new(d.file.as_deref(), d.line, d.severity == retex::Severity::Error)).collect();
    let results = batch
        .results(doc.log(), doc.pages(), &places)
        .into_iter()
        .map(|r| {
            let mut messages: Vec<String> = r.refused.map(str::to_owned).into_iter().collect();
            messages.extend(r.diagnostics.iter().filter_map(|f| all.get(f.index)).map(message));
            let t = match r.page {
                Some(page) => Ok(Typeset { pdf: Arc::default(), page, width_pt: r.width_pt, height_pt: r.height_pt, depth_pt: r.depth_pt }),
                None => Err(or_not_typeset(messages)),
            };
            (r.id, t)
        })
        .collect();
    Ok((Arc::new(doc.pdf().to_vec()), results))
}

fn message(d: &retex::Diagnostic) -> String {
    d.message.clone()
}

/// A fragment without a page and without a message was cut off by an error before it.
fn or_not_typeset(messages: Vec<String>) -> Vec<String> {
    if messages.is_empty() { vec!["Not typeset: an error before this formula ended the batch.".to_owned()] } else { messages }
}

/// Why a whole batch failed: its first error.
fn failed(diagnostics: &[retex::Diagnostic]) -> String {
    match diagnostics.iter().find(|d| d.severity == retex::Severity::Error) {
        Some(d) => format!("The formulas could not be typeset: {}", d.message),
        None => "The formulas could not be typeset.".to_owned(),
    }
}
