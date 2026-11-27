//! What the random runs found, each cut down to a few steps on a small document: the formatted
//! view's edits as the window makes them ([`super::sim`]), checked for the behaviour that was
//! wrong.

use texcraft_latex::edit;
use texcraft_latex::view::{self, BlockKind};

use super::sim::{Editor, Op};

fn after(text: &str, ops: &[Op]) -> Editor {
    let mut e = Editor::new(text);
    for op in ops {
        assert!(e.run(op).is_ok(), "{op:?} on {:?} made no sound edit", e.text);
    }
    e
}

/// The text of the line the view shows the caret on, up to the caret.
fn before_caret(e: &Editor) -> String {
    let shown = e.shown();
    let doc = shown.doc();
    let Some(p) = doc.locate(&e.text, e.head, e.hint) else { return String::new() };
    shown.lines.get(p.line).map(|l| l.chars.iter().take(p.at).collect()).unwrap_or_default()
}

fn typed(e: &mut Editor, s: &str) {
    assert!(e.run(&Op::Type(s.to_owned())).is_ok());
}

/// Whether the view shows something of `text` as its source.
fn shows_source(text: &str) -> bool {
    let mut found = false;
    view::walk(&view::view(text), &mut |b| {
        found |= matches!(b.kind, BlockKind::Source | BlockKind::UnsupportedTable(_))
            || view::inlines_of(b).iter().any(|i| matches!(i.kind, view::InlineKind::Source));
    });
    found
}

#[test]
fn a_space_typed_at_a_paragraphs_end_keeps_the_caret_in_that_paragraph() {
    let mut e = after("One.\n\nTwo.", &[Op::End { shift: false }]);
    typed(&mut e, " ");
    assert_eq!(before_caret(&e), "One.", "the caret went to the next paragraph");
    typed(&mut e, "x");
    assert_eq!(e.text, "One. x\n\nTwo.");
}

#[test]
fn bold_pressed_with_no_word_at_a_lines_end_keeps_the_caret_on_that_line() {
    let e = after("See\\footnote{x}\n\nNext.", &[Op::End { shift: false }, Op::Bar("bold")]);
    assert!(e.text.contains("\\footnote{x}\\textbf{}"), "{:?}", e.text);
    assert!(before_caret(&e).starts_with("See"), "the caret is not on the line it is typing in: {:?}", before_caret(&e));
}

#[test]
fn a_typed_tilde_is_one_character_that_backspace_removes_whole() {
    let mut e = after("a\n\nb", &[Op::End { shift: false }]);
    typed(&mut e, "~");
    assert_eq!(before_caret(&e), "a~", "the caret is not after the tilde");
    assert!(e.run(&Op::Backspace).is_ok());
    assert_eq!(e.text, "a\n\nb", "Backspace left part of \\textasciitilde{{}}");
}

#[test]
fn a_paragraph_made_a_list_or_a_heading_keeps_the_caret_at_its_end_inside() {
    let mut list = after("One.", &[Op::End { shift: false }, Op::Bar("enumerate")]);
    typed(&mut list, "x");
    assert!(list.text.contains("\\item One.x\n\\end{enumerate}"), "typed text went outside the list: {:?}", list.text);
    let mut heading = after("One.", &[Op::End { shift: false }, Op::Bar("heading_1")]);
    typed(&mut heading, "x");
    assert_eq!(heading.text, "\\section{One.x}");
}

#[test]
fn a_pasted_list_leaves_the_caret_at_the_end_of_its_last_item() {
    let mut e = after("Text.", &[Op::End { shift: false }, Op::Paste("- a\n- b".to_owned())]);
    typed(&mut e, "c");
    assert!(e.text.contains("\\item bc\n\\end{itemize}"), "typed text went after the list: {:?}", e.text);
}

