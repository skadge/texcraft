//! Tables in the formatted view (docs/ROADMAP.md, F4): a `tabular` drawn as a grid whose cells are
//! edited in place, each cell a line of the view's text ([`crate::formatted_edit`]), and a toolbar
//! over the table holding the caret: rows, columns, a column's alignment and the borders, each a
//! `table.edit` command. A table the grid cannot show faithfully stays as its source, with the
//! feature that keeps it so.
//!
//! - **Keys:** Tab and Shift+Tab go to the next and the previous cell, Up and Down to the cell
//!   above and below from a cell's first or last line; Escape goes to the toolbar, and Escape there
//!   back to the cell.
//! - **Accessibility:** the grid is a table of rows of cells, each cell with its row and column
//!   (and its span); the toolbar is a toolbar.

use std::ops::Range;

use egui::{Align, Rect, Sense, UiBuilder, pos2, vec2};
use serde_json::json;
use texcraft_latex::table::{Align as ColumnAlign, Borders, RuleKind, Table, Unsupported};
use texcraft_latex::view::{Float, Inline, InlineKind, Style, TableBlock};

use super::{Cx, Para, code, paragraph, plain};
use crate::widgets;

/// Where a line of the view stands in a grid: the table (by its first byte), the cell's row and
/// column, and the table's numbers of rows and columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellAt {
    pub table: usize,
    pub row: usize,
    pub column: usize,
    pub rows: usize,
    pub columns: usize,
}

/// The toolbar's id for the table starting at `start`, to give it the keyboard.
fn toolbar_id(start: usize) -> egui::Id {
    egui::Id::unique(("table-toolbar", start))
}

/// The cell holding byte `at`: its row and first column.
fn cell_at(table: &Table, at: usize) -> Option<(usize, usize)> {
    table.rows.iter().enumerate().find_map(|(r, row)| {
        let cell = row.cells.iter().find(|c| c.range.start <= at && at <= c.range.end.max(c.end()))?;
        Some((r, cell.column))
    })
}

pub(super) fn table_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, tb: &TableBlock) {
    let t = cx.t;
    let table = &tb.table;
    if let Some(f) = tb.float.as_ref().filter(|f| f.caption_above) {
        caption(ui, cx, range, f);
        ui.add_space(2.0 * t.unit);
    }
    let caret = cx.edit_caret.filter(|c| table.range.start < *c && *c < table.range.end);
    if let Some(at) = caret {
        // Floating over what is above the table: the grid does not move under the pointer when
        // a click in it shows the toolbar.
        let width = ui.available_width();
        let top = ui.cursor().top();
        let left = ui.cursor().left();
        let clip = ui.clip_rect();
        let area = egui::Area::new(toolbar_id(table.range.start).with("area"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(pos2(left, top - t.unit))
            .constrain_to(clip);
        area.show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_max_width(width);
                toolbar(ui, cx, table, cell_at(table, at));
            });
        });
    }
    grid(ui, cx, tb, caret.is_some());
    if let Some(f) = tb.float.as_ref().filter(|f| !f.caption_above) {
        ui.add_space(2.0 * t.unit);
        caption(ui, cx, range, f);
    }
}

/// "Table." and the caption, with the label after it.
fn caption(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, f: &Float) {
    let t = cx.t;
    let mut inlines = vec![Inline {
        kind: InlineKind::Text { text: format!("{} ", tl!("Table.")), style: Style { bold: true, sans: true, ..Style::default() } },
        range: range.start..range.start,
    }];
    inlines.extend(f.caption.iter().cloned());
    if let Some(label) = &f.label {
        inlines.push(Inline {
            kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() },
            range: label.range.start..label.range.start,
        });
        inlines.push(label.clone());
    }
    let para = Para { size: t.reading_size * 0.9, bold: false, color: t.text, role: egui::Role::Caption, empty_at: None, cell: None };
    let _ = paragraph(ui, cx, &inlines, range, &para);
}

