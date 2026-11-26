use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn double_click(harness: &mut Harness<'_, App>, at: egui::Pos2) {
    // Long enough after the last click not to make a triple-click of it.
    for _ in 0..30 {
        harness.step();
    }
    harness.event(egui::Event::PointerMoved(at));
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            harness.step();
        }
    }
    // The line reached flashes for a moment: step rather than wait for it to settle.
    harness.run_steps(5);
}

#[test]
fn the_formatted_view_shows_headings_without_their_commands_and_goes_back_to_their_source() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Some \\textbf{bold} text.\n\n\\section{Methods}\nMore.\n"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    // Steps short enough for two clicks to make a double-click.
    let mut harness =
        Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    // The heading shows its number, as the PDF does.
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Heading, "1\u{2002}Methods").is_none(), "shown formatted before asking");
    harness.get_by_label("Formatted").click();
    harness.run();
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Heading, "1\u{2002}Methods").is_some(), "no heading named 1 Methods");
    assert!(harness.query_by_label_contains("\\section").is_none(), "the command shows in the formatted view");
    assert!(harness.query_by_label("main.tex source").is_none(), "the source is still shown");

    let at = harness.get_by_role_and_label(egui::accesskit::Role::Heading, "1\u{2002}Methods").rect().center();
    double_click(&mut harness, at);
    assert!(harness.query_by_label("main.tex source").is_some(), "a double-click did not go back to the source");
    // The caret is at the heading's command, on the third line.
    assert!(harness.query_by_label_contains("Ln 3,").is_some(), "the caret is not at the heading");
}

#[test]
fn a_formula_is_drawn_typeset_once_retex_has_typeset_it() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let text = "\\documentclass{article}\n\\begin{document}\nInline $x^2$ here.\n\\[ \\frac{a}{b} \\]\nBad $\\nosuchmacro$.\n\\end{document}\n";
    s.execute("file.write", &json!({"path": "main.tex", "text": text})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run_steps(3);
    harness.get_by_label("Formatted").click();
    harness.run_steps(3);
    // Until it is typeset, the formula shows as its source.
    let image = |h: &Harness<'_, App>| h.query_by_role_and_label(egui::accesskit::Role::Image, "\\frac{a}{b}").is_some();
    let started = std::time::Instant::now();
    while !image(&harness) && started.elapsed() < std::time::Duration::from_secs(120) {
        std::thread::sleep(std::time::Duration::from_millis(50));
        harness.run_steps(2);
    }
    assert!(image(&harness), "the display formula was not drawn typeset");
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Paragraph, "Inline x^2 here.").is_some(), "the paragraph's name lost its formula");
}
