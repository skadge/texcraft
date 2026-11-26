//! retex's `--message-format=json` records (a JSON object per line of its standard output), read
//! into this crate's diagnostics, progress events and outcome. The browser build runs retex's
//! command line in a WebAssembly module and only has these, where the desktop build has the
//! library's own types.

use std::sync::Arc;

use serde_json::Value;

use crate::{Diagnostic, DiagnosticFix, Event, Options, Outcome, Output, PdfxRule, Preflight, PrintCheck, Severity, Status};

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
    /// A print check of the PDF/X.
    PrintCheck(PrintCheck),
    /// The PDF/X's rules and their problems.
    Pdfx(Vec<PdfxRule>),
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
pub(crate) fn project_file(file: &str, main_dir: Option<&str>) -> Option<String> {
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

fn severity(v: &Value) -> Option<Severity> {
    Some(match v.get("severity")?.as_str()? {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" => Severity::Note,
        _ => Severity::Hint,
    })
}

fn diagnostic(v: &Value, main_dir: Option<&str>) -> Option<Diagnostic> {
    let severity = severity(v)?;
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
    let message = v.get("message")?.as_str()?.to_owned();
    Some(Diagnostic {
        severity,
        origin: crate::origin(text(v, "origin"), &message),
        message,
        file: text(v, "file").and_then(|f| project_file(&f, main_dir)),
        line: number(v, "line"),
        column: number(v, "column"),
        end_column: number(v, "end_column"),
        help: texts(v, "help"),
        fixes,
        noticed: None,
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
        "print_check" => print_check(&v, main_dir).map(Record::PrintCheck),
        "pdfx" => Some(Record::Pdfx(
            v.get("rules")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| Some(PdfxRule { rule: text(r, "rule")?, problems: texts(r, "problems") }))
                .collect(),
        )),
        _ => None,
    }
}

/// A `print_check` record (retex's `json.rs`): its page from 1, its area in points.
fn print_check(v: &Value, main_dir: Option<&str>) -> Option<PrintCheck> {
    let area = v.get("area");
    let f = |k: &str| area.and_then(|a| a.get(k)).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Some(PrintCheck {
        kind: text(v, "kind")?,
        title: text(v, "title").unwrap_or_default(),
        severity: severity(v).unwrap_or(Severity::Note),
        page: number(v, "page")?.saturating_sub(1),
        x: f("x"),
        y: f("y"),
        width: f("width"),
        height: f("height"),
        count: number(v, "count").unwrap_or(0),
        value: v.get("value").and_then(Value::as_f64).unwrap_or(0.0),
        limit: v.get("limit").and_then(Value::as_f64),
        detail: text(v, "detail"),
        file: text(v, "file").and_then(|f| project_file(&f, main_dir)),
        line: number(v, "line"),
        image: text(v, "image"),
    })
}

/// The files a run of retex's command line wrote.
#[derive(Debug, Default)]
pub struct Written {
    pub pdf: Option<Vec<u8>>,
    pub synctex: Option<Vec<u8>>,
    /// A print build's PDF/X.
    pub pdfx: Option<Vec<u8>>,
}

/// The outcome of a run of retex's command line with `options`, from its whole output and the
/// files it wrote.
pub fn outcome(job: u64, output: &str, main_dir: Option<&str>, options: &Options, written: Written, seconds: f64) -> Outcome {
    let Written { pdf, synctex, pdfx } = written;
    let mut diagnostics = Vec::new();
    let mut symbols = None;
    let mut pages = 0;
    let mut log = String::new();
    let mut checks = Vec::new();
    let mut rules = Vec::new();
    for line in output.lines() {
        match parse(line, main_dir) {
            Some(Record::Diagnostic(d)) => diagnostics.push(d),
            Some(Record::Symbols(v)) => symbols = Some(v),
            Some(Record::Finished { pages: n }) => pages = n,
            Some(Record::PrintCheck(c)) => checks.push(c),
            Some(Record::Pdfx(r)) => rules = r,
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
        aux: None,
        output: options.output,
        preflight: (options.output == Output::Print).then(|| Preflight {
            standard: options.pdfx,
            pdfx: pdfx.filter(|p| !p.is_empty()).map(Arc::new),
            checks,
            rules,
        }),
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

    fn pdf(bytes: &[u8]) -> Written {
        Written { pdf: Some(bytes.to_vec()), ..Written::default() }
    }

    #[test]
    fn a_run_s_records_become_its_outcome() {
        let o = outcome(7, RUN, Some("paper"), &Options::default(), pdf(b"%PDF"), 1.0);
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

    // A print build's records, as retex 2.5.0 writes them (`-output-format=pdf,pdf-x4`).
    const PRINT: &str = r#"{"type":"pdfx","file":"doc.pdf-x4.pdf","variant":"PDF/X-4","rules":[{"rule":"fonts embedded","problems":[]},{"rule":"output intent with its ICC profile","problems":["no output intent"]}],"checked":true}
{"type":"print_check","kind":"low-resolution","title":"images of 150 ppi or more","severity":"warning","page":2,"area":{"x":148.71,"y":135.87,"width":226.96,"height":226.96},"count":1,"value":5.076,"limit":150,"detail":null,"file":"./doc.tex","line":6,"image":"low.png"}
{"type":"diagnostic","severity":"warning","message":"page 2: image low.png prints at 5 ppi, below 150 ppi","origin":"print","file":"doc.tex","line":6,"column":null,"end_column":null,"source":null,"notes":[],"help":[],"fixes":[]}
{"type":"diagnostic","severity":"warning","message":"image `low.png` has no alternative text","origin":null,"file":"doc.tex","line":6,"column":null,"end_column":null,"source":null,"notes":[],"help":[],"fixes":[]}
{"type":"finished","status":"ok","output":"doc.pdf","pages":2,"also_output":"doc.pdf-x4.pdf","seconds":0.5}
"#;

    #[test]
    fn a_print_run_s_checks_are_placed_on_the_page_and_in_the_source() {
        let options = Options { output: Output::Print, ..Options::default() };
        let written = Written { pdfx: Some(b"%PDF-x".to_vec()), ..pdf(b"%PDF") };
        let o = outcome(1, PRINT, Some("paper"), &options, written, 0.0);
        let preflight = o.preflight.as_ref().expect("a print build has its preflight");
        let check = &preflight.checks[0];
        assert_eq!((check.page, check.file.as_deref(), check.line), (1, Some("paper/doc.tex"), Some(6)), "pages from 0, files in the project");
        assert_eq!(preflight.rules[1].problems, ["no output intent"]);
        assert_eq!(o.place_on_page(&o.diagnostics[0]), Some(check));
        assert_eq!(o.print_problems(), Some(1));
        assert!(o.diagnostics[1].is_accessibility(), "a rewritten tagging message is not an accessibility finding");
        assert!(o.place_on_page(&o.diagnostics[1]).is_none());
        let screen = outcome(1, PRINT, None, &Options::default(), pdf(b"%PDF"), 0.0);
        assert!(screen.preflight.is_none(), "a screen build has no preflight");
    }

    #[test]
    fn a_print_message_without_a_line_is_placed_by_its_pages() {
        let options = Options { output: Output::Print, ..Options::default() };
        let line = |page: u32, message: &str| {
            format!(
                "{{\"type\":\"print_check\",\"kind\":\"no-bleed\",\"page\":{page},\"area\":{{\"x\":1,\"y\":1,\"width\":1,\"height\":1}}}}\n{{\"type\":\"diagnostic\",\"severity\":\"warning\",\"message\":\"{message}\",\"origin\":\"print\"}}\n"
            )
        };
        for (page, message, placed) in [(3, "pages 2–4: art stops short", true), (5, "pages 2, 5 and 7: art", true), (6, "pages 2–4: art", false)]
        {
            let o = outcome(1, &line(page, message), None, &options, pdf(b"%PDF"), 0.0);
            assert_eq!(o.place_on_page(&o.diagnostics[0]).is_some(), placed, "{message} on page {page}");
        }
        // A print build that wrote no PDF/X has that problem too.
        assert_eq!(outcome(1, "", None, &options, pdf(b"%PDF"), 0.0).print_problems(), Some(1));
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
        assert_eq!(outcome(1, RUN, None, &Options::default(), Written::default(), 0.0).status, Status::Failed);
        assert_eq!(outcome(1, RUN, None, &Options::default(), pdf(b""), 0.0).status, Status::Failed);
    }

    #[test]
    fn progress_says_what_is_read_then_the_pages() {
        let events: Vec<_> = RUN.lines().filter_map(|l| parse(l, None)).collect();
        assert!(matches!(&events[0], Record::Event(Event::File { name }) if name.ends_with("article.cls")));
        assert_eq!(events[1], Record::Event(Event::Pages { shipped: 1, expected: Some(2) }));
        assert_eq!(events[2], Record::Pass { n: 2, reason: Some("doc.aux changed".into()) });
    }
}