/// The toolbar of the table holding the caret; `cell` is the caret's cell.
fn toolbar(ui: &mut egui::Ui, cx: &mut Cx<'_>, table: &Table, cell: Option<(usize, usize)>) {
    let t = cx.t;
    let id = toolbar_id(table.range.start);
    let mut bar = ui.new_child(
        UiBuilder::new().id_salt(id).max_rect(ui.available_rect_before_wrap()).layout(egui::Layout::left_to_right(Align::Min).with_main_wrap(true)),
    );
    bar.spacing_mut().item_spacing = vec2(t.unit, t.unit);
    let at = table.range.start;
    let mut action = None;
    let mut focused = false;
    let (row, column) = cell.unwrap_or((0, 0));
    let no_cell = cell.is_none().then(|| tl!("Put the caret in a cell first."));
    let mut first = None;
    let mut button = |ui: &mut egui::Ui, label: &str, op: &'static str, disabled: Option<&str>| {
        let r = widgets::text_button(ui, &t, label, disabled.is_none());
        first.get_or_insert(r.id);
        focused |= r.has_focus();
        let r = match disabled {
            Some(why) => r.on_hover_text(why),
            None => r,
        };
        if r.clicked() {
            action = Some(json!({"op": op, "row": row, "column": column}));
        }
    };
    let last_row = (table.rows.len() < 2).then(|| tl!("A table keeps one row at least."));
    let last_column = (table.columns.len() < 2).then(|| tl!("A table keeps one column at least."));
    button(&mut bar, tl!("Row above"), "insert_row_above", no_cell);
    button(&mut bar, tl!("Row below"), "insert_row_below", no_cell);
    button(&mut bar, tl!("Delete row"), "delete_row", no_cell.or(last_row));
    separator(&mut bar, &t);
    button(&mut bar, tl!("Column left"), "insert_column_left", no_cell);
    button(&mut bar, tl!("Column right"), "insert_column_right", no_cell);
    button(&mut bar, tl!("Delete column"), "delete_column", no_cell.or(last_column));
    separator(&mut bar, &t);
    let align = table.columns.get(column).map(|c| c.align);
    let selected = match align {
        Some(ColumnAlign::Left) => 0,
        Some(ColumnAlign::Center) => 1,
        Some(ColumnAlign::Right) => 2,
        _ => usize::MAX,
    };
    let why = no_cell.or_else(|| (align == Some(ColumnAlign::Paragraph)).then(|| tl!("This column is a paragraph of a set width.")));
    let options = [(tl!("Left"), why), (tl!("Center"), why), (tl!("Right"), why)];
    if let Some(i) = widgets::segmented(&mut bar, &t, "table-align", &options, selected) {
        let name = ["left", "center", "right"].get(i).copied().unwrap_or("left");
        action = Some(json!({"op": "align_column", "row": row, "column": column, "align": name}));
    }
    separator(&mut bar, &t);
    let selected = match table.borders() {
        Some(Borders::None) => 0,
        Some(Borders::All) => 1,
        Some(Borders::Booktabs) => 2,
        None => usize::MAX,
    };
    let options = [(tl!("No borders"), None), (tl!("All borders"), None), (tl!("Booktabs rules"), None)];
    if let Some(i) = widgets::segmented(&mut bar, &t, "table-borders", &options, selected) {
        let name = ["none", "all", "booktabs"].get(i).copied().unwrap_or("none");
        action = Some(json!({"op": "borders", "borders": name, "row": row, "column": column}));
    }
    // Any of its controls holding the keyboard.
    focused |= bar.ctx().memory(|m| m.focused()).is_some_and(|f| bar.ctx().read_response(f).is_some_and(|r| bar.min_rect().contains_rect(r.rect)));
    let rect = bar.min_rect();
    ui.allocate_rect(rect, Sense::hover());
    let name = tl!("Table");
    bar.ctx().accesskit_node_builder(bar.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Toolbar);
        node.set_label(name);
    });
    if std::mem::take(&mut cx.focus_toolbar)
        && let Some(first) = first
    {
        ui.ctx().memory_mut(|m| m.request_focus(first));
    }
    cx.toolbar_focused |= focused;
    if let Some(mut a) = action {
        a["at"] = json!(at);
        cx.table_action = Some(a);
    }
}

fn separator(ui: &mut egui::Ui, t: &crate::theme::Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0 + 2.0 * t.unit, t.toolbar_height - 4.0 * t.unit), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), t.hairline());
}

/// How a cell's text is set: its column's alignment, or its `\multicolumn`'s.
fn align_of(cx: &Cx<'_>, table: &Table, cell: &texcraft_latex::table::Cell) -> Align {
    let letter = match &cell.multicolumn {
        Some(m) => cx.text.get(m.spec.clone()).and_then(|s| s.chars().find(|c| matches!(c, 'l' | 'c' | 'r'))),
        None => table.columns.get(cell.column).map(|c| c.align.letter().chars().next().unwrap_or('l')),
    };
    match letter {
        Some('c') => Align::Center,
        Some('r') => Align::Max,
        _ => Align::Min,
    }
}

/// The columns a partial rule (`\cline{2-3}`) runs under.
fn rule_columns(src: &str, columns: usize) -> Range<usize> {
    let inside = src.rsplit_once('{').map(|(_, r)| r.trim_end_matches('}')).unwrap_or_default();
    let mut ends = inside.split('-').map(|n| n.trim().parse::<usize>().ok());
    match (ends.next().flatten(), ends.next().flatten()) {
        (Some(a), Some(b)) if a >= 1 && a <= b => (a - 1).min(columns)..b.min(columns),
        _ => 0..columns,
    }
}

