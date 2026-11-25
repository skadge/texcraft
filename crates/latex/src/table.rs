//! `tabular` as a grid (docs/ROADMAP.md, F4): its column specification, its rows and cells with
//! the source span of each, `\multicolumn` spans and the rules between rows; and the edits of the
//! grid editor, each changing only the cells, separators and rules it must.
//!
//! [`parse`] reads a `tabular` the grid can show faithfully, or says which feature it cannot
//! ([`Unsupported`]): such a table stays as its source. Cells are split on `&` and rows on `\\`
//! (and `\tabularnewline`) at the body's top level, so what is inside a group or a command's
//! argument stays in its cell.

use std::ops::Range;

use crate::edit::Change;
use crate::tree::{self, Kind, Node};

/// How a column sets its cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
    /// `p{…}`, `m{…}`, `b{…}`: a paragraph of a given width, set flush left.
    Paragraph,
}

impl Align {
    /// The column letter.
    pub fn letter(self) -> &'static str {
        match self {
            Align::Left => "l",
            Align::Center => "c",
            Align::Right => "r",
            Align::Paragraph => "p",
        }
    }

    /// `l`, `c` or `r` (or their names), for the commands.
    pub fn from_name(name: &str) -> Option<Align> {
        match name {
            "l" | "left" => Some(Align::Left),
            "c" | "center" => Some(Align::Center),
            "r" | "right" => Some(Align::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub align: Align,
    /// Its letter in the specification, with its `{width}` for a paragraph column.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleKind {
    /// `\hline`.
    Line,
    /// booktabs' `\toprule`, `\midrule`, `\bottomrule`.
    Top,
    Mid,
    Bottom,
    /// Under some columns only: `\cline{…}`, `\cmidrule{…}`.
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub kind: RuleKind,
    /// The command with its arguments.
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Multicolumn {
    /// The whole `\multicolumn{n}{spec}{…}`.
    pub range: Range<usize>,
    /// Inside its first braces: the number of columns.
    pub count: Range<usize>,
    /// Inside its second: the cell's column specification.
    pub spec: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// From the separator before it (or the row's start) to the one after it (or the row's end).
    pub range: Range<usize>,
    /// Its text without the spaces around it; for an empty cell, where typing goes.
    pub content: Range<usize>,
    /// The first column it takes, and how many.
    pub column: usize,
    pub span: usize,
    pub multicolumn: Option<Multicolumn>,
}

impl Cell {
    /// Where its text starts, or its `\multicolumn`.
    pub fn start(&self) -> usize {
        self.multicolumn.as_ref().map_or(self.content.start, |m| m.range.start)
    }

    /// Where its text ends, or its `\multicolumn`.
    pub fn end(&self) -> usize {
        self.multicolumn.as_ref().map_or(self.content.end, |m| m.range.end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The rules above it.
    pub rules: Vec<Rule>,
    pub cells: Vec<Cell>,
    /// Its `\\` with its optional arguments; `None` for a last row without one.
    pub end: Option<Range<usize>>,
}

impl Row {
    /// The cell that takes column `c`.
    pub fn cell_at(&self, c: usize) -> Option<(usize, &Cell)> {
        self.cells.iter().enumerate().find(|(_, cell)| cell.column <= c && c < cell.column + cell.span)
    }

    /// Where it starts: its first cell's text (after the rules above it).
    pub fn start(&self) -> usize {
        self.cells.first().map_or(0, Cell::start)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The environment, from `\begin` to `\end{tabular}`.
    pub range: Range<usize>,
    /// Inside the braces of the column specification.
    pub spec: Range<usize>,
    pub columns: Vec<Column>,
    /// The `|` of the specification at each boundary: `bars[i]` before column `i`, the last one
    /// after the last column.
    pub bars: Vec<Vec<usize>>,
    pub rows: Vec<Row>,
    /// The rules after the last row.
    pub trailing: Vec<Rule>,
    /// From after the arguments to `\end`.
    pub body: Range<usize>,
}

/// What keeps a `tabular` from being shown as a grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    /// Not closed by its `\end`, or with no column specification.
    Unclosed,
    /// A column type the grid does not know (`X`, `S`), or `@{…}`, `>{…}`, `*{…}` and kin.
    ColumnType(String),
    Multirow,
    /// A table inside a cell.
    Nested,
    /// Another environment inside a cell.
    Environment(String),
    /// A rule (`\hline`) in the middle of a row.
    RuleInRow,
    /// Text in a cell besides its `\multicolumn`.
    BesideMulticolumn,
    /// A `\multicolumn` whose arguments the grid cannot read.
    Multicolumn,
    /// Row `row` (from 0) has more cells than the specification has columns.
    TooManyCells {
        row: usize,
    },
    /// A brace or `\end` that closes nothing, or a command the grid cannot place (`\noalign`).
    Structure(String),
}

impl Unsupported {
    /// In English, for agents (the window translates its own wording).
    pub fn describe(&self) -> String {
        match self {
            Unsupported::Unclosed => "the table is not closed by \\end{tabular}, or has no column specification".to_owned(),
            Unsupported::ColumnType(t) => format!("the column specification uses `{t}`"),
            Unsupported::Multirow => "a cell spans rows (\\multirow)".to_owned(),
            Unsupported::Nested => "a cell holds another table".to_owned(),
            Unsupported::Environment(name) => format!("a cell holds a `{name}` environment"),
            Unsupported::RuleInRow => "a rule is drawn in the middle of a row".to_owned(),
            Unsupported::BesideMulticolumn => "a cell holds text besides its \\multicolumn".to_owned(),
            Unsupported::Multicolumn => "a \\multicolumn's arguments cannot be read".to_owned(),
            Unsupported::TooManyCells { row } => format!("row {} has more cells than the table has columns", row + 1),
            Unsupported::Structure(what) => format!("the table's body holds {what}"),
        }
    }
}

/// The environments read as grids.
pub const GRID_ENVIRONMENTS: &[&str] = &["tabular"];

/// The `tabular` environment `node` of `text`, as a grid.
pub fn parse(text: &str, node: &Node) -> Result<Table, Unsupported> {
    let Kind::Environment { name, args, body, end, begin } = &node.kind else { return Err(Unsupported::Unclosed) };
    if !GRID_ENVIRONMENTS.contains(&name.as_str()) || end.is_none() {
        return Err(Unsupported::Unclosed);
    }
    let spec = args.iter().find(|a| !a.optional && a.closed).ok_or(Unsupported::Unclosed)?;
    let (columns, bars) = columns(text, spec.inner.clone())?;
    let body_end = end.as_ref().map_or(node.range.end, |e| e.start);
    let body_range = begin.end..body_end;
    let (rows, trailing) = Rows::new(text, body_range.clone(), columns.len()).read(body)?;
    Ok(Table { range: node.range.clone(), spec: spec.inner.clone(), columns, bars, rows, trailing, body: body_range })
}

/// The grid of the `tabular` whose source holds byte `at`, the innermost when nested.
pub fn find(text: &str, at: usize) -> Option<Result<Table, Unsupported>> {
    let tree = tree::parse(text);
    let node = innermost(&tree, at)?;
    Some(parse(text, node))
}

/// Every `tabular` of `text`, in order, outermost ones only.
pub fn all(text: &str) -> Vec<(Range<usize>, Result<Table, Unsupported>)> {
    fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
        for n in nodes {
            match &n.kind {
                Kind::Environment { name, .. } if GRID_ENVIRONMENTS.contains(&name.as_str()) => out.push(n),
                Kind::Environment { body, .. } | Kind::Group { body, .. } => walk(body, out),
                Kind::Command { args, .. } => args.iter().for_each(|a| walk(&a.body, out)),
                _ => {}
            }
        }
    }
    let tree = tree::parse(text);
    let mut found = Vec::new();
    walk(&tree, &mut found);
    found.into_iter().map(|n| (n.range.clone(), parse(text, n))).collect()
}

/// The innermost `tabular` node around `at`.
fn innermost(nodes: &[Node], at: usize) -> Option<&Node> {
    for n in nodes {
        if !(n.range.start <= at && at < n.range.end) {
            continue;
        }
        let inner = match &n.kind {
            Kind::Environment { body, .. } | Kind::Group { body, .. } => innermost(body, at),
            Kind::Command { args, .. } => args.iter().find_map(|a| innermost(&a.body, at)),
            _ => None,
        };
        return match &n.kind {
            Kind::Environment { name, .. } if GRID_ENVIRONMENTS.contains(&name.as_str()) => inner.or(Some(n)),
            _ => inner,
        };
    }
    None
}

/// The end of the balanced `{…}` at `at`, or `None`.
fn braced_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut j = at;
    while let Some(&b) = bytes.get(j) {
        match b {
            b'\\' => j += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

type Bars = Vec<Vec<usize>>;

/// The columns of a specification and its bars.
fn columns(text: &str, spec: Range<usize>) -> Result<(Vec<Column>, Bars), Unsupported> {
    let mut cols = Vec::new();
    let mut bars: Bars = vec![Vec::new()];
    let mut j = spec.start;
    let bytes = text.as_bytes();
    while j < spec.end {
        let Some(&b) = bytes.get(j) else { break };
        match b {
            b' ' | b'\t' | b'\n' | b'\r' => j += 1,
            b'|' => {
                if let Some(last) = bars.last_mut() {
                    last.push(j);
                }
                j += 1;
            }
            b'l' | b'c' | b'r' => {
                let align = match b {
                    b'l' => Align::Left,
                    b'c' => Align::Center,
                    _ => Align::Right,
                };
                cols.push(Column { align, range: j..j + 1 });
                bars.push(Vec::new());
                j += 1;
            }
            b'p' | b'm' | b'b' => {
                let at = j + 1 + text.get(j + 1..spec.end).map_or(0, |s| s.len() - s.trim_start().len());
                let end = braced_end(text, at).filter(|e| *e <= spec.end).ok_or_else(|| Unsupported::ColumnType(char::from(b).to_string()))?;
                cols.push(Column { align: Align::Paragraph, range: j..end });
                bars.push(Vec::new());
                j = end;
            }
            b'@' | b'!' | b'>' | b'<' | b'*' => return Err(Unsupported::ColumnType(format!("{}{{…}}", char::from(b)))),
            _ => {
                let c = text.get(j..).and_then(|s| s.chars().next()).unwrap_or('?');
                return Err(Unsupported::ColumnType(c.to_string()));
            }
        }
    }
    if cols.is_empty() {
        return Err(Unsupported::Unclosed);
    }
    Ok((cols, bars))
}

/// Reading a body into rows.
struct Rows<'a> {
    text: &'a str,
    body: Range<usize>,
    columns: usize,
    rows: Vec<Row>,
    /// The row being read: its rules, its cells so far, where its current cell starts, and whether
    /// it shows anything yet.
    rules: Vec<Rule>,
    cells: Vec<Cell>,
    cell_start: usize,
    multicolumn: Option<(Multicolumn, Range<usize>)>,
    content: bool,
    /// Nodes before this byte were read with the node before them (a rule's or a row end's
    /// arguments).
    skip: usize,
}

const FULL_RULES: &[(&str, RuleKind)] =
    &[("hline", RuleKind::Line), ("toprule", RuleKind::Top), ("midrule", RuleKind::Mid), ("bottomrule", RuleKind::Bottom)];

impl<'a> Rows<'a> {
    fn new(text: &'a str, body: Range<usize>, columns: usize) -> Self {
        let start = body.start;
        Rows {
            text,
            body,
            columns,
            rows: Vec::new(),
            rules: Vec::new(),
            cells: Vec::new(),
            cell_start: start,
            multicolumn: None,
            content: false,
            skip: start,
        }
    }

    fn src(&self, r: Range<usize>) -> &'a str {
        self.text.get(r).unwrap_or_default()
    }

    /// After an optional argument `[…]` at `at` (spaces before it allowed), a `(…)` when `trim`,
    /// and `braces` `{…}` arguments.
    fn after_args(&self, at: usize, trim: bool, braces: usize) -> usize {
        let mut j = at;
        let skip_space = |j: usize| j + self.src(j..self.body.end).len() - self.src(j..self.body.end).trim_start_matches([' ', '\t']).len();
        let optional: &[(u8, char)] = if trim { &[(b'[', ']'), (b'(', ')')] } else { &[(b'[', ']')] };
        for &(open, close) in optional {
            let k = skip_space(j);
            if self.text.as_bytes().get(k) == Some(&open)
                && let Some(n) = self.src(k..self.body.end).find(close)
            {
                j = k + n + 1;
            }
        }
        for _ in 0..braces {
            let k = skip_space(j);
            match braced_end(self.text, k).filter(|e| *e <= self.body.end) {
                Some(e) => j = e,
                None => break,
            }
        }
        j
    }

    fn read(mut self, body: &[Node]) -> Result<(Vec<Row>, Vec<Rule>), Unsupported> {
        let mut i = 0;
        while let Some(n) = body.get(i) {
            i += 1;
            if n.range.end <= self.skip {
                continue;
            }
            // A text node the arguments of a row end reach into: what follows them.
            let start = n.range.start.max(self.skip);
            let r = start..n.range.end;
            match &n.kind {
                Kind::Text => {
                    if !self.src(r).trim().is_empty() {
                        self.shows()?;
                    }
                }
                Kind::Comment | Kind::ParBreak => {}
                Kind::Special if self.src(r.clone()) == "&" => self.end_cell(r.start, r.end)?,
                Kind::ControlSymbol if self.src(r.clone()) == "\\\\" => {
                    let after_star = if self.text.as_bytes().get(r.end) == Some(&b'*') { r.end + 1 } else { r.end };
                    let end = self.after_args(after_star, false, 0);
                    self.end_row(r.start, r.start..end)?;
                }
                Kind::Command { name, .. } => self.command(body, i - 1, name)?,
                Kind::Environment { name, .. } if GRID_ENVIRONMENTS.contains(&name.as_str()) || name.starts_with("tabular") || name == "array" => {
                    return Err(Unsupported::Nested);
                }
                Kind::Environment { name, .. } => return Err(Unsupported::Environment(name.clone())),
                Kind::Error => return Err(Unsupported::Structure(format!("`{}`, which closes nothing", self.src(r).trim()))),
                _ => self.shows()?,
            }
        }
        // The last row, when it has no `\\`.
        if self.content || !self.cells.is_empty() {
            let end = self.body.end;
            self.end_row(end, end..end)?;
            if let Some(last) = self.rows.last_mut() {
                last.end = None;
            }
        }
        Ok((self.rows, self.rules))
    }

    /// Something shows in the current cell.
    fn shows(&mut self) -> Result<(), Unsupported> {
        if self.multicolumn.is_some() {
            return Err(Unsupported::BesideMulticolumn);
        }
        self.content = true;
        Ok(())
    }

    fn command(&mut self, body: &[Node], i: usize, name: &str) -> Result<(), Unsupported> {
        let Some(n) = body.get(i) else { return Ok(()) };
        let partial = matches!(name, "cline" | "cmidrule").then_some(RuleKind::Partial);
        if let Some(kind) = FULL_RULES.iter().find(|(r, _)| *r == name).map(|(_, k)| *k).or(partial) {
            if self.content || !self.cells.is_empty() || self.multicolumn.is_some() {
                return Err(Unsupported::RuleInRow);
            }
            let end = self.after_args(n.range.end, name == "cmidrule", usize::from(kind == RuleKind::Partial));
            self.rules.push(Rule { kind, range: n.range.start..end });
            self.skip = end;
            self.cell_start = end;
            return Ok(());
        }
        match name {
            "multirow" | "multirowcell" => Err(Unsupported::Multirow),
            "tabularnewline" => self.end_row(n.range.start, n.range.clone()),
            "noalign" | "addlinespace" | "specialrule" | "morecmidrules" | "hhline" => Err(Unsupported::Structure(format!("\\{name}"))),
            "multicolumn" => {
                if self.content || self.multicolumn.is_some() {
                    return Err(Unsupported::BesideMulticolumn);
                }
                let mut groups = Vec::new();
                let mut end = n.range.end;
                let mut k = i + 1;
                while groups.len() < 3 {
                    match body.get(k) {
                        Some(Node { kind: Kind::Text, range }) if self.src(range.clone()).trim().is_empty() => {}
                        Some(Node { kind: Kind::Group { closed: true, .. }, range }) => {
                            groups.push(range.start + 1..range.end - 1);
                            end = range.end;
                        }
                        _ => return Err(Unsupported::Multicolumn),
                    }
                    k += 1;
                }
                let [count, spec, inner] = <[Range<usize>; 3]>::try_from(groups).map_err(|_| Unsupported::Multicolumn)?;
                let span: usize = self.src(count.clone()).trim().parse().map_err(|_| Unsupported::Multicolumn)?;
                if span == 0 {
                    return Err(Unsupported::Multicolumn);
                }
                self.multicolumn = Some((Multicolumn { range: n.range.start..end, count, spec }, inner));
                self.skip = end;
                Ok(())
            }
            _ => self.shows(),
        }
    }

    /// The cell before the separator at `sep` ends; the next starts at `next`.
    fn end_cell(&mut self, sep: usize, next: usize) -> Result<(), Unsupported> {
        let range = self.cell_start..sep;
        let (content, span, multicolumn) = match self.multicolumn.take() {
            Some((m, inner)) => {
                let span = self.src(m.count.clone()).trim().parse().unwrap_or(1);
                (trimmed(self.text, inner), span, Some(m))
            }
            None => (trimmed(self.text, range.clone()), 1, None),
        };
        let column = self.cells.last().map_or(0, |c: &Cell| c.column + c.span);
        if column + span > self.columns {
            return Err(Unsupported::TooManyCells { row: self.rows.len() });
        }
        self.cells.push(Cell { range, content, column, span, multicolumn });
        self.cell_start = next;
        self.content = false;
        Ok(())
    }

    /// The row ends with `end` (its `\\`), which starts at `sep`.
    fn end_row(&mut self, sep: usize, end: Range<usize>) -> Result<(), Unsupported> {
        self.end_cell(sep, end.end)?;
        self.skip = end.end;
        let row = Row { rules: std::mem::take(&mut self.rules), cells: std::mem::take(&mut self.cells), end: Some(end) };
        self.rows.push(row);
        Ok(())
    }
}

/// `range` without the white space around it; for blank text, a place inside it to type at, with
/// a space on each side when there is room: `& x &`, or at the end of a line's indentation.
fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let s = text.get(range.clone()).unwrap_or_default();
    if s.trim().is_empty() {
        let (base, rest) = match s.rfind('\n') {
            Some(n) => (range.start + n + 1, s.get(n + 1..).unwrap_or_default()),
            None => (range.start, s),
        };
        let starts: Vec<usize> = rest.char_indices().map(|(i, _)| i).collect();
        let k = if base > range.start { starts.len().saturating_sub(1) } else { starts.len() / 2 };
        let at = base + starts.get(k).copied().unwrap_or(rest.len());
        return at..at;
    }
    let lead = s.len() - s.trim_start().len();
    let start = range.start + lead;
    start..start + s.trim().len()
}

/// Why an edit of the grid cannot be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    NoRow(usize),
    NoColumn(usize),
    /// A table keeps one row and one column.
    Last,
}

impl EditError {
    pub fn describe(&self) -> String {
        match self {
            EditError::NoRow(r) => format!("the table has no row {}", r + 1),
            EditError::NoColumn(c) => format!("the table has no column {}", c + 1),
            EditError::Last => "a table keeps one row and one column at least".to_owned(),
        }
    }
}

/// Table borders, as the toolbar offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Borders {
    None,
    /// `|` between all columns and `\hline` between all rows.
    All,
    /// booktabs: `\toprule`, `\midrule` under the first row, `\bottomrule`, no `|`.
    Booktabs,
}

impl Borders {
    pub fn from_name(name: &str) -> Option<Borders> {
        match name {
            "none" => Some(Borders::None),
            "all" => Some(Borders::All),
            "booktabs" => Some(Borders::Booktabs),
            _ => None,
        }
    }
}

/// An edit of a grid: its changes of the source (in order, not overlapping), and a package the
/// document must load for it, which is not added silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEdit {
    pub changes: Vec<Change>,
    pub package: Option<&'static str>,
}

