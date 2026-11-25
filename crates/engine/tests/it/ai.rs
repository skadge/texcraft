//! AI1 and AI2 (docs/ROADMAP.md): the connection and explaining build messages, against a fake
//! model behind the suite's provider trait. Nothing here reaches a network.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use craft_ai::{AiError, GenOpts, Provider, ProviderInfo};
use serde_json::{Value, json};
use texcraft_compile::{Backend, Diagnostic, Job, Outcome, Severity, Status};
use texcraft_engine::Session;
use texcraft_engine::ai::{Explanation, SecretString, Secrets, Target};
use texcraft_engine::review::Author;

type R = Result<(), Box<dyn std::error::Error>>;

/// What the fake model was sent, and how many times a provider was made.
#[derive(Default, Clone)]
struct Seen {
    prompts: Arc<Mutex<Vec<String>>>,
    made: Arc<Mutex<usize>>,
    /// What the model answers.
    reply: Arc<Mutex<String>>,
}

impl Seen {
    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().map(|p| p.clone()).unwrap_or_default()
    }
    fn made(&self) -> usize {
        self.made.lock().map(|m| *m).unwrap_or_default()
    }
    fn answer(&self, reply: &str) {
        if let Ok(mut r) = self.reply.lock() {
            *r = reply.to_owned();
        }
    }
}

struct Fake {
    seen: Seen,
}

impl Provider for Fake {
    fn name(&self) -> String {
        "fake".to_owned()
    }
    fn complete(&mut self, prompt: &str, _: &GenOpts) -> Result<String, AiError> {
        if let Ok(mut p) = self.seen.prompts.lock() {
            p.push(prompt.to_owned());
        }
        Ok(self.seen.reply.lock().map(|r| r.clone()).unwrap_or_default())
    }
    fn test(&mut self) -> Result<ProviderInfo, AiError> {
        Ok(ProviderInfo { label: "fake".to_owned(), models: vec!["small".to_owned(), "large".to_owned()] })
    }
    fn is_local(&self) -> bool {
        false
    }
}

/// A build that reports one error at `line` of main.tex.
struct Failing(u32);

impl Backend for Failing {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let d = Diagnostic {
            severity: Severity::Error,
            message: "Missing } inserted.".to_owned(),
            origin: None,
            file: Some("main.tex".to_owned()),
            line: Some(self.0),
            column: None,
            end_column: None,
            help: vec!["A group was not closed.".to_owned()],
            fixes: Vec::new(),
        };
        Outcome { status: Status::Failed, diagnostics: vec![d], ..Outcome::unavailable(job.id, "") }
    }
}

const BODY_LINES: usize = 30;

/// A document whose line `at` (from 1, in the body) has the error, with far-away lines that must
/// not be sent.
fn document() -> String {
    let mut text = String::from("\\documentclass{article}\n\\usepackage{private-preamble}\n\\begin{document}\n");
    for i in 4..=BODY_LINES {
        text.push_str(&match i {
            20 => "The \\textbf{bold word is here.\n".to_owned(),
            5 => "FAR-AWAY-TEXT that is not near the error.\n".to_owned(),
            _ => format!("Line {i} of the body.\n"),
        });
    }
    text.push_str("\\end{document}\n");
    text
}

fn session(line: u32, reply: &str, seen: &Seen) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Failing(line)));
    seen.answer(reply);
    let seen = seen.clone();
    s.set_ai_connect(Arc::new(move |_: &Target| {
        if let Ok(mut m) = seen.made.lock() {
            *m += 1;
        }
        Ok(Box::new(Fake { seen: seen.clone() }) as Box<dyn Provider>)
    }));
    s.set_author(Some(Author::new("Ada", "ada-key")));
    s.execute("file.write", &json!({"path": "main.tex", "text": document()}))?;
    s.execute("file.new", &json!({"path": "other.tex"}))?;
    s.execute("file.write", &json!({"path": "other.tex", "text": "OTHER-FILE-TEXT"}))?;
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    s.execute("build.compile", &json!({"wait": true}))?;
    Ok(s)
}

