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
    let mut e = after("See\\footnote{x}\n\nNext.", &[Op::End { shift: false }, Op::Bar("bold")]);
    assert!(before_caret(&e).starts_with("See"), "the caret is not on the line it is typing in: {:?}", before_caret(&e));
    typed(&mut e, "y");
    assert!(e.text.contains("\\footnote{x}\\textbf{y}"), "{:?}", e.text);
    assert!(before_caret(&e).starts_with("See"), "the caret is not on the line it typed in: {:?}", before_caret(&e));
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
    let e = after("\\begin{itemize}\\item\\textbf{o}\\end{itemize}", &[Op::DoubleClick(0, 0), Op::Bar("bold")]);
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

#[test]
fn a_table_or_an_equation_inserted_in_an_empty_document_goes_in_its_body() {
    for op in [Op::Bar("table"), Op::Math("equation", "x".to_owned())] {
        let e = after("\\begin{document}\n\\end{document}\n", &[op]);
        let end = e.text.find("\\end{document}").unwrap_or(0);
        assert!(e.text.find("\\begin{t").or(e.text.find("\\begin{eq")).is_some_and(|i| i < end), "{:?}", e.text);
    }
}

#[test]
fn a_dash_typed_before_a_dash_run_leaves_the_caret_after_the_dash_it_makes() {
    // `a|--b` shows as `a–b`; a dash typed after `a` makes an em dash, the caret after it.
    let mut e = after("a--b", &[Op::Click(0, 1)]);
    assert_eq!(before_caret(&e), "a");
    typed(&mut e, "-");
    assert_eq!(before_caret(&e), "a\u{2014}", "the caret is not after the dash it typed");
    typed(&mut e, "x");
    assert_eq!(before_caret(&e), "a\u{2014}x", "the text typed next went inside the dash: {:?}", e.text);
    // Quotes too: a backquote typed before one makes an opening double quote.
    let mut e = after("a`b", &[Op::Click(0, 1)]);
    typed(&mut e, "`");
    typed(&mut e, "x");
    assert_eq!(before_caret(&e), "a\u{201c}x", "{:?}", e.text);
}

#[test]
fn a_logo_is_one_caret_step_and_backspace_after_it_deletes_it_whole() {
    let text = "The \\LaTeX{} way";
    let e = Editor::new(text);
    let line = &e.shown().lines[0].line;
    assert!((1.."LaTeX".len()).all(|k| !line.is_stop("The ".len() + k)), "the caret stops inside the logo");
    // Right four times, then once more: past the whole logo.
    let mut e = after(text, &[Op::End { shift: false }]);
    for _ in 0.." way".len() {
        assert!(e.run(&Op::Left { shift: false, word: false }).is_ok());
    }
    assert_eq!(before_caret(&e), "The LaTeX");
    assert!(e.run(&Op::Backspace).is_ok());
    assert_eq!(e.text, "The  way", "Backspace did not delete the logo whole");
    let mut e = after(text, &[Op::Click(0, 4), Op::Delete]);
    assert!(e.run(&Op::Type("x".to_owned())).is_ok());
    assert!(!e.text.contains("LaTeX"), "Delete before the logo left it: {:?}", e.text);
}

#[test]
fn a_heading_made_in_a_list_item_leaves_the_list() {
    let text = "\\begin{itemize}\n  \\item One\n  \\item Two\n  \\item Three\n\\end{itemize}";
    let mut e = after(text, &[Op::Down { shift: false }, Op::End { shift: false }, Op::Bar("heading_1")]);
    let h = view::view(&e.text);
    assert!(matches!(h.get(1).map(|b| &b.kind), Some(BlockKind::Heading { .. })), "Two is not a heading between the lists: {:?}", e.text);
    assert_eq!(h.iter().filter(|b| matches!(b.kind, BlockKind::List { .. })).count(), 2, "{:?}", e.text);
    typed(&mut e, "x");
    assert!(e.text.contains("\\section{Twox}"), "the caret left the heading: {:?}", e.text);
    // Nested two deep: out of both.
    let nested = "\\begin{itemize}\n  \\item A\n  \\begin{itemize}\n    \\item B\n  \\end{itemize}\n\\end{itemize}";
    let e = after(nested, &[Op::Down { shift: false }, Op::Bar("heading_2")]);
    assert!(edit::here(&e.text, e.head).list.is_none(), "the heading is still in a list: {:?}", e.text);
    assert!(e.text.contains("\\subsection{B}"), "{:?}", e.text);
}

#[test]
fn bold_over_runs_with_a_line_end_between_them_leaves_no_empty_style() {
    let e = after("\\begin{document}{a b}\n{c d}\\end{document}", &[Op::SelectAll, Op::Bar("bold")]);
    assert!(!e.text.contains("\\textbf{\n}"), "the line end between the runs was made bold: {:?}", e.text);
}

