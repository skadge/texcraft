//! `cargo xtask i18n-report [OUT]` and `cargo xtask i18n-lint`.
//!
//! The report lists every English string the interface can show, as catalog keys
//! (`context<TAB>source`), read from the sources: `tl!`, `tf!`, `tn!` and `tc` literals and `trn`
//! pairs (`@plural`) in `crates/ui-egui/src`, `measure` units (`unit`), command labels (`@id`, from
//! the engine registry and the view commands, the English label in a third column), and the
//! engine's messages (`@msg`: errors and status text of the crates below the interface, which stay
//! English for the CLI and agents and are shown through these rows), their `{}` placeholders
//! named after their arguments. Then, per catalog in `i18n/` (TeXCraft's own until its cutover to
//! craft-i18n), how many keys it lacks and the rows the sources no longer use. It never fails on a
//! missing translation: that string shows in English. With `OUT`, the list is also written there.
//!
//! The lint fails on an English literal handed straight to a widget (`ui.label("…")`,
//! `Button::new("…")`, a tooltip, an accessible name) and on `tl!` of anything but a literal, which
//! the report could not see.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// A catalog key: (context, source text). `note` is the English label of an `@id` row.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    context: String,
    source: String,
}

/// The crates whose messages reach the interface's status line, dialogs and tooltips.
const ENGINE_CRATES: &[&str] = &["engine", "compile", "forge", "history", "project", "review", "templates", "gitwire", "preview", "latex"];

/// A string literal: its byte range in the file and its value (escapes as written).
struct Literal {
    start: usize,
    end: usize,
    value: String,
}

/// A Rust literal's text as a catalog writes it: `\\n`, `\\t` and `\\\\` kept (the catalog's own
/// escapes), Rust's other escapes resolved.
fn catalog_text(rust: &str) -> String {
    let mut out = String::with_capacity(rust.len());
    let mut chars = rust.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(e @ ('n' | 't' | '\\')) => {
                out.push('\\');
                out.push(e);
            }
            Some('u') => {
                let code: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                out.extend(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32));
            }
            Some('0') => out.push_str("\\0"),
            Some('r') => {}
            // A line continuation: the next line's leading spaces go too.
            Some('\n') => while chars.next_if(|c| c.is_whitespace()).is_some() {},
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Every string literal of `src` outside comments, with raw strings and char literals skipped.
fn literals(src: &str) -> Vec<Literal> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let rest = &src[i..];
        if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("/*") {
            i += rest.find("*/").map_or(rest.len(), |e| e + 2);
            continue;
        }
        let prev_ident = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if b[i] == b'r' && !prev_ident {
            let hashes = rest[1..].bytes().take_while(|c| *c == b'#').count();
            if rest.as_bytes().get(1 + hashes) == Some(&b'"') {
                let close = format!("\"{}", "#".repeat(hashes));
                let body = 2 + hashes;
                let end = rest[body..].find(&close).map_or(rest.len(), |e| body + e + close.len());
                i += end;
                continue;
            }
        }
        if b[i] == b'\'' {
            // A char literal ('a', '\n', '\u{…}'), else a lifetime.
            if rest[1..].starts_with('\\') {
                i += rest[2..].find('\'').map_or(1, |e| e + 3);
            } else {
                let width = rest[1..].chars().next().map_or(0, char::len_utf8);
                i += if rest.as_bytes().get(1 + width) == Some(&b'\'') { width + 2 } else { 1 };
            }
            continue;
        }
        if b[i] == b'"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(b.len());
            out.push(Literal { start: i, end, value: catalog_text(&src[i + 1..j.min(b.len())]) });
            i = end;
            continue;
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// The name of the call or macro whose argument list holds `pos` (`tl`, `format`, `Err`), and where
/// its `(` is.
fn callee(src: &str, pos: usize) -> Option<(&str, usize)> {
    let b = src.as_bytes();
    let mut depth = 0usize;
    let mut i = pos;
    while i > 0 {
        i -= 1;
        match b[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'(' => {
                let head = src[..i].trim_end_matches('!');
                let name_start = head.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |s| s + 1);
                return Some((&head[name_start..], i));
            }
            b'[' | b'{' => return None,
            _ => {}
        }
    }
    None
}