fn change(range: Range<usize>, text: impl Into<String>) -> Change {
    Change { range, text: text.into() }
}

fn sorted(mut changes: Vec<Change>) -> TableEdit {
    changes.sort_by_key(|c| (c.range.start, c.range.end));
    TableEdit { changes, package: None }
}

impl Table {
    fn row(&self, r: usize) -> Result<&Row, EditError> {
        self.rows.get(r).ok_or(EditError::NoRow(r))
    }

    fn column(&self, c: usize) -> Result<&Column, EditError> {
        self.columns.get(c).ok_or(EditError::NoColumn(c))
    }

    /// The text of a cell (as source) made `source`.
    pub fn set_cell(&self, r: usize, c: usize, source: &str) -> Result<TableEdit, EditError> {
        let (_, cell) = self.row(r)?.cell_at(c).ok_or(EditError::NoColumn(c))?;
        Ok(sorted(vec![change(cell.content.clone(), source)]))
    }

    /// Where a cell ends: its separator, or, for the last cell of a last row without `\\`, its text.
    fn cell_end(&self, cell: &Cell) -> usize {
        if cell.range.end >= self.body.end { cell.end() } else { cell.range.end }
    }

    /// The indentation of the line `at` is on, when only white space comes before it there.
    fn indent_before(&self, text: &str, at: usize) -> Option<(usize, String)> {
        let line_start = text.get(..at)?.rfind('\n').map_or(0, |n| n + 1);
        let lead = text.get(line_start..at)?;
        lead.trim().is_empty().then(|| (line_start, lead.to_owned()))
    }

