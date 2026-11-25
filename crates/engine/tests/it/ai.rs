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

/// Issue #15: an agreement is to a kind of request, kept with the settings (never any text) so a
/// new session restored from them does not ask again; withdrawing it asks again.
#[test]
fn an_agreement_outlives_the_session_until_it_is_withdrawn() -> R {
    let seen = Seen::default();
    // The keychain outlives the session.
    let keychain: Arc<dyn Secrets> = Arc::new(texcraft_engine::ai::SessionSecrets::default());
    let mut s = session(20, "It is a brace.", &seen)?;
    s.set_ai_secrets(keychain.clone());
    connect_remote(&mut s)?;
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    s.execute("ai.explain", &json!({"index": 0}))?;
    let kept = serde_json::to_string(s.ai_saved().0)?;
    assert!(!kept.contains("Missing } inserted") && !kept.contains("bold word"), "the settings hold what was sent: {kept}");
    let mut again = session(20, "It is a brace.", &seen)?;
    again.set_ai_secrets(keychain);
    again.restore_ai(serde_json::from_str(&kept)?);
    assert_eq!(again.execute("ai.status", &json!({}))?["agreed"], json!(["explain"]));
    // Another message, another session: the same kind is not asked about again.
    again.execute("ai.explain", &json!({"index": 0}))?;
    assert_eq!(seen.prompts().len(), 2);
    assert_eq!(
        again.execute("ai.disclose", &json!({"request": "make", "kind": "table", "description": "x", "path": "main.tex", "at": 0}))?["needs_consent"],
        true,
        "agreeing to one kind covered another"
    );
    again.execute("ai.revoke", &json!({"request": "explain"}))?;
    assert!(again.execute("ai.explain", &json!({"index": 0})).is_err(), "a withdrawn agreement still holds");
    assert_eq!(again.execute("ai.status", &json!({}))?["agreed"], json!([]));
    Ok(())
}

