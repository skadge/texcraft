//! A `figure` environment's parts as the figure dialog edits them (docs/ROADMAP.md, F4): its
//! placement, its image and the image's width, its caption and its label; and the edits that set
//! each, changing only that part's source and adding a part where the figure has none.

use std::ops::Range;

use crate::edit::Change;
use crate::tree::{self, Arg, Kind, Node};

/// The parts of a figure, with the source span of each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    pub range: Range<usize>,
    /// After `\begin{figure}`, where a placement goes when there is none.
    pub begin_end: usize,
    /// Inside `[…]` after `\begin{figure}`.
    pub placement: Option<Range<usize>>,
    pub image: Option<Graphic>,
    /// Inside the braces of `\caption{…}`, and the whole command.
    pub caption: Option<(Range<usize>, Range<usize>)>,
    /// Inside the braces of `\label{…}`.
    pub label: Option<Range<usize>>,
    /// Inside `\end{figure}`'s start: where new parts go, after the last one.
    pub body_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graphic {
    /// The whole `\includegraphics[…]{…}`.
    pub range: Range<usize>,
    /// Inside its `[…]`.
    pub options: Option<Range<usize>>,
    /// Inside its `{…}`.
    pub path: Range<usize>,
}

/// The environments the dialog edits.
pub const FIGURE_ENVIRONMENTS: &[&str] = &["figure", "figure*"];

/// The figure whose source holds byte `at`, the innermost one.
pub fn find(text: &str, at: usize) -> Option<Figure> {
    let tree = tree::parse(text);
    let node = innermost(&tree, at, FIGURE_ENVIRONMENTS)?;
    read(node)
}

/// The `table` float whose source holds byte `at`, the innermost one, read as a figure is: its
/// placement, caption and label (a table float has no image).
pub fn find_table_float(text: &str, at: usize) -> Option<Figure> {
    let tree = tree::parse(text);
    let node = innermost(&tree, at, TABLE_FLOATS)?;
    read(node).map(|f| Figure { image: None, ..f })
}

/// The table floats [`find_table_float`] reads.
pub const TABLE_FLOATS: &[&str] = &["table", "table*"];

fn innermost<'a>(nodes: &'a [Node], at: usize, envs: &[&str]) -> Option<&'a Node> {
    for n in nodes {
        if !(n.range.start <= at && at < n.range.end) {
            continue;
        }
        let inner = match &n.kind {
            Kind::Environment { body, .. } | Kind::Group { body, .. } => innermost(body, at, envs),
            Kind::Command { args, .. } => args.iter().find_map(|a| innermost(&a.body, at, envs)),
            _ => None,
        };
        return match &n.kind {
            Kind::Environment { name, end: Some(_), .. } if envs.contains(&name.as_str()) => inner.or(Some(n)),
            _ => inner,
        };
    }
    None
}

fn required(args: &[Arg]) -> Option<&Arg> {
    args.iter().rev().find(|a| !a.optional && a.closed)
}

fn read(node: &Node) -> Option<Figure> {
    let Kind::Environment { begin, args, body, end: Some(end), .. } = &node.kind else { return None };
    let placement = args.iter().find(|a| a.optional && a.closed).map(|a| a.inner.clone());
    let mut fig = Figure { range: node.range.clone(), begin_end: begin.end, placement, image: None, caption: None, label: None, body_end: end.start };
    for n in body {
        let Kind::Command { name, args, .. } = &n.kind else { continue };
        let Some(arg) = required(args) else { continue };
        match name.as_str() {
            "includegraphics" if fig.image.is_none() => {
                let options = args.iter().find(|a| a.optional && a.closed).map(|a| a.inner.clone());
                fig.image = Some(Graphic { range: n.range.clone(), options, path: arg.inner.clone() });
            }
            "caption" if fig.caption.is_none() => fig.caption = Some((arg.inner.clone(), n.range.clone())),
            "label" if fig.label.is_none() => fig.label = Some(arg.inner.clone()),
            _ => {}
        }
    }
    Some(fig)
}

/// A width as a fraction of `\linewidth` (or `\textwidth`, `\columnwidth`): `0.8\linewidth` is
/// 0.8. `None` for another width (`5cm`) or none.
pub fn width_fraction(options: &str) -> Option<f32> {
    let value = width_value(options)?.1;
    let value = value.trim();
    for unit in ["\\linewidth", "\\textwidth", "\\columnwidth"] {
        if let Some(factor) = value.strip_suffix(unit) {
            let factor = factor.trim();
            return if factor.is_empty() { Some(1.0) } else { factor.parse().ok() };
        }
    }
    None
}

/// The `width=…` key of an options list: the range of its value in `options`, and the value.
fn width_value(options: &str) -> Option<(Range<usize>, &str)> {
    let mut at = 0;
    for part in options.split(',') {
        let start = at;
        at += part.len() + 1;
        let Some((key, value)) = part.split_once('=') else { continue };
        if key.trim() == "width" {
            let value_start = start + key.len() + 1;
            return Some((value_start..value_start + value.len(), value));
        }
    }
    None
}