fn grid(ui: &mut egui::Ui, cx: &mut Cx<'_>, tb: &TableBlock, selected: bool) {
    let t = cx.t;
    let table = &tb.table;
    let columns = table.columns.len().max(1);
    let size = t.reading_size * 0.95;
    let pad = vec2(2.5 * t.unit, 1.5 * t.unit);
    let width = ui.available_width();
    // Each column as wide as its widest cell, scaled down to the measure when they are too wide.
    let mut natural = vec![size * 2.0; columns];
    // Formulas and chips take more room than their words: some to spare.
    let measure = |inlines: &[Inline]| {
        let words = plain(cx.text, inlines);
        let shown = ui.painter().layout_no_wrap(words, t.reading(size, false, false), t.text).size().x;
        shown * 1.15 + 2.0 * pad.x + 2.0
    };
    for (row, cells) in table.rows.iter().zip(&tb.cells) {
        for (cell, inlines) in row.cells.iter().zip(cells) {
            if cell.span == 1
                && let Some(w) = natural.get_mut(cell.column)
            {
                *w = w.max(measure(inlines));
            }
        }
    }
    // A merged cell wider than its columns widens them evenly.
    for (row, cells) in table.rows.iter().zip(&tb.cells) {
        for (cell, inlines) in row.cells.iter().zip(cells) {
            let span = cell.column..(cell.column + cell.span).min(columns);
            if cell.span > 1 && !span.is_empty() {
                let have: f32 = natural.get(span.clone()).map_or(0.0, |w| w.iter().sum());
                let extra = (measure(inlines) - have).max(0.0) / span.len() as f32;
                natural.get_mut(span).into_iter().flatten().for_each(|w| *w += extra);
            }
        }
    }
    let total: f32 = natural.iter().sum();
    let scale = if total > width { width / total } else { 1.0 };
    let widths: Vec<f32> = natural.iter().map(|w| w * scale).collect();
    let left = ui.cursor().left();
    let xs: Vec<f32> = std::iter::once(left)
        .chain(widths.iter().scan(left, |x, w| {
            *x += w;
            Some(*x)
        }))
        .collect();
    let right = xs.last().copied().unwrap_or(left);
    let top = ui.cursor().top();
    let area = Rect::from_min_max(pos2(left, top), pos2(right.max(left + 1.0), top + 1.0e6));
    let mut table_ui = ui.new_child(UiBuilder::new().id_salt(("grid", table.range.start)).max_rect(area));
    // The cells' backgrounds and lines go under their text.
    let under = table_ui.painter().add(egui::Shape::Noop);
    let mut row_tops = Vec::with_capacity(table.rows.len() + 1);
    let mut y = top;
    let align = cx.align;
    let rows = table.rows.len();
    for (r, (row, cells)) in table.rows.iter().zip(&tb.cells).enumerate() {
        row_tops.push(y);
        let row_rect = Rect::from_min_max(pos2(left, y), pos2(right.max(left + 1.0), y + 1.0e5));
        let mut row_ui = table_ui.new_child(UiBuilder::new().id_salt(("row", r)).max_rect(row_rect));
        let mut height = (size * t.reading_leading).round() + 2.0 * pad.y;
        for (cell, inlines) in row.cells.iter().zip(cells) {
            let (Some(x0), Some(x1)) = (xs.get(cell.column), xs.get(cell.column + cell.span).or(xs.last())) else { continue };
            let rect = Rect::from_min_max(pos2(x0 + pad.x, y + pad.y), pos2((x1 - pad.x).max(x0 + pad.x + 1.0), y + 1.0e5));
            let mut cell_ui = row_ui.new_child(UiBuilder::new().id_salt(("cell", cell.column)).max_rect(rect));
            cell_ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            cx.align = align_of(cx, table, cell);
            let at = CellAt { table: table.range.start, row: r, column: cell.column, rows, columns };
            let para = Para { size, bold: false, color: t.text, role: egui::Role::Cell, empty_at: Some(cell.content.start), cell: Some(at) };
            let resp = paragraph(&mut cell_ui, cx, inlines, &cell.content, &para);
            let (row_index, column_index, span) = (r, cell.column, cell.span);
            ui.ctx().accesskit_node_builder(resp.id, |node| {
                node.set_role(egui::accesskit::Role::Cell);
                node.set_row_index(row_index);
                node.set_column_index(column_index);
                if span > 1 {
                    node.set_column_span(span);
                }
            });
            height = height.max(resp.rect.height() + 2.0 * pad.y);
        }
        let (row_id, columns_n) = (row_ui.unique_id(), columns);
        ui.ctx().accesskit_node_builder(row_id, |node| {
            node.set_role(egui::accesskit::Role::Row);
            node.set_row_index(r);
            node.set_column_count(columns_n);
        });
        y += height;
    }
    cx.align = align;
    row_tops.push(y);
    let bottom = y.max(top + 1.0);
    let rect = Rect::from_min_max(pos2(left, top), pos2(right.max(left + 1.0), bottom));
    ui.allocate_rect(Rect::from_min_max(pos2(left, top), pos2(left + width, bottom)), Sense::hover());
    let label = tl!("Table of {rows} rows and {columns} columns").replace("{rows}", &rows.to_string()).replace("{columns}", &columns.to_string());
    let table_id = table_ui.unique_id();
    ui.ctx().accesskit_node_builder(table_id, |node| {
        node.set_role(egui::accesskit::Role::Table);
        node.set_label(label);
        node.set_row_count(rows);
        node.set_column_count(columns);
    });
    // The borders as LaTeX draws them; with the caret inside, faint lines show every cell.
    let mut shapes = Vec::new();
    if selected {
        shapes.push(egui::Shape::rect_filled(rect, 0.0, crate::theme::blend(t.surface, t.current_line)));
        let faint = egui::Stroke::new(1.0, t.separator);
        for x in &xs {
            shapes.push(egui::Shape::line_segment([pos2(*x, top), pos2(*x, bottom)], faint));
        }
        for y in &row_tops {
            shapes.push(egui::Shape::line_segment([pos2(left, *y), pos2(right, *y)], faint));
        }
    }
    let ink = egui::Stroke::new(1.0, t.text);
    // The bars between columns, row by row: not through a merged cell.
    for (i, bars) in table.bars.iter().enumerate() {
        let Some(x) = xs.get(i).filter(|_| !bars.is_empty()) else { continue };
        for (r, row) in table.rows.iter().enumerate() {
            let (Some(y0), Some(y1)) = (row_tops.get(r), row_tops.get(r + 1)) else { continue };
            if row.cells.iter().any(|c| c.column < i && i < c.column + c.span) {
                continue;
            }
            shapes.push(egui::Shape::line_segment([pos2(*x, *y0), pos2(*x, *y1)], ink));
        }
    }
    let places = table.rows.iter().map(|r| r.rules.as_slice()).chain(std::iter::once(table.trailing.as_slice()));
    for (i, rules) in places.enumerate() {
        let Some(y) = row_tops.get(i) else { continue };
        for (k, rule) in rules.iter().enumerate() {
            let heavy = matches!(rule.kind, RuleKind::Top | RuleKind::Bottom);
            let stroke = egui::Stroke::new(if heavy { 1.6 } else { 0.8 }, t.text);
            let span = match rule.kind {
                RuleKind::Partial => rule_columns(cx.text.get(rule.range.clone()).unwrap_or_default(), columns),
                _ => 0..columns,
            };
            let (Some(x0), Some(x1)) = (xs.get(span.start), xs.get(span.end)) else { continue };
            let y = y + k as f32 * 2.0;
            shapes.push(egui::Shape::line_segment([pos2(*x0, y), pos2(*x1, y)], stroke));
        }
    }
    ui.painter().set(under, egui::Shape::Vec(shapes));
}

