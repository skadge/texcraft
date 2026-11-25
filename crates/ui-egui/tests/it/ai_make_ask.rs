//! AI4 and AI5 in the window: Make with AI previews the model's LaTeX typeset and inserts it at the
//! caret; Add a Reference with AI says models invent references and suggests the entry in a
//! `.bib`; the Ask panel shows answers with links to the lines they name; Review This Section
//! leaves comments. A fake model answers; nothing reaches a network.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::{App, ui_cmd};

use crate::ai::{editor, sent, settle};

const DOC: &str = "\\documentclass{article}\n\\begin{document}\n\\section{Results}\nThe values grow fast.\nWe stop here.\n\\end{document}\n";

fn main_text(h: &Harness<'_, App>) -> String {
    ProjectPath::new("main.tex").ok().and_then(|p| h.state().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn engine(h: &mut Harness<'_, App>, command: &str, params: serde_json::Value) -> serde_json::Value {
    h.state_mut().control("engine.execute", &json!({"command": command, "params": params}))["result"].clone()
}

fn open_suggestions(h: &mut Harness<'_, App>) -> u64 {
    engine(h, "review.list", json!({"kind": "suggestion"}))["openSuggestions"].as_u64().unwrap_or(0)
}

/// The caret at the start of line 4 ("The values grow fast.").
fn caret_on_line_4(h: &mut Harness<'_, App>) {
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    for _ in 0..3 {
        h.key_press(egui::Key::ArrowDown);
    }
    h.run();
}

/// As on the desktop by default: the editor beside the PDF, too narrow for the margin of
/// suggestions. Insert puts the LaTeX in the text at the caret, where it is seen, and Undo takes it
/// out (issue #13: a suggestion there was nowhere to be seen).
#[test]
fn make_with_ai_sends_at_once_previews_the_typeset_latex_and_insert_puts_it_at_the_caret() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let reply = "<<<LATEX\n\\begin{tabular}{cc}\nA & B\\\\\n1 & 2\n\\end{tabular}\nLATEX>>>";
    let mut h = editor(reply, DOC, &prompts, false, false)?;
    ui_cmd::run(h.state_mut(), "view.layout.split");
    caret_on_line_4(&mut h);
    ui_cmd::run(h.state_mut(), "edit.ai_make");
    h.run();
    h.get_by_label("Describe it").click();
    h.run();
    h.get_by_label("Describe it").type_text("a table of two columns");
    h.run();
    h.get_by_label("Make").click();
    h.step();
    assert!(settle(&mut h), "the LaTeX was not made");
    assert!(h.query_by_label("It typesets.").is_some(), "the dialog does not say it typesets");
    assert!(h.query_by_label("The LaTeX typeset").is_some(), "no preview");
    assert_eq!(main_text(&h), DOC, "made LaTeX was put in before Insert");
    // The menu bar's Insert menu comes first; the dialog's button last.
    h.get_all_by_label("Insert").last().ok_or("no Insert button")?.click();
    h.run();
    h.run();
    let text = main_text(&h);
    let table = text.find("\\begin{tabular}").ok_or("Insert put nothing in the file")?;
    let line = text.find("The values grow fast.").ok_or("the line at the caret is gone")?;
    assert!(table < line && text.find("\\section{Results}").is_some_and(|s| s < table), "not inserted at the caret: {text}");
    assert_eq!(open_suggestions(&mut h), 0, "Insert made a suggestion as well");
    assert!(h.query_by_label_contains("Undo takes it out").is_some(), "the status line does not say what Insert did");
    // The editor shows it: the edit is the editor's to undo.
    ui_cmd::run(h.state_mut(), "edit.undo");
    h.run();
    h.run();
    assert_eq!(main_text(&h), DOC, "Undo did not take the inserted LaTeX out");
    Ok(())
}

#[test]
fn latex_that_does_not_typeset_shows_why_and_can_be_asked_for_again_with_the_error() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = editor("<<<LATEX\n\\begin{equation}\n\\nosuchcommand x\n\\end{equation}\nLATEX>>>", DOC, &prompts, false, false)?;
    caret_on_line_4(&mut h);
    ui_cmd::run(h.state_mut(), "edit.ai_make");
    h.run();
    h.get_by_label("Equation").click();
    h.get_by_label("Describe it").click();
    h.run();
    h.get_by_label("Describe it").type_text("an equation");
    h.run();
    h.get_by_label("Make").click();
    h.step();
    assert!(settle(&mut h), "nothing was made");
    assert!(h.query_by_label_contains("It does not typeset").is_some(), "the dialog does not say it does not typeset");
    h.get_by_label("Ask Again with the Error").click();
    h.step();
    assert!(settle(&mut h), "asking again did not end");
    assert_eq!(sent(&prompts), 2);
    assert!(
        prompts.lock().map(|p| p[1].contains("nosuchcommand") && p[1].contains("did not typeset")).unwrap_or(false),
        "the error was not sent back"
    );
    assert_eq!(open_suggestions(&mut h), 0, "what does not typeset was inserted");
    Ok(())
}