    /// An empty row of the table's columns: `  &  & ` .
    fn empty_row(&self) -> String {
        vec![""; self.columns.len()].join(" & ")
    }

    /// An empty row above (`below` false) or below row `r`.
    pub fn insert_row(&self, text: &str, r: usize, below: bool) -> Result<TableEdit, EditError> {
        let row = self.row(r)?;
        if below && let Some(next) = self.rows.get(r + 1) {
            // Above the next row, under the rules above it: a row under a header's rule is a row
            // of the body.
            return self.row_above(text, next);
        }
        if !below {
            return self.row_above(text, row);
        }
        let start = row.start();
        let indent = self.indent_before(text, start).map(|(_, i)| i).unwrap_or_default();
        let new = self.empty_row();
        // With every border drawn, the new row gets its rule too.
        let rule = if self.borders() == Some(Borders::All) { format!("\n{indent}\\hline") } else { String::new() };
        let changes = match &row.end {
            Some(end) => vec![change(end.end..end.end, format!("{rule}\n{indent}{new} \\\\"))],
            None => {
                // The last row gets the `\\` it lacks, and the new row ends as it did.
                let last = row.cells.last().map_or(start, Cell::end);
                vec![change(last..last, format!(" \\\\{rule}\n{indent}{new}"))]
            }
        };
        Ok(sorted(changes))
    }