/// A table the grid cannot show: its source, with the feature that keeps it so.
pub(super) fn unsupported(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, why: &Unsupported) {
    let src = cx.text.get(range.clone()).unwrap_or_default().trim_end();
    let reason = reason(why);
    let tag = tl!("Table, shown as source: {reason}").replace("{reason}", &reason);
    let r = code(ui, cx, src, range.start, None, Some(&tag));
    let _ = r.on_hover_text(tl!("The table editor shows this table as its source, because it cannot show it faithfully as a grid."));
}

/// Why a table stays as source, in the reader's language.
fn reason(why: &Unsupported) -> String {
    match why {
        Unsupported::Unclosed => tl!("it is not closed, or has no column specification").to_owned(),
        Unsupported::ColumnType(t) => tl!("its columns use {type}").replace("{type}", t),
        Unsupported::Multirow => tl!("a cell spans rows (\\multirow)").to_owned(),
        Unsupported::Nested => tl!("a cell holds another table").to_owned(),
        Unsupported::Environment(name) => tl!("a cell holds a {name} environment").replace("{name}", name),
        Unsupported::RuleInRow => tl!("a rule is drawn in the middle of a row").to_owned(),
        Unsupported::BesideMulticolumn => tl!("a cell holds text besides its \\multicolumn").to_owned(),
        Unsupported::Multicolumn => tl!("a \\multicolumn cannot be read").to_owned(),
        Unsupported::TooManyCells { row } => tl!("row {n} has more cells than the table has columns").replace("{n}", &(row + 1).to_string()),
        Unsupported::Structure(what) => tl!("its body holds {what}").replace("{what}", what),
    }
}
