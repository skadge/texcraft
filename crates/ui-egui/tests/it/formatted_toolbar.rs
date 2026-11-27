//! The formatted view's toolbar and its structural keys: each makes a minimal edit of the source
//! (the new markup in, the rest byte for byte as it was) and shows where the caret is.

use egui::accesskit::{Role, Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::ui_cmd;

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

/// The formatted view of `main.tex` (and the other files), its text holding the keyboard.
fn formatted(main: &str, others: &[(&str, &str)]) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for (path, body) in others {
        let _ = s.execute("file.new", &json!({"path": path}));
        let written = s.execute("file.write", &json!({"path": path, "text": body}));
        assert!(written.is_ok(), "{written:?}");
    }
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": main})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    h
}

fn key(h: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(modifiers, key);
    h.run();
}

/// The caret after the first `n` characters shown (Home, then Right `n` times).
fn caret_at(h: &mut Harness<'_, App>, n: usize) {
    key(h, egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0..n {
        key(h, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
}

fn run(h: &mut Harness<'_, App>, id: &str, params: serde_json::Value) {
    ui_cmd::run_with(h.state_mut(), id, &params);
    h.run();
    h.run();
}

fn pressed(h: &Harness<'_, App>, name: &str) -> bool {
    h.get_by_role_and_label(Role::Button, name).accesskit_node().data().toggled() == Some(Toggled::True)
}

/// `after` is `before` with `inserted` added at one place, nothing else changed.
fn only_added(before: &str, after: &str, inserted: &[&str]) {
    let mut rest = after.to_owned();
    for i in inserted {
        assert!(rest.contains(i), "{i:?} is not in {after:?}");
        rest = rest.replacen(i, "", 1);
    }
    assert_eq!(rest, before, "more than the markup changed");
}

#[test]
fn the_toolbar_styles_the_word_at_the_caret_and_shows_it_pressed() {
    let original = "Some words here.\n\nAnother paragraph.\n";
    let mut h = formatted(original, &[]);
    caret_at(&mut h, 7);
    assert!(!pressed(&h, "Bold"));
    h.get_by_role_and_label(Role::Button, "Bold").click();
    h.run();
    h.run();
    let bold = text(&mut h);
    only_added(original, &bold, &["\\textbf{", "}"]);
    assert!(bold.contains("\\textbf{words}"));
    assert!(pressed(&h, "Bold"), "Bold is not shown pressed in \\textbf");
    // Code and Italic the same way, each by its command alone.
    h.get_by_role_and_label(Role::Button, "Code").click();
    h.run();
    h.run();
    let code = text(&mut h);
    assert!(code.contains("\\textbf{\\texttt{words}}"), "{code}");
    assert!(pressed(&h, "Code"));
    h.run();
    h.run();
    // The text keeps the keyboard: typing goes on in it.
    assert!(h.get_by_label("main.tex formatted").accesskit_node().is_focused(), "a toolbar button kept the keyboard");
}

#[test]
fn the_paragraph_style_follows_the_document_class() {
    let article = "\\documentclass{article}\n\\begin{document}\nIntroduction\n\nText.\n\\end{document}\n";
    let mut h = formatted(article, &[]);
    assert!(h.query_by_label("Body Text, paragraph style").is_some());
    run(&mut h, "edit.heading_1", json!({}));
    assert_eq!(text(&mut h), article.replace("Introduction", "\\section{Introduction}"));
    assert!(h.query_by_label("Heading 1, paragraph style").is_some(), "the style shown did not follow the caret");
    // Heading 2 renames the command; Body Text takes it away again.
    run(&mut h, "edit.heading_2", json!({}));
    assert!(text(&mut h).contains("\\subsection{Introduction}"));
    run(&mut h, "edit.body", json!({}));
    assert_eq!(text(&mut h), article);

    let report = "\\documentclass{report}\n\\begin{document}\nIntroduction\n\\end{document}\n";
    let mut h = formatted(report, &[]);
    run(&mut h, "edit.heading_1", json!({}));
    assert!(text(&mut h).contains("\\chapter{Introduction}"), "a report's first heading level is the chapter");
}

#[test]
fn tab_shift_tab_and_enter_shape_a_list() {
    let original = "\\begin{itemize}\n  \\item one\n  \\item two\n\\end{itemize}\n";
    let mut h = formatted(original, &[]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Tab);
    let nested = text(&mut h);
    assert!(nested.contains("\\item one\n  \\begin{itemize}\n    \\item two\n  \\end{itemize}"), "{nested}");
    assert!(h.get_by_label("main.tex formatted").accesskit_node().is_focused(), "Tab left the text in a list item");
    key(&mut h, egui::Modifiers::SHIFT, egui::Key::Tab);
    assert_eq!(text(&mut h), original, "Shift+Tab did not undo Tab");
    // Enter at the end of the last item, and again on the empty item: the list ends there.
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.get_by_label("main.tex formatted").type_text("After");
    h.run();
    let after = text(&mut h);
    assert!(after.starts_with("\\begin{itemize}\n  \\item one\n  \\item two\n\\end{itemize}"), "{after}");
    assert!(h.query_by_role_and_label(Role::Paragraph, "After").is_some(), "the text after the list is not a paragraph: {after}");
    // Backspace at a paragraph's start does not touch the list; at an item's start it lifts it.
    let mut h = formatted("\\section{Title}\n\nText.\n", &[]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Backspace);
    assert_eq!(text(&mut h), "Title\n\nText.\n", "Backspace at a heading's start did not make it body text");
}

#[test]
fn a_list_button_on_an_empty_line_starts_a_list_with_the_caret_in_its_item() {
    // #47: on the empty paragraph Enter makes, Numbered List did nothing.
    let mut h = formatted("First paragraph.\n", &[]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.get_by_role_and_label(Role::Button, "Numbered List").click();
    h.run();
    h.run();
    h.get_by_label("main.tex formatted").type_text("Item");
    h.run();
    let after = text(&mut h);
    assert!(after.starts_with("First paragraph.\n\n\\begin{enumerate}\n  \\item Item\n\\end{enumerate}"), "{after}");
    assert!(pressed(&h, "Numbered List"), "the caret is not in the new list");
}

/// The bounds of the formatted view's text runs whose value is `value`.
fn runs(h: &Harness<'_, App>, value: &str) -> Vec<egui::Rect> {
    h.query_all_by_role(Role::TextRun).filter(|n| n.value().as_deref() == Some(value)).map(|n| n.rect()).collect()
}

#[test]
fn an_empty_item_is_a_line_tall_where_its_marker_is() {
    // #46: the caret of an empty item stood half a line above it, between it and the item before.
    let mut h = formatted("\\begin{enumerate}\n  \\item One\n  \\item \n\\end{enumerate}\n", &[]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    let (Some(one), Some(empty)) = (runs(&h, "One").first().copied(), runs(&h, "").first().copied()) else {
        panic!("the items' text runs are not in the tree");
    };
    assert!(empty.height() > one.height() * 0.8, "the empty item's line ({empty:?}) is not as tall as a written one's ({one:?})");
    assert!(empty.top() >= one.bottom(), "the empty item's line ({empty:?}) overlaps the item before ({one:?})");
    assert!((empty.left() - one.left()).abs() < 1.0, "the empty item's text does not start where the other's does");
}

#[test]
fn a_citation_and_a_reference_come_from_pickers_of_the_project() {
    let bib = "@book{knuth84, author = {Knuth, Donald}, title = {The TeXbook}, year = 1984}\n";
    let original = "\\documentclass{article}\n\\begin{document}\n\\section{Intro}\\label{sec:intro}\nAs shown\n\\end{document}\n";
    let mut h = formatted(original, &[("refs.bib", bib)]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowDown);
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    run(&mut h, "edit.cite", json!({}));
    assert!(h.query_by_label("Insert a Citation").is_some(), "no picker opened");
    assert!(h.query_by_label_contains("Knuth (1984)").is_some(), "the entry does not say its author and year");
    h.get_by_label("Type part of a key, an author or a title").type_text("texbook");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.run();
    only_added(original, &text(&mut h), &["~\\cite{knuth84}"]);
    run(&mut h, "edit.ref", json!({}));
    assert!(h.query_by_label_contains("Intro, main.tex").is_some(), "the label does not say what it labels");
    h.get_by_label("Type part of a label or of what it labels").type_text("intro");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.run();
    assert!(text(&mut h).contains("\\cite{knuth84}\\ref{sec:intro}"), "{}", text(&mut h));
    // A citation in the view says who and when on hover: its chip names the entry.
}

#[test]
fn a_link_and_a_footnote_ask_for_their_text() {
    let original = "Read the manual today.\n";
    let mut h = formatted(original, &[]);
    caret_at(&mut h, 9);
    key(&mut h, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    run(&mut h, "edit.link", json!({}));
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label("The address the text links to").type_text("https://example.org");
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    h.run();
    only_added(original, &text(&mut h), &["\\href{https://example.org}{", "}"]);
    run(&mut h, "edit.footnote", json!({"text": "50% off"}));
    assert!(text(&mut h).contains("\\footnote{50\\% off}"), "{}", text(&mut h));
}

#[test]
fn undo_takes_back_typing_a_word_at_a_time() {
    let mut h = formatted("Start.\n", &[]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    for word in [" one", " two"] {
        for c in word.chars() {
            h.get_by_label("main.tex formatted").type_text(&c.to_string());
            h.run();
        }
    }
    assert_eq!(text(&mut h), "Start. one two\n");
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    // As in the source: a word goes with the space typed after it.
    assert_eq!(text(&mut h), "Start. one \n", "undo did not take back the last word alone");
    key(&mut h, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    assert_eq!(text(&mut h), "Start. one two\n");
}

/// Knows a few words; offers "word" for "wrod".
struct Fake;

impl texcraft_engine::spelling::Dictionary for Fake {
    fn available(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn check(&self, word: &str, _: &str) -> bool {
        ["the", "is", "here", "word"].contains(&word.to_lowercase().as_str())
    }
    fn suggest(&self, word: &str, _: &str, _: usize) -> Vec<String> {
        if word == "wrod" { vec!["word".to_owned()] } else { Vec::new() }
    }
}

#[test]
fn a_misspelled_word_s_menu_in_the_view_offers_its_spelling() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_dictionary(Box::new(Fake));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": "The \\textbf{wrod} is here.\n"})).is_ok());
    let spelling = texcraft_engine::SpellingOptions { language: "en-GB".to_owned(), ..Default::default() };
    let app = App::with_prefs(s, Prefs { auto_compile: false, spelling, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    caret_at(&mut h, 5);
    key(&mut h, egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    assert!(h.get_by_label("Change to “word”").is_focused(), "the first spelling did not get the keyboard");
    assert!(h.query_by_label("Add to Dictionary").is_some() && h.query_by_label_contains("Cut").is_some());
    h.get_by_label("Change to “word”").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), "The \\textbf{word} is here.\n", "the word's source was not replaced alone");
    // Escape from a menu opened by the keyboard gives the keyboard back to the text.
    key(&mut h, egui::Modifiers::SHIFT, egui::Key::F10);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    h.run();
    assert!(h.get_by_label("main.tex formatted").accesskit_node().is_focused());
}

#[test]
fn italic_is_pressed_in_emph_and_takes_it_off_and_a_declaration_says_why_it_stays() {
    let original = "Some \\emph{words} and {\\bfseries strong text} here.\n";
    let mut h = formatted(original, &[]);
    caret_at(&mut h, 7);
    assert!(pressed(&h, "Italic"), "Italic is not shown pressed in \\emph");
    h.get_by_role_and_label(Role::Button, "Italic").click();
    h.run();
    h.run();
    let plain = original.replace("\\emph{words}", "words");
    assert_eq!(text(&mut h), plain);
    // In the declaration's group: Bold is pressed, and pressing it changes nothing and says why.
    h.get_by_label("main.tex formatted").focus();
    h.run();
    caret_at(&mut h, "Some words and str".len());
    assert!(pressed(&h, "Bold"), "Bold is not shown pressed after \\bfseries");
    h.get_by_role_and_label(Role::Button, "Bold").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), plain);
    assert!(h.query_by_label_contains("declaration").is_some(), "the status line does not say why");
}

#[test]
fn the_toolbars_groups_stay_clear_of_the_more_menu() {
    // A group is sent to the More menu only when it does not fit (#47's test lost its list
    // buttons by a few pixels with the interface's own faces); one shown never runs under it.
    let h = formatted("First paragraph.\n", &[]);
    let Some(more) = h.query_by_label("More formatting").map(|n| n.rect()) else { return };
    for name in
        ["Bold", "Italic", "Code", "Bulleted List", "Numbered List", "Outdent Item", "Indent Item", "Link", "Footnote", "Citation", "Table", "Figure"]
    {
        if let Some(r) = h.query_by_role_and_label(Role::Button, name).map(|n| n.rect()) {
            assert!(r.right() <= more.left(), "{name} ({r:?}) runs under the More menu ({more:?})");
        }
    }
}

#[test]
fn on_a_phone_bold_and_italic_stay_on_the_toolbar_and_the_paragraph_styles_go_to_more() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": "Some words.\n"})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(390.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    let more = h.get_by_label("More formatting").rect();
    for name in ["Bold", "Italic"] {
        let r = h.query_by_role_and_label(Role::Button, name).map(|n| n.rect());
        assert!(r.is_some_and(|r| r.right() <= more.left()), "{name} is not on the phone's toolbar, left of More");
    }
    h.get_by_role_and_label(Role::Button, "More formatting").click();
    h.run();
    h.run();
    assert!(h.query_by_label_contains("Heading 1").is_some(), "the paragraph styles are not in the More menu");
}

#[test]
fn a_style_pressed_with_nothing_selected_shows_pressed_and_waits_for_the_text_typed() {
    let original = "Some words here.\n";
    let mut h = formatted(original, &[]);
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    h.get_by_role_and_label(Role::Button, "Bold").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), original, "the source holds a style before anything is typed");
    assert!(pressed(&h, "Bold"), "Bold pressed with nothing selected is not shown pressed");
    h.event(egui::Event::Text(" New".to_owned()));
    h.run();
    h.run();
    assert!(text(&mut h).contains("here. \\textbf{New}"), "{:?}", text(&mut h));
    assert!(pressed(&h, "Bold"));
    // Pressed again at the bold text's end: what is typed next is not bold.
    h.get_by_role_and_label(Role::Button, "Bold").click();
    h.run();
    h.run();
    assert!(!pressed(&h, "Bold"), "Bold pressed off is still shown pressed");
    h.event(egui::Event::Text("er".to_owned()));
    h.run();
    h.run();
    assert!(text(&mut h).contains("\\textbf{New}er"), "{:?}", text(&mut h));
    // A style pressed, then the caret moved: it is no longer pending.
    h.get_by_role_and_label(Role::Button, "Italic").click();
    h.run();
    h.run();
    assert!(pressed(&h, "Italic"));
    key(&mut h, egui::Modifiers::NONE, egui::Key::ArrowLeft);
    assert!(!pressed(&h, "Italic"), "a style pressed stays pending after the caret moved");
}