/// The non-test part of a source file: everything before its first `#[cfg(test)]`.
fn non_test(src: &str) -> &str {
    src.find("#[cfg(test)]").map_or(src, |at| &src[..at])
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(list) = std::fs::read_dir(dir) else { return out };
    for entry in list.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The literal arguments of the call whose `(` is at `open`, in order, up to its `)`.
fn call_literals<'a>(lits: &'a [Literal], src: &str, open: usize) -> Vec<&'a Literal> {
    lits.iter().filter(|l| l.start > open && callee(src, l.start).is_some_and(|(_, o)| o == open)).collect()
}

/// The interface's keys: `tl!`, `tf!`, `tn!`, `t`, `tc`, `trn` and `measure` literals.
fn interface_keys(root: &Path) -> BTreeSet<Key> {
    let mut keys = BTreeSet::new();
    for path in rust_files(&root.join("crates/ui-egui/src")) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let src = non_test(&text);
        let lits = literals(src);
        for (i, l) in lits.iter().enumerate() {
            let Some((name, open)) = callee(src, l.start) else { continue };
            let first = call_literals(&lits, src, open).first().is_some_and(|f| f.start == l.start);
            let key = |context: &str, source: &str| Key { context: context.to_owned(), source: source.to_owned() };
            match name {
                "tl" | "tf" | "tn" | "t" if first && src[open + 1..l.start].trim().is_empty() => {
                    keys.insert(key("", &l.value));
                }
                "tc" if first => {
                    if let Some(source) = lits.get(i + 1).filter(|n| callee(src, n.start).is_some_and(|(_, o)| o == open)) {
                        keys.insert(key(&l.value, &source.value));
                    }
                }
                "trn" | "trn_plain" if first => {
                    if let Some(other) = lits.get(i + 1).filter(|n| callee(src, n.start).is_some_and(|(_, o)| o == open)) {
                        keys.insert(key("@plural", &format!("{}|{}", l.value, other.value)));
                    }
                }
                "measure" if first => {
                    keys.insert(key("unit", &l.value));
                }
                _ => {}
            }
        }
    }
    keys
}

/// The commands' (id, English label): the engine registry's `id: "…", label: "…"` and the view
/// commands' `ui("id", "label", …)`.
fn command_labels(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for path in rust_files(&root.join("crates/engine/src/cmd")).into_iter().chain([root.join("crates/engine/src/cmd.rs")]) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let lits = literals(non_test(&text));
        for pair in lits.windows(2) {
            let between = &text[pair[0].end..pair[1].start];
            let before = text[..pair[0].start].trim_end();
            if before.ends_with("id:") && between.trim().trim_start_matches(',').trim() == "label:" {
                out.insert(pair[0].value.clone(), pair[1].value.clone());
            }
        }
    }
    let path = root.join("crates/ui-egui/src/ui_cmd.rs");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let src = non_test(&text);
        let lits = literals(src);
        for l in &lits {
            if let Some(("ui", open)) = callee(src, l.start) {
                let args = call_literals(&lits, src, open);
                if args.first().is_some_and(|f| f.start == l.start)
                    && let Some(label) = args.get(1)
                {
                    out.insert(l.value.clone(), label.value.clone());
                }
            }
        }
    }
    out
}

/// The top-level comma-separated arguments of the call whose `(` is at `open`.
fn arguments(src: &str, open: usize) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let mut in_str = false;
    let mut chars = src[open + 1..].chars();
    while let Some(c) = chars.next() {
        if in_str {
            current.push(c);
            if c == '\\' {
                current.extend(chars.next());
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                current.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' | '}' if depth == 0 => break,
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => args.push(std::mem::take(&mut current).trim().to_owned()),
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        args.push(current.trim().to_owned());
    }
    args
}