#[test]
fn text_put_just_after_a_control_word_is_kept_apart_from_its_name() {
    // An empty item without a space after `\item`: a pasted word must not lengthen it.
    let e = after("\\begin{itemize}\\item\\item x\\end{itemize}", &[Op::Paste("word".to_owned())]);
    assert!(e.text.contains("\\item word"), "{:?}", e.text);
    // A bold word unbolded right after `\item`.
    let e = after("\\begin{itemize}\\item\\textbf{o}\\end{itemize}", &[Op::Bar("bold")]);
    assert_eq!(e.text, "\\begin{itemize}\\item o\\end{itemize}");
    // Three dots typed before a word.
    let e = after("word", &[Op::Type(".".to_owned()), Op::Type(".".to_owned()), Op::Type(".".to_owned())]);
    assert!(!shows_source(&e.text), "{:?}", e.text);
}

#[test]
fn deleting_a_selection_that_holds_a_styled_runs_whole_text_takes_its_command() {
    let e = after(",\\texttt{x}", &[Op::SelectAll, Op::Backspace]);
    assert!(!e.text.contains("texttt"), "an empty \\texttt{{}} was left: {:?}", e.text);
    let e = after("a.\n\n\\section{T}\n\nb", &[Op::Down { shift: true }, Op::End { shift: true }, Op::Type("e".to_owned())]);
    assert!(!e.text.contains("\\section"), "an empty heading was left: {:?}", e.text);
}

#[test]
fn enter_at_a_styled_runs_start_or_on_an_empty_style_leaves_no_empty_style() {
    let text = "a \\textbf{b}";
    let made = edit::enter(text, text.rfind('b').unwrap_or(0)).map(|e| edit::apply(text, &e.changes));
    assert_eq!(made.as_deref(), Some("a\n\n\\textbf{b}"));
    let text = "a \\textbf{}";
    let made = edit::enter(text, text.len() - 1).map(|e| edit::apply(text, &e.changes)).unwrap_or_default();
    assert!(!made.contains("textbf"), "{made:?}");
}

#[test]
fn backspace_in_an_empty_style_takes_it_with_the_character_before() {
    let e = after("ab.", &[Op::End { shift: false }, Op::Bar("bold"), Op::Backspace]);
    assert_eq!(e.text, "ab");
}

#[test]
fn a_paragraph_that_starts_with_a_styled_run_made_a_heading_wraps_the_run_whole() {
    let e = after("\\textbf{Note} that.", &[Op::Bar("heading_1")]);
    assert_eq!(e.text, "\\section{\\textbf{Note} that.}");
}

#[test]
fn outdenting_an_empty_item_between_two_leaves_the_caret_in_the_paragraph_made() {
    let text = "\\begin{itemize}\\item a\\item\\item b\\end{itemize}";
    let at = text.find("\\item\\item").map_or(0, |i| i + "\\item".len());
    let e = edit::outdent(text, at);
    let (new, caret) = e.map(|e| (edit::apply(text, &e.changes), e.caret)).unwrap_or_default();
    assert!(edit::here(&new, caret).list.is_none(), "the caret is in a list: {:?}", new.split_at(caret));
}

#[test]
fn typing_over_whole_items_puts_the_text_in_the_item_left() {
    // From the first item's start to the third's: the first two go whole, the text goes in the third.
    let e = after(
        "\\begin{itemize}\\item one\\item two\\item three\\end{itemize}",
        &[Op::Down { shift: true }, Op::Down { shift: true }, Op::Type("%".to_owned())],
    );
    assert!(!shows_source(&e.text), "the list became source: {:?}", e.text);
    assert_eq!(e.text, "\\begin{itemize}\\item \\%three\\end{itemize}");
}

#[test]
fn an_empty_headings_caret_is_inside_its_braces() {
    let mut e = Editor::new("\\chapter{}\\label{x}A");
    typed(&mut e, "a");
    assert_eq!(e.text, "\\chapter{a}\\label{x}A");
}

#[test]
fn typing_at_the_end_of_a_headings_line_after_its_label_extends_the_heading() {
    let mut e = after("\\section{T}\\label{x}\n\nText", &[Op::End { shift: false }]);
    typed(&mut e, "x");
    assert_eq!(e.text, "\\section{Tx}\\label{x}\n\nText");
}

