//! Signing in to GitLab and GitHub (C5, step 4) through the engine's commands: the desktop's
//! loopback redirect, the browser's round trip, GitHub's device flow, a sign-in kept across
//! restarts and renewed when it expired, signing out, and tokens kept out of every result. The
//! forges are a scripted transport and the fake GitLab; no test reaches the network.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::ai::{SecretString, Secrets};
use texcraft_engine::forge::fake::{FakeGitLab, Script};
use texcraft_engine::forge::oauth::{self, Clients};
use texcraft_engine::forge::{Request, Sending, Service, Transport};

pub(crate) const ACCESS: &str = "gl-oauth-access-1";
pub(crate) const REFRESH: &str = "gl-oauth-refresh-1";

/// A keychain in memory, shared between sessions (a restart).
#[derive(Clone, Default)]
pub(crate) struct Keychain(pub(crate) Arc<Mutex<BTreeMap<String, String>>>);

impl Keychain {
    pub(crate) fn dump(&self) -> String {
        format!("{:?}", self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Secrets for Keychain {
    fn get(&self, handle: &str) -> Result<Option<SecretString>, String> {
        Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(handle).map(|s| SecretString::new(s.clone())))
    }
    fn put(&self, handle: &str, value: &SecretString) -> Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(handle.to_owned(), value.expose().to_owned());
        Ok(())
    }
    fn remove(&self, handle: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(handle);
        Ok(())
    }
    fn lasting(&self) -> bool {
        true
    }
}

/// OAuth's endpoints and `/user` go to the script, the project's API to the fake GitLab.
struct Router {
    script: Script,
    fake: FakeGitLab,
}

impl Transport for Router {
    fn send(&self, request: Request) -> Sending<'_> {
        let auth = request.url.contains("/oauth/") || request.url.contains("/login/") || request.url.ends_with("/user");
        if auth { self.script.send(request) } else { Box::pin(async move { self.fake.transport().send(request).await }) }
    }
}

pub(crate) fn fake() -> FakeGitLab {
    FakeGitLab::new("https://gitlab.com", "group/paper", ACCESS, &[("main.tex", b"\\documentclass{article}\n")])
}

pub(crate) fn session(script: &Script, fake: &FakeGitLab, keychain: &Keychain) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let (script, fake) = (script.clone(), fake.clone());
    s.set_forge_transport(move || Box::new(Router { script: script.clone(), fake: fake.clone() }));
    s.set_forge_clients(Clients { gitlab: "gl-client".into(), github: "gh-client".into() });
    s.set_forge_loopback("127.0.0.1:0");
    s.set_forge_poll_second(Duration::from_millis(1));
    s.set_forge_secrets(Arc::new(keychain.clone()));
    s
}

/// Polls the session until a forge operation ends.
pub(crate) fn outcome(s: &mut Session) -> Result<Value, String> {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if let Some(done) = s.poll_forge() {
            return done;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("the operation did not end".to_owned())
}

fn query_of(url: &str) -> Vec<(String, String)> {
    oauth::parse_query(url.split_once('?').map(|(_, q)| q).unwrap_or_default())
}

fn get<'a>(q: &'a [(String, String)], k: &str) -> &'a str {
    q.iter().find(|(n, _)| n == k).map_or("", |(_, v)| v.as_str())
}

