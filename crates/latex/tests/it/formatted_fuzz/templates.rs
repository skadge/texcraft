//! The templates' structures shown formatted (round three): beamer's frames, a title page, a
//! bibliography, the commands that print what the view names, the document's own macros and a
//! letter's recipient, each edited as text with minimal source edits.

use texcraft_latex::edit;
use texcraft_latex::view::{self, BlockKind, ChipKind, InlineKind};

use super::sim::{Editor, Op};

/// Whether the view shows something of `text` as its source.
fn shows_source(text: &str) -> bool {
    let mut found = false;
    view::walk(&view::view(text), &mut |b| {
        found |= matches!(b.kind, BlockKind::Source | BlockKind::UnsupportedTable(_))
            || view::inlines_of(b).iter().any(|i| matches!(i.kind, InlineKind::Source));
    });
    found
}

/// The chips of `kind` the view of `text` shows, by their command and key.
fn chips(text: &str, kind: ChipKind) -> Vec<(String, String)> {
    let mut out = Vec::new();
    view::walk(&view::view(text), &mut |b| {
        for i in view::inlines_of(b) {
            if let InlineKind::Chip { kind: k, command, key } = &i.kind
                && *k == kind
            {
                out.push((command.clone(), key.clone()));
            }
        }
    });
    out
}

fn plain(text: &str) -> String {
    super::lines::shown(text, None).plain()
}

/// `s` typed with the caret at byte `at`.
fn typed_at(text: &str, at: usize, s: &str) -> String {
    let mut e = Editor::new(text);
    (e.anchor, e.head) = (at, at);
    assert!(e.run(&Op::Type(s.to_owned())).is_ok());
    e.text
}

#[test]
fn a_beamer_frame_shows_its_title_and_its_content_as_text() {
    let text = "\\begin{document}\n\\begin{frame}{Why}\n  \\begin{itemize}\n    \\item<2-> First\n  \\end{itemize}\n  \\pause\n  Then \\only<3>{later}.\n\\end{frame}\n\\begin{frame}\n  \\frametitle{Named}\n  \\begin{columns}\n    \\begin{column}{0.5\\textwidth}\n      Left.\n    \\end{column}\n    \\begin{column}{0.5\\textwidth}\n      \\begin{block}{Note}\n        Right.\n      \\end{block}\n    \\end{column}\n  \\end{columns}\n\\end{frame}\n\\end{document}";
    assert!(!shows_source(text), "a frame is shown as source");
    let shown = plain(text);
    for words in ["Why", "First", "Then", "later", "Named", "Left.", "Note", "Right."] {
        assert!(shown.contains(words), "{words:?} is not shown: {shown:?}");
    }
    let overlays = chips(text, ChipKind::Overlay);
    assert!(overlays.iter().any(|(c, k)| c == "item" && k == "2-"), "the item's overlay is no chip: {overlays:?}");
    assert!(overlays.iter().any(|(c, _)| c == "pause"), "\\pause is no chip: {overlays:?}");
    // The title is edited in its braces; a line break cannot go there.
    let at = text.find("Why").map_or(0, |i| i + 3);
    assert!(typed_at(text, at, " not").contains("\\begin{frame}{Why not}"));
    assert!(edit::enter(text, at).is_none(), "Enter breaks the frame's title");
    // Typed after an overlay's chip (at the item's start), the text goes after the specification.
    let item = text.find("\\item").map_or(0, |i| i + "\\item".len());
    assert!(typed_at(text, item, "A ").contains("\\item<2->A  First") || typed_at(text, item, "A ").contains("\\item<2->A First"));
}

#[test]
fn a_title_page_shows_its_lines_and_its_spacing_shows_nothing() {
    let text = "\\begin{document}\n\\begin{titlepage}\n  \\centering\n  \\vspace*{3cm}\n  {\\Huge\\bfseries The Title\\par}\n  \\vspace{1cm}\n  Your University\\\\[0.5cm]\n  \\vfill\n  \\today\n\\end{titlepage}\n\\end{document}";
    assert!(!shows_source(text), "the title page shows source");
    let shown = plain(text);
    assert!(shown.contains("The Title") && shown.contains("Your University"), "{shown:?}");
    assert!(!shown.contains("0.5cm") && !shown.contains("3cm"), "spacing shows as text: {shown:?}");
    assert!(view::centred(text, &(text.find("\\begin{titlepage}").unwrap_or(0)..text.find("\\end{document}").unwrap_or(0))));
    assert!(chips(text, ChipKind::Placeholder).iter().any(|(c, _)| c == "today"), "\\today is not a placeholder");
    // The title's size is the page's.
    let mut big = false;
    view::walk(&view::view(text), &mut |b| {
        big |= view::inlines_of(b)
            .iter()
            .any(|i| matches!(&i.kind, InlineKind::Text { text, style } if text.contains("Title") && style.size > 2 && style.bold));
    });
    assert!(big, "\\Huge\\bfseries is not a large bold title");
}

