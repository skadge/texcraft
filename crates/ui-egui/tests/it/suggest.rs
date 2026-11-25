//! R2: suggested changes in the editor (docs/collaboration-roadmap.md): Suggest mode, the cards'
//! Accept and Reject, and undoing an accept.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_engine::review::State;
use texcraft_ui_egui::prefs::{Identity, Prefs};
use texcraft_ui_egui::{App, ui_cmd};

fn harness(text: &str) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    let prefs = Prefs { auto_compile: false, layout: "editor".to_owned(), identity: Some(Identity::new("Ada")?), ..Prefs::default() };
    Ok(Harness::builder().with_size(egui::vec2(1500.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs)))
}

fn file(h: &mut Harness<'static, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn suggest(h: &mut Harness<'static, App>, start: usize, end: usize, text: &str) {
    let params = json!({"command": "review.suggest", "params": {"path": "main.tex", "start": start, "end": end, "text": text}});
    let made = h.state_mut().control("engine.execute", &params);
    assert_eq!(made["ok"], true, "{made}");
    h.run();
}

#[test]
fn in_suggest_mode_typing_over_a_word_makes_a_suggestion_card_and_accept_applies_it() {
    let mut h = harness("The colour of the sky.\n").unwrap();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.suggest_mode");
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    // Select "colour" and type "color" over it.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Home);
    for _ in 0..4 {
        h.key_press(egui::Key::ArrowRight);
    }
    for _ in 0..6 {
        h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    }
    h.run();
    for c in "color".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    assert_eq!(file(&mut h), "The colour of the sky.\n", "typing in Suggest mode changed the file");
    let threads = h.state_mut().session().review().threads().to_vec();
    assert_eq!(threads.len(), 1, "typing made {} suggestions", threads.len());
    assert!(
        h.query_by_label_contains("Suggestion by Ada: Replace “colour” with “color”").is_some(),
        "no suggestion card in the margin: {:?}",
        threads[0].suggestion
    );
    h.get_by_role_and_label(Role::Button, "Accept").click();
    h.run();
    h.run();
    assert_eq!(file(&mut h), "The color of the sky.\n", "Accept did not apply the suggestion");
    assert!(matches!(h.state_mut().session().review().threads()[0].state, State::Accepted { .. }));
}

#[test]
fn reject_leaves_the_text_as_it_is() {
    let mut h = harness("one two three\n").unwrap();
    h.run();
    suggest(&mut h, 4, 7, "2");
    assert!(h.query_by_label_contains("Suggestion by Ada: Replace “two” with “2”").is_some(), "no card for the suggestion");
    h.get_by_role_and_label(Role::Button, "Reject").click();
    h.run();
    h.run();
    assert_eq!(file(&mut h), "one two three\n");
    assert!(matches!(h.state_mut().session().review().threads()[0].state, State::Rejected { .. }));
}

#[test]
fn undo_after_accept_restores_the_text_and_reopens_the_suggestion() {
    let mut h = harness("one two three\n").unwrap();
    h.run();
    suggest(&mut h, 4, 7, "2");
    h.get_by_role_and_label(Role::Button, "Accept").click();
    h.run();
    h.run();
    assert_eq!(file(&mut h), "one 2 three\n");
    ui_cmd::run(h.state_mut(), "edit.undo");
    h.run();
    h.run();
    assert_eq!(file(&mut h), "one two three\n", "Undo did not take the accept back");
    assert!(h.state_mut().session().review().threads()[0].is_open(), "the undone accept left its suggestion closed");
}
