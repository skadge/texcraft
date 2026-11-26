//! Where a build's problems start, not only where TeX noticed them.
//!
//! TeX reports a problem at the line it was reading when it noticed: an `itemize` left open in
//! `sections/intro.tex` is reported at `main.tex`'s `\end{document}`; a `\textbf{` never closed in
//! an included file, at the `\input` line that read it. The log says more than the engine's
//! messages carry: the files TeX opened (`(./sections/intro.tex` … `)`) and, for an environment,
//! the line its `\begin` was on ("\begin{itemize} on input line 12 ended by \end{document}"), but
//! not in which file. [`place_where_started`] reads the log for that and looks in the project's
//! files for the `\begin` (or the unclosed `{`), then places the message there and keeps where TeX
//! noticed it in [`Diagnostic::noticed`].

use std::sync::Arc;

use texcraft_project::ProjectPath;

use crate::{Diagnostic, Place, Severity};

/// TeX's log lines are cut at this many characters and go on on the next line.
const LOG_WIDTH: usize = 79;

/// A problem the log reports, with the files TeX had opened by then.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Problem {
    /// The message after `! ` (and after `LaTeX Error: `).
    message: String,
    /// The `l.N` of its context: the line TeX was reading.
    line: Option<u32>,
    /// The project files open when TeX noticed, the innermost first.
    open: Vec<String>,
    /// Every project file opened by then, the latest first.
    opened: Vec<String>,
    /// The project file closed last before it, when it is the last file event.
    closed: Option<String>,
}

/// The log's lines with the ones TeX cut at [`LOG_WIDTH`] joined again.
fn unwrapped(log: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut joining = false;
    for line in log.lines() {
        if joining && let Some(last) = out.last_mut() {
            last.push_str(line);
        } else {
            out.push(line.to_owned());
        }
        joining = line.chars().count() == LOG_WIDTH;
    }
    out
}

