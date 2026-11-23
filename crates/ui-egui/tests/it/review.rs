//! R1: commenting in the editor (docs/collaboration-roadmap.md).

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::review::State;
use texcraft_ui_egui::prefs::{Identity, Prefs};
use texcraft_ui_egui::{App, ui_cmd};

fn harness(text: &str) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    let prefs = Prefs { auto_compile: false, layout: "editor".to_owned(), identity: Some(Identity::new("Ada")?), ..Prefs::default() };
    Ok(Harness::builder().with_size(egui::vec2(1500.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs)))
}

#[test]
fn a_comment_on_a_selection_shows_in_the_margin_resolve_folds_it_and_the_filter_hides_it() {
    let mut h = harness("\\section{Intro}\nThe quick brown fox.\n").unwrap();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.select_all");
    ui_cmd::run(h.state_mut(), "edit.comment");
    h.run();
    h.get_by_role_and_label(Role::MultilineTextInput, "Comment").type_text("Is this right?");
    h.run();
    h.get_by_role_and_label(Role::Button, "Comment").click();
    h.run();
    h.run();
    let threads = h.state_mut().session().review().threads().to_vec();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].messages[0].text, "Is this right?");
    assert!(h.query_by_label_contains("Comment by Ada on “\\section{Intro}").is_some(), "no card in the margin");

    h.get_by_label("Resolve").click();
    h.run();
    h.run();
    assert!(matches!(h.state_mut().session().review().threads()[0].state, State::Resolved { .. }));
    assert!(h.query_by_label_contains("Resolved by Ada").is_some(), "the resolved thread did not fold");
    assert!(h.query_by_label("Is this right?").is_none(), "the folded card still shows its message");

    h.get_by_role_and_label(Role::RadioButton, "Open").click();
    h.run();
    assert!(h.query_by_label_contains("Comment by Ada").is_none(), "the Open filter shows a resolved thread");
}

#[test]
fn a_narrow_editor_shows_the_threads_in_the_review_panel() {
    let mut h = harness("one two three\n").unwrap();
    let made = h
        .state_mut()
        .control("engine.execute", &json!({"command": "review.comment", "params": {"path": "main.tex", "start": 4, "end": 7, "text": "Two?"}}));
    assert_eq!(made["ok"], true, "{made}");
    h.set_size(egui::vec2(800.0, 700.0));
    h.run();
    assert!(h.query_by_label_contains("Comment by Ada").is_none(), "a margin in a narrow pane");
    h.get_by_label("1 comment").click();
    h.run();
    assert!(h.query_by_label_contains("Comment by Ada on “two”").is_some(), "the Review panel does not list the thread");
}
