//! Placing a project's image in a document: `\includegraphics` with the image's path as LaTeX
//! looks it up, from the main document's folder.
//!
//! The line is a bare `\includegraphics[width=\linewidth]{…}`, not a `figure` environment: it is
//! often dropped into a figure, a minipage or a table cell that is already there, where a float
//! would be wrong, and the Figure toolbar entry makes the environment when one is wanted. The
//! width keeps a large image inside the text block.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Value, json};
use texcraft_project::ProjectPath;

use super::{CommandSpec, always, path_param};
use crate::{Change, Origin, Result, Session, Transaction};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "file.insert_image",
        label: "Insert Image",
        menu: &[],
        shortcut: None,
        params: schema,
        enabled: always,
        run: insert_image,
        changes: true,
    }]
}

fn schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "The text file to insert into, such as `chapters/intro.tex`."},
        "image": {"type": "string", "description": "The image in the project (png, jpg, jpeg, gif, webp, pdf, eps or svg)."},
        "at": {"type": "integer", "minimum": 0, "description": "Where to insert, in characters from the start of the file. The `\\includegraphics` goes on a line of its own there."}
    }, "required": ["path", "image", "at"], "additionalProperties": false})
}

/// Whether `path` names a graphic `\includegraphics` takes, by its extension.
pub fn is_image(path: &ProjectPath) -> bool {
    matches!(path.extension().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "pdf" | "eps" | "svg")
}

/// `image` as written from the folder of `from` (a file): `../figures/a.png`.
pub fn relative(from: &ProjectPath, image: &ProjectPath) -> String {
    let folder: Vec<&str> = from.as_str().split('/').collect();
    let folder = &folder[..folder.len().saturating_sub(1)];
    let target: Vec<&str> = image.as_str().split('/').collect();
    let common = folder.iter().zip(&target).take_while(|(a, b)| a == b).count().min(target.len().saturating_sub(1));
    let mut parts: Vec<&str> = vec![".."; folder.len() - common];
    parts.extend(&target[common..]);
    parts.join("/")
}

/// Whether the preamble of `main` loads graphicx, itself or through a package or class that does.
fn loads_graphics(main: &str) -> bool {
    static LOADS: OnceLock<Option<Regex>> = OnceLock::new();
    let Some(loads) = LOADS.get_or_init(|| Regex::new(r"\\(usepackage|RequirePackage|documentclass)\s*(\[[^\]]*\])?\s*\{([^}]*)\}").ok()) else {
        return false;
    };
    let preamble = main.split("\\begin{document}").next().unwrap_or_default();
    let code: String = preamble.lines().map(|l| uncommented(l).to_owned() + "\n").collect();
    loads.captures_iter(&code).any(|c| {
        let class = c.get(1).is_some_and(|m| m.as_str() == "documentclass");
        c.get(3).is_some_and(|names| {
            names
                .as_str()
                .split(',')
                .map(str::trim)
                .any(|n| if class { n == "beamer" } else { matches!(n, "graphicx" | "tikz" | "pgf" | "pgfplots") })
        })
    })
}

/// `line` up to its comment, if any (a `%` not escaped by a backslash).
fn uncommented(line: &str) -> &str {
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            '%' if !escaped => return &line[..i],
            '\\' => escaped = !escaped,
            _ => escaped = false,
        }
    }
    line
}

fn insert_image(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let image = path_param(p, "image")?;
    let at = p.get("at").and_then(Value::as_u64).ok_or("the parameter `at` (a character offset) is missing")?;
    if !is_image(&image) {
        return Err(format!("`{image}` is not an image `\\includegraphics` takes (png, jpg, jpeg, gif, webp, pdf, eps or svg)"));
    }
    if s.project.get(&image).is_none() {
        return Err(format!("`{image}` is not among the project's files"));
    }
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    let len = text.chars().count();
    let at = usize::try_from(at).ok().filter(|a| *a <= len).ok_or_else(|| format!("`at` is past the end of `{path}` ({len} characters)"))?;
    // LaTeX looks the image up from the folder of the document it compiles.
    let main = s.project.main().cloned().unwrap_or_else(|| path.clone());
    let written = relative(&main, &image);
    let before = at.checked_sub(1).and_then(|i| text.chars().nth(i));
    let after = text.chars().nth(at);
    let line = format!(
        "{}\\includegraphics[width=\\linewidth]{{{written}}}{}",
        if before.is_none_or(|c| c == '\n') { "" } else { "\n" },
        if after.is_none_or(|c| c == '\n') { "" } else { "\n" },
    );
    let graphics = s.project.text(&main).is_ok_and(loads_graphics);
    let tx = Transaction::new(vec![Change { start: at, end: at, text: line.clone() }]).map_err(|e| e.to_string())?;
    s.apply_edit(&path, &tx, Origin::Local)?;
    Ok(json!({"revision": s.revision, "inserted": line, "image": written, "main": main.as_str(), "graphicx": graphics}))
}
