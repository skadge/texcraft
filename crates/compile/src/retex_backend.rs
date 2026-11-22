//! retex linked in process, through its embedding API (`retex::Retex`, retex's
//! `crates/retex-cli/src/embed.rs`).
//!
//! The project is written to `<cache>/src/` and built there with the build files kept in
//! `<cache>/out/`, so the auxiliary files of one build (`.aux`, `.toc`, `.bbl`) are read by the
//! next and a rebuild usually needs one pass.
//!
//! From retex 2.1.0 a build reports its progress, can be cancelled, and returns the language
//! server's `symbols` record, the SyncTeX data, and its diagnostics' spans and fixes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::{Backend, Canceller, Diagnostic, DiagnosticFix, Event, Job, Outcome, Severity, Status};

pub struct Retex {
    dir: PathBuf,
    /// The running build's cancel token.
    running: Arc<Mutex<Option<retex::CancelToken>>>,
}

impl Retex {
    /// Builds in `cache_dir`, or in a folder of the system's temporary directory.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let dir = cache_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("texcraft-build-{}", std::process::id())));
        Self { dir, running: Arc::new(Mutex::new(None)) }
    }

    fn write_sources(&self, job: &Job) -> Result<PathBuf, String> {
        let src = self.dir.join("src");
        if src.exists() {
            std::fs::remove_dir_all(&src).map_err(|e| format!("clearing {}: {e}", src.display()))?;
        }
        for (path, bytes) in &job.files {
            let target = src.join(path.as_str());
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
            }
            std::fs::write(&target, bytes).map_err(|e| format!("writing {}: {e}", target.display()))?;
        }
        Ok(src.join(job.main.as_str()))
    }
}

impl Backend for Retex {
    fn name(&self) -> String {
        "retex".to_owned()
    }

    fn compile(&mut self, job: &Job) -> Outcome {
        self.compile_reporting(job, &mut |_| {})
    }

    fn can_cancel(&self) -> bool {
        true
    }

    fn canceller(&self) -> Option<Canceller> {
        let running = self.running.clone();
        Some(Arc::new(move || {
            if let Ok(slot) = running.lock()
                && let Some(token) = slot.as_ref()
            {
                token.cancel();
            }
        }))
    }

    fn compile_reporting(&mut self, job: &Job, report: &mut dyn FnMut(Event)) -> Outcome {
        let started = Instant::now();
        let main = match self.write_sources(job) {
            Ok(m) => m,
            Err(e) => return Outcome { log: e.clone(), ..Outcome::unavailable(job.id, e) },
        };
        let main_dir = job.main.as_str().rsplit_once('/').map(|(d, _)| d.to_owned());
        let src = std::fs::canonicalize(self.dir.join("src")).unwrap_or_else(|_| self.dir.join("src"));
        let place = |d: &retex::Diagnostic| convert(d, &src, main_dir.as_deref());
        let token = retex::CancelToken::new();
        if let Ok(mut slot) = self.running.lock() {
            *slot = Some(token.clone());
        }
        // retex calls back on its build thread; the events go to `report` here, on ours.
        let (tx, rx) = std::sync::mpsc::channel();
        let retex = retex::Retex::new().keep_files(self.dir.join("out")).symbols(true).synctex(true).cancel_token(token).on_progress(move |p| {
            let _ = tx.send(p);
        });
        let handle = std::thread::scope(|scope| {
            let build = scope.spawn(|| retex.compile_file_outcome(&main));
            while !build.is_finished() {
                while let Ok(p) = rx.try_recv() {
                    if let Some(e) = event(p) {
                        report(e);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            build.join()
        });
        if let Ok(mut slot) = self.running.lock() {
            *slot = None;
        }
        let seconds = started.elapsed().as_secs_f64();
        let Ok(outcome) = handle else {
            return Outcome { seconds, ..Outcome::unavailable(job.id, "The typesetting engine stopped unexpectedly; the build was abandoned.") };
        };
        let symbols = outcome.outputs.symbols().cloned();
        let synctex = outcome.outputs.synctex().map(|s| Arc::new(s.to_vec()));
        let base = Outcome { symbols, synctex, seconds, ..Outcome::unavailable(job.id, "") };
        if outcome.cancelled {
            return Outcome { status: Status::Cancelled, log: "The build was stopped.".to_owned(), ..base };
        }
        match outcome.result {
            Ok(doc) => Outcome {
                status: Status::Built,
                pdf: Some(Arc::new(doc.pdf().to_vec())),
                diagnostics: doc.diagnostics().iter().map(place).collect(),
                log: doc.log().to_owned(),
                pages: doc.pages(),
                ..base
            },
            Err(retex::Error::Failed { diagnostics, log }) => {
                Outcome { status: Status::Failed, diagnostics: diagnostics.iter().map(place).collect(), log, ..base }
            }
            Err(other) => {
                Outcome { log: other.to_string(), symbols: base.symbols, synctex: base.synctex, seconds, ..failure(job.id, &other.to_string()) }
            }
        }
    }
}

fn failure(job: u64, message: &str) -> Outcome {
    let diagnostic = Diagnostic {
        severity: Severity::Error,
        message: message.to_owned(),
        origin: Some("retex".to_owned()),
        file: None,
        line: None,
        column: None,
        end_column: None,
        help: Vec::new(),
        fixes: Vec::new(),
    };
    Outcome { status: Status::Failed, diagnostics: vec![diagnostic], ..Outcome::unavailable(job, "") }
}

/// The project path of a file retex names: relative to the main document's folder, or absolute
/// inside the build's copy of the project (`src`). Files elsewhere (the TeX tree) have none.
fn project_file(file: &str, src: &Path, main_dir: Option<&str>) -> Option<String> {
    let path = Path::new(file);
    if path.is_absolute() {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let rel = canonical.strip_prefix(src).ok()?;
        return Some(rel.to_string_lossy().replace('\\', "/"));
    }
    Some(match main_dir {
        Some(dir) => format!("{dir}/{file}"),
        None => file.to_owned(),
    })
}

fn convert(d: &retex::Diagnostic, src: &Path, main_dir: Option<&str>) -> Diagnostic {
    let file = d.file.as_deref().and_then(|f| project_file(f, src, main_dir));
    if d.file.is_some() && file.is_none() {
        log::debug!("diagnostic outside the project: {:?}", d.file);
    }
    Diagnostic {
        severity: match d.severity {
            retex::Severity::Hint => Severity::Hint,
            retex::Severity::Note => Severity::Note,
            retex::Severity::Warning => Severity::Warning,
            retex::Severity::Error => Severity::Error,
        },
        message: d.message.clone(),
        origin: d.origin.clone(),
        file,
        line: d.line,
        column: d.column,
        end_column: d.end_column,
        help: d.help.clone(),
        fixes: d.fixes.iter().map(|f| DiagnosticFix { column: f.column, end_column: f.end_column, replacement: f.replacement.clone() }).collect(),
    }
}

fn event(p: retex::Progress) -> Option<Event> {
    Some(match p {
        retex::Progress::Pass { n, reason } => Event::Pass { n, reason },
        retex::Progress::Pages { shipped, expected } => Event::Pages { shipped, expected },
        retex::Progress::File { name } => Event::File { name },
        retex::Progress::Step { what } => Event::Step { what },
        _ => return None,
    })
}
