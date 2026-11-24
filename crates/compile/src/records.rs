//! retex's `--message-format=json` records (a JSON object per line of its standard output), read
//! into this crate's diagnostics, progress events and outcome. The browser build runs retex's
//! command line in a WebAssembly module and only has these, where the desktop build has the
//! library's own types.

use std::sync::Arc;

use serde_json::Value;

use crate::{Diagnostic, DiagnosticFix, Event, Outcome, Severity, Status};

/// One line of retex's output, read.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Diagnostic(Diagnostic),
    Event(Event),
    /// A pass ended and pass `n` follows, because these files changed: the PDF of the pass that
    /// ended is a draft worth showing.
    Pass {
        n: u32,
        reason: Option<String>,
    },
    Symbols(Value),
    /// The build is over: pages written.
    Finished {
        pages: u32,
    },
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn number(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(Value::as_f64).filter(|n| *n >= 0.0).map(|n| n as u32)
}

fn texts(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

/// Where the browser build puts the project's files (crates/compile/src/web_engine.rs).
const PROJECT_ROOT: &str = "/project/";

/// The project path of a file retex names relative to the main document's folder (`../shared/x.tex`
/// too) or under the browser's project folder; other absolute paths (the TeX tree,
/// `/texlive/...`) and paths above the project have none.
fn project_file(file: &str, main_dir: Option<&str>) -> Option<String> {
    let joined = match (file.strip_prefix(PROJECT_ROOT), file.starts_with('/'), main_dir) {
        (Some(inside), _, _) => inside.to_owned(),
        (None, true, _) => return None,
        (None, false, Some(dir)) => format!("{dir}/{file}"),
        (None, false, None) => file.to_owned(),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn diagnostic(v: &Value, main_dir: Option<&str>) -> Option<Diagnostic> {
    let severity = match v.get("severity")?.as_str()? {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" => Severity::Note,
        _ => Severity::Hint,
    };
    let fixes = v
        .get("fixes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(DiagnosticFix {
                column: number(f, "column")?,
                end_column: number(f, "end_column")?,
                replacement: f.get("replacement")?.as_str()?.to_owned(),
                title: text(f, "title"),
            })
        })
        .collect();
    Some(Diagnostic {
        severity,
        message: v.get("message")?.as_str()?.to_owned(),
        origin: text(v, "origin"),
        file: text(v, "file").and_then(|f| project_file(&f, main_dir)),
        line: number(v, "line"),
        column: number(v, "column"),
        end_column: number(v, "end_column"),
        help: texts(v, "help"),
        fixes,
    })
}

/// Reads one line of retex's output; `None` for anything that is not a record this crate uses.
/// `main_dir`: the main document's folder in the project, which retex's relative names start
/// from.
pub fn parse(line: &str, main_dir: Option<&str>) -> Option<Record> {
    if !line.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    match v.get("type")?.as_str()? {
        "diagnostic" => diagnostic(&v, main_dir).map(Record::Diagnostic),
        "progress" => {
            if let Some(name) = text(&v, "file") {
                Some(Record::Event(Event::File { name }))
            } else if let Some(what) = text(&v, "note") {
                Some(Record::Event(Event::Step { what }))
            } else {
                Some(Record::Event(Event::Pages { shipped: number(&v, "pages")?, expected: number(&v, "expected") }))
            }
        }
        "pass" => {
            let names: Vec<String> = texts(&v, "reason").iter().map(|f| f.rsplit('/').next().unwrap_or(f).to_owned()).collect();
            let reason = (!names.is_empty()).then(|| format!("{} changed", names.join(", ")));
            Some(Record::Pass { n: number(&v, "pass")?, reason })
        }
        "symbols" => Some(Record::Symbols(v)),
        "finished" => Some(Record::Finished { pages: number(&v, "pages").unwrap_or(0) }),
        _ => None,
    }
}