#[test]
fn add_a_reference_warns_that_models_invent_them_and_suggests_the_entry_in_a_new_bib() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = editor("<<<BIB\n@book{knuth84,\n  title = {The {TeX}book}\n}\nBIB>>>", DOC, &prompts, false, false)?;
    h.run();
    ui_cmd::run(h.state_mut(), "edit.ai_reference");
    h.run();
    assert!(h.query_by_label_contains("Models invent references").is_some(), "the dialog does not warn that references are invented");
    h.get_by_label("The work's title or DOI").click();
    h.run();
    h.get_by_label("The work's title or DOI").type_text("The TeXbook");
    h.run();
    h.get_by_label("Draft Entry").click();
    h.run();
    h.step();
    assert!(settle(&mut h), "no entry was drafted");
    assert!(h.query_by_label_contains("suggested change at the end of references.bib").is_some(), "the dialog does not say where the draft is");
    h.get_by_label("Done").click();
    h.run();
    let bib = ProjectPath::new("references.bib").map_err(|e| e.to_string())?;
    assert_eq!(h.state().session().project().text(&bib).map(str::to_owned), Ok(String::new()), "the entry was written, not suggested");
    assert_eq!(open_suggestions(&mut h), 1);
    Ok(())
}

#[test]
fn the_ask_panel_shows_the_answer_and_its_link_goes_to_the_line() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = editor("The section starts at main.tex:3 of the file.", DOC, &prompts, false, false)?;
    h.run();
    ui_cmd::run(h.state_mut(), "view.side.ask");
    h.run();
    h.get_by_label("Your question").click();
    h.run();
    h.get_by_label("Your question").type_text("Where are the results?");
    h.run();
    h.get_by_label("Ask").click();
    h.run();
    h.step();
    assert!(settle(&mut h), "no answer");
    assert!(prompts.lock().map(|p| p[0].contains("3| \\section{Results}")).unwrap_or(false), "the open file was not sent with numbered lines");
    h.get_by_label("main.tex:3").click();
    h.run();
    h.run();
    let line = h.state_mut().control("ui.inspect", &json!({}))["result"]["cursor"]["line"].as_u64();
    assert_eq!(line, Some(3), "the link did not go to the line");
    Ok(())
}

#[test]
fn review_this_section_leaves_comments_and_says_how_many_remarks_were_dropped() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let reply = "QUOTE: The values grow fast.\nREMARK: How fast?\n\nQUOTE: words not in the text\nREMARK: Lost.";
    let mut h = editor(reply, DOC, &prompts, false, false)?;
    caret_on_line_4(&mut h);
    ui_cmd::run(h.state_mut(), "edit.ai_review");
    h.run();
    h.step();
    assert!(settle(&mut h), "the review did not end");
    assert!(prompts.lock().map(|p| p[0].contains("\\section{Results}") && !p[0].contains("documentclass")).unwrap_or(false), "not the section alone");
    assert!(h.query_by_label_contains("The review left 1 comment. 1 remark was dropped").is_some(), "the status does not count the comments");
    let list = engine(&mut h, "review.list", json!({"kind": "comment", "author": "AI (small)"}));
    assert_eq!(list["threads"].as_array().map(Vec::len), Some(1), "{list}");
    Ok(())
}
