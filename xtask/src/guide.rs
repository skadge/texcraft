//! `cargo xtask guide-check`: the user guide (`docs/guide/`) is static HTML, so nothing else checks
//! it. Every page must say its language, have a title, one `h1` and a `main`; every image must
//! have alternative text; ids must be unique on a page; every link to a file of the repository
//! must reach it, and a link to `page.html#id` must reach an element with that id there; and the
//! overview must link every page. Links to other sites are not fetched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

/// The values of `attr="…"` in `html`, in order.
fn values<'a>(html: &'a str, attr: &str) -> Vec<&'a str> {
    let needle = format!(" {attr}=\"");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        match rest.find('"') {
            Some(end) => {
                out.push(&rest[..end]);
                rest = &rest[end..];
            }
            None => break,
        }
    }
    out
}

/// The text between `open` and `close` after the first `open`, if both are there.
fn between<'a>(html: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = html.find(open)? + open.len();
    let end = html[start..].find(close)? + start;
    Some(&html[start..end])
}

/// `path` with `.` and `..` resolved without touching the disk.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The problems of the pages in `dir` (empty when there are none).
pub fn problems(dir: &Path) -> Result<Vec<String>, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut pages = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "html") {
            let text = read(&path)?;
            pages.insert(normalise(&path), text);
        }
    }
    if pages.is_empty() {
        return Err(format!("{} has no pages", dir.display()));
    }
    let ids: BTreeMap<&PathBuf, BTreeSet<&str>> = pages.iter().map(|(p, html)| (p, values(html, "id").into_iter().collect())).collect();
    let mut found: Vec<String> = pages.iter().flat_map(|(path, html)| page_problems(path, html, dir, &ids)).collect();
    let index = normalise(&dir.join("index.html"));
    match pages.get(&index) {
        Some(html) => {
            let linked: BTreeSet<PathBuf> =
                values(html, "href").into_iter().map(|l| normalise(&dir.join(l.split('#').next().unwrap_or(l)))).collect();
            for path in pages.keys().filter(|p| **p != index && !linked.contains(*p)) {
                found.push(format!("index.html does not link {}", path.display()));
            }
        }
        None => found.push("there is no index.html".into()),
    }
    Ok(found)
}

/// The problems of one page: its structure, its ids, its images and its links.
fn page_problems(path: &Path, html: &str, dir: &Path, ids: &BTreeMap<&PathBuf, BTreeSet<&str>>) -> Vec<String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut found = Vec::new();
    let mut say = |what: String| found.push(format!("{name}: {what}"));
    if !html.trim_start().to_ascii_lowercase().starts_with("<!doctype html>") {
        say("does not start with <!doctype html>".into());
    }
    if between(html, "<html lang=\"", "\"").is_none_or(str::is_empty) {
        say("the html element has no lang".into());
    }
    if between(html, "<title>", "</title>").is_none_or(|t| t.trim().is_empty()) {
        say("no title".into());
    }
    if html.matches("<h1").count() != 1 {
        say("not exactly one h1".into());
    }
    if !html.contains("<main") {
        say("no main element".into());
    }
    let mut seen = BTreeSet::new();
    for id in values(html, "id") {
        if !seen.insert(id) {
            say(format!("the id {id} is used twice"));
        }
    }
    for img in html.split("<img").skip(1) {
        let tag = img.split('>').next().unwrap_or_default();
        if values(tag, "alt").first().is_none_or(|a| a.trim().is_empty()) {
            say(format!("an image without alternative text: <img{tag}>"));
        }
    }
    let base = path.parent().unwrap_or(dir);
    for link in values(html, "href").into_iter().chain(values(html, "src")) {
        if link.contains("://") || link.starts_with("mailto:") || link.is_empty() {
            continue;
        }
        let (file, anchor) = link.split_once('#').map_or((link, None), |(f, a)| (f, Some(a)));
        let target = if file.is_empty() { path.to_path_buf() } else { normalise(&base.join(file)) };
        if !target.exists() {
            say(format!("{link}: no such file"));
            continue;
        }
        if let Some(anchor) = anchor {
            match ids.get(&target) {
                Some(on) if on.contains(anchor) => {}
                Some(_) => say(format!("{link}: no element with the id {anchor}")),
                None => say(format!("{link}: an anchor into a file that is not a page of the guide")),
            }
        }
    }
    found
}

pub fn check() -> Result<(), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/guide");
    let found = problems(&dir)?;
    if found.is_empty() {
        eprintln!("guide: every page, link and anchor checks out");
        return Ok(());
    }
    for f in &found {
        eprintln!("guide: {f}");
    }
    Err(format!("the guide has {} problem(s)", found.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> String {
        format!("<!doctype html>\n<html lang=\"en\"><head><title>T</title></head><body><main><h1>T</h1>{body}</main></body></html>")
    }

    fn guide(pages: &[(&str, String)]) -> Vec<String> {
        // A folder per call: the tests run in parallel in one process.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("texcraft-guide-check-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in pages {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let found = problems(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        found
    }

    #[test]
    fn a_broken_anchor_and_a_missing_alt_are_found_and_good_links_pass() {
        let index = page(r##"<a href="b.html#there">b</a> <a href="#top">top</a> <img src="b.html" alt="x"> <p id="top"></p>"##);
        assert!(guide(&[("index.html", index.clone()), ("b.html", page(r#"<p id="there"></p>"#))]).is_empty());

        let bad = page(r##"<a href="b.html#gone">b</a> <img src="b.html"> <a href="nowhere.html">n</a>"##);
        let found = guide(&[("index.html", bad), ("b.html", page(""))]);
        assert!(found.iter().any(|f| f.contains("#gone")), "{found:?}");
        assert!(found.iter().any(|f| f.contains("alternative text")), "{found:?}");
        assert!(found.iter().any(|f| f.contains("nowhere.html")), "{found:?}");
    }

    #[test]
    fn a_page_the_overview_does_not_link_is_found() {
        let found = guide(&[("index.html", page("")), ("lost.html", page(""))]);
        assert!(found.iter().any(|f| f.contains("lost.html")), "{found:?}");
    }
}