/// The outcome of a run of retex's command line, from its whole output and the files it
/// wrote.
pub fn outcome(job: u64, output: &str, main_dir: Option<&str>, pdf: Option<Vec<u8>>, synctex: Option<Vec<u8>>, seconds: f64) -> Outcome {
    let mut diagnostics = Vec::new();
    let mut symbols = None;
    let mut pages = 0;
    let mut log = String::new();
    for line in output.lines() {
        match parse(line, main_dir) {
            Some(Record::Diagnostic(d)) => diagnostics.push(d),
            Some(Record::Symbols(v)) => symbols = Some(v),
            Some(Record::Finished { pages: n }) => pages = n,
            Some(_) => {}
            None => {
                log.push_str(line);
                log.push('\n');
            }
        }
    }
    let pdf = pdf.filter(|p| !p.is_empty());
    Outcome {
        job,
        status: if pdf.is_some() { Status::Built } else { Status::Failed },
        pdf: pdf.map(Arc::new),
        diagnostics,
        log,
        seconds,
        pages,
        symbols,
        synctex: synctex.map(Arc::new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Records as retex 2.4.1 writes them (its json.rs and progress.rs).
    const RUN: &str = r#"This is retex
{"type":"progress","pass":1,"pages":0,"file":"/texlive/texmf-dist/tex/latex/base/article.cls"}
{"type":"progress","pass":1,"pages":1,"expected":2}
{"type":"pass","pass":2,"reason":[".retex/doc-6924a2cb/doc.aux"]}
{"type":"diagnostic","severity":"warning","message":"undefined reference `x`","origin":null,"file":"./doc.tex","line":3,"column":12,"end_column":13,"source":"Hello","notes":[],"help":["did you mean `a`?"],"fixes":[{"column":12,"end_column":13,"replacement":"a","title":"Use a"}]}
{"type":"diagnostic","severity":"error","message":"File `nope.sty' not found","origin":"LaTeX","file":"/texlive/texmf-dist/tex/latex/base/latex.ltx","line":10,"notes":[],"help":[],"fixes":[]}
{"type":"symbols","files":[]}
{"type":"finished","status":"errors","pages":2,"seconds":0.3}
"#;

    #[test]
    fn a_run_s_records_become_its_outcome() {
        let o = outcome(7, RUN, Some("paper"), Some(b"%PDF".to_vec()), None, 1.0);
        assert_eq!(o.status, Status::Built);
        assert_eq!((o.errors(), o.warnings(), o.pages), (1, 1, 2));
        let w = &o.diagnostics[0];
        assert_eq!(w.file.as_deref(), Some("paper/doc.tex"), "relative to the main document's folder");
        assert_eq!((w.line, w.column, w.end_column), (Some(3), Some(12), Some(13)));
        assert_eq!(w.fixes[0].replacement, "a");
        assert_eq!(o.diagnostics[1].file, None, "a file of the TeX tree is not in the project");
        assert!(o.symbols.is_some());
        assert!(o.log.contains("This is retex") && !o.log.contains("\"type\""), "the log keeps the text, not the records");
    }

    #[test]
    fn files_outside_the_main_folder_keep_their_project_path() {
        assert_eq!(project_file("../shared/macros.tex", Some("paper")).as_deref(), Some("shared/macros.tex"));
        assert_eq!(project_file("./doc.tex", Some("paper")).as_deref(), Some("paper/doc.tex"));
        assert_eq!(project_file("/project/shared/macros.tex", Some("paper")).as_deref(), Some("shared/macros.tex"));
        assert_eq!(project_file("../../outside.tex", Some("paper")), None, "above the project");
        assert_eq!(project_file("/texlive/texmf-dist/tex/latex/base/article.cls", None), None);
    }

    #[test]
    fn no_pdf_is_a_failed_build() {
        assert_eq!(outcome(1, RUN, None, None, None, 0.0).status, Status::Failed);
        assert_eq!(outcome(1, RUN, None, Some(Vec::new()), None, 0.0).status, Status::Failed);
    }

    #[test]
    fn progress_says_what_is_read_then_the_pages() {
        let events: Vec<_> = RUN.lines().filter_map(|l| parse(l, None)).collect();
        assert!(matches!(&events[0], Record::Event(Event::File { name }) if name.ends_with("article.cls")));
        assert_eq!(events[1], Record::Event(Event::Pages { shipped: 1, expected: Some(2) }));
        assert_eq!(events[2], Record::Pass { n: 2, reason: Some("doc.aux changed".into()) });
    }
}
