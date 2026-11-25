//! AI1 and AI2 in the window: Settings › AI connects through the engine, keeps no token in the
//! preferences and lists each kind of request with its switch; a build message's Explain sends at
//! once (connecting agreed to it) and shows the answer in its card. A fake model answers; nothing
//! reaches a network.

use std::sync::{Arc, Mutex};

use craft_ai::{AiError, GenOpts, Provider, ProviderInfo};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::{Backend, Diagnostic, Job, Outcome, Severity, Status};
use texcraft_engine::Session;
use texcraft_engine::ai::Target;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

const TOKEN: &str = "sk-ui-test-secret";

struct Fake(Arc<Mutex<Vec<String>>>);

impl Provider for Fake {
    fn name(&self) -> String {
        "fake".into()
    }
    fn complete(&mut self, prompt: &str, _: &GenOpts) -> Result<String, AiError> {
        if let Ok(mut p) = self.0.lock() {
            p.push(prompt.to_owned());
        }
        Ok("A brace opened on this line is never closed.".into())
    }
    fn test(&mut self) -> Result<ProviderInfo, AiError> {
        Ok(ProviderInfo { label: "fake".into(), models: vec!["small".into(), "large".into()] })
    }
    fn is_local(&self) -> bool {
        false
    }
}

/// One error at line 2 of main.tex.
struct Failing;

impl Backend for Failing {
    fn name(&self) -> String {
        "test".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let d = Diagnostic {
            severity: Severity::Error,
            message: "Missing } inserted.".into(),
            origin: None,
            file: Some("main.tex".into()),
            line: Some(2),
            column: None,
            end_column: None,
            help: Vec::new(),
            fixes: Vec::new(),
        };
        Outcome { status: Status::Failed, diagnostics: vec![d], ..Outcome::unavailable(job.id, "") }
    }
}

pub(crate) fn session(sent: &Arc<Mutex<Vec<String>>>) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Failing));
    let sent = sent.clone();
    s.set_ai_connect(Arc::new(move |_: &Target| Ok(Box::new(Fake(sent.clone())) as Box<dyn Provider>)));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\textbf{x\n"}))?;
    Ok(s)
}

pub(crate) fn harness(s: Session) -> Harness<'static, App> {
    let prefs = Prefs { auto_compile: false, ai: s.ai_saved().0.clone(), ..Prefs::default() };
    let app = App::with_prefs(s, prefs);
    Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), app)
}

