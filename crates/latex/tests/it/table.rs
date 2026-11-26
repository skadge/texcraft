//! `tabular` as a grid: what it reads, what it refuses, and edits that change only what they must.

use texcraft_latex::edit::apply;
use texcraft_latex::table::{self, Align, Borders, RuleKind, Table, TableEdit, Unsupported};

const BOOKTABS: &str = "\\begin{tabular}{rrr}
    \\toprule
    Length (m) & Measured $T$ (s) & Predicted \\& more \\\\
    \\midrule
    0.25 & {1 & 2} & 1.003 \\\\
    0.50 & 1.42 & 1.419 \\\\
    \\bottomrule
  \\end{tabular}";

type R = Result<(), String>;

/// A table and a test of why it is refused.
type Case = (&'static str, fn(&Unsupported) -> bool);

fn grid(text: &str) -> Result<Table, String> {
    match table::find(text, 1) {
        Some(Ok(t)) => Ok(t),
        other => Err(format!("not a grid: {other:?} in {text:?}")),
    }
}

fn refusal(text: &str) -> Result<Unsupported, String> {
    match table::find(text, 1) {
        Some(Err(why)) => Ok(why),
        other => Err(format!("read as a grid: {other:?}")),
    }
}

/// The source of each cell, row by row.
fn cells(text: &str) -> Result<Vec<Vec<String>>, String> {
    Ok(grid(text)?.rows.iter().map(|r| r.cells.iter().map(|c| text[c.content.clone()].to_owned()).collect()).collect())
}

fn edited(text: &str, e: Result<TableEdit, table::EditError>) -> Result<String, String> {
    let e = e.map_err(|err| err.describe())?;
    assert!(e.changes.windows(2).all(|w| w[0].range.end <= w[1].range.start), "overlapping changes: {:?}", e.changes);
    Ok(apply(text, &e.changes))
}

#[test]
fn rows_cells_and_rules_are_read_with_groups_and_escapes_kept_in_their_cell() -> R {
    let t = grid(BOOKTABS)?;
    assert_eq!(t.columns.iter().map(|c| c.align).collect::<Vec<_>>(), [Align::Right; 3]);
    assert_eq!(cells(BOOKTABS)?, [["Length (m)", "Measured $T$ (s)", "Predicted \\& more"], ["0.25", "{1 & 2}", "1.003"], ["0.50", "1.42", "1.419"]]);
    assert_eq!(t.rows[0].rules.iter().map(|r| r.kind).collect::<Vec<_>>(), [RuleKind::Top]);
    assert_eq!(t.rows[1].rules.iter().map(|r| r.kind).collect::<Vec<_>>(), [RuleKind::Mid]);
    assert_eq!(t.trailing.iter().map(|r| r.kind).collect::<Vec<_>>(), [RuleKind::Bottom]);
    assert_eq!(t.borders(), Some(Borders::Booktabs));
    Ok(())
}

#[test]
fn multicolumn_spans_and_a_last_row_without_its_line_end() -> R {
    let text = "\\begin{tabular}{|l|c|r|}\\hline\n\\multicolumn{2}{|c|}{Both} & x\\\\ \\hline\na & b & c\n\\end{tabular}";
    let t = grid(text)?;
    assert_eq!(t.rows.len(), 2);
    let first = &t.rows[0].cells;
    assert_eq!((first[0].column, first[0].span, &text[first[0].content.clone()]), (0, 2, "Both"));
    assert_eq!(first[1].column, 2);
    assert!(t.rows[1].end.is_none());
    assert_eq!(t.borders(), None, "no rule under the last row: not every border");
    Ok(())
}

#[test]
fn what_the_grid_cannot_show_is_named() -> R {
    let cases: [Case; 7] = [
        ("\\begin{tabular}{ll}\\multirow{2}{*}{a} & b\\end{tabular}", |w| *w == Unsupported::Multirow),
        ("\\begin{tabular}{ll}\\begin{tabular}{l}a\\end{tabular} & b\\end{tabular}", |w| *w == Unsupported::Nested),
        ("\\begin{tabular}{@{}ll@{}}a & b\\end{tabular}", |w| matches!(w, Unsupported::ColumnType(t) if t.starts_with('@'))),
        ("\\begin{tabular}{>{\\bfseries}ll}a & b\\end{tabular}", |w| matches!(w, Unsupported::ColumnType(t) if t.starts_with('>'))),
        ("\\begin{tabular}{X}a\\end{tabular}", |w| matches!(w, Unsupported::ColumnType(t) if t == "X")),
        ("\\begin{tabular}{ll}a & b & c\\end{tabular}", |w| *w == Unsupported::TooManyCells { row: 0 }),
        ("\\begin{tabular}{ll}a & \\hline b\\end{tabular}", |w| *w == Unsupported::RuleInRow),
    ];
    for (text, ok) in cases {
        let why = refusal(text)?;
        assert!(ok(&why), "{text}: {why:?}");
        assert!(!why.describe().is_empty());
    }
    assert_eq!(refusal("\\begin{tabular}{ll}a & b")?, Unsupported::Unclosed);
    Ok(())
}

#[test]
fn a_cell_edit_changes_only_that_cell() -> R {
    let t = grid(BOOKTABS)?;
    let out = edited(BOOKTABS, t.set_cell(1, 2, "9.99"))?;
    assert_eq!(out, BOOKTABS.replacen("1.003", "9.99", 1));
    // An empty cell takes the text between its separators' spaces.
    let text = "\\begin{tabular}{lll}\na &  & c \\\\\n\\end{tabular}";
    let out = edited(text, grid(text)?.set_cell(0, 1, "b"))?;
    assert_eq!(out, "\\begin{tabular}{lll}\na & b & c \\\\\n\\end{tabular}");
    Ok(())
}

#[test]
fn inserting_and_deleting_rows_keeps_the_other_rows_and_the_rules() -> R {
    let t = grid(BOOKTABS)?;
    let below = edited(BOOKTABS, t.insert_row(BOOKTABS, 1, true))?;
    assert_eq!(cells(&below)?.len(), 4);
    assert!(cells(&below)?[2].iter().all(String::is_empty), "{below}");
    // Every line of the original is still there, in order, and the new one is indented as the rows.
    let added: Vec<&str> = below.lines().filter(|l| !BOOKTABS.lines().any(|o| o == *l)).collect();
    assert_eq!(added.len(), 1, "{below}");
    assert!(added[0].starts_with("    ") && added[0].trim_end().ends_with("\\\\"), "{below}");

    let above = edited(BOOKTABS, t.insert_row(BOOKTABS, 0, false))?;
    let g = grid(&above)?;
    assert!(g.rows[1].rules.is_empty() && g.rows[0].rules.len() == 1, "the new first row went above the top rule: {above}");
    assert_eq!(cells(&above)?[1][0], "Length (m)");

    let deleted = edited(BOOKTABS, t.delete_row(BOOKTABS, 1))?;
    assert_eq!(deleted, BOOKTABS.replacen("    0.25 & {1 & 2} & 1.003 \\\\\n", "", 1));
    // Inserting below a last row without its line end gives it one.
    let text = "\\begin{tabular}{ll}\n  a & b \\\\\n  c & d\n\\end{tabular}";
    let out = edited(text, grid(text)?.insert_row(text, 1, true))?;
    assert_eq!(cells(&out)?, [["a", "b"], ["c", "d"], ["", ""]]);
    let back = edited(&out, grid(&out)?.delete_row(&out, 2))?;
    assert_eq!(cells(&back)?, cells(text)?);
    // With every border drawn, a new row has its rule and a deleted one takes one away.
    let boxed = "\\begin{tabular}{|l|}\n\\hline\n  a \\\\\n\\hline\n  b \\\\\n\\hline\n\\end{tabular}";
    let more = edited(boxed, grid(boxed)?.insert_row(boxed, 0, true))?;
    assert_eq!(grid(&more)?.borders(), Some(Borders::All), "{more}");
    let fewer = edited(&more, grid(&more)?.delete_row(&more, 1))?;
    assert_eq!((cells(&fewer)?, fewer.matches("\\hline").count()), (cells(boxed)?, 3), "{fewer}");
    assert_eq!(grid(&fewer)?.borders(), Some(Borders::All), "{fewer}");
    Ok(())
}

#[test]
fn columns_are_inserted_and_deleted_in_the_specification_and_every_row() -> R {
    let text = "\\begin{tabular}{|l|c|}\n\\hline\n\\multicolumn{2}{|c|}{Head} \\\\\na & b \\\\\n\\hline\n\\end{tabular}";
    let t = grid(text)?;
    let out = edited(text, t.insert_column(text, 0, true))?;
    let g = grid(&out)?;
    assert_eq!(g.columns.len(), 3);
    assert_eq!(&out[g.spec.clone()], "|l|l|c|");
    assert_eq!(g.rows[0].cells[0].span, 3, "the merged cell did not grow: {out}");
    assert_eq!(cells(&out)?[1], ["a", "", "b"]);

    let back = edited(&out, g.delete_column(1))?;
    assert_eq!(back, text);

    let left = edited(text, t.insert_column(text, 0, false))?;
    assert_eq!(cells(&left)?[1], ["", "a", "b"]);
    assert_eq!(t.delete_column(0).map(|e| e.changes.len()).ok(), Some(3));
    Ok(())
}

#[test]
fn alignment_is_the_columns_letter_alone() -> R {
    let t = grid(BOOKTABS)?;
    let out = edited(BOOKTABS, t.set_align(1, Align::Center))?;
    assert_eq!(out, BOOKTABS.replacen("{rrr}", "{rcr}", 1));
    Ok(())
}

#[test]
fn borders_switch_between_none_all_and_booktabs_and_back() -> R {
    let t = grid(BOOKTABS)?;
    let none = edited(BOOKTABS, t.set_borders(BOOKTABS, Borders::None))?;
    assert_eq!(grid(&none)?.borders(), Some(Borders::None));
    assert_eq!(cells(&none)?, cells(BOOKTABS)?);
    assert!(!none.contains("rule"), "{none}");

    let all = edited(&none, grid(&none)?.set_borders(&none, Borders::All))?;
    assert_eq!(grid(&all)?.borders(), Some(Borders::All), "{all}");
    assert_eq!(cells(&all)?, cells(BOOKTABS)?);

    let e = grid(&all)?.set_borders(&all, Borders::Booktabs).unwrap_or_else(|e| panic!("{}", e.describe()));
    assert_eq!(e.package, Some("booktabs"), "booktabs is needed and must be said");
    let back = apply(&all, &e.changes);
    assert_eq!(grid(&back)?.borders(), Some(Borders::Booktabs), "{back}");
    assert_eq!(cells(&back)?, cells(BOOKTABS)?);
    // From booktabs to booktabs nothing changes.
    assert_eq!(t.set_borders(BOOKTABS, Borders::Booktabs).map(|e| e.changes.len()).ok(), Some(0));
    Ok(())
}

#[test]
fn a_header_row_is_its_cells_in_bold_with_a_rule_under_it() -> R {
    let text = "\\begin{tabular}{ll}\n  \\hline\n  Name & Value \\\\\n  a & 1 \\\\\n  \\hline\n\\end{tabular}";
    let t = grid(text)?;
    assert!(!t.header(text));
    let on = edited(text, t.set_header(text, true))?;
    assert_eq!(on, text.replace("Name & Value", "\\textbf{Name} & \\textbf{Value}").replace("\\\\\n  a", "\\\\\n  \\hline\n  a"));
    let t = grid(&on)?;
    assert!(t.header(&on));
    // Off takes the bold away and keeps the rules.
    let off = edited(&on, t.set_header(&on, false))?;
    assert_eq!(off, on.replace("\\textbf{Name}", "Name").replace("\\textbf{Value}", "Value"));
    // Booktabs gets its own rule; a table without rules gets none.
    let book = edited(BOOKTABS, grid(BOOKTABS)?.set_header(BOOKTABS, true))?;
    assert_eq!(book.matches("\\midrule").count(), 1, "{book}");
    let bare = "\\begin{tabular}{ll}\na & b \\\\\nc & d\n\\end{tabular}";
    let made = edited(bare, grid(bare)?.set_header(bare, true))?;
    assert_eq!(made, bare.replace("a & b", "\\textbf{a} & \\textbf{b}"));
    Ok(())
}

#[test]
fn cells_merge_into_a_multicolumn_and_split_back_leaving_other_rows_alone() -> R {
    let text = "\\begin{tabular}{|l|c|r|}\n  a & b & c \\\\\n  d & e & f \\\\\n\\end{tabular}";
    let t = grid(text)?;
    let merged = edited(text, t.merge(text, 0, 0, 2))?;
    assert_eq!(merged, text.replace("a & b & c", "\\multicolumn{2}{|l|}{a b} & c"));
    let t = grid(&merged)?;
    assert_eq!((t.rows[0].cells[0].span, t.rows[1].cells.len()), (2, 3));
    let split = edited(&merged, t.split(&merged, 0, 1))?;
    assert_eq!(split, text.replace("a & b & c", "a b & & c"));
    // The middle and last columns: the bar on the right only.
    let right = edited(text, grid(text)?.merge(text, 1, 1, 2))?;
    assert!(right.contains("d & \\multicolumn{2}{c|}{e f} \\\\"), "{right}");
    assert_eq!(grid(text)?.merge(text, 0, 2, 2).map_err(|e| e.describe()), Err("the table has no column 4".to_owned()));
    Ok(())
}

#[test]
fn a_bare_table_is_put_in_a_float_without_changing_its_lines() -> R {
    let text = "Before.\n\n  \\begin{tabular}{l}\n  a \\\\\n  \\end{tabular}\n\nAfter.\n";
    let t = table::find(text, text.find("tabular").unwrap_or(0)).ok_or("no table")?.map_err(|e| e.describe())?;
    let out = apply(text, &t.wrap_in_float(text, "Results.", "tab:r").changes);
    assert!(out.contains("  \\begin{table}[htbp]\n  \\centering\n  \\begin{tabular}{l}\n  a \\\\\n  \\end{tabular}\n  \\caption{Results.}\n  \\label{tab:r}\n  \\end{table}\n"), "{out}");
    assert!(out.starts_with("Before.\n\n") && out.ends_with("\n\nAfter.\n"));
    Ok(())
}

#[test]
fn a_package_is_loaded_only_by_usepackage_outside_comments() {
    assert!(table::loads_package("\\usepackage{amsmath, booktabs}\n", "booktabs"));
    assert!(!table::loads_package("% \\usepackage{booktabs}\n\\usepackage{array}", "booktabs"));
}

#[test]
fn hostile_input_never_breaks_the_grid_or_its_edits() -> R {
    let samples = [
        BOOKTABS,
        "\\begin{tabular}{|l|c|}\\hline\\multicolumn{2}{c}{é} \\\\[2pt] a&\\\\\\cline{1-2}\\cmidrule(lr){1-1}\\end{tabular}",
        "\\begin{tabular}{p{3cm}l}% x\n ü & {\\\\} \\tabularnewline\n\\end{tabular}",
    ];
    let mut checked = 0;
    for sample in samples {
        let bounds: Vec<usize> = (0..=sample.len()).filter(|i| sample.is_char_boundary(*i)).collect();
        let mut variants: Vec<String> = Vec::new();
        for &i in &bounds {
            variants.push(sample[..i].to_owned());
            for c in ['&', '\\', '{', '}', '|', '\n', '%'] {
                let mut s = sample.to_owned();
                s.insert(i, c);
                variants.push(s);
            }
        }
        for text in variants {
            for (_, found) in table::all(&text) {
                let Ok(t) = found else { continue };
                checked += 1;
                let mut edits = vec![t.set_cell(0, 0, "x"), t.set_align(0, Align::Left), t.delete_column(0)];
                for r in [0, t.rows.len().saturating_sub(1)] {
                    edits.extend([t.insert_row(&text, r, true), t.insert_row(&text, r, false), t.delete_row(&text, r)]);
                }
                for c in [0, t.columns.len().saturating_sub(1)] {
                    edits.extend([t.insert_column(&text, c, true), t.insert_column(&text, c, false)]);
                }
                edits.extend([Borders::None, Borders::All, Borders::Booktabs].map(|b| t.set_borders(&text, b)));
                edits.extend([t.set_header(&text, true), t.set_header(&text, false), t.merge(&text, 0, 0, 2), t.split(&text, 0, 0)]);
                edits.push(Ok(t.wrap_in_float(&text, "c", "l")));
                for e in edits.into_iter().flatten() {
                    assert!(e.changes.windows(2).all(|w| w[0].range.end <= w[1].range.start), "overlap in {text:?}: {:?}", e.changes);
                    assert!(e.changes.iter().all(|c| text.get(c.range.clone()).is_some()), "off the text in {text:?}");
                }
            }
        }
    }
    assert!(checked > 100, "too few grids were read: {checked}");
    Ok(())
}
