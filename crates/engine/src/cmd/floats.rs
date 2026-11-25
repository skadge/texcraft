//! Tables and figures (docs/ROADMAP.md, F4): the grid editor's and the figure dialog's edits, for
//! the window and for agents alike. Each writes only the source it must ([`texcraft_latex::table`],
//! [`texcraft_latex::figure`]), as one edit of the file.
//!
//! Positions are characters from the start of the file, as everywhere in the commands: a table or
//! figure is named by any position inside it, and a cell by its row and column (from 0), or by a
//! position inside it.

use serde_json::{Value, json};
use texcraft_latex::edit::Change as ByteChange;
use texcraft_latex::figure::{self, FigureChanges};
use texcraft_latex::table::{self, Align, Borders, Table};
use texcraft_project::ProjectPath;

use super::{CommandSpec, always, path_param, string};
use crate::{Change, Origin, Result, Session, Transaction};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "table.inspect",
            label: "Read Tables",
            menu: &[],
            shortcut: None,
            params: inspect_schema,
            enabled: always,
            run: inspect,
            changes: false,
        },
        CommandSpec {
            id: "table.edit",
            label: "Edit Table",
            menu: &[],
            shortcut: None,
            params: edit_schema,
            enabled: always,
            run: edit,
            changes: true,
        },
        CommandSpec {
            id: "figure.edit",
            label: "Edit Figure",
            menu: &[],
            shortcut: None,
            params: figure_schema,
            enabled: always,
            run: edit_figure,
            changes: true,
        },
        CommandSpec {
            id: "figure.insert",
            label: "Insert Figure",
            menu: &[],
            shortcut: None,
            params: insert_figure_schema,
            enabled: always,
            run: insert_figure,
            changes: true,
        },
    ]
}

fn inspect_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "The .tex file."},
        "at": {"type": "integer", "minimum": 0, "description": "Only the table around this position (characters from the file's start)."}
    }, "required": ["path"], "additionalProperties": false})
}

fn edit_schema() -> Value {
    json!({"type": "object", "properties": {
        "path": {"type": "string", "description": "The .tex file."},
        "at": {"type": "integer", "minimum": 0, "description": "A position inside the tabular (characters from the file's start); without `row` and `column`, the cell there is the one edited."},
        "op": {"type": "string", "enum": ["set_cell", "insert_row_above", "insert_row_below", "delete_row", "insert_column_left", "insert_column_right", "delete_column", "align_column", "borders"]},
        "row": {"type": "integer", "minimum": 0, "description": "The row, from 0."},
        "column": {"type": "integer", "minimum": 0, "description": "The column, from 0."},
        "text": {"type": "string", "description": "set_cell: the cell's new text; LaTeX's special characters are escaped unless `latex` is true."},
        "latex": {"type": "boolean", "description": "set_cell: `text` is LaTeX source, written as it is."},
        "align": {"type": "string", "enum": ["left", "center", "right"], "description": "align_column."},
        "borders": {"type": "string", "enum": ["none", "all", "booktabs"], "description": "borders: none; `|` and `\\hline` everywhere; or booktabs' rules (the document must load booktabs: the result says when it does not)."}
    }, "required": ["path", "at", "op"], "additionalProperties": false})
}

fn figure_props() -> Value {
    json!({
        "image": {"type": "string", "description": "The image as `\\includegraphics` takes it, from the main document's folder; for figure.insert, a project file is written so."},
        "width": {"type": "number", "exclusiveMinimum": 0, "maximum": 1, "description": "The image's width as a fraction of \\linewidth."},
        "placement": {"type": "string", "pattern": "^[htbpH!]*$", "description": "Where LaTeX may place it: letters of htbp (and !); empty removes the placement."},
        "caption": {"type": "string", "description": "The caption, as LaTeX source."},
        "label": {"type": "string", "description": "The label other text refers to it by, such as `fig:plot`."}
    })
}

fn figure_schema() -> Value {
    let mut props = figure_props();
    props["path"] = json!({"type": "string", "description": "The .tex file."});
    props["at"] = json!({"type": "integer", "minimum": 0, "description": "A position inside the figure environment."});
    json!({"type": "object", "properties": props, "required": ["path", "at"], "additionalProperties": false})
}

fn insert_figure_schema() -> Value {
    let mut props = figure_props();
    props["path"] = json!({"type": "string", "description": "The .tex file."});
    props["at"] = json!({"type": "integer", "minimum": 0, "description": "Where the figure goes; it takes lines of its own there."});
    json!({"type": "object", "properties": props, "required": ["path", "at", "image"], "additionalProperties": false})
}

