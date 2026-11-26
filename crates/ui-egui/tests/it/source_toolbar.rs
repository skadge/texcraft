//! The source toolbar at the default split (docs/ROADMAP.md, E13): small groups that give way to
//! More formatting one at a time, formulas and references as drop-downs, and the pickers of the
//! formatted view for citations and cross-references, inserting at the caret.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

/// The toolbar's buttons after the mode switch, in their order.
const BUTTONS: [&str; 9] = ["Heading", "Bold", "Italic", "Formula", "References", "Link", "Figure", "Table", "Bulleted List"];

fn harness(main: &str, others: &[(&str, &str)], size: egui::Vec2) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for (path, body) in others {
        let _ = s.execute("file.new", &json!({"path": path}));
        assert!(s.execute("file.write", &json!({"path": path, "text": body})).is_ok());
    }
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": main})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let prefs = Prefs { auto_compile: false, layout: "split".to_owned(), side: "files".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(size).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    for _ in 0..4 {
        h.run();
    }
    h
}

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

/// The toolbar's buttons shown, with their places.
fn shown(h: &Harness<'_, App>) -> Vec<(String, egui::Rect)> {
    BUTTONS
        .iter()
        .filter_map(|name| h.query_by_role_and_label(Role::Button, name).map(|n| ((*name).to_owned(), n.rect())))
        .filter(|(_, r)| r.width() > 0.0)
        .collect()
}

#[test]
fn at_the_default_split_most_groups_show_and_the_rest_are_in_more() {
    let mut h = harness("\\documentclass{article}\n\\begin{document}\nHello.\n\\end{document}\n", &[], egui::vec2(1400.0, 900.0));
    let buttons = shown(&h);
    let names: Vec<&str> = buttons.iter().map(|(n, _)| n.as_str()).collect();
    // Before, Heading, Bold and Italic alone: one group of six.
    assert!(names.len() >= 6, "the toolbar shows only {names:?}");
    assert_eq!(names, BUTTONS[..names.len()], "the groups give way from the end, one at a time");
    let more = h.query_by_role_and_label(Role::Button, "More formatting").map(|n| n.rect());
    let find = h.query_by_role_and_label(Role::Button, "Find and replace").map(|n| n.rect());
    let mut all = buttons.clone();
    all.extend(more.map(|r| ("More formatting".to_owned(), r)));
    all.extend(find.map(|r| ("Find and replace".to_owned(), r)));
    all.extend(
        h.query_all_by(|n| matches!(n.role(), Role::RadioButton | Role::CheckBox))
            .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect())),
    );
    for (i, (a, ra)) in all.iter().enumerate() {
        for (b, rb) in all.iter().skip(i + 1) {
            let o = ra.intersect(*rb);
            assert!(!(o.width() > 1.0 && o.height() > 1.0), "{a} overlaps {b}");
        }
    }
    let pane = h.get_by_label("main.tex source").rect();
    assert!(all.iter().all(|(_, r)| r.right() <= pane.right() + 1.0), "a button reaches past the editor pane");
    if names.len() < BUTTONS.len() {
        h.get_by_role_and_label(Role::Button, "More formatting").click();
        h.run();
        h.run();
        assert!(h.query_by_label("Numbered List").is_some(), "the hidden groups are not in More formatting");
        assert!(h.query_all_by_label("Bold").count() == 1, "a shown group is in More formatting too");
    }
}

#[test]
fn formulas_and_references_are_each_one_drop_down() {
    let mut h = harness("Hello.\n", &[], egui::vec2(1400.0, 900.0));
    for (menu, entries) in [("Formula", &["Inline Formula", "Numbered Equation", "Symbols…"][..]), ("References", &["Cross-Reference", "Citation"])]
    {
        h.get_by_role_and_label(Role::Button, menu).click();
        h.run();
        h.run();
        for e in entries {
            assert!(h.query_by_label(e).is_some(), "{menu} does not offer {e}");
        }
        h.key_press(egui::Key::Escape);
        h.run();
    }
}

#[test]
fn citations_and_cross_references_in_the_source_come_from_the_pickers() {
    let bib = "@book{knuth84, author = {Knuth, Donald}, title = {The TeXbook}, year = 1984}\n";
    let original = "\\begin{figure}\\caption{A plot}\\label{fig:plot}\\end{figure}\nAs shown\n";
    let mut h = harness(original, &[("refs.bib", bib)], egui::vec2(1400.0, 900.0));
    h.get_by_label("main.tex source").focus();
    h.run();
    // The caret at the end of "As shown".
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    h.run();
    h.key_press(egui::Key::ArrowLeft);
    h.run();
    ui_cmd::run(h.state_mut(), "edit.cite");
    h.run();
    assert!(h.query_by_label("Insert a Citation").is_some(), "no picker opened in the source");
    h.get_by_label("Type part of a key, an author or a title").type_text("texbook");
    h.run();
    h.key_press(egui::Key::Enter);
    for _ in 0..3 {
        h.run();
    }
    assert!(text(&mut h).contains("As shown~\\cite{knuth84}\n"), "{:?}", text(&mut h));
    ui_cmd::run(h.state_mut(), "edit.ref");
    h.run();
    assert!(h.query_by_label_contains("A plot").is_some(), "the label does not say its caption");
    h.get_by_label("Type part of a label or of what it labels").type_text("plot");
    h.run();
    h.key_press(egui::Key::Enter);
    for _ in 0..3 {
        h.run();
    }
    assert!(text(&mut h).contains("\\cite{knuth84}\\ref{fig:plot}\n"), "{:?}", text(&mut h));
    assert!(h.query_by_label("Insert a Cross-Reference").is_none(), "the picker stayed open");
}

#[test]
fn a_narrow_pane_keeps_the_mode_switch_named_when_it_shows_icons() {
    let h = harness("Hello.\n", &[], egui::vec2(900.0, 700.0));
    assert!(h.query_by_role_and_label(Role::RadioButton, "Source").is_some(), "the mode switch lost its name");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Suggest changes").is_some(), "the Suggest switch lost its name");
    assert!(h.query_by_role_and_label(Role::Button, "Heading").is_some(), "at 900 px the first group is in More");
}
