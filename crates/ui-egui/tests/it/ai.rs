//! AI1 and AI2 in the window: Settings › AI connects through the engine and keeps no token in the
//! preferences; a build message's Explain asks before the first request and shows the answer in
//! its card. A fake model answers; nothing reaches a network.

use std::sync::{Arc, Mutex};

use craft_ai::{AiError, GenOpts, Provider, ProviderInfo};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
fn settle(h: &mut Harness<'_, App>) -> bool {
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

fn sent(s: &Arc<Mutex<Vec<String>>>) -> usize {
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

#[test]
fn explain_asks_once_shows_what_is_sent_and_puts_the_answer_in_the_card() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut s = session(&prompts)?;
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": TOKEN, "model": "small"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let mut h = harness(s);
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    // Declined: nothing goes.
    h.get_by_label("Explain").click();
    h.run();
    assert!(h.query_by_label_contains("https://api.example.com").is_some(), "the dialog does not say where it goes");
    assert!(h.query_by_label_contains("\\textbf{x").is_some(), "the dialog does not show the source lines sent");
    h.get_by_label("Don't Send").click();
    h.run();
    assert_eq!(sent(&prompts), 0, "a declined request was sent");
    // Agreed: the answer comes into the card.
    h.get_by_label("Explain").click();
    h.run();
    h.get_by_label("Agree and Explain").click();
    h.run();
    assert!(settle(&mut h), "the explanation did not arrive");
    assert!(h.query_by_label_contains("never closed").is_some(), "the explanation is not in the card");
    assert_eq!(sent(&prompts), 1);
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
/// words are suggested as one types (and the user agreed to it).
fn editor(reply: &'static str, text: &str, sent: &Arc<Mutex<Vec<String>>>, complete: bool, vim: bool) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Failing));
    let sent = sent.clone();
    s.set_ai_connect(Arc::new(move |_: &Target| Ok(Box::new(Says(reply, sent.clone())) as Box<dyn Provider>)));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": TOKEN, "model": "small"}))?;
    if complete {
        s.execute("ai.consent", &json!({"request": "complete"}))?;
    }
    let prefs =
        Prefs { auto_compile: false, layout: "editor".to_owned(), ai: s.ai_saved().0.clone(), ai_complete: complete, vim, ..Prefs::default() };
    Ok(Harness::builder()
        .with_size(egui::vec2(1500.0, 800.0))
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs)))
}

#[test]
fn rewrite_shorter_on_a_selection_asks_first_and_makes_a_suggestion_card() -> Result<(), String> {
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
    assert!(h.query_by_label_contains("https://api.example.com").is_some(), "the dialog does not say where the selection goes");
    assert_eq!(sent(&prompts), 0, "the selection was sent before the user agreed");
    h.get_by_label("Agree and Rewrite").click();
    h.run();
    assert!(settle(&mut h), "the rewrite did not arrive");
    assert_eq!(sent(&prompts), 1);
    let file =
        h.state().session().project().text(&texcraft_engine::project::ProjectPath::new("main.tex").map_err(|e| e.to_string())?).map(str::to_owned);
    assert_eq!(file.ok().as_deref(), Some(text), "the rewrite was applied directly");
    h.run();
    assert!(h.query_by_label_contains("Suggestion by AI (small)").is_some(), "no suggestion card for the rewrite");
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