/// The browser's visit to the redirect URI the authorize page names.
fn come_back(authorize: &str, code: &str) -> String {
    let q = query_of(authorize);
    let (redirect, state) = (get(&q, "redirect_uri"), get(&q, "state"));
    let addr = redirect.trim_start_matches("http://").trim_end_matches("/callback").to_owned();
    let mut answer = String::new();
    if let Ok(mut stream) = std::net::TcpStream::connect(addr) {
        let _ = write!(stream, "GET /callback?code={code}&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let _ = stream.read_to_string(&mut answer);
    }
    answer
}

fn tokens(scope: &str) -> Value {
    json!({"access_token": ACCESS, "refresh_token": REFRESH, "expires_in": 7200, "scope": scope, "token_type": "Bearer"})
}

/// Signed in to GitLab on the desktop with `scope`; everything the commands returned.
pub(crate) fn sign_in_gitlab(s: &mut Session, script: &Script, scope: &str) -> Result<String, String> {
    script.answer(200, tokens(scope));
    script.answer(200, json!({"name": "Ada Lovelace", "username": "ada"}));
    let started = s.execute("forge.sign_in", &json!({"service": "gitlab"}))?;
    let url = started["open"].as_str().ok_or("no page to open")?.to_owned();
    assert!(url.starts_with("https://gitlab.com/oauth/authorize?"), "{url}");
    assert_eq!(get(&query_of(&url), "scope"), "read_api read_user");
    assert!(matches!(s.forge_signing_in(), Some((Service::GitLab, texcraft_engine::SignInProgress::Browser { .. }))));
    let page = come_back(&url, "code-1");
    assert!(page.contains("You can close this tab"), "{page}");
    let done = outcome(s)?;
    assert_eq!((done["operation"].as_str(), done["username"].as_str()), (Some("sign_in"), Some("ada")));
    Ok(format!("{started} {done}"))
}

#[test]
fn signing_in_on_the_desktop_opens_and_reads_with_no_token_and_lasts_a_restart() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    let mut seen = sign_in_gitlab(&mut s, &script, "read_api read_user")?;
    let form = script.form(0);
    assert_eq!(get(&form, "grant_type"), "authorization_code");
    assert_eq!(get(&form, "code"), "code-1");

    let opened = s.execute("forge.open", &json!({"url": "https://gitlab.com/group/paper"}))?;
    assert_eq!(opened["files"], 1, "opened with the sign-in's token");
    seen += &opened.to_string();
    seen += &s.execute("forge.status", &json!({}))?.to_string();
    seen += &s.execute("forge.accounts", &json!({}))?.to_string();

    // Read only: the commit asks to sign in again, allowing commits.
    s.execute("file.write", &json!({"path": "main.tex", "text": "x\n"}))?;
    assert!(s.forge_needs_write());
    let refused = s.execute("forge.commit", &json!({"message": "x"})).err().unwrap_or_default();
    assert!(refused.contains("write"), "{refused}");
    assert_eq!(s.forge_problem(), Some(&texcraft_engine::ForgeProblem::NeedsWrite));

    // A restart: the keychain still has it.
    let mut again = session(&script, &fake, &keychain);
    assert_eq!(again.forge_signed_in(Service::GitLab).and_then(|g| g.user.clone()).map(|u| u.username), Some("ada".to_owned()));
    seen += &again.execute("forge.accounts", &json!({}))?.to_string();
    seen += &format!("{:?}", again.forge_signed_in(Service::GitLab));

    for secret in [ACCESS, REFRESH] {
        assert!(!seen.contains(secret), "{secret} showed: {seen}");
        assert!(keychain.dump().contains(secret), "the keychain keeps it");
    }
    Ok(())
}

#[test]
fn committing_asks_for_the_api_scope_and_then_commits() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    sign_in_gitlab(&mut s, &script, "read_api read_user")?;
    s.execute("forge.open", &json!({"url": "https://gitlab.com/group/paper"}))?;
    s.execute("file.write", &json!({"path": "main.tex", "text": "x\n"}))?;

    script.answer(200, tokens("api read_user"));
    script.answer(200, json!({"name": "Ada Lovelace", "username": "ada"}));
    let started = s.execute("forge.sign_in", &json!({"service": "gitlab", "write": true}))?;
    let url = started["open"].as_str().unwrap_or_default().to_owned();
    assert_eq!(get(&query_of(&url), "scope"), "api read_user");
    come_back(&url, "code-2");
    assert_eq!(outcome(&mut s)?["can_commit"], true);
    assert!(!s.forge_needs_write());
    let done = s.execute("forge.commit", &json!({"message": "x"}))?;
    assert!(done["commit"].is_string(), "{done}");
    assert_eq!(fake.files("main")["main.tex"], b"x\n");
    Ok(())
}

#[test]
fn an_expired_sign_in_is_renewed_before_the_operation_and_kept() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let old = json!({"service": "gitlab", "instance": "https://gitlab.com", "access": "gl-expired", "refresh": "gl-old-refresh",
        "expires_at": 1, "scopes": ["read_api"], "user": {"name": "Ada", "username": "ada"}, "redirect": "http://127.0.0.1:47321/callback"});
    keychain.put("texcraft.forge.gitlab.com", &SecretString::new(old.to_string()))?;
    let mut s = session(&script, &fake, &keychain);
    script.answer(200, tokens("read_api"));
    s.execute("forge.open", &json!({"url": "https://gitlab.com/group/paper"}))?;
    let form = script.form(0);
    assert_eq!((get(&form, "grant_type"), get(&form, "refresh_token")), ("refresh_token", "gl-old-refresh"));
    let kept = keychain.dump();
    assert!(kept.contains(REFRESH) && !kept.contains("gl-old-refresh"), "the new refresh token replaces the old: {kept}");
    Ok(())
}

