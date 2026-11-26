//! The view model: what each construct is shown as, and that everything keeps its source span.

use texcraft_latex::view::{Block, BlockKind, ChipKind, Inline, InlineKind, ListKind, Style, view};

const MAIN: &str = include_str!("../../../../examples/article/main.tex");

/// The paragraphs' inlines of `blocks`, flattened.
fn inlines(blocks: &[Block]) -> Vec<&Inline> {
    blocks.iter().filter_map(|b| if let BlockKind::Paragraph(p) = &b.kind { Some(p) } else { None }).flatten().collect()
}

/// The texts of the inlines with their styles.
fn texts(inlines: &[&Inline]) -> Vec<(String, Style)> {
    inlines.iter().filter_map(|i| if let InlineKind::Text { text, style } = &i.kind { Some((text.clone(), *style)) } else { None }).collect()
}

fn shown(text: &str) -> String {
    texts(&inlines(&view(text))).into_iter().map(|(t, _)| t).collect()
}

#[test]
fn a_document_maps_to_blocks_in_source_order() {
    let blocks = view(MAIN);
    let begin = MAIN.find("\\begin{document}").unwrap_or_default();
    assert!(matches!(blocks.first(), Some(Block { kind: BlockKind::Preamble, range }) if range.end == begin));
    let mut at = 0;
    for b in &blocks {
        assert!(b.range.start >= at && b.range.end <= MAIN.len(), "{b:?} out of order");
        at = b.range.end;
    }
    let Some(BlockKind::Title { title, author, .. }) = blocks.iter().map(|b| &b.kind).find(|k| matches!(k, BlockKind::Title { .. })) else {
        panic!("no title block")
    };
    assert!(!title.is_empty() && !author.is_empty());
    let headings: Vec<_> = blocks.iter().filter(|b| matches!(b.kind, BlockKind::Heading { .. })).collect();
    assert_eq!(headings.len(), 3);
    for h in headings {
        assert!(MAIN[h.range.clone()].starts_with("\\section"));
    }
    // Display formulas hold their math; the table is a grid with its caption and label.
    let formula = blocks.iter().find_map(|b| if let BlockKind::Formula { source, .. } = &b.kind { Some(&MAIN[source.clone()]) } else { None });
    assert!(formula.is_some_and(|f| f.contains("\\ddot\\theta") && !f.contains("\\begin")));
    let table = blocks.iter().find_map(|b| if let BlockKind::Table(t) = &b.kind { Some(t) } else { None });
    let Some(table) = table else { panic!("the table is not a grid") };
    assert_eq!((table.cells.len(), table.cells.iter().map(Vec::len).max()), (4, Some(3)));
    let float = table.float.as_ref();
    assert!(float.is_some_and(|f| !f.caption.is_empty() && !f.caption_above && f.label.is_some()));
}