    /// An empty row above `row`, under the rules above it.
    fn row_above(&self, text: &str, row: &Row) -> Result<TableEdit, EditError> {
        let start = row.start();
        let new = self.empty_row();
        let all = self.borders() == Some(Borders::All);
        let changes = match self.indent_before(text, start) {
            Some((line_start, indent)) => {
                let rule = if all { format!("{indent}\\hline\n") } else { String::new() };
                vec![change(line_start..line_start, format!("{indent}{new} \\\\\n{rule}"))]
            }
            None => vec![change(start..start, format!("{new} \\\\ {}", if all { "\\hline " } else { "" }))],
        };
        Ok(sorted(changes))
    }

    /// Row `r` removed with its line, keeping the rules above it.
    pub fn delete_row(&self, text: &str, r: usize) -> Result<TableEdit, EditError> {
        let row = self.row(r)?;
        if self.rows.len() < 2 {
            return Err(EditError::Last);
        }
        let start = row.start();
        let end = row.end.as_ref().map_or_else(|| row.cells.last().map_or(start, Cell::end), |e| e.end);
        let range = match self.indent_before(text, start) {
            Some((line_start, _)) => line_start..line_end(text, end),
            None => start..end,
        };
        let mut changes = vec![change(range, "")];
        if r > 0 && self.borders() == Some(Borders::All) {
            // One rule between the rows that are left.
            changes.extend(row.rules.iter().map(|rule| change(removal(text, rule.range.clone()), "")));
        }
        if row.end.is_none()
            && let Some(prev) = r.checked_sub(1).and_then(|p| self.rows.get(p))
            && let Some(e) = &prev.end
        {
            // The row before becomes the last: its `\\` goes too, when nothing follows it.
            if self.trailing.is_empty() {
                changes.push(change(spaces_before(text, e.start)..e.end, ""));
            }
        }
        Ok(sorted(changes))
    }