#[test]
fn in_the_browser_the_page_goes_to_gitlab_and_its_return_is_checked() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    s.set_forge_redirect("https://texcraft.example/");
    assert!(s.forge_sign_in_unavailable(Service::GitHub).is_some(), "GitHub needs a token in the browser");

    let started = s.execute("forge.sign_in", &json!({}))?;
    let url = started["navigate"].as_str().ok_or("no page to go to")?.to_owned();
    let q = query_of(&url);
    assert_eq!(get(&q, "redirect_uri"), "https://texcraft.example/");
    let wrong = s.execute("forge.sign_in_return", &json!({"code": "c", "state": "forged"}));
    assert!(wrong.is_err(), "another state is refused");
    assert!(script.requests().is_empty(), "and nothing is sent");

    let started = s.execute("forge.sign_in", &json!({}))?;
    let q = query_of(started["navigate"].as_str().unwrap_or_default());
    script.answer(200, tokens("read_api read_user"));
    script.answer(200, json!({"name": "Ada", "username": "ada"}));
    s.execute("forge.sign_in_return", &json!({"code": "c", "state": get(&q, "state")}))?;
    assert_eq!(outcome(&mut s)?["username"], "ada");
    assert_eq!(get(&script.form(0), "code_verifier").len(), 43, "the verifier kept across the round trip");
    assert!(s.execute("forge.sign_in_return", &json!({"code": "c", "state": get(&q, "state")})).is_err(), "a code is used once");
    Ok(())
}

#[test]
fn github_s_device_flow_shows_a_code_and_signs_in_when_it_is_entered() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    script.answer(200, json!({"device_code": "dev-secret", "user_code": "WDJB-MJHT", "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 5}));
    script.answer(200, json!({"error": "authorization_pending"}));
    script.answer(200, json!({"access_token": "gho_secret", "token_type": "bearer", "scope": "repo"}));
    script.answer(200, json!({"name": "Octo Cat", "login": "octo"}));
    let started = s.execute("forge.sign_in", &json!({"service": "github"}))?;
    let done = outcome(&mut s)?;
    assert_eq!((done["service"].as_str(), done["username"].as_str(), done["can_commit"].as_bool()), (Some("github"), Some("octo"), Some(true)));
    let accounts = s.execute("forge.accounts", &json!({}))?;
    assert_eq!(accounts["accounts"].as_array().map(Vec::len), Some(1));
    let seen = format!("{started} {done} {accounts}");
    assert!(!seen.contains("gho_secret") && !seen.contains("dev-secret"), "{seen}");

    // Signing out of GitHub forgets the token and says where to revoke it.
    let out = s.execute("forge.sign_out", &json!({"service": "github"}))?;
    assert_eq!(out["revoked"], false);
    assert!(out["revoke"].as_str().is_some_and(|w| w.contains("GitHub")));
    assert!(s.forge_signed_in(Service::GitHub).is_none() && !keychain.dump().contains("gho_secret"));
    Ok(())
}

#[test]
fn the_device_code_is_shown_while_github_waits_and_cancel_stops_it() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    s.set_forge_poll_second(Duration::from_secs(1));
    script.answer(200, json!({"device_code": "d", "user_code": "ABCD-1234", "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 5}));
    s.execute("forge.sign_in", &json!({"service": "github"}))?;
    let end = Instant::now() + Duration::from_secs(5);
    let mut code = None;
    while code.is_none() && Instant::now() < end {
        if let Some((_, texcraft_engine::SignInProgress::Code { user_code, .. })) = s.forge_signing_in() {
            code = Some(user_code);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(code.as_deref(), Some("ABCD-1234"));
    assert_eq!(s.execute("forge.accounts", &json!({}))?["signing_in"]["user_code"], "ABCD-1234");
    s.execute("forge.cancel_sign_in", &json!({}))?;
    assert!(s.forge_signing_in().is_none());
    Ok(())
}

#[test]
fn signing_out_of_gitlab_revokes_and_forgets() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    sign_in_gitlab(&mut s, &script, "read_api read_user")?;
    script.answer(200, json!({}));
    s.execute("forge.sign_out", &json!({"service": "gitlab"}))?;
    assert!(s.forge_signed_in(Service::GitLab).is_none());
    assert!(!keychain.dump().contains(ACCESS), "gone from the keychain");
    assert_eq!(outcome(&mut s)?["revoked"], true);
    let revoke = script.requests().into_iter().last().map(|r| r.url).unwrap_or_default();
    assert_eq!(revoke, "https://gitlab.com/oauth/revoke");
    assert!(s.execute("forge.open", &json!({"url": "https://gitlab.com/group/paper"})).is_err(), "no token any more");
    Ok(())
}

#[test]
fn a_build_without_an_application_cannot_sign_in_and_says_so() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_forge_clients(Clients { gitlab: "TEXCRAFT_GITLAB_CLIENT_ID_PENDING".into(), github: "TEXCRAFT_GITHUB_CLIENT_ID_PENDING".into() });
    for service in ["gitlab", "github"] {
        let why = s.execute("forge.sign_in", &json!({"service": service})).err().unwrap_or_default();
        assert!(why.contains("no Git") && why.contains("personal access token"), "{why}");
    }
    assert!(s.forge_signing_in().is_none());
}