#[test]
fn headings_follow_the_sectioning_levels() {
    let names = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];
    let text: String = names.iter().map(|n| format!("\\{n}*{{{n}}}\n")).collect();
    let levels: Vec<u8> = view(&text).iter().filter_map(|b| if let BlockKind::Heading { level, .. } = b.kind { Some(level) } else { None }).collect();
    assert_eq!(levels.len(), names.len());
    assert!(levels.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn styles_nest_and_their_commands_are_hidden() {
    let blocks = view("\\textbf{bold \\emph{both}} plain {\\itshape slanted}");
    let all = texts(&inlines(&blocks));
    let style_of = |word: &str| all.iter().find(|(t, _)| t.contains(word)).map(|(_, s)| *s);
    assert!(style_of("bold").is_some_and(|s| s.bold && !s.italic));
    assert!(style_of("both").is_some_and(|s| s.bold && s.italic));
    assert!(style_of("plain").is_some_and(|s| s == Style::default()));
    assert!(style_of("slanted").is_some_and(|s| s.italic));
    assert!(all.iter().all(|(t, _)| !t.contains('\\')));
}

#[test]
fn text_reads_as_typeset() {
    let out = shown("``Quoted'' and `single' -- en --- em,\n   spaced~out 50\\% \\& co. % a comment\nend");
    assert_eq!(out, "“Quoted” and ‘single’ – en — em, spaced\u{a0}out 50% & co. end");
    let blocks = view("one\n\ntwo\\\\three");
    assert_eq!(blocks.len(), 2, "a blank line ends a paragraph");
    assert!(inlines(&blocks).iter().any(|i| i.kind == InlineKind::LineBreak));
}

#[test]
fn references_citations_footnotes_and_links() {
    let text = "See~\\ref{sec:a}, \\cite[p.~2]{knuth}\\footnote{A \\emph{note}.} and \\href{https://example.org}{a site}.";
    let blocks = view(text);
    let all = inlines(&blocks);
    let chips: Vec<_> =
        all.iter().filter_map(|i| if let InlineKind::Chip { kind, key, .. } = &i.kind { Some((*kind, key.as_str())) } else { None }).collect();
    assert_eq!(chips, [(ChipKind::Reference, "sec:a"), (ChipKind::Citation, "knuth")]);
    assert!(all.iter().any(|i| matches!(&i.kind, InlineKind::Footnote(n) if !n.is_empty())));
    assert!(all.iter().any(|i| matches!(&i.kind, InlineKind::Link { url, .. } if url == "https://example.org")));
}

#[test]
fn lists_and_figures() {
    let text = "\\begin{enumerate}\n\\item one\n\\item two\n  \\begin{itemize}\\item inner\\end{itemize}\n\\end{enumerate}";
    let blocks = view(text);
    let Some(BlockKind::List { kind: ListKind::Numbers, items }) = blocks.first().map(|b| &b.kind) else { panic!("{blocks:?}") };
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| text[i.range.clone()].starts_with("\\item")));
    assert!(items[1].blocks.iter().any(|b| matches!(b.kind, BlockKind::List { kind: ListKind::Bullets, .. })));

    let figure = "\\begin{figure}\\centering\\includegraphics[width=2cm]{bob}\\caption{The bob.}\\label{fig:bob}\\end{figure}";
    let Some(BlockKind::Figure { image: Some(image), caption, label: Some(_) }) = view(figure).first().map(|b| b.kind.clone()) else {
        panic!("no figure card")
    };
    assert_eq!((image.path.as_str(), image.options.as_str()), ("bob", "width=2cm"));
    assert!(!caption.is_empty());
    // A figure holding what the card cannot show stays as its source.
    let busy = "\\begin{figure}\\includegraphics{a}\\hfill\\includegraphics{b}\\end{figure}";
    assert!(matches!(view(busy).first(), Some(Block { kind: BlockKind::Source, range }) if range.len() == busy.len()));
}

#[test]
fn what_the_view_does_not_know_keeps_its_source() {
    let text = "Before \\foo{bar} after.\n\n\\begin{tikzpicture}\\draw (0,0);\\end{tikzpicture}";
    let blocks = view(text);
    let sources: Vec<&str> = inlines(&blocks).iter().filter(|i| i.kind == InlineKind::Source).map(|i| &text[i.range.clone()]).collect();
    assert_eq!(sources, ["\\foo{bar}"]);
    assert!(matches!(blocks.last(), Some(Block { kind: BlockKind::Source, range }) if text[range.clone()].starts_with("\\begin{tikzpicture}")));
}

#[test]
fn an_input_alone_is_the_file_in_place_and_one_in_a_sentence_stays_a_chip() {
    let blocks = view("\\section{Intro}\n\\input{sections/intro}\n\\include{b}\n\nText \\input{c} here.\n");
    let included: Vec<&str> =
        blocks.iter().filter_map(|b| if let BlockKind::Include { name, .. } = &b.kind { Some(name.as_str()) } else { None }).collect();
    assert_eq!(included, ["sections/intro", "b"]);
    assert!(inlines(&blocks).iter().any(|i| matches!(&i.kind, InlineKind::Chip { kind: ChipKind::File, key, .. } if key == "c")));
}