/// Runs frames until no AI request is in flight.
pub(crate) fn settle(h: &mut Harness<'_, App>) -> bool {
    for _ in 0..300 {
        h.step();
        if !h.state().session().ai_busy() {
            h.run();
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

pub(crate) fn sent(s: &Arc<Mutex<Vec<String>>>) -> usize {
    s.lock().map(|p| p.len()).unwrap_or_default()
}

#[test]
fn settings_connect_lists_models_and_keep_the_token_out_of_the_preferences() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = harness(session(&prompts)?);
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("AI").click();
    h.run();
    h.get_by_label("Endpoint").click();
    h.run();
    h.get_by_label("Endpoint").type_text("https://api.example.com/v1");
    h.get_by_label("Token").click();
    h.run();
    h.get_by_label("Token").type_text(TOKEN);
    h.run();
    h.get_by_label("Connect").scroll_to_me();
    h.run();
    h.get_by_label("Connect").click();
    h.run();
    assert!(settle(&mut h), "listing the models did not end");
    assert_eq!(h.state().session().ai_models(), ["small", "large"], "the models were not listed");
    let prefs = h.state_mut().take_changed_prefs().ok_or("the connection was not kept in the preferences")?;
    assert!(prefs.ai.connection.is_some());
    let kept = serde_json::to_string(&prefs).map_err(|e| e.to_string())?;
    assert!(!kept.contains(TOKEN), "the token is in the preferences");
    assert_eq!(sent(&prompts), 0, "connecting sent document text");
    Ok(())
}

/// The dialog open, by its name (null for none).
fn dialog(h: &mut Harness<'_, App>) -> serde_json::Value {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["dialog"].clone()
}

/// Issue #15: connected, Explain sends at once: no dialog asks first.
#[test]
fn explain_sends_at_once_without_a_dialog_and_puts_the_answer_in_the_card() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut s = session(&prompts)?;
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": TOKEN, "model": "small"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let mut h = harness(s);
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    h.get_by_label("Explain").click();
    h.run();
    assert_eq!(dialog(&mut h), json!(null), "a dialog opened to ask first");
    assert!(settle(&mut h), "the explanation did not arrive");
    assert!(h.query_by_label_contains("never closed").is_some(), "the explanation is not in the card");
    assert_eq!(sent(&prompts), 1);
    Ok(())
}

/// Issue #15: Settings › AI lists each kind of request with what it sends and a switch; one
/// switched off is not sent and its button is disabled with the reason, kept over a restart; the
/// request log still shows what a request sent; another model asks to agree again.
#[test]
fn a_kind_switched_off_in_settings_disables_its_entry_and_another_model_asks_again() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let keychain: Arc<dyn texcraft_engine::ai::Secrets> = Arc::new(texcraft_engine::ai::SessionSecrets::default());
    let compiled = |prompts: &Arc<Mutex<Vec<String>>>| -> Result<Session, String> {
        let mut s = session(prompts)?;
        s.set_ai_secrets(keychain.clone());
        s.execute("build.compile", &json!({}))?;
        s.wait();
        Ok(s)
    };
    let mut s = compiled(&prompts)?;
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": TOKEN, "model": "small"}))?;
    // Tall enough for the whole of Settings › AI.
    let prefs = Prefs { auto_compile: false, ai: s.ai_saved().0.clone(), ..Prefs::default() };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 2000.0))
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    h.get_by_label("Explain").click();
    h.run();
    assert!(settle(&mut h), "the explanation did not arrive");
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("AI").click();
    h.run();
    assert!(h.query_by_label_contains("The section at the caret.").is_some(), "Settings › AI does not say what a kind sends");
    h.get_by_label_contains("1 request sent").scroll_to_me();
    h.run();
    h.get_by_label_contains("1 request sent").click();
    h.run();
    let shown = |h: &Harness<'_, App>| h.query_all_by_label_contains("Missing } inserted").count();
    let before = shown(&h);
    // Settings scrolls a tab taller than the window.
    h.get_by_label("Show What Was Sent").scroll_to_me();
    h.run();
    h.get_by_label("Show What Was Sent").click();
    h.run();
    assert!(shown(&h) > before, "what was sent is not shown");
    // Switched off from the keyboard: kept in the preferences.
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Explain").focus();
    h.run();
    h.key_press(egui::Key::Space);
    h.run();
    let prefs = h.state_mut().take_changed_prefs().ok_or("the switch was not kept in the preferences")?;
    assert_eq!(prefs.ai.off, vec!["explain".to_owned()]);
    let kept = serde_json::to_string(&prefs).map_err(|e| e.to_string())?;
    assert!(!kept.contains("Missing } inserted"), "the preferences hold what was sent");
    // The next run of the app: the Explain button is disabled, and nothing more is sent.
    let prefs: Prefs = serde_json::from_str(&kept).map_err(|e| e.to_string())?;
    let mut again = compiled(&prompts)?;
    again.restore_ai(prefs.ai.clone());
    let app = App::with_prefs(again, Prefs { auto_compile: false, ..prefs });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    let explain = h.get_by_role_and_label(egui::accesskit::Role::Button, "Explain");
    assert!(explain.accesskit_node().is_disabled(), "the Explain button of a kind switched off is enabled");
    h.state_mut().control("engine.execute", &json!({"command": "ai.explain", "params": {"index": 0}}));
    assert_eq!(sent(&prompts), 1, "a kind switched off was sent");
    // Another model: Settings › AI asks to agree again.
    h.state_mut().control("engine.execute", &json!({"command": "ai.model", "params": {"model": "large"}}));
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("AI").click();
    h.run();
    assert!(h.query_by_label("Agree to Send").is_some(), "another model did not ask to agree again");
    h.get_by_label("Agree to Send").scroll_to_me();
    h.run();
    h.get_by_label("Agree to Send").click();
    h.run();
    assert!(h.query_by_label("Agree to Send").is_none(), "agreeing did not hold");
    Ok(())
}

/// A model that answers every prompt with the same text.
struct Says(&'static str, Arc<Mutex<Vec<String>>>);

impl Provider for Says {
    fn name(&self) -> String {
        "fake".into()
    }
    fn complete(&mut self, prompt: &str, _: &GenOpts) -> Result<String, AiError> {
        if let Ok(mut p) = self.1.lock() {
            p.push(prompt.to_owned());
        }
        Ok(self.0.into())
    }
    fn test(&mut self) -> Result<ProviderInfo, AiError> {
        Ok(ProviderInfo { label: "fake".into(), models: vec!["small".into()] })
    }
    fn is_local(&self) -> bool {
        false
    }
}

/// The editor on `text`, connected to a model that answers `reply`; with `complete`, the next
/// words are suggested as one types.
pub(crate) fn editor(
    reply: &'static str,
    text: &str,
    sent: &Arc<Mutex<Vec<String>>>,
    complete: bool,
    vim: bool,
) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Failing));
    let sent = sent.clone();
    s.set_ai_connect(Arc::new(move |_: &Target| Ok(Box::new(Says(reply, sent.clone())) as Box<dyn Provider>)));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": TOKEN, "model": "small"}))?;
    let prefs =
        Prefs { auto_compile: false, layout: "editor".to_owned(), ai: s.ai_saved().0.clone(), ai_complete: complete, vim, ..Prefs::default() };
    Ok(Harness::builder()
        .with_size(egui::vec2(1500.0, 800.0))
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs)))
}