fn connect_remote(s: &mut Session) -> Result<Value, String> {
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": "sk-test-secret-123", "model": "small"}))
}

fn text(s: &Session) -> String {
    texcraft_engine::project::ProjectPath::new("main.tex").ok().and_then(|p| s.project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn without_a_connection_every_request_is_refused_plainly() -> R {
    let seen = Seen::default();
    let mut s = session(20, "fine", &seen)?;
    for (id, p) in [("ai.models", json!({})), ("ai.explain", json!({"index": 0})), ("ai.disclose", json!({"index": 0}))] {
        let e = s.execute(id, &p).err().ok_or(format!("{id} ran without a connection"))?;
        assert!(e.contains("connect a model"), "{id}: {e}");
    }
    assert_eq!(s.execute("ai.status", &json!({}))?["connected"], false);
    assert_eq!(seen.made(), 0, "a provider was made without a connection");
    Ok(())
}

#[test]
fn an_explanation_is_refused_until_the_user_agrees_and_nothing_is_sent_before() -> R {
    let seen = Seen::default();
    let mut s = session(20, "It is a brace.", &seen)?;
    connect_remote(&mut s)?;
    let e = s.execute("ai.explain", &json!({"index": 0})).err().ok_or("explained without consent")?;
    assert!(e.contains("Agree") && e.contains("https://api.example.com"), "{e}");
    assert!(seen.prompts().is_empty(), "something was sent before the user agreed");
    let d = s.execute("ai.disclose", &json!({"index": 0}))?;
    assert_eq!(d["needs_consent"], true);
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    assert_eq!(s.execute("ai.disclose", &json!({"index": 0}))?["needs_consent"], false, "agreeing did not stick");
    s.execute("ai.explain", &json!({"index": 0}))?;
    assert_eq!(seen.prompts().len(), 1);
    // A new address is a new destination: asked again.
    s.execute("ai.connect", &json!({"endpoint": "https://other.example.org/v1"}))?;
    assert!(s.execute("ai.explain", &json!({"index": 0})).is_err(), "consent carried over to another endpoint");
    Ok(())
}

#[test]
fn what_an_explanation_sends_is_what_was_disclosed_and_no_more() -> R {
    let seen = Seen::default();
    let mut s = session(20, "The bold group is not closed.", &seen)?;
    connect_remote(&mut s)?;
    let disclosed = s.execute("ai.disclose", &json!({"index": 0}))?;
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    let r = s.execute("ai.explain", &json!({"index": 0}))?;
    assert!(r["explanation"].as_str().unwrap_or_default().contains("not closed"), "{r}");
    let sent = seen.prompts();
    assert_eq!(sent.first().map(String::as_str), disclosed["sends"]["prompt"].as_str(), "the request differs from its disclosure");
    let prompt = sent.first().cloned().unwrap_or_default();
    assert!(prompt.contains("Missing } inserted.") && prompt.contains("A group was not closed."), "the message or its help is missing");
    assert!(prompt.contains("\\textbf{bold word"), "the line itself is missing");
    for far in ["FAR-AWAY-TEXT", "OTHER-FILE-TEXT", "private-preamble"] {
        assert!(!prompt.contains(far), "{far} was sent with an error in the body");
    }
    assert!(!prompt.contains("sk-test-secret-123"), "the token is in the prompt");
    // The request is in the log: its size and where it went, not its text.
    let status = s.execute("ai.status", &json!({}))?;
    let log = status["requests"].to_string();
    assert!(log.contains("https://api.example.com") && !log.contains("bold word"), "{log}");
    Ok(())
}

#[test]
fn an_error_in_the_preamble_sends_the_preamble() -> R {
    let seen = Seen::default();
    let mut s = session(2, "The package does not exist.", &seen)?;
    connect_remote(&mut s)?;
    let d = s.execute("ai.disclose", &json!({"path": "main.tex", "line": 2}))?;
    assert!(d["sends"]["preamble"].as_str().unwrap_or_default().contains("private-preamble"), "{d}");
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    s.execute("ai.explain", &json!({"path": "main.tex", "line": 2}))?;
    assert!(seen.prompts().first().is_some_and(|p| p.contains("PREAMBLE")));
    Ok(())
}

#[test]
fn a_proposed_fix_becomes_a_suggestion_by_the_model_never_an_edit() -> R {
    let seen = Seen::default();
    let mut s = session(20, "", &seen)?;
    connect_remote(&mut s)?;
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    // The model gives back the lines it was sent, with the brace closed.
    let lines = s.execute("ai.disclose", &json!({"index": 0}))?["sends"]["lines"]["text"].as_str().unwrap_or_default().to_owned();
    let fixed = lines.replace("\\textbf{bold word", "\\textbf{bold} word");
    seen.answer(&format!("The group is not closed.\n<<<FIX\n{fixed}\nFIX>>>\n"));
    let before = text(&s);
    let r = s.execute("ai.explain", &json!({"index": 0}))?;
    assert_eq!(text(&s), before, "the fix was applied directly");
    assert!(r["suggestion"].is_string(), "no suggestion was made: {r}");
    assert!(!r["explanation"].as_str().unwrap_or_default().contains("FIX"), "{r}");
    let list = s.execute("review.list", &json!({"kind": "suggestion", "author": "AI (small)"}))?;
    assert_eq!(list["openSuggestions"], 1, "{list}");
    s.execute("review.accept", &json!({"id": r["suggestion"]}))?;
    assert!(text(&s).contains("\\textbf{bold} word"), "accepting did not make the fix");
    assert!(text(&s).contains("FAR-AWAY-TEXT"), "accepting changed more than the fix");
    Ok(())
}

#[test]
fn a_fix_for_lines_changed_since_is_not_offered() -> R {
    let seen = Seen::default();
    let mut s = session(20, "", &seen)?;
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    let lines = s.execute("ai.disclose", &json!({"index": 0}))?["sends"]["lines"]["text"].as_str().unwrap_or_default().to_owned();
    seen.answer(&format!("Close it.\n<<<FIX\n{}\nFIX>>>", lines.replace("bold word", "bold} word")));
    s.execute("ai.explain", &json!({"index": 0, "background": true}))?;
    // Typed while the model answers, on a line it was sent.
    let at = text(&s).find("Line 18").ok_or("no line 18")?;
    let at = text(&s)[..at].chars().count();
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": at, "end": at, "text": "New "}]}))?;
    let mut event = None;
    for _ in 0..500 {
        event = s.poll_ai();
        if event.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(event, Some(texcraft_engine::ai::Event::Explained(Ok(false))), "a suggestion was made on changed lines");
    let list = s.execute("review.list", &json!({"kind": "suggestion"}))?;
    assert_eq!(list["openSuggestions"], 0);
    Ok(())
}

#[test]
fn a_local_endpoint_needs_no_token_and_no_consent() -> R {
    let seen = Seen::default();
    let mut s = session(20, "Close the brace.", &seen)?;
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    let status = s.execute("ai.status", &json!({}))?;
    assert_eq!(status["local"], true);
    assert_eq!(status["token"], false);
    s.execute("ai.explain", &json!({"index": 0}))?;
    assert_eq!(seen.prompts().len(), 1);
    Ok(())
}

/// A keychain stand-in that shows what it holds.
#[derive(Default)]
struct Shown(Mutex<Vec<(String, String)>>);

impl Secrets for Shown {
    fn get(&self, handle: &str) -> Result<Option<SecretString>, String> {
        Ok(self.0.lock().ok().and_then(|v| v.iter().find(|(h, _)| h == handle).map(|(_, s)| SecretString::new(s.clone()))))
    }
    fn put(&self, handle: &str, value: &SecretString) -> Result<(), String> {
        self.remove(handle)?;
        self.0.lock().map_err(|e| e.to_string())?.push((handle.to_owned(), value.expose().to_owned()));
        Ok(())
    }
    fn remove(&self, handle: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.retain(|(h, _)| h != handle);
        Ok(())
    }
    fn lasting(&self) -> bool {
        true
    }
}

impl Shown {
    fn values(&self) -> Vec<String> {
        self.0.lock().map(|v| v.iter().map(|(_, s)| s.clone()).collect()).unwrap_or_default()
    }
}

#[test]
fn the_token_is_in_the_secret_store_never_in_the_saved_settings() -> R {
    let seen = Seen::default();
    let mut s = session(20, "", &seen)?;
    let store = Arc::new(Shown::default());
    s.set_ai_secrets(store.clone());
    connect_remote(&mut s)?;
    assert_eq!(store.values(), vec!["sk-test-secret-123".to_owned()]);
    let saved = serde_json::to_string(s.ai_saved().0)?;
    assert!(!saved.contains("sk-test-secret-123"), "the token is in the saved settings: {saved}");
    assert!(!s.execute("ai.status", &json!({}))?.to_string().contains("sk-test-secret-123"));
    let models = s.execute("ai.models", &json!({}))?;
    assert_eq!(models["models"], json!(["small", "large"]));
    // Another host does not inherit the token.
    s.execute("ai.connect", &json!({"endpoint": "https://elsewhere.example.net/v1"}))?;
    assert_eq!(s.execute("ai.status", &json!({}))?["token"], false, "the token went with the connection to another host");
    assert!(store.values().is_empty(), "the old host's token was kept");
    connect_remote(&mut s)?;
    s.execute("ai.forget", &json!({}))?;
    assert!(store.values().is_empty(), "the token outlived Forget");
    assert_eq!(s.execute("ai.status", &json!({}))?["connected"], false);
    Ok(())
}

fn folder(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-ai-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("main.tex"), document())?;
    Ok(dir)
}

#[test]
fn a_project_can_turn_ai_off_for_itself() -> R {
    let dir = folder("optout")?;
    let seen = Seen::default();
    let mut s = session(20, "fine", &seen)?;
    s.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    s.execute("build.compile", &json!({"wait": true}))?;
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    s.execute("ai.project", &json!({"enabled": false}))?;
    let e = s.execute("ai.explain", &json!({"index": 0})).err().ok_or("explained in a project that turned AI off")?;
    assert!(e.contains("this project"), "{e}");
    // Kept with the project: another session refuses too.
    let mut again = session(20, "fine", &seen)?;
    again.execute("project.open", &json!({"path": dir.display().to_string()}))?;
    again.execute("build.compile", &json!({"wait": true}))?;
    again.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    assert!(again.execute("ai.explain", &json!({"index": 0})).is_err(), "the project's setting was not kept");
    assert_eq!(seen.prompts().len(), 0);
    again.execute("ai.project", &json!({"enabled": true}))?;
    again.execute("ai.explain", &json!({"index": 0}))?;
    assert_eq!(seen.prompts().len(), 1);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_cancelled_explanation_drops_its_answer() -> R {
    let seen = Seen::default();
    let mut s = session(20, "late answer", &seen)?;
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    s.execute("ai.explain", &json!({"index": 0, "background": true}))?;
    s.execute("ai.cancel", &json!({}))?;
    for _ in 0..50 {
        assert_eq!(s.poll_ai(), None, "an answer arrived after cancelling");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let d = s.last_outcome().and_then(|o| o.diagnostics.first().cloned()).ok_or("no message")?;
    assert_eq!(s.ai_explanation(&d), Some(&Explanation::Cancelled));
    Ok(())
}
