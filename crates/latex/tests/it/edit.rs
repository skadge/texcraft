//! Editing in the formatted view: where its caret stands in the source, and the edits its keys
//! make.

use texcraft_latex::edit::{self, Change, Doc, Line, Part, Pos};
use texcraft_latex::view::{Block, BlockKind, Inline, InlineKind, ListKind, Style, view};

/// The lines of the paragraphs and headings of `text`, as the formatted view makes them (atoms one
/// character wide).
fn lines(text: &str) -> Vec<Line> {
    fn walk(text: &str, blocks: &[Block], out: &mut Vec<Line>) {
        for b in blocks {
            match &b.kind {
                BlockKind::Paragraph(p) | BlockKind::Heading { content: p, .. } => out.push(line(text, p)),
                BlockKind::List { items, .. } => items.iter().for_each(|i| walk(text, &i.blocks, out)),
                BlockKind::Container { blocks, .. } => walk(text, blocks, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(text, &view(text), &mut out);
    out
}

fn line(text: &str, inlines: &[Inline]) -> Line {
    let parts = inlines
        .iter()
        .map(|i| match &i.kind {
            InlineKind::Text { text: shown, .. } => match edit::offsets(text, i.range.clone(), shown) {
                Some(o) => Part::Text(o),
                None => Part::Atom { chars: shown.chars().count(), range: i.range.clone() },
            },
            _ => Part::Atom { chars: 1, range: i.range.clone() },
        })
        .collect();
    Line::new(parts)
}

/// The text runs of the view with their styles.
fn runs(text: &str) -> Vec<(String, Style)> {
    fn walk(blocks: &[Block], out: &mut Vec<(String, Style)>) {
        for b in blocks {
            match &b.kind {
                BlockKind::Paragraph(p) | BlockKind::Heading { content: p, .. } => {
                    for i in p {
                        if let InlineKind::Text { text, style } = &i.kind {
                            out.push((text.clone(), *style));
                        }
                    }
                }
                BlockKind::List { items, .. } => items.iter().for_each(|i| walk(&i.blocks, out)),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&view(text), &mut out);
    out
}

fn paragraphs(text: &str) -> Vec<String> {
    view(text)
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Paragraph(p) => {
                Some(p.iter().filter_map(|i| if let InlineKind::Text { text, .. } = &i.kind { Some(text.as_str()) } else { None }).collect())
            }
            _ => None,
        })
        .collect()
}

fn insert(text: &str, at: usize, typed: &str) -> String {
    edit::apply(text, &[Change { range: at..at, text: edit::escape(typed) }])
}

#[test]
fn every_character_of_text_stands_for_its_own_source() {
    let text = "Some \\textbf{bold} -- text, 50\\% off: ``quoted'' and~tied, a\n  line % note\n  end.";
    let mut checked = 0;
    for b in view(text) {
        let BlockKind::Paragraph(p) = &b.kind else { continue };
        for i in p {
            let InlineKind::Text { text: shown, .. } = &i.kind else { continue };
            let offsets = edit::offsets(text, i.range.clone(), shown);
            assert!(offsets.is_some(), "{shown:?} does not map to its source");
            let offsets = offsets.unwrap_or_default();
            assert_eq!(offsets.len(), shown.chars().count() + 1);
            assert!(offsets.windows(2).all(|w| w[0] <= w[1]), "offsets go backwards in {shown:?}");
            for (c, w) in shown.chars().zip(offsets.windows(2)) {
                let src = &text[w[0]..w[1]];
                // Each character's source shows that character, alone.
                let back: String = paragraphs(src).concat();
                let expected = if c.is_whitespace() { String::new() } else { c.to_string() };
                assert_eq!(back.trim(), expected, "{c:?} stands for {src:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 40);
}

#[test]
fn the_caret_round_trips_between_view_and_source() {
    let text = "Plain \\emph{styled} and \\ref{x} then text.\n\n\\section{A \\textit{head}}\n";
    for l in lines(text) {
        let mut v = l.first_stop();
        while let Some(at) = v {
            let byte = l.source_at(at);
            assert_eq!(byte.and_then(|b| l.view_at(b)), Some(at), "stop {at} does not come back");
            v = l.next_stop(at);
        }
    }
}

#[test]
fn typing_at_a_style_boundary_continues_the_style_before_the_caret() {
    let text = "foo \\textbf{bar} baz";
    let l = &lines(text)[0];
    // After "foo ": plain, before the bold run.
    let before_bold = l.source_at(4).unwrap_or_default();
    let typed = insert(text, before_bold, "x");
    assert!(runs(&typed).iter().any(|(t, s)| t.contains("foo x") && !s.bold), "{typed}");
    // After "bar": still bold.
    let after_bold = l.source_at(7).unwrap_or_default();
    let typed = insert(text, after_bold, "y");
    assert!(runs(&typed).iter().any(|(t, s)| t == "bary" && s.bold), "{typed}");
}

#[test]
fn typed_special_characters_show_as_typed() {
    let typed = "50% & $x_1$ #2 {a} ~^\\";
    let text = insert("Say .", 4, typed);
    assert_eq!(paragraphs(&text).concat(), format!("Say {typed}."));
}

#[test]
fn a_deletion_across_a_style_boundary_keeps_the_source_balanced() {
    let text = "foo \\textbf{bar} baz";
    let doc = Doc { lines: lines(text) };
    // From "fo|o" to "ba|r".
    let pieces = doc.pieces(text, Pos { line: 0, at: 2 }, Pos { line: 0, at: 6 });
    let changes: Vec<Change> = pieces.into_iter().map(|range| Change { range, text: String::new() }).collect();
    let after = edit::apply(text, &changes);
    assert_eq!(after, "fo\\textbf{r} baz");
}

#[test]
fn an_atom_is_one_stop_and_deletes_whole() {
    let text = "See \\ref{sec:one} here.";
    let l = &lines(text)[0];
    let atom = l.atom_before(5);
    assert_eq!(atom.clone().map(|r| &text[r]), Some("\\ref{sec:one}"));
    assert!(!l.is_stop(4) || l.next_stop(4) == Some(5), "the chip has a stop inside");
    let after = edit::apply(text, &[Change { range: atom.unwrap_or_default(), text: String::new() }]);
    assert_eq!(after, "See  here.");
}

#[test]
fn deleting_the_break_between_paragraphs_joins_them() {
    let text = "One.\n\nTwo.";
    let doc = Doc { lines: lines(text) };
    let gap = doc.gap_after(text, 0);
    let after = edit::apply(text, &[Change { range: gap.unwrap_or_default(), text: String::new() }]);
    assert_eq!(paragraphs(&after), ["One.Two."]);
    // A heading's command is not white space: nothing joins it.
    let text = "\\section{A}\nTwo.";
    assert!(Doc { lines: lines(text) }.gap_after(text, 0).is_none());
}

fn enter(text: &str, at: usize) -> (String, usize) {
    let e = edit::enter(text, at);
    assert!(e.is_some(), "Enter does nothing at {at} in {text:?}");
    let e = e.unwrap_or_else(|| edit::SourceEdit { changes: Vec::new(), caret: at });
    (edit::apply(text, &e.changes), e.caret)
}

#[test]
fn enter_splits_a_paragraph_and_the_styles_open_at_the_caret() {
    let (text, _) = enter("One two.", 3);
    assert_eq!(paragraphs(&text), ["One", "two."]);
    let (text, caret) = enter("Here \\textbf{bold text} ends.", 18);
    assert_eq!(paragraphs(&text), ["Here bold", "text ends."]);
    let bold: Vec<String> = runs(&text).into_iter().filter(|(_, s)| s.bold).map(|(t, _)| t).collect();
    assert_eq!(bold, ["bold", "text"]);
    // Typing goes on in the second paragraph, in bold.
    let typed = insert(&text, caret, "x");
    assert!(runs(&typed).iter().any(|(t, s)| t == "xtext" && s.bold), "{typed}");
}

#[test]
fn enter_at_a_paragraphs_end_opens_an_empty_one() {
    let text = "One.\nTwo.\n\nThree.";
    let (after, caret) = enter(text, 9);
    // Nothing shows yet; what is typed there is a paragraph of its own.
    assert_eq!(paragraphs(&after), paragraphs(text));
    assert_eq!(paragraphs(&insert(&after, caret, "New")), ["One. Two.", "New", "Three."]);
}

#[test]
fn enter_after_a_heading_opens_a_paragraph_under_it() {
    let text = "\\section{Methods}\nMore.";
    let (after, caret) = enter(text, 15);
    assert!(after.starts_with("\\section{Methods}"), "{after}");
    assert_eq!(paragraphs(&insert(&after, caret, "New")), ["New", "More."]);
}

#[test]
fn enter_in_a_list_starts_an_item() {
    let text = "\\begin{itemize}\n  \\item one two\n  \\item three\n\\end{itemize}";
    let at = text.find(" two").unwrap_or_default();
    let (after, caret) = enter(text, at);
    assert!(after.contains("\n  \\item one\n  \\item two\n"), "{after}");
    let count = |t: &str| view(t).iter().map(|b| if let BlockKind::List { items, .. } = &b.kind { items.len() } else { 0 }).sum::<usize>();
    assert_eq!(count(&after), 3);
    assert!(after[caret..].starts_with("two"));
    // Nowhere for a break: a caption.
    assert!(edit::enter("\\begin{figure}\\caption{A b}\\end{figure}", 25).is_none());
}

#[test]
fn bold_wraps_the_selected_spans_and_unwraps_them_again() {
    let text = "Some words here.";
    let changes = edit::toggle_style(text, std::slice::from_ref(&(5..10)), "textbf");
    let bold = edit::apply(text, &changes);
    assert_eq!(bold, "Some \\textbf{words} here.");
    let inner = 13..18;
    let plain = edit::apply(&bold, &edit::toggle_style(&bold, &[inner], "textbf"));
    assert_eq!(plain, text);
    // Across a style boundary each span is wrapped on its own, so the source stays balanced.
    let text = "ab \\textit{cd}";
    let both = edit::apply(text, &edit::toggle_style(text, &[1..3, 11..12], "textbf"));
    assert_eq!(both, "a\\textbf{b }\\textit{\\textbf{c}d}");
}

#[test]
fn headings_and_lists_change_by_their_command_alone() {
    let text = "\\subsection{Title}\n\nA paragraph.";
    let h = edit::apply(text, &edit::set_heading(text, 13, "section").unwrap_or_default());
    assert_eq!(h, "\\section{Title}\n\nA paragraph.");
    let p = edit::apply(text, &edit::set_heading(text, 22, "section").unwrap_or_default());
    assert_eq!(p, "\\subsection{Title}\n\n\\section{A paragraph.}");
    let list = "\\begin{itemize}\n  \\item one\n\\end{itemize}";
    let n = edit::apply(list, &edit::set_list(list, 26, "enumerate").unwrap_or_default());
    assert_eq!(n, "\\begin{enumerate}\n  \\item one\n\\end{enumerate}");
    assert!(edit::set_list(list, 26, "itemize").is_none());
    let made = edit::apply("One.", &edit::set_list("One.", 1, "itemize").unwrap_or_default());
    assert_eq!(made, "\\begin{itemize}\n  \\item One.\n\\end{itemize}");
}

#[test]
fn pasted_text_is_escaped_and_kept_on_one_line_inside_a_style() {
    assert_eq!(edit::paste("a b", 1, "50%\n\nmore"), "50\\%\n\nmore");
    assert_eq!(edit::paste("\\textbf{ab}", 9, "x\n\ny"), "x y");
}

#[test]
fn hostile_input_never_breaks_the_edits() {
    let main = include_str!("../../../../examples/article/main.tex");
    let nested = "\\begin{itemize}\n  \\item a\n  \\begin{enumerate}\\item b \\item\\end{enumerate}\\item c\n\\end{itemize}x";
    for sample in [
        main,
        "é$\\(ü\\[ö{\\verb\u{2603}x\u{2603}\\begin{ä}%ß\n\n}\\item \\textbf{\\section{",
        "\\begin{itemize}\\item\\textbf{",
        nested,
        "\\begin{itemize}\\item",
    ] {
        for at in (0..=sample.len()).filter(|i| sample.is_char_boundary(*i)) {
            let mut all: Vec<Vec<Change>> = Vec::new();
            all.extend(edit::enter(sample, at).map(|e| e.changes));
            all.extend(edit::set_heading(sample, at, "section"));
            all.extend(edit::set_list(sample, at, "enumerate"));
            all.push(vec![Change { range: at..at, text: edit::paste(sample, at, "a\n\nb%\n- c\n- d") }]);
            all.extend(edit::indent(sample, at).map(|e| e.changes));
            all.extend(edit::outdent(sample, at).map(|e| e.changes));
            all.extend(edit::backspace_at_start(sample, at).map(|e| e.changes));
            all.extend(edit::set_body(sample, at));
            let _ = edit::here(sample, at);
            for changes in all {
                assert!(changes.windows(2).all(|w| w[0].range.end <= w[1].range.start), "overlapping changes at {at}");
                assert!(changes.iter().all(|c| sample.get(c.range.clone()).is_some()), "a change off the text at {at}");
            }
        }
        let _ = lines(sample);
    }
}

/// The items of each list of the view, nested lists as their own entries, in order.
fn lists(text: &str) -> Vec<Vec<String>> {
    fn walk(blocks: &[Block], out: &mut Vec<Vec<String>>) {
        for b in blocks {
            if let BlockKind::List { items, .. } = &b.kind {
                let at = out.len();
                out.push(Vec::new());
                for item in items {
                    let text: String = item
                        .blocks
                        .iter()
                        .filter_map(|b| match &b.kind {
                            BlockKind::Paragraph(p) => Some(
                                p.iter()
                                    .filter_map(|i| if let InlineKind::Text { text, .. } = &i.kind { Some(text.as_str()) } else { None })
                                    .collect::<String>(),
                            ),
                            _ => None,
                        })
                        .collect();
                    if let Some(list) = out.get_mut(at) {
                        list.push(text);
                    }
                    walk(&item.blocks, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&view(text), &mut out);
    out
}

fn made(e: Option<edit::SourceEdit>, text: &str) -> (String, usize) {
    assert!(e.is_some(), "no edit in {text:?}");
    let e = e.unwrap_or_else(|| edit::SourceEdit { changes: Vec::new(), caret: 0 });
    (edit::apply(text, &e.changes), e.caret)
}

const THREE: &str = "\\begin{itemize}\n  \\item one\n  \\item two\n  \\item three\n\\end{itemize}\n";

#[test]
fn tab_nests_an_item_under_the_one_before_and_shift_tab_takes_it_back() {
    let at = THREE.find("two").unwrap_or_default();
    let (nested, caret) = made(edit::indent(THREE, at), THREE);
    assert_eq!(lists(&nested), [vec!["one", "three"], vec!["two"]]);
    assert!(nested[caret..].starts_with("two"), "the caret left the item's text");
    // Only lines of their own were added: the items' lines are as they were.
    for line in THREE.lines().filter(|l| l.contains("\\item")) {
        assert!(nested.contains(line.trim()), "{line} changed");
    }
    // The next item joins the nested list rather than making another.
    let (both, _) = made(edit::indent(&nested, nested.find("three").unwrap_or_default()), &nested);
    assert_eq!(lists(&both), [vec!["one"], vec!["two", "three"]]);
    // Shift+Tab undoes Tab exactly.
    let (back, caret) = made(edit::outdent(&nested, caret), &nested);
    assert_eq!(back, THREE);
    assert!(back[caret..].starts_with("two"));
    // The first item has no item before it to go under.
    assert!(edit::indent(THREE, THREE.find("one").unwrap_or_default()).is_none());
}

#[test]
fn shift_tab_on_a_top_level_item_makes_it_a_paragraph_between_the_list_parts() {
    let (out, caret) = made(edit::outdent(THREE, THREE.find("two").unwrap_or_default()), THREE);
    assert_eq!(lists(&out), [vec!["one"], vec!["three"]]);
    assert_eq!(paragraphs(&out), ["two"]);
    assert!(out[caret..].starts_with("two"));
    // A list's only item: the list goes, its text stays.
    let one = "Before.\n\n\\begin{enumerate}\n  \\item alone\n\\end{enumerate}\nAfter.";
    let (out, _) = made(edit::outdent(one, one.find("alone").unwrap_or_default()), one);
    assert!(lists(&out).is_empty(), "{out}");
    assert_eq!(paragraphs(&out), ["Before.", "alone", "After."]);
}

#[test]
fn enter_on_an_empty_item_ends_the_list() {
    let text = "\\begin{itemize}\n  \\item one\n  \\item \n\\end{itemize}\n\nAfter.";
    let at = text.find("\\item \n").map_or(0, |i| i + "\\item".len());
    let (out, caret) = enter(text, at);
    assert_eq!(lists(&out), [vec!["one"]]);
    assert_eq!(paragraphs(&insert(&out, caret, "New")), ["New", "After."]);
    // In a nested list, the empty item goes one level out instead.
    let text = "\\begin{itemize}\n  \\item one\n  \\begin{itemize}\n    \\item two\n    \\item \n  \\end{itemize}\n\\end{itemize}";
    let at = text.find("\\item \n").map_or(0, |i| i + "\\item".len());
    let (out, caret) = enter(text, at);
    assert_eq!(lists(&insert(&out, caret, "x")), [vec!["one", "x"], vec!["two"]]);
}

#[test]
fn backspace_at_a_line_start_lifts_a_heading_or_an_item() {
    let text = "\\section{Methods}\n\nText.";
    let (out, caret) = made(edit::backspace_at_start(text, text.find("Methods").unwrap_or_default()), text);
    assert_eq!(out, "Methods\n\nText.");
    assert_eq!(caret, 0);
    assert!(edit::backspace_at_start(text, text.find("thods").unwrap_or_default()).is_none(), "inside a heading's text nothing is lifted");
    let (out, _) = made(edit::backspace_at_start(THREE, THREE.find("three").unwrap_or_default()), THREE);
    assert_eq!(lists(&out), [vec!["one", "two"]]);
    assert_eq!(paragraphs(&out), ["three"]);
    assert!(edit::backspace_at_start("Plain text.", 0).is_none());
}

#[test]
fn the_caret_knows_its_styles_heading_and_list() {
    let text = "\\section{A \\textit{b}}\n\nSome \\textbf{bold \\texttt{code}} \\emph{e}.\n\n\\begin{itemize}\\item a\\begin{enumerate}\\item b\\end{enumerate}\\end{itemize}";
    let at = |s: &str| text.find(s).unwrap_or_default() + 1;
    let h = edit::here(text, at("b}}"));
    assert_eq!(h.heading, Some(2), "a section is level 2");
    assert!(h.italic && !h.bold);
    let c = edit::here(text, at("ode"));
    assert!(c.bold && c.code && c.style.mono && c.heading.is_none());
    let e = edit::here(text, at("e}."));
    assert!(e.style.italic && !e.italic, "\\emph is italic text but not the Italic button's command");
    assert_eq!(edit::here(text, at("a\\begin")).list, Some((ListKind::Bullets, 1)));
    assert_eq!(edit::here(text, at("b\\end")).list, Some((ListKind::Numbers, 2)));
    assert_eq!(edit::here(text, at("Some")), edit::Here::default());
}

#[test]
fn heading_levels_follow_the_document_class() {
    assert_eq!(edit::heading_commands("\\documentclass[11pt]{report}\n"), ["chapter", "section", "subsection"]);
    assert_eq!(edit::heading_commands("\\documentclass{article}"), ["section", "subsection", "subsubsection"]);
    assert_eq!(edit::heading_commands("no class at all"), ["section", "subsection", "subsubsection"]);
}

#[test]
fn pasted_lines_with_bullets_or_numbers_become_lists() {
    let pasted = edit::paste("Text.", 5, "Steps:\n1. mix\n2) bake 50%\n\n- salt\n* pepper\nDone");
    let text = format!("{pasted}\n");
    assert_eq!(lists(&text), [vec!["mix", "bake 50%"], vec!["salt", "pepper"]]);
    assert!(pasted.contains("50\\%"), "an item's text is escaped: {pasted}");
    // One line is text as typed, even with a dash in front.
    assert_eq!(edit::paste("Text.", 5, "- 5 degrees"), "- 5 degrees");
}