#[test]
fn a_theorem_shows_its_name_title_and_label_and_its_body_as_text() {
    let text = "\\newtheorem{thm}{Theorem}[section]\n\\newtheorem{lem}[thm]{Lemma}\n\\begin{document}\n\\begin{thm}[Pythagoras]\\label{t:p}\nThe \\emph{square}.\n\\end{thm}\n\\begin{lem} [Small] Easy.\\end{lem}\n\\begin{proof}Clear.\\end{proof}\n\\end{document}\n";
    let blocks = view(text);
    let theorems: Vec<_> = blocks
        .iter()
        .filter_map(|b| if let BlockKind::Theorem { kind, title, label, blocks } = &b.kind { Some((kind, title, label, blocks)) } else { None })
        .collect();
    assert_eq!(theorems.len(), 3, "{blocks:#?}");
    let (kind, title, label, body) = theorems[0];
    assert_eq!((kind.name.as_str(), kind.within.as_deref(), label.as_deref()), ("Theorem", Some("section"), Some("t:p")));
    assert!(matches!(&title[..], [Inline { kind: InlineKind::Text { text, .. }, .. }] if text == "Pythagoras"));
    assert!(texts(&inlines(body)).iter().any(|(t, s)| t == "square" && s.italic), "the body is formatted text");
    let (lemma, small, ..) = theorems[1];
    assert_eq!((lemma.counter.as_str(), lemma.within.as_deref()), ("thm", Some("section")), "a shared counter is reset as its owner is");
    assert!(
        matches!(&small[..], [Inline { kind: InlineKind::Text { text, .. }, .. }] if text == "Small"),
        "an undeclared optional argument is read too"
    );
    assert!(!theorems[2].0.numbered, "a proof has no number");
}

#[test]
fn an_elsarticle_front_matter_is_a_title_block_with_its_abstract_and_keywords() {
    // #44: it was shown as its source.
    let text = include_str!("../fixtures/frontmatter.tex");
    let blocks = view(text);
    let Some(fm) = blocks.iter().find_map(|b| if let BlockKind::FrontMatter(fm) = &b.kind { Some(fm) } else { None }) else {
        panic!("no front matter in {blocks:?}")
    };
    let words = |inlines: &[Inline]| texts(&inlines.iter().collect::<Vec<_>>()).into_iter().map(|(t, _)| t).collect::<String>();
    assert_eq!(words(&fm.title), "Scheduling Lighthouse Shifts by the Tides", "the note's mark is not the title's text");
    assert_eq!(fm.title_notes, ["t1"]);
    let names: Vec<String> = fm.authors.iter().map(|a| words(&a.name)).collect();
    assert_eq!(names, ["Ada Quill", "Bram Ostrander", "Cleo Marsh"]);
    assert_eq!(
        fm.authors.get(1).map(|a| (a.affiliations.clone(), a.notes.clone())),
        Some((vec!["harbour".into(), "tide".into()], vec!["fn1".into()]))
    );
    assert!(fm.authors.iter().all(|a| a.emails.len() == 1), "each author's e-mail is theirs");
    let places: Vec<String> = fm.affiliations.iter().map(|a| words(&a.text)).collect();
    assert_eq!(places, ["Harbour Institute, Port Example, Nowhere", "Tide Research Group, Example University"]);
    assert_eq!(fm.notes.len(), 3);
    // The text stands for its source: the title's words are where they are written.
    for i in &fm.title {
        if let InlineKind::Text { text: shown, .. } = &i.kind {
            assert_eq!(&text[i.range.clone()], shown.as_str());
        }
    }
    let containers: Vec<&str> =
        fm.blocks.iter().filter_map(|b| if let BlockKind::Container { name, .. } = &b.kind { Some(name.as_str()) } else { None }).collect();
    assert_eq!(containers, ["abstract", "keyword"]);
    assert!(!fm.blocks.iter().any(|b| matches!(b.kind, BlockKind::Source)), "something in it is still shown as source");
    let keywords = fm.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Container { name, blocks } if name == "keyword" => Some(shown_blocks(blocks)),
        _ => None,
    });
    assert_eq!(keywords.as_deref(), Some("lighthouses · tides · scheduling"));
}

fn shown_blocks(blocks: &[Block]) -> String {
    texts(&inlines(blocks)).into_iter().map(|(t, _)| t).collect()
}