    /// The letter of a new column next to column `c`.
    fn new_letter(&self, c: usize) -> &'static str {
        match self.columns.get(c).map(|col| col.align) {
            Some(a @ (Align::Left | Align::Center | Align::Right)) => a.letter(),
            _ => "l",
        }
    }

    /// An empty column to the left (`right` false) or the right of column `c`. A merged cell that
    /// takes column `c` and the new column's place grows by one.
    pub fn insert_column(&self, text: &str, c: usize, right: bool) -> Result<TableEdit, EditError> {
        let col = self.column(c)?;
        let letter = self.new_letter(c);
        let mut changes = Vec::new();
        let barred = |i: usize| self.bars.get(i).is_some_and(|b| !b.is_empty());
        if right {
            let bar = if barred(c + 1) { "|" } else { "" };
            changes.push(change(col.range.end..col.range.end, format!("{bar}{letter}")));
        } else {
            let bar = if barred(c) { "|" } else { "" };
            changes.push(change(col.range.start..col.range.start, format!("{letter}{bar}")));
        }
        for row in &self.rows {
            let Some((_, cell)) = row.cell_at(c) else { continue };
            let inside = if right { cell.column + cell.span - 1 > c } else { cell.column < c };
            if inside {
                if let Some(m) = &cell.multicolumn {
                    changes.push(change(m.count.clone(), (cell.span + 1).to_string()));
                }
                continue;
            }
            if right {
                let at = self.cell_end(cell);
                let spaced = text.get(..at).is_some_and(|s| s.ends_with(char::is_whitespace));
                changes.push(change(at..at, if spaced { "& " } else { " & " }));
            } else {
                let at = cell.start();
                changes.push(change(at..at, "& "));
            }
        }
        Ok(sorted(changes))
    }

    /// Column `c` removed with its cells; a merged cell that takes it shrinks by one.
    pub fn delete_column(&self, c: usize) -> Result<TableEdit, EditError> {
        let col = self.column(c)?;
        if self.columns.len() < 2 {
            return Err(EditError::Last);
        }
        let mut changes = Vec::new();
        // The column's letter with the bars after it (or, for the last column, before it).
        let after = self.bars.get(c + 1).cloned().unwrap_or_default();
        let before = self.bars.get(c).cloned().unwrap_or_default();
        let spec = if c + 1 < self.columns.len() || before.is_empty() {
            col.range.start..after.last().map_or(col.range.end, |b| b + 1)
        } else {
            before.first().copied().unwrap_or(col.range.start)..col.range.end
        };
        changes.push(change(spec, ""));
        for row in &self.rows {
            let Some((k, cell)) = row.cell_at(c) else { continue };
            if cell.span > 1 {
                if let Some(m) = &cell.multicolumn {
                    changes.push(change(m.count.clone(), (cell.span - 1).to_string()));
                }
                continue;
            }
            let range = match (k.checked_sub(1).and_then(|p| row.cells.get(p)), row.cells.get(k + 1)) {
                // From the `&` before it to its end.
                (Some(prev), _) => prev.range.end..self.cell_end(cell),
                (None, Some(next)) => cell.start()..next.start(),
                (None, None) => cell.multicolumn.as_ref().map_or(cell.content.clone(), |m| m.range.clone()),
            };
            changes.push(change(range, ""));
        }
        Ok(sorted(changes))
    }

    /// Column `c` set flush left, centred or flush right.
    pub fn set_align(&self, c: usize, align: Align) -> Result<TableEdit, EditError> {
        let col = self.column(c)?;
        Ok(sorted(vec![change(col.range.clone(), align.letter())]))
    }

    /// The borders as they are now, when they are one of the three styles.
    pub fn borders(&self) -> Option<Borders> {
        let bars = self.bars.iter().filter(|b| !b.is_empty()).count();
        let all_rules = self.rules().all(|(_, rules)| rules.iter().any(|r| r.kind != RuleKind::Partial));
        let no_rules = self.rules().all(|(_, rules)| rules.is_empty());
        if bars == 0 && no_rules {
            return Some(Borders::None);
        }
        if bars == self.bars.len() && all_rules && self.rules().all(|(_, rules)| rules.iter().all(|r| r.kind == RuleKind::Line)) {
            return Some(Borders::All);
        }
        let booktabs = self.rules().all(|(i, rules)| {
            let kinds: Vec<RuleKind> = rules.iter().map(|r| r.kind).filter(|k| *k != RuleKind::Partial).collect();
            match i {
                0 => kinds == [RuleKind::Top],
                1 if self.rows.len() > 1 => kinds == [RuleKind::Mid],
                _ if i == self.rows.len() => kinds == [RuleKind::Bottom],
                _ => kinds.is_empty(),
            }
        });
        (bars == 0 && booktabs).then_some(Borders::Booktabs)
    }

    /// The rules at each place between rows: place `i` above row `i`, the last below the last row.
    fn rules(&self) -> impl Iterator<Item = (usize, &[Rule])> {
        self.rows.iter().map(|r| r.rules.as_slice()).chain(std::iter::once(self.trailing.as_slice())).enumerate()
    }

    /// The borders set to `style`: the specification's bars and the rules between rows, each
    /// added, renamed or removed where it differs.
    pub fn set_borders(&self, text: &str, style: Borders) -> Result<TableEdit, EditError> {
        let mut changes = Vec::new();
        // The bars of the specification.
        for (i, bars) in self.bars.iter().enumerate() {
            match style {
                Borders::All if bars.is_empty() => {
                    let at = match self.columns.get(i) {
                        Some(col) => col.range.start,
                        None => self.columns.last().map_or(self.spec.end, |c| c.range.end),
                    };
                    changes.push(change(at..at, "|"));
                }
                Borders::None | Borders::Booktabs => changes.extend(bars.iter().map(|b| change(*b..b + 1, ""))),
                Borders::All => {}
            }
        }
        let places: Vec<(usize, Vec<Rule>)> = self.rules().map(|(i, r)| (i, r.to_vec())).collect();
        let last = self.rows.len();
        for (i, rules) in places {
            let want = match style {
                Borders::None => None,
                Borders::All => Some(RuleKind::Line),
                Borders::Booktabs => match i {
                    0 => Some(RuleKind::Top),
                    _ if i == last => Some(RuleKind::Bottom),
                    1 => Some(RuleKind::Mid),
                    _ => None,
                },
            };
            let name = |k: RuleKind| match k {
                RuleKind::Top => "\\toprule",
                RuleKind::Mid => "\\midrule",
                RuleKind::Bottom => "\\bottomrule",
                RuleKind::Line | RuleKind::Partial => "\\hline",
            };
            let mut kept = false;
            for rule in &rules {
                if rule.kind == RuleKind::Partial {
                    // Under some columns: kept, in the style's own command.
                    let src = text.get(rule.range.clone()).unwrap_or_default();
                    match style {
                        Borders::None => changes.push(change(removal(text, rule.range.clone()), "")),
                        Borders::Booktabs if src.starts_with("\\cline") => {
                            changes.push(change(rule.range.start..rule.range.start + "\\cline".len(), "\\cmidrule"));
                        }
                        Borders::All if src.starts_with("\\cmidrule") => {
                            let args = src.find('{').map_or(rule.range.end, |n| rule.range.start + n);
                            changes.push(change(rule.range.start..args, "\\cline"));
                        }
                        _ => {}
                    }
                    continue;
                }
                match want {
                    Some(k) if !kept => {
                        kept = true;
                        if rule.kind != k {
                            changes.push(change(rule.range.clone(), name(k)));
                        }
                    }
                    _ => changes.push(change(removal(text, rule.range.clone()), "")),
                }
            }
            if let Some(k) = want
                && !kept
            {
                changes.extend(self.add_rule(text, i, name(k)));
            }
        }
        let mut edit = sorted(changes);
        if style == Borders::Booktabs {
            edit.package = Some("booktabs");
        }
        Ok(edit)
    }

    /// A rule at place `i` (above row `i`, or below the last row), on a line of its own.
    fn add_rule(&self, text: &str, i: usize, name: &str) -> Vec<Change> {
        if let Some(row) = self.rows.get(i) {
            let start = row.rules.first().map_or(row.start(), |r| r.range.start);
            return match self.indent_before(text, start) {
                Some((line_start, indent)) => vec![change(line_start..line_start, format!("{indent}{name}\n"))],
                None => vec![change(start..start, format!("{name} "))],
            };
        }
        let Some(row) = self.rows.last() else { return Vec::new() };
        let indent = self.indent_before(text, row.start()).map(|(_, i)| i).unwrap_or_default();
        match &row.end {
            Some(end) => {
                // Above the rules already below the last row (partial ones).
                let at = self.trailing.first().map_or(end.end, |r| r.range.start);
                if at == end.end { vec![change(at..at, format!("\n{indent}{name}"))] } else { vec![change(at..at, format!("{name}\n{indent}"))] }
            }
            None => {
                let at = row.cells.last().map_or(row.start(), Cell::end);
                vec![change(at..at, format!(" \\\\\n{indent}{name}"))]
            }
        }
    }
}