#[test]
fn rewrite_shorter_on_a_selection_sends_at_once_without_a_dialog_and_makes_a_suggestion_card() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let text = "The quick brown fox jumps over the lazy dog again and again.\n";
    let mut h = editor("<<<TEXT\nThe fox jumps over the dog.\nTEXT>>>", text, &prompts, false, false)?;
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.select_all");
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    h.get_by_label("Rewrite with AI").click();
    h.run();
    h.get_by_label("Shorter").click();
    h.run();
    assert_eq!(dialog(&mut h), json!(null), "a dialog opened to ask first");
    assert!(settle(&mut h), "the rewrite did not arrive");
    assert_eq!(sent(&prompts), 1);
    let file =
        h.state().session().project().text(&texcraft_engine::project::ProjectPath::new("main.tex").map_err(|e| e.to_string())?).map(str::to_owned);
    assert_eq!(file.ok().as_deref(), Some(text), "the rewrite was applied directly");
    h.run();
    assert!(h.query_by_label_contains("Suggestion by AI (small)").is_some(), "no suggestion card for the rewrite");
    Ok(())
}

/// Issue #14: bullet points selected, Ask AI to Change This… in the context menu, an instruction:
/// the longer text arrives as a suggestion, and the instruction is offered again next time.
#[test]
fn ask_ai_to_change_this_sends_the_instruction_and_the_change_is_a_suggestion() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let text = "\\begin{itemize}\n\\item fast\n\\item small\n\\end{itemize}\n";
    let reply = "<<<TEXT\nThe method is fast, much faster than the others, and small enough to fit anywhere.\nTEXT>>>";
    let mut h = editor(reply, text, &prompts, false, false)?;
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.select_all");
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    h.get_by_label("Rewrite with AI").click();
    h.run();
    h.get_by_label("Ask AI to Change This…").click();
    h.run();
    h.get_by_label("How to change the selection").type_text("Flesh out these bullet points");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(settle(&mut h), "the change did not arrive");
    assert!(prompts.lock().map(|p| p[0].contains("Flesh out these bullet points")).unwrap_or(false), "the instruction was not sent");
    let file =
        h.state().session().project().text(&texcraft_engine::project::ProjectPath::new("main.tex").map_err(|e| e.to_string())?).map(str::to_owned);
    assert_eq!(file.ok().as_deref(), Some(text), "the change was applied directly");
    h.run();
    assert!(h.query_by_label_contains("Suggestion by AI (small)").is_some(), "no suggestion card for the change");
    // Asked again: the instruction is offered, and Escape leaves.
    ui_cmd::run(h.state_mut(), "edit.select_all");
    ui_cmd::run(h.state_mut(), "edit.instruct");
    h.run();
    assert!(h.query_by_label("Flesh out these bullet points").is_some(), "the last instruction is not offered");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("How to change the selection").is_none(), "Escape did not close the dialog");
    Ok(())
}

/// Types `typed` at the end of the first line, waits for the next words and takes them with Tab:
/// the file's text after.
fn take_the_next_words(vim: bool, typed: &str) -> Result<String, String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = editor(" over the lazy dog.", "\\begin{document}\nThe quick brown fox\n\\end{document}\n", &prompts, true, vim)?;
    let main = texcraft_engine::project::ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    // To the end of "The quick brown fox".
    h.get_by_label("main.tex source").type_text(if vim { "jA" } else { "" });
    if !vim {
        h.key_press(egui::Key::ArrowDown);
        h.key_press(egui::Key::End);
    }
    h.run();
    h.get_by_label("main.tex source").type_text(typed);
    h.run();
    assert_eq!(sent(&prompts), 0, "the next words were asked for at once, without a pause");
    let mut shown = false;
    for _ in 0..200 {
        h.step();
        if h.query_by_label_contains("Suggested next words").is_some() {
            shown = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(shown, "no next words were shown after a pause");
    assert_eq!(sent(&prompts), 1);
    assert!(
        prompts.lock().map(|p| p[0].contains("The quick brown fox jumps") && !p[0].contains("end{document}")).unwrap_or(false),
        "not the paragraph up to the caret"
    );
    h.key_press(egui::Key::Tab);
    h.run();
    Ok(h.state().session().project().text(&main).map(str::to_owned).unwrap_or_default())
}

#[test]
fn the_next_words_appear_after_a_pause_and_tab_takes_them() -> Result<(), String> {
    let text = take_the_next_words(false, " jumps")?;
    assert!(text.contains("The quick brown fox jumps over the lazy dog.\n"), "{text}");
    Ok(())
}

#[test]
fn in_vims_insert_mode_tab_takes_the_next_words_too() -> Result<(), String> {
    let text = take_the_next_words(true, " jumps")?;
    assert!(text.contains("The quick brown fox jumps over the lazy dog.\n"), "{text}");
    Ok(())
}