/// The byte of character `at`, or an error past the end.
fn byte_at(text: &str, at: u64, path: &ProjectPath) -> Result<usize> {
    let chars = text.chars().count();
    let at = usize::try_from(at).ok().filter(|a| *a <= chars).ok_or_else(|| format!("`at` is past the end of `{path}` ({chars} characters)"))?;
    Ok(text.char_indices().nth(at).map_or(text.len(), |(b, _)| b))
}

fn chars(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |s| s.chars().count())
}

fn position(p: &Value) -> Result<u64> {
    p.get("at").and_then(Value::as_u64).ok_or_else(|| "the parameter `at` (a character offset) is missing".to_owned())
}

/// Makes byte `changes` of `text` in `path` as one edit.
fn commit(s: &mut Session, path: &ProjectPath, text: &str, changes: &[ByteChange]) -> Result<()> {
    let list = changes.iter().map(|c| Change { start: chars(text, c.range.start), end: chars(text, c.range.end), text: c.text.clone() }).collect();
    let tx = Transaction::new(list).map_err(|e| e.to_string())?;
    s.apply_edit(path, &tx, Origin::Local)
}

fn describe(t: &Table, text: &str) -> Value {
    let rows: Vec<Value> = t
        .rows
        .iter()
        .map(|r| {
            let cells: Vec<Value> = r
                .cells
                .iter()
                .map(|c| json!({"column": c.column, "span": c.span, "text": text.get(c.content.clone()).unwrap_or_default()}))
                .collect();
            json!(cells)
        })
        .collect();
    let borders = t.borders().map(|b| match b {
        Borders::None => "none",
        Borders::All => "all",
        Borders::Booktabs => "booktabs",
    });
    let columns: Vec<&str> = t.columns.iter().map(|c| c.align.letter()).collect();
    json!({"columns": columns, "rows": rows, "borders": borders})
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?;
    let at = p.get("at").and_then(Value::as_u64).map(|a| byte_at(text, a, &path)).transpose()?;
    let mut tables = Vec::new();
    for (range, found) in table::all(text) {
        if at.is_some_and(|a| !(range.start <= a && a < range.end)) {
            continue;
        }
        let mut v = match found {
            Ok(t) => describe(&t, text),
            Err(why) => json!({"unsupported": why.describe()}),
        };
        v["start"] = json!(chars(text, range.start));
        v["end"] = json!(chars(text, range.end));
        tables.push(v);
    }
    Ok(json!({"tables": tables}))
}

/// The row and column of the cell to edit: given, or the cell around byte `at`.
fn cell_of(t: &Table, p: &Value, at: usize) -> Result<(usize, usize)> {
    let given = |k: &str| p.get(k).and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok());
    let found = t.rows.iter().enumerate().find_map(|(r, row)| {
        let end = row.end.as_ref().map_or(t.body.end, |e| e.end);
        let start = row.cells.first().map_or(end, |c| c.range.start);
        if !(start <= at && at <= end) {
            return None;
        }
        let cell = row.cells.iter().find(|c| c.range.start <= at && at <= c.range.end).or(row.cells.last())?;
        Some((r, cell.column))
    });
    match (given("row"), given("column"), found) {
        (Some(r), Some(c), _) => Ok((r, c)),
        (r, c, Some((fr, fc))) => Ok((r.unwrap_or(fr), c.unwrap_or(fc))),
        (Some(r), None, None) => Ok((r, 0)),
        (None, Some(c), None) => Ok((0, c)),
        (None, None, None) => Err("`at` is not inside a cell: give `row` and `column`".to_owned()),
    }
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let op = string(p, "op")?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?.to_owned();
    let at = byte_at(&text, position(p)?, &path)?;
    let t = match table::find(&text, at) {
        Some(Ok(t)) => t,
        Some(Err(why)) => return Err(format!("this table stays as source: {}", why.describe())),
        None => return Err(format!("there is no tabular at that position in `{path}`")),
    };
    let (row, column) = cell_of(&t, p, at)?;
    let done = match op {
        "set_cell" => {
            let typed = string(p, "text")?;
            let latex = p.get("latex").and_then(Value::as_bool).unwrap_or(false);
            let source = if latex { typed.to_owned() } else { texcraft_latex::edit::escape(&typed.split_whitespace().collect::<Vec<_>>().join(" ")) };
            t.set_cell(row, column, &source)
        }
        "insert_row_above" | "insert_row_below" => t.insert_row(&text, row, op.ends_with("below")),
        "delete_row" => t.delete_row(&text, row),
        "insert_column_left" | "insert_column_right" => t.insert_column(&text, column, op.ends_with("right")),
        "delete_column" => t.delete_column(column),
        "align_column" => {
            let align = Align::from_name(string(p, "align")?).ok_or("`align` is left, center or right")?;
            t.set_align(column, align)
        }
        "borders" => {
            let borders = Borders::from_name(string(p, "borders")?).ok_or("`borders` is none, all or booktabs")?;
            t.set_borders(&text, borders)
        }
        other => return Err(format!("`{other}` is not a table operation")),
    }
    .map_err(|e| e.describe())?;
    commit(s, &path, &text, &done.changes)?;
    let mut out = json!({"revision": s.revision, "row": row, "column": column, "changed": !done.changes.is_empty()});
    if let Some(package) = done.package {
        // Where the preamble is: the main document's, or this file's own.
        let main = s.project.main().cloned().unwrap_or_else(|| path.clone());
        let loaded = [&main, &path].iter().any(|f| s.project.text(f).is_ok_and(|t| table::loads_package(t, package)));
        out["package"] = json!(package);
        out["loaded"] = json!(loaded);
        if !loaded {
            out["note"] = json!(format!("{main} does not load {package}: add \\usepackage{{{package}}} to its preamble."));
        }
    }
    Ok(out)
}

