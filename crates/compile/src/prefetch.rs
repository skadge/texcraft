//! What the browser's engine fetches before a project's first build, and what it tells the editor
//! while a build stops and starts again for files (#16; `web_engine.rs`).
//!
//! WASI's reads are synchronous: a file the engine has not fetched yet stops the run, and the run
//! starts again from the start once the file is there, reading every file again. A first build of
//! a document using packages beyond a plain article's then stopped once per package folder. So
//! before a project's first build, the engine fetches together what the document's class and
//! packages read ([`loads`]: the tree's read list of each, `seed/reads/class/NAME.txt` and
//! `seed/reads/package/NAME.txt`, made by `cargo xtask tree-site`; a package without one is found
//! by its file's name, and what that file loads in turn), and while a run that started again reads
//! the files the last one read, the editor keeps saying that files are being fetched
//! ([`Reported`]) instead of showing the same file names again.

use std::collections::HashSet;

/// What `text` (a document, a class or a package) loads: `class/NAME` for `\documentclass` and
/// `\LoadClass`, `package/NAME` for `\usepackage` and `\RequirePackage` (each of a list), and
/// `bst/NAME` for `\bibliographystyle`, in their order, each once. Comments are skipped; names
/// made by macros (`\RequirePackage{\x}`) are not names.
pub fn loads(text: &str) -> Vec<String> {
    const COMMANDS: [(&str, &str); 7] = [
        ("documentclass", "class"),
        ("LoadClass", "class"),
        ("LoadClassWithOptions", "class"),
        ("usepackage", "package"),
        ("RequirePackage", "package"),
        ("RequirePackageWithOptions", "package"),
        ("bibliographystyle", "bst"),
    ];
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = uncommented(line);
        let mut rest = line;
        while let Some(at) = rest.find('\\') {
            rest = &rest[at + 1..];
            let name_len = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
            let Some((_, kind)) = COMMANDS.iter().find(|(c, _)| *c == &rest[..name_len]) else { continue };
            let mut after = rest[name_len..].trim_start();
            // Options, possibly over several brackets' worth of text on this line.
            if let Some(opts) = after.strip_prefix('[') {
                let Some(end) = opts.find(']') else { continue };
                after = opts[end + 1..].trim_start();
            }
            let Some(args) = after.strip_prefix('{') else { continue };
            let Some(end) = args.find('}') else { continue };
            for name in args[..end].split(',').map(str::trim) {
                let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
                let entry = format!("{kind}/{name}");
                if plain && !out.contains(&entry) {
                    out.push(entry);
                }
            }
            rest = &args[end..];
        }
    }
    out
}

/// `line` up to its comment: the first `%` not escaped by a backslash.
fn uncommented(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' => return &line[..i],
            _ => i += 1,
        }
    }
    line
}

/// The file a [`loads`] entry names: `package/x` → `x.sty`, `class/x` → `x.cls`, `bst/x` → `x.bst`.
pub fn file_name(entry: &str) -> Option<String> {
    let (kind, name) = entry.split_once('/')?;
    let ext = match kind {
        "package" => "sty",
        "class" => "cls",
        "bst" => "bst",
        _ => return None,
    };
    Some(format!("{name}.{ext}"))
}

/// The [`loads`] entry of a file of the tree, by its name (`natbib.sty` → `package/natbib`): when
/// a run stops for a package's file, what that package reads is fetched with it.
pub fn entry_of(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    let kind = match ext {
        "sty" => "package",
        "cls" => "class",
        _ => return None,
    };
    Some(format!("{kind}/{stem}"))
}

/// Which of retex's progress lines a build's runs pass on to the editor. A run that started again
/// after a stop for files reads the files the runs before it read, and starts its first pass
/// again: those lines are not passed on, so the editor goes on saying files are being fetched
/// until the run gets further than the last.
#[derive(Debug, Default)]
pub struct Reported {
    files: HashSet<String>,
    pass: u32,
}

impl Reported {
    /// Whether to pass `line` (a `progress` or `pass` record of retex's) on: a file not read
    /// before in this build, a pass after the last passed on, and anything else.
    pub fn fresh(&mut self, line: &str) -> bool {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return true };
        match v.get("type").and_then(serde_json::Value::as_str) {
            Some("progress") => match v.get("file").and_then(serde_json::Value::as_str) {
                Some(file) => self.files.insert(file.to_owned()),
                None => true,
            },
            Some("pass") => {
                let n = v.get("pass").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
                let fresh = n > self.pass;
                self.pass = self.pass.max(n);
                fresh
            }
            _ => true,
        }
    }
}

/// The progress record that says `done` of `total` files are fetched (`{"type":"progress",
/// "fetching":…}`, read as [`crate::Event::Fetching`]).
pub fn fetching_record(done: usize, total: usize) -> String {
    format!("{{\"type\":\"progress\",\"fetching\":{total},\"fetched\":{done}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_documents_class_packages_and_bibliography_style_are_found_and_comments_are_not() {
        let doc = "\\documentclass[11pt,a4paper]{article}\n\\usepackage[utf8]{inputenc}\n\\usepackage{amsmath, booktabs}\n% \\usepackage{natbib}\n\\usepackage{graphicx} % \\usepackage{hyperref}\n\\RequirePackage{\\x}\n\\usepackage{amsmath}\n50\\% \\usepackage{tikz}\n\\bibliographystyle{plain}\n";
        assert_eq!(
            loads(doc),
            ["class/article", "package/inputenc", "package/amsmath", "package/booktabs", "package/graphicx", "package/tikz", "bst/plain"]
        );
    }

    #[test]
    fn a_restarted_run_passes_on_only_what_the_last_did_not_read() {
        let mut r = Reported::default();
        let pass = "{\"type\":\"pass\",\"pass\":1}";
        let natbib = "{\"type\":\"progress\",\"pass\":1,\"pages\":0,\"file\":\"/texlive/texmf-dist/tex/latex/natbib/natbib.sty\"}";
        let tikz = "{\"type\":\"progress\",\"pass\":1,\"pages\":0,\"file\":\"/texlive/texmf-dist/tex/latex/pgf/frontendlayer/tikz.sty\"}";
        assert!(r.fresh(pass) && r.fresh(natbib));
        // The run stopped for a file and started again.
        assert!(!r.fresh(pass) && !r.fresh(natbib), "the second run's first pass and files are said again");
        assert!(r.fresh(tikz));
        assert!(r.fresh("{\"type\":\"pass\",\"pass\":2}"), "a later pass is not said");
        assert!(r.fresh("{\"type\":\"progress\",\"pass\":1,\"pages\":1}"), "pages are not said");
    }
}