/// A placeholder's name for a `format!` argument: the last name in `path.display()`,
/// `self.dir.to_string_lossy()`, `e`; `value` for anything else.
fn argument_name(arg: &str) -> String {
    let arg = arg.trim_start_matches('&').trim_start_matches('*');
    let mut head = arg;
    for suffix in [".display()", ".to_string()", ".to_owned()", ".as_str()", ".to_string_lossy()", ".len()", ".as_deref().unwrap_or_default()"] {
        head = head.strip_suffix(suffix).unwrap_or(head);
    }
    let name = head.rsplit(['.', ':']).next().unwrap_or(head);
    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit()) {
        name.to_owned()
    } else {
        "value".to_owned()
    }
}

/// `format!`'s template as a catalog template: `{}` and `{:?}` named after their arguments,
/// `{name:.1}` as `{name}`; `None` for one with literal braces.
fn template(format: &str, args: &[String]) -> Option<String> {
    if format.contains("{{") || format.contains("}}") {
        return None;
    }
    let mut out = String::new();
    let mut rest = format;
    let mut positional = args.iter();
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}')? + open;
        out.push_str(&rest[..open]);
        let spec = &rest[open + 1..close];
        let name = spec.split(':').next().unwrap_or_default();
        let name = if name.is_empty() { argument_name(positional.next().map_or("value", String::as_str)) } else { name.to_owned() };
        let _ = write!(out, "{{{name}}}");
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Whether a literal reads as a message for people: words with spaces, not code or LaTeX.
fn reads_as_message(s: &str) -> bool {
    let words = s.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2).count();
    words >= 2 && !s.starts_with('\\') && !s.contains("\\n\\") && !s.starts_with('-') && !s.contains("://") && s.chars().any(char::is_lowercase)
}

/// Calls whose message argument is for people.
const MESSAGE_CALLS: &[&str] = &[
    "Err",
    "format",
    "Error",
    "Download",
    "Unavailable",
    "Message",
    "Some",
    "ok_or",
    "ok_or_else",
    "bail",
    "err",
    "Custom",
    "new",
    "Invalid",
    "Failed",
];

/// The engine's messages: `format!` templates and sentence literals of the crates below the
/// interface, as `@msg` keys.
fn engine_messages(root: &Path) -> BTreeSet<Key> {
    let mut keys = BTreeSet::new();
    for name in ENGINE_CRATES {
        for path in rust_files(&root.join("crates").join(name).join("src")) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let src = non_test(&text);
            let lits = literals(src);
            for l in &lits {
                if !reads_as_message(&l.value) {
                    continue;
                }
                let line_start = src[..l.start].rfind('\n').map_or(0, |n| n + 1);
                let line = &src[line_start..src[l.start..].find('\n').map_or(src.len(), |n| l.start + n)];
                if ["log::", "debug_assert", "assert!", "panic!", "#[", "expect(", "description:", "\"description\"", "doc ="]
                    .iter()
                    .any(|s| line.contains(s))
                {
                    continue;
                }
                let after = src[l.end..].trim_start();
                let called = callee(src, l.start);
                let message = match called {
                    Some(("format", open)) | Some(("write", open)) | Some(("writeln", open)) => {
                        let args = arguments(src, open);
                        let first_lit = args.iter().position(|a| a.starts_with('"'));
                        first_lit.and_then(|f| template(&l.value, args.get(f + 1..).unwrap_or_default()))
                    }
                    Some((name, _)) if MESSAGE_CALLS.contains(&name) => Some(l.value.clone()),
                    _ if src[..l.start].trim_end().ends_with('=')
                        && src[src[..l.start].rfind([';', '}']).map_or(0, |n| n + 1)..l.start].contains("const ") =>
                    {
                        Some(l.value.clone())
                    }
                    _ if after.starts_with(".into()") || after.starts_with(".to_owned()") || after.starts_with(".to_string()") => {
                        Some(l.value.clone())
                    }
                    _ => None,
                };
                // Protocol text (git objects, prompts, multi-line records) is not for people.
                let protocol = |m: &str| m.contains("\\0") || m.contains("\\n") || m.contains("<<<");
                if let Some(m) = message.filter(|m| !m.trim().is_empty() && !protocol(m)) {
                    keys.insert(Key { context: "@msg".to_owned(), source: m });
                }
            }
        }
    }
    keys
}