#[test]
fn a_cells_caret_stays_in_its_cell_when_the_caption_is_above_and_the_label_below() {
    let text = "\\begin{table}\n\\caption{C}\n\\begin{tabular}{ll}\nab & cd \\\\\n\\end{tabular}\n\\label{t}\n\\end{table}";
    let mut e = Editor::new(text);
    let at = text.find("cd").map_or(0, |i| i + 2);
    (e.anchor, e.head) = (at, at);
    typed(&mut e, "x");
    assert_eq!(before_caret(&e), "cdx", "the caret is shown on another line");
}

#[test]
fn merged_cells_keep_the_caret_in_their_text() {
    let text = "\\begin{tabular}{lll}\n & & \\\\\n\\end{tabular}";
    let mut e = Editor::new(text);
    let at = text.find(" &").map_or(0, |i| i + 1);
    (e.anchor, e.head) = (at, at);
    assert!(e.run(&Op::Table("merge", 0)).is_ok());
    assert!(e.text.contains("\\multicolumn"), "{:?}", e.text);
    typed(&mut e, "x");
    assert!(!shows_source(&e.text), "typing after the merge broke the table: {:?}", e.text);
}

#[test]
fn a_table_inserted_with_the_caret_in_the_title_goes_after_the_title_block() {
    let e = after("\\title{T}\n\\begin{document}\n\\maketitle\n\\end{document}", &[Op::Bar("table")]);
    let table = e.text.find("\\begin{table}").unwrap_or(0);
    assert!(table > e.text.find("\\maketitle").unwrap_or(usize::MAX), "the table went into the preamble: {:?}", e.text);
}

#[test]
fn a_paste_over_a_selection_reads_as_where_it_goes() {
    let e = after("\\section{Head}\n\nbody", &[Op::Down { shift: true }, Op::Paste("- a\n- b".to_owned())]);
    assert!(!shows_source(&e.text), "a list went into the heading: {:?}", e.text);
}

#[test]
fn a_typed_no_break_space_shows() {
    let mut e = after("a", &[Op::End { shift: false }]);
    typed(&mut e, "\u{a0}");
    typed(&mut e, "b");
    assert_eq!(before_caret(&e), "a\u{a0}b");
}

#[test]
fn tab_on_a_new_last_item_keeps_the_caret_in_it() {
    let mut e = after("\\begin{itemize}\n  \\item a\n\\end{itemize}", &[Op::End { shift: false }, Op::Enter, Op::Tab]);
    typed(&mut e, "x");
    let inner = e.text.rfind("\\begin{itemize}").unwrap_or(0);
    let x = e.text.find('x').unwrap_or(0);
    let inner_end = e.text.find("\\end{itemize}").unwrap_or(0);
    assert!(inner < x && x < inner_end, "x is not in the inner list: {:?}", e.text);
}

#[test]
fn deleting_whole_items_whose_text_ends_in_a_styled_word_takes_their_items() {
    let e = after("\\begin{itemize}\\item \\textbf{a}\\item b\\end{itemize}", &[Op::Down { shift: true }, Op::Delete]);
    assert_eq!(view::view(&e.text).len(), 1);
    assert!(!e.text.contains("textbf"), "{:?}", e.text);
}

#[test]
fn typing_or_citing_at_the_end_of_a_caption_line_goes_in_the_caption() {
    let text = "\\begin{figure}\n\\includegraphics{a}\n\\caption{Cap}\n\\label{fig:a}\n\\end{figure}";
    let mut e = after(text, &[Op::End { shift: false }]);
    typed(&mut e, "x");
    assert!(e.text.contains("\\caption{Capx}"), "{:?}", e.text);
    let e = after(text, &[Op::End { shift: false }, Op::Cite("k".to_owned())]);
    assert!(e.text.contains("\\caption{Cap~\\cite{k}}"), "{:?}", e.text);
    assert!(!shows_source(&e.text), "the figure became source: {:?}", e.text);
}

#[test]
fn shift_tab_on_a_labelled_item_keeps_its_label_in_the_paragraph() {
    let e = after("Before.\n\n\\begin{description}\\item[Term] Its text.\\end{description}", &[Op::Down { shift: false }, Op::ShiftTab]);
    assert!(e.text.contains("Term Its text."), "{:?}", e.text);
}
