//! retex linked in process, through its embedding API (`retex::Retex`, retex's
//! `crates/retex-cli/src/embed.rs`).
//!
//! The project is written to `<cache>/src/` and built there with the build files kept in
//! `<cache>/out/`, so the auxiliary files of one build (`.aux`, `.toc`, `.bbl`) are read by the
//! next and a rebuild usually needs one pass. Known limits, tracked in docs/plan.md under "retex
//! changes": the embedding API serialises builds process-wide, cannot be cancelled, and returns
//! neither the SyncTeX data nor the quick fixes of its diagnostics.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::{Backend, Diagnostic, Job, Outcome, Severity, Status};

pub struct Retex {
    dir: PathBuf,
}

impl Retex {
    /// Builds in `cache_dir`, or in a folder of the system's temporary directory.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let dir = cache_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("texcraft-build-{}", std::process::id())));
        Self { dir }
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
        let started = Instant::now();
        let main = match self.write_sources(job) {
            Ok(m) => m,
            Err(e) => return Outcome { log: e.clone(), ..Outcome::unavailable(job.id, e) },
        };
        let main_dir = job.main.as_str().rsplit_once('/').map(|(d, _)| d.to_owned());
        let place = |d: &retex::Diagnostic| convert(d, main_dir.as_deref());
        let result = retex::Retex::new().keep_files(self.dir.join("out")).compile_file(&main);
        let seconds = started.elapsed().as_secs_f64();
        match result {
            Ok(doc) => Outcome {
                job: job.id,
                status: Status::Built,
                pdf: Some(Arc::new(doc.pdf().to_vec())),
                diagnostics: doc.diagnostics().iter().map(place).collect(),
                log: doc.log().to_owned(),
                seconds,
                pages: doc.pages(),
            },
            Err(retex::Error::Failed { diagnostics, log }) => Outcome {
                job: job.id,
                status: Status::Failed,
                pdf: None,
                diagnostics: diagnostics.iter().map(place).collect(),
                log,
                seconds,
                pages: 0,
            },
            Err(other) => Outcome { job: job.id, status: Status::Failed, log: other.to_string(), seconds, ..failure(job.id, &other.to_string()) },
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
        help: Vec::new(),
    };
    Outcome { job, status: Status::Failed, pdf: None, diagnostics: vec![diagnostic], log: String::new(), seconds: 0.0, pages: 0 }
}

/// retex names files relative to the main document's folder; the project names them from its root.
fn convert(d: &retex::Diagnostic, main_dir: Option<&str>) -> Diagnostic {
    let file = d.file.as_ref().filter(|f| !Path::new(f).is_absolute()).map(|f| match main_dir {
        Some(dir) => format!("{dir}/{f}"),
        None => f.clone(),
    });
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
        help: d.help.clone(),
    }
}