/// The template catalogue's titles (`templates/catalog.toml`), shown through `@msg` rows.
fn template_titles(root: &Path) -> BTreeSet<Key> {
    let text = std::fs::read_to_string(root.join("templates/catalog.toml")).unwrap_or_default();
    text.lines()
        .filter_map(|l| l.strip_prefix("title = \"")?.strip_suffix('"'))
        .map(|t| Key { context: "@msg".to_owned(), source: catalog_text(t) })
        .collect()
}

/// One catalog's keys: (context, source) of every row.
fn catalog_keys(text: &str) -> BTreeSet<Key> {
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut cols = l.splitn(3, '\t');
            Some(Key { context: cols.next()?.to_owned(), source: cols.next()?.to_owned() })
        })
        .collect()
}

pub fn report(root: &Path, out: Option<&Path>) -> Result<(), String> {
    let interface = interface_keys(root);
    let commands = command_labels(root);
    let mut messages = engine_messages(root);
    messages.extend(template_titles(root));
    if interface.is_empty() || commands.is_empty() {
        return Err("no interface strings or command labels were found: the scanner no longer reads the sources".to_owned());
    }
    // The key every lookup can reach, for the stale check: an `@id` row falls back to the label's
    // plain row, an `@msg` row is also a plain row.
    let mut all: BTreeSet<Key> = interface.iter().chain(&messages).cloned().collect();
    all.extend(commands.keys().map(|id| Key { context: "@id".to_owned(), source: id.clone() }));
    all.insert(Key { context: "@id".to_owned(), source: "language.name".to_owned() });

    let mut list = String::new();
    let _ = writeln!(list, "# TeXCraft's English interface strings, as catalog keys: context<TAB>English source text.");
    let _ = writeln!(list, "# Written by `cargo xtask i18n-report`. `@id` rows: the command id, then its English label in a third column.");
    let _ = writeln!(list, "# `@msg` rows: the engine's messages, shown in the status line and dialogs; `{{name}}` placeholders match any text.");
    let _ = writeln!(list, "# English is British English (colour, licence, organise).\n");
    let section = |list: &mut String, title: &str, keys: &mut dyn Iterator<Item = String>| {
        let _ = writeln!(list, "# {title}");
        for k in keys {
            let _ = writeln!(list, "{k}");
        }
        list.push('\n');
    };
    let escape = |s: &str| s.replace('\t', "\\t");
    section(
        &mut list,
        &format!("Interface ({})", interface.iter().filter(|k| k.context != "@plural" && k.context != "unit").count()),
        &mut interface.iter().filter(|k| k.context != "@plural" && k.context != "unit").map(|k| format!("{}\t{}", k.context, escape(&k.source))),
    );
    section(
        &mut list,
        &format!("Counts, `@plural` one|other ({})", interface.iter().filter(|k| k.context == "@plural").count()),
        &mut interface.iter().filter(|k| k.context == "@plural").map(|k| format!("@plural\t{}", escape(&k.source))),
    );
    section(&mut list, "Units", &mut interface.iter().filter(|k| k.context == "unit").map(|k| format!("unit\t{}", k.source)));
    section(&mut list, &format!("Command labels ({})", commands.len()), &mut commands.iter().map(|(id, label)| format!("@id\t{id}\t{label}")));
    section(&mut list, &format!("Engine messages ({})", messages.len()), &mut messages.iter().map(|k| format!("@msg\t{}", escape(&k.source))));

    let mut summary = String::new();
    let _ = writeln!(
        summary,
        "TeXCraft interface strings: {} plain, {} counts, {} command labels, {} engine messages.",
        interface.iter().filter(|k| k.context != "@plural").count(),
        interface.iter().filter(|k| k.context == "@plural").count(),
        commands.len(),
        messages.len()
    );
    let wanted = all.len() - 1;
    for path in rust_files_like(&root.join("i18n"), "tsv") {
        let code = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let rows = catalog_keys(&text);
        let has = |k: &Key| {
            rows.contains(k)
                || (k.context == "@id"
                    && commands.get(&k.source).is_some_and(|label| rows.contains(&Key { context: String::new(), source: label.clone() })))
                || (k.context == "@msg" && rows.contains(&Key { context: String::new(), source: k.source.clone() }))
        };
        let missing = all.iter().filter(|k| k.source != "language.name" && !has(k)).count();
        let stale: Vec<&Key> =
            rows.iter().filter(|k| !(all.contains(k) || k.context.is_empty() && commands.values().any(|l| *l == k.source))).collect();
        let _ =
            writeln!(summary, "  {code}: {} of {wanted} translated, {missing} missing (shown in English), {} unused", wanted - missing, stale.len());
        for k in stale {
            let _ = writeln!(summary, "      unused: {}\t{}", k.context, k.source);
        }
    }
    print!("{summary}");
    if let Some(out) = out {
        std::fs::write(out, &list).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("The English strings are in {}.", out.display());
    }
    Ok(())
}

