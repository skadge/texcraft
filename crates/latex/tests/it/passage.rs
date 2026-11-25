//! E12: the prose as passages a grammar checker reads, mapped back to the source.

use texcraft_latex::passage::{PLACEHOLDER, Passage, passages};
use texcraft_latex::prose::Options;

fn all(text: &str) -> Vec<Passage> {
    passages(text, Options::default())
}

/// The passage holding `needle`, and the characters of `needle` in it.
fn find<'a>(ps: &'a [Passage], needle: &str) -> Option<(&'a Passage, std::ops::Range<usize>)> {
    let p = ps.iter().find(|p| p.text.contains(needle))?;
    let at = p.text[..p.text.find(needle)?].chars().count();
    Some((p, at..at + needle.chars().count()))
}

#[test]
fn commands_drop_out_and_math_reads_as_a_word_so_the_sentence_reads_on() {
    let text = "This are \\textbf{wrong} $a$ sentence.";
    let ps = all(text);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].text, format!("This are wrong {PLACEHOLDER} sentence."));
    // Words written as they are map back to themselves, exactly.
    let (p, chars) = find(&ps, "This are").unwrap();
    let spot = p.source(chars).unwrap();
    assert_eq!(&text[spot.range.clone()], "This are");
    assert!(spot.exact && !spot.placeholder);
    // Across a command the source span holds it, and is not a place for a replacement.
    let (p, chars) = find(&ps, "are wrong").unwrap();
    let spot = p.source(chars).unwrap();
    assert_eq!(&text[spot.range.clone()], "are \\textbf{wrong");
    assert!(!spot.exact);
    // The formula's placeholder covers the formula.
    let (p, chars) = find(&ps, PLACEHOLDER).unwrap();
    let spot = p.source(chars).unwrap();
    assert_eq!(&text[spot.range], "$a$");
    assert!(spot.placeholder);
}

#[test]
fn paragraphs_items_cells_and_environments_are_passages_of_their_own() {
    let text = "One sentence\nacross lines.\n\nAnother paragraph. \\begin{itemize}\\item First thing.\\item Second\\end{itemize}\n\
                \\begin{tabular}{ll} cell one & cell two \\\\ three & four\\end{tabular}";
    let texts: Vec<String> = all(text).into_iter().map(|p| p.text.trim().to_owned()).collect();
    assert_eq!(texts, ["One sentence\nacross lines.", "Another paragraph.", "First thing.", "Second", "cell one", "cell two", "three", "four"]);
}

#[test]
fn headings_and_footnotes_are_apart_and_the_sentence_around_a_footnote_goes_on() {
    let text = "\\section[Short]{Long title}\\label{s}\nA word\\footnote{A note.} goes on.";
    let texts: Vec<String> = all(text).into_iter().map(|p| p.text.trim().to_owned()).collect();
    assert!(texts.contains(&"Short Long title".to_owned()), "{texts:?}");
    assert!(texts.contains(&"A note.".to_owned()), "{texts:?}");
    assert!(texts.contains(&"A word goes on.".to_owned()), "{texts:?}");
}

#[test]
fn references_citations_and_display_math_are_placeholders_verbatim_is_left_out() {
    let text = "See \\ref{sec:a} and \\cite[p.~3]{knuth}, then \\[ x = 1 \\] where \\verb|code| runs.\n\
                \\begin{equation} y \\end{equation} so 10\\% of it. \\begin{verbatim}not prose\\end{verbatim} End.";
    let joined: Vec<String> = all(text).into_iter().map(|p| p.text).collect();
    let joined = joined.join("|");
    assert!(joined.contains("See X and X, then X where X runs."), "{joined}");
    assert!(joined.contains("X so 10% of it."), "{joined}");
    assert!(!joined.contains("not prose") && !joined.contains("sec:a") && !joined.contains("knuth"), "{joined}");
}

#[test]
fn quotes_dashes_ties_and_accented_words_read_as_typeset() {
    let text = "A caf\\'e, ``quoted'' -- and Figure~2.";
    let ps = all(text);
    assert_eq!(ps[0].text, "A cafe, “quoted” – and Figure 2.");
    // What was written otherwise is no place for a replacement.
    let (p, chars) = find(&ps, "cafe").unwrap();
    let spot = p.source(chars).unwrap();
    assert!(text[spot.range].starts_with("caf\\'e"));
    assert!(!spot.exact);
}

#[test]
fn hostile_input_never_panics_and_maps_inside_the_text() {
    for text in [
        "\\",
        "{{{{}",
        "}}}} a",
        "\\begin{",
        "$$ unclosed",
        "\\verb",
        "\\footnote{\\footnote{\\footnote{deep",
        "a\\'",
        "\\c{",
        "é\\'{é}ü \\ss{} \\o",
        "\\section[",
        "x & & \\\\[",
        "\\begin{equation} no end",
    ] {
        for p in all(text) {
            let n = p.text.chars().count();
            for a in 0..=n {
                for b in a..=n {
                    if let Some(spot) = p.source(a..b) {
                        assert!(spot.range.end <= text.len() && text.get(spot.range.clone()).is_some(), "{text:?} {a}..{b} {spot:?}");
                    }
                }
            }
        }
    }
}
