//! TeXCraft's own catalogs (`i18n/<locale>.tsv`, until the cutover to craft-i18n) follow
//! craft-i18n's catalog format, so a mistake is caught here rather than by craft-i18n's CI after
//! the move: the header, three columns, known contexts and escapes, the same placeholders and
//! trailing "…" in both columns, a translation in every row, each row once, plural forms as many as
//! the language has, templates the matcher can use, and the language's name.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

fn catalogs() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../i18n");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let code = path.file_name()?.to_str()?.strip_suffix(".tsv")?.to_owned();
            Some((code, std::fs::read_to_string(&path).ok()?))
        })
        .collect();
    out.sort();
    out
}

fn placeholders(s: &str) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        out.insert(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
    }
    out
}

/// What is wrong with one column's escapes, if anything.
fn bad_escape(s: &str) -> Option<String> {
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 't' | '\\') => {}
                other => return Some(format!("unknown escape \\{}", other.map(String::from).unwrap_or_default())),
            }
        }
    }
    None
}

/// The problems of a `@msg` template's English, as craft-i18n's CI rejects them.
fn bad_template(source: &str) -> Option<&'static str> {
    if !source.contains('{') {
        return None;
    }
    let opens = source.matches('{').count();
    if opens != source.matches('}').count() {
        return Some("a { with no }");
    }
    if source.contains("}{") {
        return Some("two placeholders in a row");
    }
    let mut literal = source.to_owned();
    for p in placeholders(source) {
        literal = literal.replace(&format!("{{{p}}}"), "");
    }
    literal.trim().is_empty().then_some("no literal text")
}

#[test]
fn every_catalog_follows_the_catalog_format() {
    let all = catalogs();
    assert!(!all.is_empty(), "no catalogs in i18n/");
    let mut problems = Vec::new();
    let offered: Vec<&str> = texcraft_ui_egui::i18n::LANGUAGES.iter().map(|l| l.code).collect();
    for (code, text) in &all {
        let file = format!("i18n/{code}.tsv");
        if code != &code.to_ascii_lowercase() {
            problems.push(format!("{file}: the locale is not lowercase"));
        }
        if !offered.contains(&code.as_str()) {
            problems.push(format!("{file}: {code} is not in i18n::LANGUAGES"));
        }
        if !text.lines().any(|l| l.starts_with("# Clean room:")) {
            problems.push(format!("{file}: the header has no `# Clean room:` line"));
        }
        let mut seen = HashSet::new();
        let mut named = false;
        for (i, line) in text.lines().enumerate() {
            let at = format!("{file}:{}", i + 1);
            let line = line.trim_start_matches('\u{feff}').trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [context, source, translation] = cols[..] else {
                problems.push(format!("{at}: {} columns, not 3", cols.len()));
                continue;
            };
            if context.starts_with('@') && !["@plural", "@id", "@msg"].contains(&context) {
                problems.push(format!("{at}: unknown context {context}"));
            }
            if !context.starts_with('@') && !context.chars().all(|c| c.is_ascii_alphanumeric()) {
                problems.push(format!("{at}: a context is one plain word"));
            }
            if translation.trim().is_empty() {
                problems.push(format!("{at}: an empty translation (leave the row out instead)"));
            }
            for col in [source, translation] {
                if let Some(e) = bad_escape(col) {
                    problems.push(format!("{at}: {e}"));
                }
            }
            if !seen.insert((context, source)) {
                problems.push(format!("{at}: ({context}, {source}) is in the catalog twice"));
            }
            if (context, source) == ("@id", "language.name") {
                named = true;
            }
            if context != "@id" && placeholders(source) != placeholders(translation) {
                problems.push(format!("{at}: the placeholders differ between the English and the translation"));
            }
            if context != "@id" && source.ends_with('…') != translation.ends_with('…') {
                problems.push(format!("{at}: a trailing … in one column needs one in the other"));
            }
            if context == "@msg"
                && let Some(why) = bad_template(source)
            {
                problems.push(format!("{at}: a template the apps could never match ({why})"));
            }
            if context == "@plural" && source.split('|').count() != 2 {
                problems.push(format!("{at}: a plural's English is `one|other`"));
            }
        }
        if !named {
            problems.push(format!("{file}: no `@id\tlanguage.name` row"));
        }
    }
    // Malformed rows, unknown contexts and plural form counts per language, as the app reads them.
    let embedded: Vec<(&'static str, &'static str, &'static str)> = all
        .iter()
        .map(|(c, t)| -> (&'static str, &'static str, &'static str) {
            (Box::leak(c.clone().into_boxed_str()), "", Box::leak(t.clone().into_boxed_str()))
        })
        .collect();
    let embedded: &'static craft_i18n::Embedded = Box::leak(embedded.into_boxed_slice());
    let (i18n, skipped) = craft_i18n::I18n::new(embedded, &offered);
    problems.extend(skipped);
    problems.extend(i18n.check_all());
    assert!(problems.is_empty(), "catalog problems:\n{}", problems.join("\n"));
}