#[test]
fn a_reference_inserted_after_pressing_italic_goes_in_the_italic() {
    let e = after("Word, more", &[Op::Click(0, 5), Op::Bar("italic"), Op::Ref("r".to_owned())]);
    assert!(e.text.contains("\\textit{\\ref{r}}"), "{:?}", e.text);
}

#[test]
fn italic_then_bold_with_nothing_typed_nests_the_styles_for_the_text_to_come() {
    let mut e = after("e t", &[Op::Bar("italic"), Op::Backspace, Op::Bar("bold")]);
    typed(&mut e, "x");
    assert!(e.text.contains("\\textit{\\textbf{x}}"), "{:?}", e.text);
}

#[test]
fn deleting_a_paragraph_and_the_start_of_a_theorem_leaves_the_caret_before_what_is_left() {
    let mut e = after("Para.\\begin{theorem}Claim\\end{theorem}", &[Op::Down { shift: true }, Op::Right { shift: true, word: false }, Op::Backspace]);
    typed(&mut e, "x");
    assert!(e.text.contains("\\begin{theorem}xlaim"), "the text typed next went outside the theorem: {:?}", e.text);
}

#[test]
fn enter_before_a_trailing_declaration_shows_the_new_paragraph() {
    let mut e = after("\\begin{document}{Bold\\bfseries}\n\nNext\\end{document}", &[Op::End { shift: false }, Op::Enter]);
    typed(&mut e, "x");
    assert!(e.text.contains("\\bfseries}\n\nx\n\nNext"), "the line was broken inside the declaration's group: {:?}", e.text);
}

#[test]
fn a_heading_whose_whole_text_a_selection_deletes_goes_unless_it_starts_there() {
    // From the title's end to a heading's middle and on: the heading's command goes with its text.
    let text = "\\title{T}\\begin{document}\\maketitle\n\n\\section{Head}\n\nBody\\end{document}";
    let e = after(text, &[Op::End { shift: false }, Op::Down { shift: true }, Op::Down { shift: true }, Op::End { shift: true }, Op::Delete]);
    assert!(!e.text.contains("\\section{}"), "an empty heading was left: {:?}", e.text);
}

#[test]
fn text_typed_just_before_a_headings_label_goes_in_the_heading() {
    let text = "\\section{Head}\\label{sec:h}\n\nBody";
    let mut e = after(text, &[Op::End { shift: false }, Op::Left { shift: false, word: false }]);
    typed(&mut e, "s");
    assert!(e.text.starts_with("\\section{Heads}"), "{:?}", e.text);
}

#[test]
fn a_table_inserted_after_pressing_bold_leaves_no_empty_bold() {
    let e = after("Word", &[Op::End { shift: false }, Op::Bar("bold"), Op::Bar("table")]);
    assert!(!e.text.contains("\\textbf{}"), "{:?}", e.text);
}

#[test]
fn a_selection_ending_in_a_labelled_item_keeps_its_label() {
    let text = "\\begin{description}\\item[Term]\\item\\end{description}";
    let e = after(text, &[Op::End { shift: false }, Op::Down { shift: true }, Op::Backspace]);
    assert!(e.text.contains("Term"), "the label went with an item the selection did not hold: {:?}", e.text);
}