fn figure_changes(p: &Value) -> Result<FigureChanges> {
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_owned);
    let placement = text("placement");
    if placement.as_ref().is_some_and(|pl| !pl.chars().all(|c| "htbpH!".contains(c))) {
        return Err("`placement` is made of the letters h, t, b, p, H and !".to_owned());
    }
    let label = text("label");
    if label.as_ref().is_some_and(|l| l.contains(['{', '}', '\\', '%', '#'])) {
        return Err("a label cannot hold braces, backslashes, % or #".to_owned());
    }
    let image = text("image");
    if image.as_ref().is_some_and(|i| i.contains(['{', '}', '%', '\n'])) {
        return Err("an image path cannot hold braces, % or line ends".to_owned());
    }
    let width = match p.get("width") {
        None | Some(Value::Null) => None,
        Some(w) => {
            Some(w.as_f64().filter(|w| *w > 0.0 && *w <= 1.0).ok_or("`width` is a fraction of the line's width, above 0 and at most 1")? as f32)
        }
    };
    Ok(FigureChanges { image, width, placement, caption: text("caption"), label })
}

fn edit_figure(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let changes = figure_changes(p)?;
    let text = s.project.text(&path).map_err(|e| e.to_string())?.to_owned();
    let at = byte_at(&text, position(p)?, &path)?;
    let fig = figure::find(&text, at).ok_or_else(|| format!("there is no closed figure environment at that position in `{path}`"))?;
    let done = fig.edit(&text, &changes);
    commit(s, &path, &text, &done)?;
    Ok(json!({"revision": s.revision, "changed": !done.is_empty()}))
}

fn insert_figure(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_param(p, "path")?;
    let mut changes = figure_changes(p)?;
    let Some(image) = changes.image.clone() else { return Err("the parameter `image` is missing".to_owned()) };
    // A project file is written as LaTeX finds it, from the main document's folder.
    if let Ok(file) = ProjectPath::new(&image)
        && s.project.get(&file).is_some()
    {
        let main = s.project.main().cloned().unwrap_or_else(|| path.clone());
        changes.image = Some(super::image::relative(&main, &file));
    }
    let text = s.project.text(&path).map_err(|e| e.to_string())?.to_owned();
    let at = byte_at(&text, position(p)?, &path)?;
    let before = text.get(..at).unwrap_or_default();
    let after = text.get(at..).unwrap_or_default();
    let lead = if before.is_empty() || before.ends_with('\n') { "" } else { "\n" };
    let trail = if after.is_empty() || after.starts_with('\n') { "" } else { "\n" };
    let inserted = format!("{lead}{}{trail}", figure::new_figure(&changes));
    commit(s, &path, &text, &[ByteChange { range: at..at, text: inserted.clone() }])?;
    Ok(json!({"revision": s.revision, "inserted": inserted}))
}
