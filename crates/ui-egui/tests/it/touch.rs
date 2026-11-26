//! Touch screens (#26): a finger's drag scrolls the text instead of selecting it; a long press,
//! then moving the finger, selects. Events as a browser on a phone sends them: the touch and the
//! pointer it moves.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn harness(text: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": text})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h =
        Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn finger(h: &mut Harness<'_, App>, phase: egui::TouchPhase, pos: egui::Pos2) {
    h.event(egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(1), phase, pos, force: None });
    h.event(egui::Event::PointerMoved(pos));
    let button = match phase {
        egui::TouchPhase::Start => Some(true),
        egui::TouchPhase::End => Some(false),
        _ => None,
    };
    if let Some(pressed) = button {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.step();
}

/// A finger put down at `from`, held for `hold` seconds, moved to `to` and lifted.
fn gesture(h: &mut Harness<'_, App>, from: egui::Pos2, hold: f32, to: egui::Pos2) {
    finger(h, egui::TouchPhase::Start, from);
    for _ in 0..(hold * 60.0) as usize {
        h.step();
    }
    for k in 1..=10 {
        finger(h, egui::TouchPhase::Move, from + (to - from) * (k as f32 / 10.0));
    }
    finger(h, egui::TouchPhase::End, to);
    h.event(egui::Event::PointerGone);
    h.run_steps(10);
}

/// The top of the source's line `value` (with its line break), as assistive technology has it.
fn line_top(h: &Harness<'_, App>, value: &str) -> f32 {
    h.query_all_by_role(Role::TextRun).find(|n| n.value().as_deref() == Some(value)).map_or(f32::NAN, |n| n.rect().min.y)
}

/// The middle of the start of the source's line `value`.
fn run_center(h: &Harness<'_, App>, value: &str) -> Option<egui::Pos2> {
    let r = h.query_all_by_role(Role::TextRun).find(|n| n.value().as_deref() == Some(value))?.rect();
    Some(egui::pos2(r.min.x + 10.0, r.center().y))
}

fn center(n: &egui_kittest::Node<'_>) -> egui::Pos2 {
    n.rect().center()
}

#[test]
fn a_finger_scrolls_the_source_and_a_long_press_selects() {
    let source: String = (1..=120).map(|n| format!("line {n}\n")).collect();
    let mut h = harness(&source);
    let before = line_top(&h, "line 1\n");
    let Some(from) = run_center(&h, "line 20\n") else { panic!("line 20 is not shown") };
    gesture(&mut h, from, 0.0, from - egui::vec2(0.0, 300.0));
    let after = line_top(&h, "line 1\n");
    assert!(after < before - 100.0, "the drag did not scroll the source with the finger: {before} → {after}");
    // Nothing was selected: typed text goes in, nothing is replaced.
    h.get_by_label("main.tex source").focus();
    h.run_steps(2);
    h.get_by_label("main.tex source").type_text("X");
    h.run();
    let typed = text(&mut h);
    assert_eq!(typed.len(), source.len() + 1, "the finger's drag selected text: {:?}", &typed[..60]);

    // A long press on a line, then the finger moved down a line: what is typed replaces from the
    // pressed word to the finger.
    let mut h = harness(&source);
    let Some(third) = run_center(&h, "line 3\n") else { panic!("line 3 is not shown") };
    let height = line_top(&h, "line 4\n") - line_top(&h, "line 3\n");
    gesture(&mut h, third, 1.2, third + egui::vec2(0.0, height));
    h.get_by_label("main.tex source").focus();
    h.run_steps(2);
    h.get_by_label("main.tex source").type_text("X");
    h.run();
    let typed = text(&mut h);
    assert!(typed.starts_with("line 1\nline 2\n") && typed.len() < source.len(), "a long press and a move selected nothing: {:?}", &typed[..40]);
}

#[test]
fn a_finger_scrolls_the_formatted_view_without_moving_the_caret() {
    let source: String = (1..=60).map(|n| format!("Paragraph {n} of the text.\n\n")).collect();
    let mut h = harness(&source);
    h.get_by_label("Formatted").click();
    h.run_steps(20);
    let first = h.get_by_role_and_label(Role::Paragraph, "Paragraph 1 of the text.").rect().min.y;
    let at = center(&h.get_by_role_and_label(Role::Paragraph, "Paragraph 3 of the text."));
    gesture(&mut h, at + egui::vec2(0.0, 200.0), 0.0, at - egui::vec2(0.0, 100.0));
    let moved = h.query_by_role_and_label(Role::Paragraph, "Paragraph 1 of the text.").map_or(f32::NEG_INFINITY, |n| n.rect().min.y);
    assert!(moved < first - 100.0, "the drag did not scroll the formatted view: {first} → {moved}");
    assert_eq!(text(&mut h), source);
}
