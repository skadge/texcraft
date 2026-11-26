//! Build messages hidden by kind: "Overfull \hbox", BibTeX's "entry `…` has no pages", a font
//! shape substituted. A message's kind is its origin and its wording with what varies taken out
//! (what is quoted, file names, numbers), so every message made from the same template has the
//! same kind ([`kind`]). Kinds are hidden for the project (in its folder's
//! `.texcraft/hidden-messages.txt`; for the session when it has no folder, as in the browser) or
//! for every project (the user's preferences, `Settings::hidden_messages`). Errors are never
//! hidden. The log says how many messages are hidden, and lists the kinds to show again.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Value, json};
use texcraft_compile::{Diagnostic, Severity};

use crate::{Result, Session};

/// The project's hidden kinds, in its folder.
pub const PROJECT_FILE: &str = ".texcraft/hidden-messages.txt";

/// Longest kind kept, in characters.
const MAX_KIND: usize = 300;

/// What stands for a part of a message that varies.
const VARIES: &str = "…";

/// The project's hidden kinds, read from its folder once per folder; and those hidden for this
/// session in a project without one.
#[derive(Debug, Default)]
pub(crate) struct State {
    project: Option<(Option<PathBuf>, BTreeSet<String>)>,
    session: BTreeSet<String>,
}

/// Where a kind is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// This project only.
    Project,
    /// Every project (the user's preferences).
    User,
}

/// The kind of message `d`: its origin, then its wording with what is quoted, file names and
/// numbers taken out. `Overfull \hbox (12.0pt too wide) in paragraph at lines 3--5` and
/// `Overfull \hbox (3.1pt too wide) in paragraph at lines 40--41` are one kind.
pub fn kind(d: &Diagnostic) -> String {
    let origin = d.origin.as_deref().unwrap_or("TeX");
    let kind = format!("{origin}: {}", template(&d.message));
    kind.chars().take(MAX_KIND).collect()
}

/// `message` with its quoted parts, file names and numbers each replaced by "…".
pub fn template(message: &str) -> String {
    let message = message.trim();
    // BibTeX's own wording names the entry last: `empty journal in knuth84`.
    let message = match message.strip_prefix("empty ").and_then(|rest| rest.rsplit_once(" in ")) {
        Some((field, _)) => format!("empty {field} in {VARIES}"),
        None => message.to_owned(),
    };
    let unquoted = quoted_out(&message);
    let words: Vec<String> = unquoted.split_whitespace().map(|w| if is_file_name(w) { file_out(w) } else { w.to_owned() }).collect();
    numbers_out(&words.join(" "))
}