/// What the dialog sets; `None` keeps the part as it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FigureChanges {
    /// As `\includegraphics` takes it.
    pub image: Option<String>,
    /// A fraction of `\linewidth`.
    pub width: Option<f32>,
    /// `htbp` letters; empty removes the placement.
    pub placement: Option<String>,
    /// LaTeX source.
    pub caption: Option<String>,
    pub label: Option<String>,
}

/// `0.8\linewidth`, or `\linewidth` for the whole width.
pub fn width_source(fraction: f32) -> String {
    let f = (fraction.clamp(0.05, 1.0) * 100.0).round() / 100.0;
    if f >= 1.0 { "\\linewidth".to_owned() } else { format!("{f}\\linewidth") }
}

fn change(range: Range<usize>, text: impl Into<String>) -> Change {
    Change { range, text: text.into() }
}

/// The indentation of the line `at` is on.
fn indent_at(text: &str, at: usize) -> &str {
    let line_start = text.get(..at).and_then(|s| s.rfind('\n')).map_or(0, |n| n + 1);
    let line = text.get(line_start..).unwrap_or_default();
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

impl Figure {
    /// The changes that set `c` on the figure, each touching only its part.
    pub fn edit(&self, text: &str, c: &FigureChanges) -> Vec<Change> {
        let mut out = Vec::new();
        // New parts go on lines of their own, indented as the figure's first part (or two spaces
        // in from `\begin`).
        let first = [self.image.as_ref().map(|g| g.range.start), self.caption.as_ref().map(|c| c.1.start)].into_iter().flatten().min();
        let indent = match first {
            Some(at) => indent_at(text, at).to_owned(),
            None => format!("{}  ", indent_at(text, self.range.start)),
        };
        let line_after = |at: usize, s: String| change(at..at, format!("\n{indent}{s}"));
        if let Some(p) = &c.placement {
            match (&self.placement, p.is_empty()) {
                (Some(r), true) => out.push(change(r.start - 1..r.end + 1, "")),
                (Some(r), false) => out.push(change(r.clone(), p.as_str())),
                (None, false) => out.push(change(self.begin_end..self.begin_end, format!("[{p}]"))),
                (None, true) => {}
            }
        }
        // Where a new caption goes: after the image, or after `\begin` when there is none.
        // After `\begin{figure}` and its placement.
        let after_begin = self.placement.as_ref().map_or(self.begin_end, |r| r.end + 1);
        let after_image = self.image.as_ref().map_or(after_begin, |g| g.range.end);
        match (&self.image, &c.image) {
            (Some(g), Some(path)) => out.push(change(g.path.clone(), path.as_str())),
            (None, Some(path)) => {
                let width = width_source(c.width.unwrap_or(1.0));
                out.push(line_after(after_begin, format!("\\centering\n{indent}\\includegraphics[width={width}]{{{path}}}")));
            }
            _ => {}
        }
        if let (Some(g), Some(w)) = (&self.image, c.width) {
            let width = width_source(w);
            match &g.options {
                Some(o) => {
                    let options = text.get(o.clone()).unwrap_or_default();
                    match width_value(options) {
                        Some((r, _)) => out.push(change(o.start + r.start..o.start + r.end, width)),
                        None if options.trim().is_empty() => out.push(change(o.clone(), format!("width={width}"))),
                        None => out.push(change(o.start..o.start, format!("width={width},"))),
                    }
                }
                None => {
                    let at = g.range.start + "\\includegraphics".len();
                    out.push(change(at..at, format!("[width={width}]")));
                }
            }
        }
        match (&self.caption, &c.caption) {
            (Some((inner, _)), Some(s)) => out.push(change(inner.clone(), s.as_str())),
            // An empty caption too: the place to type one.
            (None, Some(s)) => out.push(line_after(after_image, format!("\\caption{{{s}}}"))),
            _ => {}
        }
        match (&self.label, &c.label) {
            (Some(inner), Some(s)) => out.push(change(inner.clone(), s.as_str())),
            (None, Some(s)) if !s.is_empty() => {
                let at = self.caption.as_ref().map_or(after_image, |c| c.1.end);
                out.push(line_after(at, format!("\\label{{{s}}}")));
            }
            _ => {}
        }
        // Inserts at one place keep the order they were made in.
        out.sort_by_key(|c| (c.range.start, c.range.end));
        out
    }
}

/// A new figure environment, on lines of its own.
pub fn new_figure(c: &FigureChanges) -> String {
    let mut s = format!("\\begin{{figure}}[{}]\n  \\centering\n", c.placement.as_deref().filter(|p| !p.is_empty()).unwrap_or("htbp"));
    if let Some(path) = &c.image {
        s.push_str(&format!("  \\includegraphics[width={}]{{{path}}}\n", width_source(c.width.unwrap_or(0.8))));
    }
    s.push_str(&format!("  \\caption{{{}}}\n", c.caption.as_deref().unwrap_or_default()));
    if let Some(label) = c.label.as_deref().filter(|l| !l.is_empty()) {
        s.push_str(&format!("  \\label{{{label}}}\n"));
    }
    s.push_str("\\end{figure}");
    s
}