/// The problems of `log` and the files open at each; `file` maps a name TeX opened to its project
/// path (`None` for the TeX tree's files and anything that is not a file name).
fn problems(log: &str, main: &str, file: &dyn Fn(&str) -> Option<String>) -> Vec<Problem> {
    let lines = unwrapped(log);
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut opened: Vec<String> = vec![main.to_owned()];
    let mut closed: Option<String> = None;
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if let Some(message) = line.strip_prefix("! ") {
            let message = message.strip_prefix("LaTeX Error: ").unwrap_or(message).trim().to_owned();
            // The context: until the `l.N` line, which the next line continues; at most a few
            // lines, so a message without one does not swallow the log.
            let mut at = None;
            let mut j = i + 1;
            while j < lines.len() && j < i + 12 {
                if let Some(rest) = lines[j].strip_prefix("l.")
                    && let Some(n) = rest.split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok())
                {
                    at = Some(n);
                    j += 1;
                    break;
                }
                j += 1;
            }
            let mut open: Vec<String> = stack.iter().rev().flatten().cloned().collect();
            if !open.iter().any(|f| f == main) {
                open.push(main.to_owned());
            }
            out.push(Problem { message, line: at, open, opened: opened.iter().rev().cloned().collect(), closed: closed.clone() });
            i = if at.is_some() { j + 1 } else { i + 1 };
            continue;
        }
        // Source text TeX quotes: a runaway argument's, a box's.
        if line.starts_with("Runaway ") {
            i += 2;
            continue;
        }
        if line.starts_with("Overfull \\") || line.starts_with("Underfull \\") {
            while i < lines.len() && !lines[i].trim().is_empty() {
                i += 1;
            }
            continue;
        }
        for (at, c) in line.char_indices() {
            match c {
                '(' => {
                    let rest = &line[at + 1..];
                    let name: &str = rest.split(|c: char| c.is_whitespace() || c == '(' || c == ')').next().unwrap_or("");
                    let path = (!name.is_empty()).then(|| file(name)).flatten();
                    if let Some(p) = &path {
                        opened.push(p.clone());
                        closed = None;
                    }
                    stack.push(path);
                }
                ')' => {
                    if let Some(Some(p)) = stack.pop() {
                        closed = Some(p);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    out
}

/// The line `n` (from 1) of `text`.
fn line_of(text: &str, n: u32) -> Option<&str> {
    text.lines().nth(n.checked_sub(1)? as usize)
}

/// The column (from 1, in characters) of `needle` in `line`.
fn column_of(line: &str, needle: &str) -> Option<u32> {
    let at = line.find(needle)?;
    u32::try_from(line[..at].chars().count() + 1).ok()
}

/// The environment opened and the line of its `\begin` of "\begin{X} on input line N ended by
/// \end{Y}."
fn ended_by(message: &str) -> Option<(&str, u32)> {
    let rest = message.strip_prefix("\\begin{")?;
    let (env, rest) = rest.split_once('}')?;
    let (_, rest) = rest.split_once(" on input line ")?;
    let n = rest.split(' ').next()?.parse().ok()?;
    rest.contains("ended by \\end{").then_some((env, n))
}

/// The command of "File ended while scanning use of \x." (TeX writes "\x ." for a control word).
fn file_ended(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("File ended while scanning use of ")?;
    Some(rest.trim_end_matches('.').trim_end())
}

/// The command just before an opening brace at the end of `before`: `\textbf` of `\textbf`,
/// `\section*[short]`.
fn command_before(before: &str) -> Option<&str> {
    let mut s = before.trim_end();
    if s.ends_with(']') {
        s = s[..s.rfind('[')?].trim_end();
    }
    let s = s.strip_suffix('*').unwrap_or(s);
    let start = s.rfind('\\')?;
    let name = &s[start..];
    (name.len() > 1 && name[1..].chars().all(|c| c.is_ascii_alphabetic() || c == '@')).then_some(name)
}

/// The braces of `text` left open at its end, as (line from 1, column from 1, the command before
/// it); comments and `\{` skipped.
fn open_braces(text: &str) -> Vec<(u32, u32, Option<String>)> {
    let mut open = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut chars = line.char_indices().enumerate();
        while let Some((col, (at, c))) = chars.next() {
            match c {
                '\\' => {
                    chars.next();
                }
                '%' => break,
                '{' => {
                    let command = command_before(&line[..at]).map(str::to_owned);
                    open.push((u32::try_from(n + 1).unwrap_or(u32::MAX), u32::try_from(col + 1).unwrap_or(u32::MAX), command));
                }
                '}' => {
                    open.pop();
                }
                _ => {}
            }
        }
    }
    open
}

/// Where a problem starts: the file, line and the span's columns.
struct Start {
    file: String,
    line: u32,
    column: u32,
    end_column: u32,
}

/// The `\begin{env}` on line `n` of the first of `files` that has it there: the files TeX had
/// open, then those it had read.
fn begin_in(files: &[String], text: &dyn Fn(&str) -> Option<String>, env: &str, n: u32) -> Option<Start> {
    let needle = format!("\\begin{{{env}}}");
    files.iter().find_map(|f| {
        let source = text(f)?;
        let column = column_of(line_of(&source, n)?, &needle)?;
        let width = u32::try_from(needle.chars().count()).unwrap_or(1);
        Some(Start { file: f.clone(), line: n, column, end_column: column + width })
    })
}

/// The unclosed `{` after `command` (or the first unclosed one) in the first of `files` with one.
fn brace_in(files: &[String], text: &dyn Fn(&str) -> Option<String>, command: &str) -> Option<Start> {
    files.iter().find_map(|f| {
        let open = open_braces(&text(f)?);
        let (line, column, _) = open.iter().rev().find(|(_, _, c)| c.as_deref() == Some(command)).or_else(|| open.first())?;
        Some(Start { file: f.clone(), line: *line, column: *column, end_column: column + 1 })
    })
}

/// `files` in order, each once.
fn unique(files: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in files {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Places each error of `diagnostics` that TeX noticed away from where it starts (an environment
/// never closed, an argument whose `}` is missing) at its start, read from `log` and the job's
/// `files`; where TeX noticed it is kept in [`Diagnostic::noticed`]. Messages already placed at
/// their start, and those whose start is not found, are left as they are. `main_dir`: the main
/// document's folder, which the log's names start from.
pub fn place_where_started(
    diagnostics: &mut [Diagnostic],
    log: &str,
    files: &[(ProjectPath, Arc<[u8]>)],
    main: &ProjectPath,
    main_dir: Option<&str>,
) {
    let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    let file = |name: &str| {
        let path = crate::records::project_file(name, main_dir)?;
        let tex = format!("{path}.tex");
        names.iter().find(|n| **n == path || **n == tex).map(|n| (*n).to_owned())
    };
    let text = |path: &str| files.iter().find(|(p, _)| p.as_str() == path).and_then(|(_, bytes)| std::str::from_utf8(bytes).ok().map(str::to_owned));
    let mut used = vec![false; diagnostics.len()];
    for problem in problems(log, main.as_str(), &file) {
        let Some(noticed_line) = problem.line else { continue };
        let (start, shown) = if let Some((env, n)) = ended_by(&problem.message) {
            let candidates = unique(problem.open.iter().chain(&problem.opened).cloned());
            (begin_in(&candidates, &text, env, n), format!("\\begin{{{env}}}"))
        } else if let Some(command) = file_ended(&problem.message) {
            let candidates = unique(problem.closed.iter().chain(&problem.opened).cloned());
            (brace_in(&candidates, &text, command), "`}`".to_owned())
        } else {
            continue;
        };
        let Some(start) = start else { continue };
        let found = diagnostics.iter().enumerate().position(|(i, d)| {
            !used[i] && d.severity == Severity::Error && d.line == Some(noticed_line) && d.file.is_some() && d.message.contains(&shown)
        });
        let Some(i) = found else { continue };
        used[i] = true;
        let d = &mut diagnostics[i];
        if d.file.as_deref() == Some(start.file.as_str()) && d.line == Some(start.line) {
            continue;
        }
        d.noticed = d.file.take().map(|file| Place { file, line: d.line });
        d.file = Some(start.file);
        d.line = Some(start.line);
        d.column = Some(start.column);
        d.end_column = Some(start.end_column);
        // The fixes were for the line where TeX noticed.
        d.fixes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> ProjectPath {
        ProjectPath::new(p).expect("a test's path")
    }

    fn files(list: &[(&str, &str)]) -> Vec<(ProjectPath, Arc<[u8]>)> {
        list.iter().map(|(p, t)| (path(p), Arc::from(t.as_bytes()))).collect()
    }

    fn error(message: &str, file: &str, line: u32) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            message: message.to_owned(),
            origin: None,
            file: Some(file.to_owned()),
            line: Some(line),
            column: Some(1),
            end_column: Some(2),
            help: Vec::new(),
            fixes: Vec::new(),
            noticed: None,
        }
    }

    // The logs below are our own, written in the shape of a TeX log for these tests.
    const MAIN: &str = "\\documentclass{article}\n\\begin{document}\nHello.\n\\input{sections/intro}\nMore.\n\\end{document}\n";

    const UNCLOSED: &str = "This is a TeX log.\n(./main.tex (/texmf/tex/latex/base/article.cls\nDocument Class: article (/texmf/size10.clo))\n (./sections/intro.tex)\n\n! LaTeX Error: \\begin{itemize} on input line 3 ended by \\end{document}.\n\nSee the LaTeX manual.\n ...\n\nl.6 \\end{document}\n\nYour command was ignored.\n)\n";

    #[test]
    fn an_environment_left_open_in_an_included_file_is_placed_at_its_begin() {
        let files = files(&[("main.tex", MAIN), ("sections/intro.tex", "Intro.\n\n  \\begin{itemize}\n\\item One\n")]);
        let mut d = vec![error("\\begin{itemize} is never closed", "main.tex", 6)];
        place_where_started(&mut d, UNCLOSED, &files, &path("main.tex"), None);
        assert_eq!((d[0].file.as_deref(), d[0].line, d[0].column), (Some("sections/intro.tex"), Some(3), Some(3)));
        assert_eq!(d[0].noticed, Some(Place { file: "main.tex".into(), line: Some(6) }), "where TeX noticed is kept");
    }

    #[test]
    fn a_main_folder_s_files_are_read_from_it() {
        let files = files(&[("paper/main.tex", MAIN), ("paper/sections/intro.tex", "\\begin{itemize}\n\n\n")]);
        let log = UNCLOSED.replace("line 3", "line 1");
        let mut d = vec![error("\\begin{itemize} is never closed", "paper/main.tex", 6)];
        place_where_started(&mut d, &log, &files, &path("paper/main.tex"), Some("paper"));
        assert_eq!((d[0].file.as_deref(), d[0].line), (Some("paper/sections/intro.tex"), Some(1)));
    }

    #[test]
    fn a_nested_include_and_a_wrong_end_are_placed_at_their_begins() {
        let files = files(&[
            ("main.tex", MAIN),
            ("sections/intro.tex", "\\begin{quote}\n\\input{sections/deep}\n\\end{quote}\n"),
            ("sections/deep.tex", "x\n\\begin{center}\ny\n"),
        ]);
        let log = "(./main.tex\n(./sections/intro.tex\n(./sections/deep.tex)\n\n! LaTeX Error: \\begin{center} on input line 2 ended by \\end{quote}.\n\nl.3 \\end{quote}\n\n)\n! LaTeX Error: \\begin{quote} on input line 1 ended by \\end{document}.\n\nl.6 \\end{document}\n\n)\n";
        let mut d =
            vec![error("\\begin{center} is ended by \\end{quote}", "sections/intro.tex", 3), error("\\begin{quote} is never closed", "main.tex", 6)];
        place_where_started(&mut d, log, &files, &path("main.tex"), None);
        assert_eq!((d[0].file.as_deref(), d[0].line), (Some("sections/deep.tex"), Some(2)));
        assert_eq!(d[0].noticed.as_ref().map(|p| p.file.as_str()), Some("sections/intro.tex"));
        assert_eq!((d[1].file.as_deref(), d[1].line), (Some("sections/intro.tex"), Some(1)));
    }

    #[test]
    fn a_missing_brace_at_an_included_file_s_end_is_placed_at_its_opening() {
        let files = files(&[("main.tex", MAIN), ("sections/intro.tex", "Intro {\\em a} \\textbf{bold\n\nnext.\n")]);
        let log = "(./main.tex\n (./sections/intro.tex)\nRunaway argument?\n{bold \\par next. (\n! File ended while scanning use of \\textbf .\n<inserted text>\n                \\par\nl.4 \\input{sections/intro}\n\nI suspect you have forgotten a `}'.\n)\n";
        let mut d = vec![error("missing `}` after the argument of \\textbf", "main.tex", 4)];
        place_where_started(&mut d, log, &files, &path("main.tex"), None);
        assert_eq!((d[0].file.as_deref(), d[0].line, d[0].column), (Some("sections/intro.tex"), Some(1), Some(22)));
        assert!(d[0].noticed.is_some());
    }

    #[test]
    fn a_message_already_at_its_start_or_without_one_found_is_left() {
        // The `\begin` is in main.tex, where the engine placed it already.
        let files = files(&[("main.tex", "\\documentclass{article}\n\\begin{document}\n\\begin{itemize}\n\\end{document}\n")]);
        let log = "(./main.tex\n! LaTeX Error: \\begin{itemize} on input line 3 ended by \\end{document}.\n\nl.4 \\end{document}\n\n)\n";
        let mut placed = vec![error("\\begin{itemize} is never closed", "main.tex", 3)];
        place_where_started(&mut placed, log, &files, &path("main.tex"), None);
        assert_eq!((placed[0].line, &placed[0].noticed), (Some(3), &None));
        // No file has a `\begin{itemize}` on that line: left where TeX noticed it.
        let mut lost = vec![error("\\begin{itemize} is never closed", "main.tex", 4)];
        place_where_started(&mut lost, &log.replace("line 3", "line 2"), &files, &path("main.tex"), None);
        assert_eq!((lost[0].line, &lost[0].noticed), (Some(4), &None));
    }

    #[test]
    fn a_name_cut_at_the_log_s_width_is_read_whole() {
        let long = format!("chapters/{}/intro.tex", "a".repeat(70));
        let files = files(&[("main.tex", MAIN), (long.as_str(), "\\begin{itemize}\n")]);
        let opened = format!("(./main.tex (./{long})");
        let (first, rest) = opened.split_at(LOG_WIDTH);
        let log =
            format!("{first}\n{rest}\n! LaTeX Error: \\begin{{itemize}} on input line 1 ended by \\end{{document}}.\n\nl.6 \\end{{document}}\n\n)\n");
        let mut d = vec![error("\\begin{itemize} is never closed", "main.tex", 6)];
        place_where_started(&mut d, &log, &files, &path("main.tex"), None);
        assert_eq!(d[0].file.as_deref(), Some(long.as_str()));
    }
}