fn rust_files_like(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|l| l.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == ext)).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// Widget calls whose text argument people read.
const SINKS: &[&str] = &[
    "label",
    "heading",
    "button",
    "small_button",
    "selectable_label",
    "checkbox",
    "radio_value",
    "colored_label",
    "hint_text",
    "on_hover_text",
    "on_disabled_hover_text",
    "accessible_name",
    "menu_button",
    "hyperlink_to",
    "labeled",
    "text_button",
    "icon_button",
    "title",
];

/// Words that are the same in every language: names of products and formats, and glyphs.
const KEPT: &[&str] = &["TeXCraft", "LaTeX", "pdfLaTeX", "LuaLaTeX", "XeLaTeX", "BibTeX", "TikZ", "PDF", "Aa", "B", "I", "GitLab", "GitHub"];

pub fn lint(root: &Path) -> Result<(), String> {
    let mut problems = Vec::new();
    for path in rust_files(&root.join("crates/ui-egui/src")) {
        if path.ends_with("i18n.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let src = non_test(&text);
        let lits = literals(src);
        let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        let line = |at: usize| src[..at].matches('\n').count() + 1;
        for l in &lits {
            let Some((name, open)) = callee(src, l.start) else { continue };
            // Words for people: a space or a capital first; not an icon's or a command's name.
            let words = l.value.contains(' ') || l.value.starts_with(|c: char| c.is_uppercase());
            let english = words && l.value.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !KEPT.contains(&l.value.as_str());
            if english
                && (SINKS.contains(&name)
                    || name == "new" && src[..open].ends_with("Button::new")
                    || name == "new" && src[..open].ends_with("RichText::new"))
            {
                problems.push(format!("{rel}:{}: \"{}\" goes to {name} without tl!", line(l.start), l.value));
            }
        }
        // `tl!` of a non-literal: the report cannot see the key.
        let mut rest = src;
        let mut base = 0;
        while let Some(at) = rest.find("tl!(") {
            let arg = rest[at + 4..].trim_start();
            if !arg.starts_with('"') {
                problems.push(format!("{rel}:{}: tl! of a non-literal; mark the table's strings with tn! and use i18n::t", line(base + at)));
            }
            base += at + 4;
            rest = &rest[at + 4..];
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(format!("{} interface strings bypass translation:\n{}", problems.len(), problems.join("\n"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_templates_are_named_after_their_arguments() {
        let args = vec!["path.display()".to_owned(), "e".to_owned()];
        assert_eq!(template("could not read {}: {}", &args).as_deref(), Some("could not read {path}: {e}"));
        assert_eq!(template("{n:.1} s", &[]).as_deref(), Some("{n} s"));
        assert!(template("a {{literal}}", &[]).is_none());
    }

    #[test]
    fn the_scanner_sees_through_comments_and_raw_strings() {
        let src = "// tl!(\"no\")\nlet a = r#\"x \"q\" y\"#; tl!(\"yes \\\"it\\\"\"); let c = '\"';\n";
        let found: Vec<String> = literals(src).into_iter().map(|l| l.value).collect();
        assert_eq!(found, vec!["yes \"it\"".to_owned()]);
    }
}