/// The text with what is between quotes replaced: TeX's `x', retex's `x`, "x", ‘x’ and “x”. An
/// unclosed quote keeps the rest as it is.
fn quoted_out(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(['`', '"', '‘', '“']) {
        let Some(open) = rest[start..].chars().next() else { break };
        let closes: &[char] = match open {
            '`' => &['\'', '`'],
            '"' => &['"'],
            '‘' => &['’'],
            _ => &['”'],
        };
        let after = &rest[start + open.len_utf8()..];
        let Some(end) = after.find(closes) else { break };
        let Some(close) = after[end..].chars().next() else { break };
        out.push_str(&rest[..start]);
        out.push(open);
        out.push_str(VARIES);
        out.push(close);
        rest = &after[end + close.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// Whether `word` (with any punctuation around it) names a file: a path, or a name with an
/// extension (`main.tex`, `fig/a.png`), not a command (`\hbox`).
fn is_file_name(word: &str) -> bool {
    let w = word.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';' | ':' | '[' | ']') || c == '.' && word.ends_with('.'));
    if w.is_empty() || w.starts_with('\\') || w.contains(VARIES) {
        return false;
    }
    if w.contains('/') {
        return true;
    }
    match w.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// `word` with the file name in it replaced, the punctuation around it kept.
fn file_out(word: &str) -> String {
    let start = word.find(|c: char| !matches!(c, '(' | '[')).unwrap_or(0);
    let end = word.rfind(|c: char| !matches!(c, ')' | ']' | ',' | ';' | ':' | '.')).map_or(word.len(), |i| i + 1);
    format!("{}{VARIES}{}", &word[..start], &word[end..])
}

/// The text with each number (`12`, `3.5`, `1,000`) replaced.
fn numbers_out(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            while i < chars.len()
                && (chars[i].is_ascii_digit() || (matches!(chars[i], '.' | ',') && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                i += 1;
            }
            out.push_str(VARIES);
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The kinds of a hidden-messages file: one a line, `#` lines being comments.
fn parse(text: &str) -> BTreeSet<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned).collect()
}

impl Session {
    /// The project's folder, when it keeps its hidden kinds there (not in the browser).
    fn hidden_dir(&self) -> Option<PathBuf> {
        self.dir.clone().filter(|_| self.library.is_none())
    }

    /// Reads the project's hidden kinds again when its folder changed. Cheap otherwise: the
    /// window calls it every frame.
    pub fn refresh_hidden(&mut self) {
        let dir = self.hidden_dir();
        if self.hidden.project.as_ref().is_some_and(|(d, _)| *d == dir) {
            return;
        }
        let kinds = dir
            .as_ref()
            .map(|d| d.join(PROJECT_FILE))
            .filter(|f| self.roots.as_ref().is_none_or(|r| r.check(f, crate::file_access::Access::Read).is_ok()))
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|t| parse(&t))
            .unwrap_or_default();
        // Kinds hidden for the session belonged to the project before.
        if self.hidden.project.is_some() {
            self.hidden.session.clear();
        }
        self.hidden.project = Some((dir, kinds));
    }

    /// The kinds hidden for this project, sorted.
    pub fn hidden_for_project(&self) -> Vec<String> {
        let mut kinds: BTreeSet<String> = self.hidden.session.clone();
        if let Some((_, k)) = &self.hidden.project {
            kinds.extend(k.iter().cloned());
        }
        kinds.into_iter().collect()
    }

    /// The kinds hidden in every project, as the preferences keep them.
    pub fn hidden_for_user(&self) -> &[String] {
        &self.settings.hidden_messages
    }

    /// Whether message `d` is hidden: not an error, and of a kind hidden here or everywhere.
    pub fn is_hidden(&self, d: &Diagnostic) -> bool {
        if d.severity == Severity::Error {
            return false;
        }
        let k = kind(d);
        self.settings.hidden_messages.contains(&k)
            || self.hidden.session.contains(&k)
            || self.hidden.project.as_ref().is_some_and(|(_, kinds)| kinds.contains(&k))
    }

    /// How many of the last build's messages are hidden.
    pub fn hidden_count(&self) -> usize {
        self.last_outcome().map_or(0, |o| o.diagnostics.iter().filter(|d| self.is_hidden(d)).count())
    }

    /// The last build's warnings that are not hidden.
    pub fn warnings_shown(&self) -> usize {
        self.last_outcome().map_or(0, |o| o.diagnostics.iter().filter(|d| d.severity == Severity::Warning && !self.is_hidden(d)).count())
    }

    /// Hides messages of `kind` in `scope`; where it is kept: `project` (its folder), `session`
    /// (a project without one) or `user`.
    pub fn hide_kind(&mut self, kind: &str, scope: Scope) -> Result<&'static str> {
        let kind = kind.trim();
        if kind.is_empty() || kind.contains('\n') {
            return Err("Name the kind of message to hide, on one line.".to_owned());
        }
        self.refresh_hidden();
        match scope {
            Scope::User => {
                if !self.settings.hidden_messages.iter().any(|k| k == kind) {
                    self.settings.hidden_messages.push(kind.to_owned());
                    self.settings.hidden_messages.sort();
                }
                Ok("user")
            }
            Scope::Project => {
                let mut kinds = self.hidden.project.as_ref().map(|(_, k)| k.clone()).unwrap_or_default();
                kinds.insert(kind.to_owned());
                self.keep_project_kinds(kinds, Some(kind))
            }
        }
    }

    /// Shows messages of `kind` again, in `scope` or (`None`) everywhere.
    pub fn show_kind(&mut self, kind: &str, scope: Option<Scope>) -> Result<()> {
        self.refresh_hidden();
        if scope != Some(Scope::Project) {
            self.settings.hidden_messages.retain(|k| k != kind);
        }
        if scope != Some(Scope::User) {
            self.hidden.session.remove(kind);
            if let Some(mut kinds) = self.hidden.project.as_ref().map(|(_, k)| k.clone())
                && kinds.remove(kind)
            {
                self.keep_project_kinds(kinds, None)?;
            }
        }
        Ok(())
    }

    /// Writes the project's hidden kinds to its folder, or keeps `added` for the session when it
    /// has none.
    fn keep_project_kinds(&mut self, kinds: BTreeSet<String>, added: Option<&str>) -> Result<&'static str> {
        let Some(dir) = self.hidden_dir() else {
            if let Some(k) = added {
                self.hidden.session.insert(k.to_owned());
            }
            return Ok("session");
        };
        let file = dir.join(PROJECT_FILE);
        if let Some(roots) = &self.roots {
            roots.check(&file, crate::file_access::Access::Write)?;
        }
        // Comment lines are kept as they were.
        let old = std::fs::read_to_string(&file).unwrap_or_default();
        let mut text: String = old.lines().filter(|l| l.trim().starts_with('#')).map(|l| format!("{l}\n")).collect();
        for k in &kinds {
            text.push_str(k);
            text.push('\n');
        }
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Could not make {}: {e}", parent.display()))?;
        }
        std::fs::write(&file, text).map_err(|e| format!("Could not write {}: {e}", file.display()))?;
        self.hidden.project = Some((Some(dir), kinds));
        Ok("project")
    }

    /// What `build.hidden_messages` says.
    pub(crate) fn hidden_info(&mut self) -> Value {
        self.refresh_hidden();
        json!({"project": self.hidden_for_project(), "user": self.settings.hidden_messages, "hiddenNow": self.hidden_count()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(origin: Option<&str>, text: &str) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            message: text.to_owned(),
            origin: origin.map(str::to_owned),
            file: None,
            line: None,
            column: None,
            end_column: None,
            help: Vec::new(),
            fixes: Vec::new(),
            noticed: None,
        }
    }

    fn same(origin: Option<&str>, a: &str, b: &str) -> bool {
        kind(&message(origin, a)) == kind(&message(origin, b))
    }

    #[test]
    fn messages_from_one_template_are_one_kind_and_other_templates_are_not() {
        let o = Some("BibTeX");
        assert!(same(o, "entry `knuth84` has no pages", "entry `lamport94` has no pages"));
        assert!(!same(o, "entry `knuth84` has no pages", "entry `knuth84` has no journal"));
        assert!(same(o, "empty pages in knuth84", "empty pages in lamport94"));
        assert!(same(
            None,
            "Overfull \\hbox (12.0pt too wide) in paragraph at lines 3--5",
            "Overfull \\hbox (0.5pt too wide) in paragraph at lines 40--41"
        ));
        assert!(!same(
            None,
            "Overfull \\hbox (12.0pt too wide) in paragraph at lines 3--5",
            "Underfull \\hbox (badness 10000) in paragraph at lines 3--5"
        ));
        let font = Some("LaTeX Font");
        assert!(same(
            font,
            "Font shape `T1/cmr/m/sc' undefined using `T1/cmr/m/n' instead",
            "Font shape `OT1/lmr/bx/it' undefined using `OT1/lmr/m/n' instead"
        ));
        let latex = Some("LaTeX");
        assert!(same(latex, "File `fig/a.png' not found", "File `b.pdf' not found"));
        assert!(same(latex, "Unused file chapters/a.tex", "Unused file b.tex"));
        // The same wording from another origin is another kind.
        assert!(
            kind(&message(Some("LaTeX"), "There were undefined references.")) != kind(&message(Some("BibTeX"), "There were undefined references."))
        );
    }

    #[test]
    fn a_hidden_kind_hides_its_messages_but_never_an_error() {
        let mut s = Session::with_backend(Box::new(texcraft_compile::Unavailable("tests".into())));
        let warning = message(None, "Overfull \\hbox (1.0pt too wide) in paragraph at lines 1--2");
        assert_eq!(s.hide_kind(&kind(&warning), Scope::Project), Ok("session"));
        assert!(s.is_hidden(&message(None, "Overfull \\hbox (9.0pt too wide) in paragraph at lines 7--9")));
        let error = Diagnostic { severity: Severity::Error, ..warning.clone() };
        assert!(!s.is_hidden(&error), "an error is hidden");
        assert!(s.show_kind(&kind(&warning), None).is_ok());
        assert!(!s.is_hidden(&warning), "shown again, it is still hidden");
    }
}