#[test]
fn an_author_after_maketitle_is_not_in_the_title_block() {
    let text = "\\title{T}\\author{A}\\begin{document}\\maketitle\n\nText.\n\n\\author{Late}\\end{document}";
    let blocks = view::view(text);
    let Some(BlockKind::Title { author, .. }) = blocks.iter().map(|b| &b.kind).find(|k| matches!(k, BlockKind::Title { .. })) else {
        panic!("no title block: {blocks:?}");
    };
    let shown: Vec<_> = author
        .iter()
        .filter_map(|i| match &i.kind {
            view::InlineKind::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(shown, ["A"], "the title block shows an author it does not print");
    // Deleting the title block's text cannot reach across the body.
    let e = after(text, &[Op::Down { shift: false }, Op::End { shift: false }, Op::Backspace]);
    assert!(e.text.contains("\\begin{document}\\maketitle"), "{:?}", e.text);
}

#[test]
fn text_typed_over_a_selection_ending_in_a_comment_goes_before_the_comment() {
    let e = after("{%\n\n}", &[Op::SelectAll, Op::Type(".".to_owned())]);
    assert!(super::checks::balanced(&e.text), "the typed text commented out the brace: {:?}", e.text);
    assert_eq!(edit::out_of_comment("ab % c", 5), 3);
    assert_eq!(edit::out_of_comment("50\\% c", 5), 5, "an escaped percent sign is no comment");
}

#[test]
fn an_empty_caption_with_a_style_just_made_in_it_holds_the_caret() {
    let text = "\\begin{figure}\n\\caption{}\n\\label{fig:a}\n\\end{figure}";
    let mut e = after(text, &[Op::End { shift: false }, Op::Bar("italic")]);
    typed(&mut e, "x");
    assert!(e.text.contains("\\caption{\\textit{x}}"), "{:?}", e.text);
    assert!(before_caret(&e).ends_with('x'), "the caret is not after the typed text: {:?}", before_caret(&e));
}

#[test]
fn an_emptied_front_matter_field_keeps_a_line_for_the_caret() {
    let text = "\\begin{document}\\begin{frontmatter}\\author{A}\\affiliation{Lab}\\end{frontmatter}\\end{document}";
    let mut e = after(text, &[Op::Down { shift: false }, Op::End { shift: false }]);
    for _ in 0.."Lab".len() {
        assert!(e.run(&Op::Backspace).is_ok());
    }
    typed(&mut e, "Shop");
    assert!(e.text.contains("\\affiliation{Shop}"), "{:?}", e.text);
}

#[test]
fn body_text_on_an_empty_first_heading_leaves_a_line_for_the_caret() {
    let mut e = after("\\section{}\\begin{theorem}T\\end{theorem}", &[Op::Bar("body")]);
    typed(&mut e, "x");
    assert!(e.text.find('x').is_some_and(|x| x < e.text.find("\\begin{theorem}").unwrap_or(0)), "{:?}", e.text);
    assert_eq!(before_caret(&e), "x");
}

#[test]
fn a_title_command_before_maketitle_in_the_body_shows_once() {
    let text = "\\begin{document}\\author{Ann}\\maketitle\n\nText.\\end{document}";
    let shown = super::lines::shown(text, None).plain();
    assert_eq!(shown.matches("Ann").count(), 1, "{shown:?}");
    assert!(!shown.contains("\\author"), "the command shows as its source too: {shown:?}");
}

#[test]
fn a_list_with_options_is_shown_as_a_list_and_split_keeps_them() {
    let text = "\\begin{enumerate}[label=(\\alph*)]\n  \\item One\n  \\item Two\n  \\item Three\n\\end{enumerate}";
    assert!(!shows_source(text), "a list with enumitem's options is shown as source");
    let e = after(text, &[Op::Down { shift: false }, Op::ShiftTab]);
    assert_eq!(e.text.matches("[label=(\\alph*)]").count(), 2, "the list's second part lost its options: {:?}", e.text);
}

#[test]
fn a_letter_shows_its_opening_paragraphs_and_closing_as_text() {
    let text =
        "\\begin{document}\n\\begin{letter}{Them \\\\ There}\n\\opening{Dear you,}\n\nBody.\n\n\\closing{Yours,}\n\\end{letter}\n\\end{document}";
    assert!(!shows_source(text), "the letter is shown as its source");
    let shown = super::lines::shown(text, None).plain();
    for words in ["Dear you,", "Body.", "Yours,"] {
        assert!(shown.contains(words), "{words:?} is not shown: {shown:?}");
    }
    // Its recipient first, the opening under it.
    assert!(shown.starts_with("Them") && shown.contains("There"), "the recipient is not shown first: {shown:?}");
    let mut e = after(text, &[Op::Down { shift: false }, Op::End { shift: false }]);
    typed(&mut e, " friend");
    assert!(e.text.contains("\\opening{Dear you, friend}"), "{:?}", e.text);
    // Not a heading or a list: it is the opening's text.
    let e = after(text, &[Op::Bar("heading_1")]);
    assert!(!e.text.contains("\\section"), "{:?}", e.text);
}

#[test]
fn sizes_spacing_and_par_show_as_the_page_reads() {
    let text = "{\\Huge Name}\\par\\vspace{0.3em}Title\\quad mail";
    let shown = super::lines::shown(text, None);
    assert!(!shows_source(text), "{:?}", shown.plain());
    assert!(shown.lines.len() >= 2, "\\par does not end the paragraph: {:?}", shown.plain());
}

#[test]
fn deleting_the_only_character_of_a_line_leaves_no_paragraph_break() {
    let mut e = Editor::new("one\nx\ntwo");
    let at = "one\n".len();
    (e.anchor, e.head) = (at, at);
    assert!(e.run(&Op::Delete).is_ok());
    assert_eq!(view::view(&e.text).len(), 1, "a blank line split the paragraph: {:?}", e.text);
}

#[test]
fn code_pressed_twice_with_nothing_typed_leaves_nothing() {
    let e = after("Word, more", &[Op::Click(0, 5), Op::Bar("code"), Op::Bar("code")]);
    assert_eq!(e.text, "Word, more");
}

#[test]
fn a_displayed_formula_is_not_put_in_a_heading() {
    let e = after("\\section{}", &[Op::Math("display", "x".to_owned())]);
    assert_eq!(e.text, "\\section{}", "the formula went into the heading");
}

#[test]
fn backspace_in_nested_empty_styles_takes_them_all() {
    let e = after("ab.", &[Op::End { shift: false }, Op::Bar("bold"), Op::Bar("italic"), Op::Backspace]);
    assert_eq!(e.text, "ab");
}

#[test]
fn a_paragraph_whose_braces_do_not_close_is_not_made_a_heading() {
    let e = after("$}$", &[Op::Bar("heading_1")]);
    assert_eq!(e.text, "$}$");
}

#[test]
fn a_label_chip_alone_is_not_made_a_link() {
    let text = "\\begin{figure}\n\\caption{Cap}\n\\label{fig:a}\n\\end{figure}";
    let e =
        after(text, &[Op::End { shift: false }, Op::Left { shift: true, word: false }, Op::Link("https://example.org".to_owned(), String::new())]);
    assert!(!e.text.contains("\\href"), "{:?}", e.text);
}

#[test]
fn a_selection_of_a_label_chip_and_the_gap_before_it_is_not_made_a_link() {
    let text = "\\begin{figure}\n\\caption{Cap}\n\\label{fig:a}\n\\end{figure}";
    let left = Op::Left { shift: false, word: false };
    let e = after(
        text,
        &[Op::End { shift: false }, left.clone(), left, Op::End { shift: true }, Op::Link("https://example.org".to_owned(), "site".to_owned())],
    );
    assert!(!e.text.contains("\\href"), "{:?}", e.text);
}

#[test]
fn delete_row_leaves_the_caret_in_a_cell_of_the_row_after() {
    let mut e = after("", &[Op::Bar("table")]);
    assert!(e.run(&Op::Table("delete_row", 0)).is_ok());
    typed(&mut e, "x");
    assert!(e.text.contains("xText & Text"), "what is typed after Delete Row is not in the next row's cell: {:?}", e.text);
}

#[test]
fn an_empty_author_or_date_with_maketitle_and_the_title_block_deleted_show_no_source() {
    let text = "\\begin{document}\\author{}\\date{}\\maketitle\n\nText.\\end{document}";
    assert!(!shows_source(text), "an empty title block or its empty fields show as source");
    let e = after(text, &[Op::SelectAll, Op::Backspace]);
    assert!(!shows_source(&e.text), "what the deletion left shows as source: {:?}", e.text);
}

#[test]
fn a_style_pressed_with_nothing_selected_waits_for_the_text_typed() {
    let mut e = after("One two.", &[Op::End { shift: false }, Op::Bar("bold"), Op::Bar("italic")]);
    assert_eq!(e.text, "One two.", "the source holds the styles before anything is typed");
    assert_eq!(e.pending.len(), 2);
    typed(&mut e, "x");
    assert_eq!(e.text, "One two.\\textbf{\\textit{x}}");
    typed(&mut e, "y");
    assert_eq!(e.text, "One two.\\textbf{\\textit{xy}}", "the typing goes on in the styles");
    // Moved away, the style pressed is dropped.
    let mut e = after("One two.", &[Op::End { shift: false }, Op::Bar("bold"), Op::Left { shift: false, word: false }]);
    typed(&mut e, "x");
    assert_eq!(e.text, "One twox.");
    // Pressed twice, nothing is pending.
    let mut e = after("One two.", &[Op::End { shift: false }, Op::Bar("code"), Op::Bar("code")]);
    typed(&mut e, "x");
    assert_eq!(e.text, "One two.x");
}

#[test]
fn a_style_pressed_at_the_end_of_styled_text_ends_it_for_the_text_typed() {
    let mut e = after("Say \\textbf{loud}", &[Op::End { shift: false }, Op::Bar("bold")]);
    typed(&mut e, " ");
    typed(&mut e, "soft");
    assert!(e.text.starts_with("Say \\textbf{loud"), "{:?}", e.text);
    assert!(e.text.ends_with("}soft"), "the word typed is still bold: {:?}", e.text);
    // In the middle of the styled text (after a space), the text typed goes in a gap in it.
    let text = "\\textit{one two}";
    let mut e = Editor::new(text);
    e.anchor = text.find("two").unwrap_or(0);
    e.head = e.anchor;
    assert!(e.run(&Op::Bar("italic")).is_ok());
    typed(&mut e, "x");
    assert_eq!(e.text, "\\textit{one }x\\textit{two}");
}

#[test]
fn deleting_a_styled_runs_last_letter_leaves_no_empty_style_and_the_next_letter_styled() {
    let mut e = after("A \\textbf{b}", &[Op::End { shift: false }, Op::Backspace]);
    assert!(!e.text.contains("textbf"), "an empty \\textbf{{}} was left: {:?}", e.text);
    typed(&mut e, "c");
    assert_eq!(e.text, "A \\textbf{c}");
}