/// The start of the spaces and tabs before `at`.
fn spaces_before(text: &str, at: usize) -> usize {
    let before = text.get(..at).unwrap_or_default();
    at - (before.len() - before.trim_end_matches([' ', '\t']).len())
}

/// After the rest of the line from `at` when it is only white space (with its line end).
fn line_end(text: &str, at: usize) -> usize {
    let rest = text.get(at..).unwrap_or_default();
    match rest.find('\n') {
        Some(n) if rest.get(..n).is_some_and(|s| s.trim().is_empty()) => at + n + 1,
        _ => at,
    }
}

/// What removing the source `range` takes: its whole line when nothing else is on it, else the
/// spaces before it.
fn removal(text: &str, range: Range<usize>) -> Range<usize> {
    let line_start = text.get(..range.start).and_then(|s| s.rfind('\n')).map_or(0, |n| n + 1);
    let alone_before = text.get(line_start..range.start).is_some_and(|s| s.trim().is_empty());
    let end = line_end(text, range.end);
    if alone_before && end > range.end {
        return line_start..end;
    }
    spaces_before(text, range.start)..range.end
}

/// Whether `text` loads package `name` with `\usepackage` (outside comments).
pub fn loads_package(text: &str, name: &str) -> bool {
    let tree = tree::parse(text);
    tree.iter().any(|n| match &n.kind {
        Kind::Command { name: c, args, .. } if c == "usepackage" || c == "RequirePackage" => {
            args.iter().filter(|a| !a.optional).any(|a| text.get(a.inner.clone()).is_some_and(|s| s.split(',').any(|p| p.trim() == name)))
        }
        _ => false,
    })
}