/// What a request sent can be shown again afterwards, from this session's memory: the request log
/// and the settings hold only its size.
#[test]
fn what_a_request_sent_can_be_shown_afterwards_but_is_not_logged() -> R {
    let seen = Seen::default();
    let mut s = session(20, "It is a brace.", &seen)?;
    connect_remote(&mut s)?;
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    s.execute("ai.explain", &json!({"index": 0}))?;
    let status = s.execute("ai.status", &json!({}))?;
    let run = status["requests"][0]["run"].as_str().ok_or("no run in the log")?.to_owned();
    assert!(!status["requests"].to_string().contains("Missing } inserted"), "the log holds the text sent");
    let shown = s.execute("ai.sent", &json!({"run": run}))?;
    assert_eq!(shown["text"].as_str(), seen.prompts().first().map(String::as_str), "not what was sent");
    assert!(s.execute("ai.sent", &json!({"run": "texcraft-0-0"})).is_err());
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

#[test]
fn an_agent_session_cannot_agree_to_send_text_for_the_user() -> R {
    let seen = Seen::default();
    let mut s = session(3, "", &seen)?;
    connect_remote(&mut s)?;
    let dir = std::env::temp_dir().join(format!("texcraft-ai-roots-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let roots = texcraft_engine::file_access::AutomationRoots::new(std::slice::from_ref(&dir), std::slice::from_ref(&dir))?;
    s.confine(roots);
    assert!(s.execute("ai.consent", &json!({"request": "explain"})).is_err(), "an agent agreed for the user");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

// ----- AI3: rewriting and continuing -----

/// A document whose selection ([`SELECTED`]) is far from its first paragraph and its preamble.
fn paper_text() -> String {
    let filler = "A line of filler text that keeps the intro far away.\n".repeat(8);
    format!(
        "\\documentclass{{article}}\n\\usepackage{{secret-preamble}}\n\\begin{{document}}\nFAR-AWAY-INTRO is not near the selection.\n\n{filler}\nLine one.\nLine two.\n{SELECTED}\nLine four.\n\n\\end{{document}}\n"
    )
}
const SELECTED: &str = "As Section~\\ref{sec:a} shows, the value $x^2$ is very very large indeed.";

fn paper(seen: &Seen, reply: &str) -> Result<(Session, usize, usize), String> {
    let mut s = session(20, reply, seen)?;
    let paper = paper_text();
    s.execute("file.write", &json!({"path": "main.tex", "text": paper}))?;
    let at = paper.find(SELECTED).ok_or("no selection")?;
    let start = paper[..at].chars().count();
    Ok((s, start, start + SELECTED.chars().count()))
}

fn suggestions(s: &mut Session) -> Result<Value, String> {
    s.execute("review.list", &json!({"kind": "suggestion"}))
}

#[test]
fn a_rewrite_sends_what_was_disclosed_and_becomes_a_suggestion_on_the_selection_not_an_edit() -> R {
    let seen = Seen::default();
    let (mut s, start, end) = paper(&seen, "<<<TEXT\nSection~\\ref{sec:a} shows that $x^2$ is large.\nTEXT>>>")?;
    connect_remote(&mut s)?;
    let p = json!({"path": "main.tex", "start": start, "end": end, "how": "shorter"});
    let e = s.execute("ai.rewrite", &p).err().ok_or("rewrote without consent")?;
    assert!(e.contains("Agree") && e.contains("https://api.example.com"), "{e}");
    assert!(seen.prompts().is_empty(), "something was sent before the user agreed");
    // Agreeing to explanations does not cover rewrites.
    s.execute("ai.consent", &json!({"request": "explain"}))?;
    assert!(s.execute("ai.rewrite", &p).is_err(), "consent to explain covered rewriting");
    let mut d = p.clone();
    d["request"] = json!("rewrite");
    let disclosed = s.execute("ai.disclose", &d)?;
    assert_eq!(disclosed["needs_consent"], true);
    s.execute("ai.consent", &json!({"request": "rewrite"}))?;
    let r = s.execute("ai.rewrite", &p)?;
    let sent = seen.prompts();
    assert_eq!(sent.first().map(String::as_str), disclosed["sends"]["prompt"].as_str(), "the request differs from its disclosure");
    let prompt = sent.first().cloned().unwrap_or_default();
    assert!(prompt.contains(SELECTED) && prompt.contains("Line two.") && prompt.contains("Line four."), "the selection or its context is missing");
    for far in ["FAR-AWAY-INTRO", "secret-preamble", "OTHER-FILE-TEXT"] {
        assert!(!prompt.contains(far), "{far} was sent with a rewrite");
    }
    assert_eq!(text(&s), paper_text(), "the rewrite was applied directly");
    let list = s.execute("review.list", &json!({"kind": "suggestion", "author": "AI (small)"}))?;
    assert_eq!(list["openSuggestions"], 1, "{list}");
    s.execute("review.accept", &json!({"id": r["suggestion"]}))?;
    assert_eq!(
        text(&s),
        paper_text().replace(SELECTED, "Section~\\ref{sec:a} shows that $x^2$ is large."),
        "accepting changed more or less than the selection"
    );
    Ok(())
}

#[test]
fn a_rewrite_that_drops_a_reference_is_refused_saying_so() -> R {
    let seen = Seen::default();
    let (mut s, start, end) = paper(&seen, "<<<TEXT\nThe value $x^2$ is large.\nTEXT>>>")?;
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    let e = s
        .execute("ai.rewrite", &json!({"path": "main.tex", "start": start, "end": end, "how": "shorter"}))
        .err()
        .ok_or("a rewrite without its \\ref was offered")?;
    assert!(e.contains("\\ref{sec:a}"), "the refusal does not say what was dropped: {e}");
    assert_eq!(suggestions(&mut s)?["openSuggestions"], 0);
    assert_eq!(text(&s), paper_text());
    Ok(())
}

#[test]
fn a_continuation_needs_consent_and_typing_on_drops_it() -> R {
    let seen = Seen::default();
    let (mut s, _, end) = paper(&seen, " It grows fast.")?;
    connect_remote(&mut s)?;
    let p = json!({"path": "main.tex", "at": end});
    let e = s.execute("ai.complete", &p).err().ok_or("continued without consent")?;
    assert!(e.contains("Agree"), "{e}");
    assert!(seen.prompts().is_empty(), "the paragraph was sent before the user agreed");
    s.execute("ai.consent", &json!({"request": "complete"}))?;
    let r = s.execute("ai.complete", &p)?;
    assert_eq!(r["text"], " It grows fast.");
    let prompt = seen.prompts().first().cloned().unwrap_or_default();
    assert!(prompt.contains(SELECTED) && prompt.contains("Line one."), "the paragraph is missing");
    assert!(
        !prompt.contains("Line four.") && !prompt.contains("filler") && !prompt.contains("FAR-AWAY-INTRO") && !prompt.contains("secret-preamble"),
        "more than the paragraph up to the caret was sent"
    );
    let main = texcraft_engine::project::ProjectPath::new("main.tex")?;
    assert_eq!(s.ai_ghost(&main), Some((end, " It grows fast.")));
    // Asked again at once: refused, for the rate limit.
    assert!(s.execute("ai.complete", &p).is_err(), "continuations are not rate-limited");

    // Typing on while the model answers: the answer is not shown.
    std::thread::sleep(std::time::Duration::from_millis(texcraft_engine::ai::COMPLETE_EVERY_MS + 50));
    s.execute("ai.complete", &json!({"path": "main.tex", "at": end, "background": true}))?;
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": end, "end": end, "text": " Yes"}]}))?;
    let mut event = None;
    for _ in 0..500 {
        event = s.poll_ai();
        if event.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(event, Some(texcraft_engine::ai::Event::Completed(Ok(false))), "a continuation of text typed over was shown");
    assert_eq!(s.ai_ghost(&main), None);
    Ok(())
}

#[test]
fn cancelling_a_continuation_leaves_an_explanation_in_flight_alone() -> R {
    let seen = Seen::default();
    let (mut s, _, end) = paper(&seen, "fine")?;
    s.execute("build.compile", &json!({"wait": true}))?;
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    s.execute("ai.explain", &json!({"index": 0, "background": true}))?;
    assert!(s.execute("ai.complete", &json!({"path": "main.tex", "at": end})).is_err(), "a continuation was asked for over an explanation");
    assert_eq!(s.execute("ai.cancel", &json!({"only": "completion"}))?["cancelled"], false);
    assert!(s.ai_busy(), "cancelling continuations cancelled the explanation");
    Ok(())
}

// ----- AI4: making LaTeX and drafting references -----

/// A typesetter that records what it was given and refuses `\bad`.
fn typesetter(s: &mut Session) -> Arc<Mutex<Vec<String>>> {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let kept = seen.clone();
    s.set_ai_typeset(Arc::new(move |scratch: &texcraft_engine::ai::Scratch| {
        if let Ok(mut k) = kept.lock() {
            k.push(format!("{}\n---\n{}", scratch.preamble, scratch.body));
        }
        if scratch.body.contains("\\bad") { Err(vec!["line 2: undefined command \\bad".to_owned()]) } else { Ok(b"%PDF-fake".to_vec()) }
    }));
    seen
}

/// The caret on the line "Line 10 of the body.", at its start.
fn caret(s: &Session) -> usize {
    let t = text(s);
    t[..t.find("Line 10 of").unwrap_or_default()].chars().count()
}

#[test]
fn making_sends_what_was_disclosed_and_a_typeset_answer_becomes_suggestions_never_an_edit() -> R {
    let seen = Seen::default();
    let mut s = session(
        20,
        "<<<LATEX\n\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}\nLATEX>>>\n<<<PACKAGES\n\\usepackage{tikz}\nPACKAGES>>>",
        &seen,
    )?;
    let typeset = typesetter(&mut s);
    connect_remote(&mut s)?;
    let at = caret(&s);
    let p = json!({"kind": "tikz", "description": "a line from the origin", "path": "main.tex", "at": at});
    let e = s.execute("ai.make", &p).err().ok_or("made without consent")?;
    assert!(e.contains("Agree") && e.contains("https://api.example.com"), "{e}");
    assert!(seen.prompts().is_empty(), "something was sent before the user agreed");
    let mut d = p.clone();
    d["request"] = json!("make");
    let disclosed = s.execute("ai.disclose", &d)?;
    assert_eq!(disclosed["needs_consent"], true);
    s.execute("ai.consent", &json!({"request": "make"}))?;
    let before = text(&s);
    let r = s.execute("ai.make", &p)?;
    let prompt = seen.prompts().first().cloned().unwrap_or_default();
    assert_eq!(Some(prompt.as_str()), disclosed["sends"]["prompt"].as_str(), "the request differs from its disclosure");
    assert!(prompt.contains("a line from the origin") && prompt.contains("private-preamble"), "the description or the preamble is missing");
    assert!(!prompt.contains("FAR-AWAY-TEXT") && !prompt.contains("OTHER-FILE-TEXT"), "the document's body was sent");
    assert!(
        typeset
            .lock()
            .map(|t| t.first().is_some_and(|x| x.contains("private-preamble") && x.contains("\\usepackage{tikz}") && x.contains("\\draw")))
            .unwrap_or(false),
        "it was not typeset after the preamble with its package"
    );
    assert_eq!(r["typeset"], true, "{r}");
    assert_eq!(text(&s), before, "making edited the file");
    assert_eq!(r["suggestions"].as_array().map(Vec::len), Some(2), "the picture and its package are not two suggestions: {r}");
    for id in r["suggestions"].as_array().cloned().unwrap_or_default() {
        s.execute("review.accept", &json!({"id": id}))?;
    }
    let after = text(&s);
    let (pkg, doc, pic, line10) =
        (after.find("\\usepackage{tikz}"), after.find("\\begin{document}"), after.find("\\begin{tikzpicture}"), after.find("Line 10 of"));
    assert!(pkg < doc && doc < pic && pic < line10 && pkg.is_some(), "not in their places:\n{after}");
    Ok(())
}

/// What the person saw and chose to insert goes in as one edit of the file: the packages before
/// `\begin{document}` and the LaTeX at the caret, its place given back with the packages counted.
#[test]
fn inserting_as_an_edit_puts_the_latex_at_the_caret_and_its_packages_in_the_preamble_at_once() -> R {
    let seen = Seen::default();
    let mut s = session(
        20,
        "<<<LATEX\n\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}\nLATEX>>>\n<<<PACKAGES\n\\usepackage{tikz}\nPACKAGES>>>",
        &seen,
    )?;
    typesetter(&mut s);
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    let p = json!({"kind": "tikz", "description": "a line", "path": "main.tex", "at": caret(&s), "insert": false});
    s.execute("ai.make", &p)?;
    let main = texcraft_engine::project::ProjectPath::new("main.tex")?;
    let before = s.revision();
    let r = s.execute("ai.insert_made", &json!({"edit": true}))?;
    assert_eq!(r["edited"], true, "{r}");
    assert_eq!(suggestions(&mut s)?["openSuggestions"], 0, "an edit made suggestions too");
    let after = text(&s);
    let (pkg, doc, pic, line10) =
        (after.find("\\usepackage{tikz}"), after.find("\\begin{document}"), after.find("\\begin{tikzpicture}"), after.find("Line 10 of"));
    assert!(pkg.is_some() && pkg < doc && doc < pic && pic < line10, "not in their places:\n{after}");
    let start = r["start"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or_default();
    let shown: String = after.chars().skip(start).take("\\begin{tikzpicture}".len()).collect();
    assert_eq!(shown, "\\begin{tikzpicture}", "`start` is not where the LaTeX went");
    assert_eq!(s.edits_since(&main, before).map(|e| e.len()), Some(1), "the packages and the LaTeX are not one edit");
    assert!(s.execute("ai.insert_made", &json!({"edit": true})).is_err(), "inserted twice");
    Ok(())
}

#[test]
fn an_answer_that_does_not_typeset_is_reported_and_asking_again_sends_the_error() -> R {
    let seen = Seen::default();
    let mut s = session(20, "<<<LATEX\n\\begin{equation}\n\\bad x\n\\end{equation}\nLATEX>>>", &seen)?;
    typesetter(&mut s);
    s.execute("ai.connect", &json!({"endpoint": "http://127.0.0.1:11434/v1", "model": "small"}))?;
    let p = json!({"kind": "equation", "description": "the bad one", "path": "main.tex", "at": caret(&s)});
    let r = s.execute("ai.make", &p)?;
    assert_eq!(r["typeset"], false);
    assert!(r["messages"].to_string().contains("undefined command"), "retex's message is not reported: {r}");
    assert_eq!(suggestions(&mut s)?["openSuggestions"], 0, "what does not typeset was inserted");
    assert!(s.execute("ai.insert_made", &json!({})).is_err(), "inserted without `anyway`");
    let mut again = p.clone();
    again["again"] = json!(true);
    seen.answer("<<<LATEX\n\\begin{equation}\nx\n\\end{equation}\nLATEX>>>");
    let r = s.execute("ai.make", &again)?;
    assert_eq!(r["typeset"], true);
    let second = seen.prompts().get(1).cloned().unwrap_or_default();
    assert!(second.contains("undefined command \\bad") && second.contains("\\bad x"), "asking again did not send the answer and its error");
    assert_eq!(suggestions(&mut s)?["openSuggestions"], 1);
    Ok(())
}

#[test]
fn a_reference_is_a_suggestion_in_the_bib_marked_as_drafted_by_the_model() -> R {
    let seen = Seen::default();
    let mut s = session(20, "<<<BIB\n@book{knuth84,\n  title = {The {TeX}book},\n  year = {1984}\n}\nBIB>>>", &seen)?;
    s.execute("file.new", &json!({"path": "refs.bib", "text": "@article{knuth84,\n  title = {Other}\n}\n"}))?;
    connect_remote(&mut s)?;
    let p = json!({"query": "The TeXbook", "bib": "refs.bib"});
    assert!(s.execute("ai.reference", &p).is_err(), "drafted without consent");
    let mut d = p.clone();
    d["request"] = json!("reference");
    let disclosed = s.execute("ai.disclose", &d)?;
    s.execute("ai.consent", &json!({"request": "reference"}))?;
    let r = s.execute("ai.reference", &p)?;
    let prompt = seen.prompts().first().cloned().unwrap_or_default();
    assert_eq!(Some(prompt.as_str()), disclosed["sends"]["prompt"].as_str());
    assert!(!prompt.contains("private-preamble") && !prompt.contains("Other"), "project text was sent with a reference");
    assert!(r["note"].as_str().is_some_and(|n| n.contains("invent")), "the answer does not say models invent references: {r}");
    let bib = texcraft_engine::project::ProjectPath::new("refs.bib")?;
    assert!(!s.project().text(&bib)?.contains("TeXbook"), "the entry was written directly");
    s.execute("review.accept", &json!({"id": r["suggestion"]}))?;
    let after = s.project().text(&bib)?.to_owned();
    assert!(after.contains("% Drafted by AI (small): check it\n@book{knuth84a,"), "no comment line or the key clashes:\n{after}");
    // A .bib that does not exist is made for it.
    let r = s.execute("ai.reference", &json!({"query": "10.1000/xyz", "bib": "new.bib"}))?;
    assert_eq!(r["created"], true);
    Ok(())
}

// ----- AI5: asking and reviewing -----

#[test]
fn a_question_sends_the_open_file_or_the_project_and_its_answer_links_the_lines_it_names() -> R {
    let seen = Seen::default();
    let mut s = session(20, "The bold group opens at main.tex:20 and other.tex:1; nowhere.tex:3 is not a file.", &seen)?;
    connect_remote(&mut s)?;
    let p = json!({"question": "Where is the bold word?", "scope": "file", "path": "main.tex"});
    assert!(s.execute("ai.ask", &p).is_err(), "asked without consent");
    let mut d = p.clone();
    d["request"] = json!("ask");
    let disclosed = s.execute("ai.disclose", &d)?;
    s.execute("ai.consent", &json!({"request": "ask"}))?;
    let r = s.execute("ai.ask", &p)?;
    let prompt = seen.prompts().first().cloned().unwrap_or_default();
    assert_eq!(Some(prompt.as_str()), disclosed["sends"]["prompt"].as_str());
    assert!(prompt.contains("20| The \\textbf{bold word") && !prompt.contains("OTHER-FILE-TEXT"), "not the open file alone, numbered");
    let links: Vec<(String, u64)> = r["links"]
        .as_array()
        .map(|l| l.iter().map(|l| (l["path"].as_str().unwrap_or_default().to_owned(), l["line"].as_u64().unwrap_or_default())).collect())
        .unwrap_or_default();
    assert_eq!(links, vec![("main.tex".to_owned(), 20), ("other.tex".to_owned(), 1)]);
    s.execute("ai.ask", &json!({"question": "And elsewhere?", "scope": "project"}))?;
    assert!(seen.prompts().get(1).is_some_and(|p| p.contains("OTHER-FILE-TEXT")), "the project's other file was not sent");
    assert_eq!(s.ai_answers().len(), 2);
    Ok(())
}

#[test]
fn a_review_leaves_comments_on_the_quotes_it_finds_and_counts_those_it_does_not() -> R {
    let seen = Seen::default();
    let reply = "QUOTE: \"bold word is\nhere\"\nREMARK: The group is not closed.\n\nQUOTE: Line 18 of the body.\nREMARK: Say more.\n\nQUOTE: words that are not there\nREMARK: Lost.";
    let mut s = session(20, reply, &seen)?;
    connect_remote(&mut s)?;
    let t = text(&s);
    let start = t[..t.find("Line 15").unwrap_or_default()].chars().count();
    let end = t[..t.find("Line 25").unwrap_or_default()].chars().count();
    let p = json!({"path": "main.tex", "start": start, "end": end});
    assert!(s.execute("ai.review", &p).is_err(), "reviewed without consent");
    s.execute("ai.consent", &json!({"request": "review"}))?;
    let r = s.execute("ai.review", &p)?;
    let prompt = seen.prompts().first().cloned().unwrap_or_default();
    assert!(prompt.contains("Line 16") && !prompt.contains("FAR-AWAY-TEXT") && !prompt.contains("Line 26"), "more or less than the section was sent");
    assert_eq!(r["comments"].as_array().map(Vec::len), Some(2), "{r}");
    assert_eq!(r["dropped"], 1);
    assert_eq!(text(&s), t, "the review edited the file");
    let list = s.execute("review.list", &json!({"author": "AI (small)"}))?;
    let quoted: Vec<String> = list["threads"]
        .as_array()
        .map(|th| th.iter().filter_map(|t| t["anchor"]["quote"]["exact"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    assert!(quoted.iter().any(|q| q == "bold word is here") && quoted.iter().any(|q| q == "Line 18 of the body."), "{list}");
    Ok(())
}

#[test]
fn an_agent_session_cannot_agree_to_any_new_kind_of_request() -> R {
    let seen = Seen::default();
    let mut s = session(3, "", &seen)?;
    connect_remote(&mut s)?;
    let dir = std::env::temp_dir().join(format!("texcraft-ai-roots45-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let roots = texcraft_engine::file_access::AutomationRoots::new(std::slice::from_ref(&dir), std::slice::from_ref(&dir))?;
    s.confine(roots);
    for kind in ["make", "reference", "ask", "review"] {
        assert!(s.execute("ai.consent", &json!({"request": kind})).is_err(), "an agent agreed to {kind} for the user");
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