#[test]
fn a_bibliography_shows_its_entries_with_their_keys_and_their_text_is_edited() {
    let text = "\\begin{document}\n\\begin{thebibliography}{9}\n\\bibitem{knuth} D. Knuth, The book.\n\\bibitem[Ann]{ann} A. Writer.\n\\end{thebibliography}\n\\end{document}";
    assert!(!shows_source(text), "the bibliography is shown as source");
    let keys: Vec<String> = chips(text, ChipKind::Entry).into_iter().map(|(_, k)| k).collect();
    assert_eq!(keys, ["knuth", "ann"]);
    assert!(plain(text).contains("D. Knuth, The book."));
    // Typed before an entry's chip, the text goes in the entry, not before its `\bibitem`.
    let at = text.find("\\bibitem{knuth}").unwrap_or(0);
    let after = typed_at(text, at, "x");
    assert!(after.contains("\\bibitem{knuth}x"), "{after:?}");
}

#[test]
fn generated_lists_page_breaks_and_today_are_placeholders_not_source() {
    let text = "\\begin{document}\n\\tableofcontents\n\\listoffigures\n\nText.\n\\newpage\n\\appendix\nMore, \\today.\n\\end{document}";
    assert!(!shows_source(text));
    let names: Vec<String> = chips(text, ChipKind::Placeholder).into_iter().map(|(c, _)| c).collect();
    for n in ["tableofcontents", "listoffigures", "newpage", "appendix", "today"] {
        assert!(names.iter().any(|c| c == n), "{n} is not a placeholder: {names:?}");
    }
}

#[test]
fn a_macros_text_arguments_show_as_text_and_others_keep_their_source() {
    let defs = "\\newcommand{\\cventry}[3]{\\begin{tabular}{ll}#1 & \\textbf{#2}\\\\ & #3\\end{tabular}\\par}\n\\newcommand{\\gap}[1]{\\vspace{#1}}\n\\newcommand{\\pic}[1]{\\includegraphics{#1}}\n\\newcommand{\\task}[1][]{\\par\\noindent Task\\ifx\\relax#1\\relax\\else\\ (#1)\\fi.\\ }\n\\NewDocumentCommand{\\pair}{m m}{#1 and #2}\n\\NewDocumentCommand{\\verbish}{v}{#1}\n\\def\\old#1{#1}\n";
    let shown = |body: &str| format!("{defs}\\begin{{document}}\n{body}\n\\end{{document}}");
    let entry = shown("\\cventry{2020}{Keeper}{North harbour.}");
    assert!(!shows_source(&entry), "a macro of text arguments is shown as source");
    let p = plain(&entry);
    assert!(p.contains("2020") && p.contains("Keeper") && p.contains("North harbour."), "{p:?}");
    assert!(chips(&entry, ChipKind::Macro).iter().any(|(c, _)| c == "cventry"));
    // Its second argument is bold, as the definition sets it.
    let mut bold = false;
    view::walk(&view::view(&entry), &mut |b| {
        bold |= view::inlines_of(b).iter().any(|i| matches!(&i.kind, InlineKind::Text { text, style } if text == "Keeper" && style.bold));
    });
    assert!(bold, "the bold argument is not bold");
    // A width, a file, a verbatim argument and `\def`: their source.
    for body in ["\\gap{2cm}", "\\pic{gauge}", "\\verbish{x}", "\\old{x}"] {
        assert!(shows_source(&shown(body)), "{body} is not shown as its source");
    }
    // Optional arguments tested for emptiness are text.
    let task = shown("\\task[2 points] Explain.");
    assert!(!shows_source(&task) && plain(&task).contains("2 points"), "{:?}", plain(&task));
    assert!(!shows_source(&shown("\\pair{North}{South}")));
    // Typed right after the chip, text goes in the first argument; a line break cannot split one.
    let at = entry.find("\\cventry{").map_or(0, |i| i + "\\cventry".len());
    assert!(typed_at(&entry, at, "x").contains("\\cventry{x2020}"));
    assert!(edit::enter(&entry, entry.find("Keeper").map_or(0, |i| i + 2)).is_none(), "Enter splits an argument");
    assert!(edit::enter(&entry, at).is_none(), "Enter splits the call from its arguments");
}

#[test]
fn bold_on_a_selection_holding_a_macros_chip_leaves_its_name_unwrapped() {
    let text = "\\newcommand{\\cventry}[2]{#1, #2}\n\\begin{document}\n\\cventry{a}{b}\n\\end{document}";
    let start = text.find("\\cventry{a}").unwrap_or(0);
    let pieces = [start..start + "\\cventry".len(), start + "\\cventry{".len()..start + "\\cventry{a".len()];
    let changes = edit::toggle_style(text, &pieces, "textbf");
    let after = edit::apply(text, &changes);
    assert!(after.contains("\\cventry{\\textbf{a}}{b}"), "{after:?}");
}

#[test]
fn a_selection_taking_a_macros_chip_and_text_deletes_the_whole_call() {
    let text = "\\newcommand{\\task}[1][]{Task (#1)}\n\\begin{document}\nA \\task[] b\n\\end{document}";
    let mut e = Editor::new(text);
    let start = text.rfind("\\task").unwrap_or(0);
    (e.anchor, e.head) = (start, start + "\\task[]".len());
    let shown = e.shown();
    let doc = shown.doc();
    let (Some(a), Some(b)) = (doc.locate(text, start, None), doc.locate(text, start + "\\task".len(), None)) else { panic!("not shown") };
    let deleted = doc.deleted(text, a, b);
    let after = edit::apply(text, &deleted.into_iter().map(|r| edit::Change { range: r, text: String::new() }).collect::<Vec<_>>());
    let body = after.split("\\begin{document}").nth(1).unwrap_or_default();
    assert!(!body.contains("\\task") && !body.contains("[]"), "the call's brackets are left: {after:?}");
}
