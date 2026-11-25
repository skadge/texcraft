//! Folding: the gutter's fold markers, the Fold commands and their shortcuts, Vim's `zc` and
//! `zo`, and jumps into a fold opening it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

const DOC: &str =
    "\\documentclass{article}\n\\begin{document}\n\\section{Intro}\nAlpha line.\nBeta line.\n\\section{Next}\nGamma.\n\\end{document}\n";

fn harness(vim: bool) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": DOC}))?;
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    Ok(harness)
}

/// The height of the text run of a line of the source, by its text.
fn line_height(harness: &Harness<'_, App>, line: &str) -> f32 {
    harness.get_by_value(&format!("{line}\n")).rect().height()
}

fn go_to_line(harness: &mut Harness<'_, App>, line: u32) {
    ui_cmd::run(harness.state_mut(), "edit.goto_line");
    harness.run();
    harness.get_by_label("Line number").type_text(&line.to_string());
    harness.run();
    harness.get_by_label("Go").click();
    // The line gone to flashes for a moment.
    harness.run_steps(4);
}

#[test]
fn folding_a_section_hides_its_lines_and_its_gutter_marker_unfolds_it() -> Result<(), String> {
    let mut harness = harness(false)?;
    let shown = line_height(&harness, "Alpha line.");
    assert!(shown > 1.0);
    harness.get_by_label("Fold lines 3–5, section Intro").click();
    harness.run_steps(2);
    assert!(line_height(&harness, "Alpha line.") < shown / 2.0, "the section's lines are still shown");
    assert!(line_height(&harness, "\\section{Next}") > 1.0, "the next section was hidden too");
    assert!(harness.query_by_label_contains("Folded lines 3–5").is_some(), "the fold was not announced");
    // The marker and the "…" chip both unfold; the marker is the first.
    let unfold = harness.get_all_by_label("Unfold lines 3–5, section Intro").count();
    assert_eq!(unfold, 2, "the folded line has its marker and its chip");
    harness.get_all_by_label("Unfold lines 3–5, section Intro").next().unwrap().click();
    harness.run_steps(2);
    assert!(line_height(&harness, "Alpha line.") > shown / 2.0, "the marker did not unfold the section");
    Ok(())
}

#[test]
fn the_fold_shortcut_folds_at_the_caret_and_going_to_a_hidden_line_unfolds() -> Result<(), String> {
    let mut harness = harness(false)?;
    let shown = line_height(&harness, "Beta line.");
    go_to_line(&mut harness, 4);
    harness.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::OpenBracket);
    harness.run_steps(2);
    assert!(line_height(&harness, "Beta line.") < shown / 2.0, "Cmd+Shift+[ did not fold the section around the caret");
    assert!(harness.query_by_label_contains("Ln 3,").is_some(), "the caret stayed inside the fold");
    go_to_line(&mut harness, 5);
    assert!(line_height(&harness, "Beta line.") > shown / 2.0, "going to a hidden line left it hidden");
    assert!(harness.query_by_label_contains("Ln 5,").is_some());
    Ok(())
}

#[test]
fn after_going_to_a_line_with_the_go_button_typing_goes_to_the_editor() -> Result<(), String> {
    let mut harness = harness(false)?;
    go_to_line(&mut harness, 4);
    harness.run_steps(4);
    for c in "Q!".chars() {
        harness.event(egui::Event::Text(c.to_string()));
    }
    harness.run_steps(2);
    let path = texcraft_engine::project::ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    let text = harness.state_mut().session().project().text(&path).map(str::to_owned).map_err(|e| e.to_string())?;
    assert!(text.contains("Q!Alpha line."), "the text typed did not reach the editor at the line gone to: {text:?}");
    Ok(())
}

#[test]
fn vims_zc_and_zo_fold_and_unfold_and_j_steps_over_a_fold() -> Result<(), String> {
    let mut harness = harness(true)?;
    let shown = line_height(&harness, "Alpha line.");
    go_to_line(&mut harness, 3);
    harness.get_by_label("main.tex source").type_text("zc");
    harness.run_steps(2);
    assert!(line_height(&harness, "Alpha line.") < shown / 2.0, "zc did not fold");
    harness.get_by_label("main.tex source").type_text("j");
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("Ln 6,").is_some(), "j did not step over the fold");
    assert!(line_height(&harness, "Alpha line.") < shown / 2.0, "j opened the fold");
    harness.get_by_label("main.tex source").type_text("kzo");
    harness.run_steps(2);
    assert!(line_height(&harness, "Alpha line.") > shown / 2.0, "zo did not unfold");
    Ok(())
}
