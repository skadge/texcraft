//! Self-managed servers through the engine's commands (docs/forges.md, "Self-managed servers"):
//! checking one, adding and removing it, signing in to it with its own application (the desktop's
//! loopback redirect, the browser's round trip, GitHub Enterprise's device flow), the
//! organisation's servers, and no token in what is kept. A scripted transport and the fake GitLab:
//! no test reaches the network.

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
use texcraft_engine::forge::servers::Server;
use texcraft_engine::forge::{Request, Sending, Service, Transport};

const SERVER: &str = "https://git.example.org";
const ACCESS: &str = "server-oauth-access";
const REFRESH: &str = "server-oauth-refresh";

#[derive(Clone, Default)]
struct Keychain(Arc<Mutex<BTreeMap<String, String>>>);

impl Keychain {
    fn dump(&self) -> String {
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

/// The probe, OAuth's endpoints and `/user` go to the script, the project's API to the fake.
struct Router {
    script: Script,
    fake: FakeGitLab,
}

impl Transport for Router {
    fn send(&self, request: Request) -> Sending<'_> {
        let u = &request.url;
        let scripted = u.contains("/oauth/") || u.contains("/login/") || u.ends_with("/user") || u.ends_with("/version") || u.ends_with("/meta");
        if scripted { self.script.send(request) } else { Box::pin(async move { self.fake.transport().send(request).await }) }
    }
}

fn fake() -> FakeGitLab {
    FakeGitLab::new(SERVER, "lab/paper", ACCESS, &[("main.tex", b"\\documentclass{article}\n")])
}

fn session(script: &Script, fake: &FakeGitLab, keychain: &Keychain) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let (script, fake) = (script.clone(), fake.clone());
    s.set_forge_transport(move || Box::new(Router { script: script.clone(), fake: fake.clone() }));
    s.set_forge_clients(Clients { gitlab: "gitlab-com-app".into(), github: "github-com-app".into() });
    s.set_forge_loopback("127.0.0.1:0");
    s.set_forge_poll_second(Duration::from_millis(1));
    s.set_forge_secrets(Arc::new(keychain.clone()));
    s
}

fn outcome(s: &mut Session) -> Result<Value, String> {
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

fn come_back(authorize: &str, code: &str) {
    let q = query_of(authorize);
    let addr = get(&q, "redirect_uri").trim_start_matches("http://").trim_end_matches("/callback").to_owned();
    if let Ok(mut stream) = std::net::TcpStream::connect(addr) {
        let _ = write!(stream, "GET /callback?code={code}&state={} HTTP/1.1\r\nHost: localhost\r\n\r\n", get(&q, "state"));
        let _ = stream.read_to_string(&mut String::new());
    }
}

fn add(s: &mut Session) -> Result<Value, String> {
    s.execute("forge.add_server", &json!({"address": "git.example.org", "client_id": "server-app"}))
}

#[test]
fn checking_a_server_says_what_to_register_and_adding_it_keeps_it_for_the_shell() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    script.answer(401, json!({"message": "401 Unauthorized"}));
    let checked = s.execute("forge.check_server", &json!({"address": "https://git.example.org/lab/paper"}))?;
    assert_eq!((checked["instance"].as_str(), checked["added"].as_bool()), (Some(SERVER), Some(false)));
    assert_eq!(checked["register"]["redirect_uris"], json!(["http://127.0.0.1:47321/callback"]));
    assert_eq!(checked["register"]["confidential"], false);

    let (_, before) = s.forge_servers_saved();
    assert!(s.forge_sign_in_unavailable_at(Service::GitLab, SERVER).is_some(), "no application before it is added");
    add(&mut s)?;
    let (saved, after) = s.forge_servers_saved();
    assert!(after > before, "the shell is told to keep it");
    assert_eq!(saved.iter().map(|s| (s.instance.as_str(), s.client_id.as_str())).collect::<Vec<_>>(), [(SERVER, "server-app")]);
    assert!(s.forge_sign_in_unavailable_at(Service::GitLab, SERVER).is_none());
    let accounts = s.execute("forge.accounts", &json!({}))?;
    assert_eq!(accounts["servers"][0]["instance"], SERVER);

    // Adding again gives the new id in place of the old one.
    s.execute("forge.add_server", &json!({"address": SERVER, "client_id": "second-app"}))?;
    assert_eq!(s.forge_servers().len(), 1);
    assert_eq!(s.forge_client_id(SERVER, Service::GitLab).as_deref(), Some("second-app"));

    s.execute("forge.remove_server", &json!({"address": SERVER}))?;
    assert!(s.forge_servers().is_empty() && s.forge_servers_saved().0.is_empty());
    assert!(s.execute("forge.remove_server", &json!({"address": SERVER})).is_err(), "nothing left to remove");
    Ok(())
}

#[test]
fn a_server_that_is_not_gitlab_is_refused_by_the_check_in_the_background_too() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    script.answer(200, json!("<html>a wiki</html>"));
    let started = s.execute("forge.check_server", &json!({"address": "wiki.example.org", "background": true}))?;
    assert_eq!(started["started"], true);
    let refused = outcome(&mut s).err().unwrap_or_default();
    assert!(refused.contains("does not answer as a GitLab server"), "{refused}");
    Ok(())
}

#[test]
fn signing_in_to_an_added_server_uses_its_application_and_opens_with_no_token() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    let unknown = s.execute("forge.sign_in", &json!({"instance": SERVER})).err().unwrap_or_default();
    assert!(unknown.contains("add it first"), "{unknown}");
    add(&mut s)?;

    script.answer(200, json!({"access_token": ACCESS, "refresh_token": REFRESH, "expires_in": 7200, "scope": "read_api read_user"}));
    script.answer(200, json!({"name": "Ada Lovelace", "username": "ada"}));
    let started = s.execute("forge.sign_in", &json!({"instance": SERVER}))?;
    let url = started["open"].as_str().ok_or("no page to open")?.to_owned();
    assert!(url.starts_with("https://git.example.org/oauth/authorize?"), "{url}");
    assert_eq!(get(&query_of(&url), "client_id"), "server-app");
    assert_eq!(s.forge_signing_in_instance(), Some(SERVER));
    come_back(&url, "code-1");
    let done = outcome(&mut s)?;
    assert_eq!((done["instance"].as_str(), done["username"].as_str()), (Some(SERVER), Some("ada")));
    assert_eq!(script.requests()[0].url, "https://git.example.org/oauth/token");
    assert_eq!(get(&script.form(0), "client_id"), "server-app");
    assert!(s.forge_signed_in(Service::GitLab).is_none(), "gitlab.com is another account");

    let opened = s.execute("forge.open", &json!({"url": "https://git.example.org/lab/paper"}))?;
    assert_eq!(opened["files"], 1, "opened with the sign-in's token");

    // A restart: the server comes back from the preferences, its sign-in from the keychain.
    let saved = s.forge_servers_saved().0.to_vec();
    let mut again = session(&script, &fake, &keychain);
    again.restore_forge_servers(saved.clone());
    assert!(again.forge_signed_in_at(SERVER).is_some(), "the sign-in lasts a restart");
    let kept = serde_json::to_string(&saved).map_err(|e| e.to_string())?;
    let accounts = again.execute("forge.accounts", &json!({}))?.to_string();
    for secret in [ACCESS, REFRESH] {
        assert!(!kept.contains(secret) && !accounts.contains(secret), "{secret} in what is kept or said");
        assert!(keychain.dump().contains(secret), "the keychain keeps it");
    }

    // Removing the server signs out of it, and revokes with its application.
    script.answer(200, json!({}));
    let removed = again.execute("forge.remove_server", &json!({"address": SERVER}))?;
    assert_eq!(removed["signed_out"]["revoking"], true);
    assert!(again.forge_signed_in_at(SERVER).is_none() && !keychain.dump().contains(ACCESS));
    outcome(&mut again)?;
    let revoke = script.requests().into_iter().last().ok_or("no revoke")?;
    assert_eq!(revoke.url, "https://git.example.org/oauth/revoke");
    Ok(())
}

#[test]
fn in_the_browser_the_page_goes_to_the_server_and_comes_back_to_it() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    s.set_forge_redirect("https://texcraft.example/");
    add(&mut s)?;
    let checked_register = s.forge_registration(Service::GitLab, SERVER).redirect_uris;
    assert!(checked_register.contains(&"https://texcraft.example/".to_owned()), "the page's own address is to be registered too");

    let started = s.execute("forge.sign_in", &json!({"instance": SERVER}))?;
    let url = started["navigate"].as_str().ok_or("no page to go to")?;
    assert!(url.starts_with("https://git.example.org/oauth/authorize?"), "{url}");
    let state = get(&query_of(url), "state").to_owned();
    script.answer(200, json!({"access_token": ACCESS, "expires_in": 7200, "scope": "read_api read_user"}));
    script.answer(200, json!({"name": "Ada", "username": "ada"}));
    s.execute("forge.sign_in_return", &json!({"code": "c", "state": state}))?;
    assert_eq!(outcome(&mut s)?["instance"], SERVER);
    assert_eq!(script.requests()[0].url, "https://git.example.org/oauth/token");
    Ok(())
}

#[test]
fn github_enterprise_signs_in_with_the_device_flow_on_its_host() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    s.execute("forge.add_server", &json!({"address": "github.example.org", "service": "github", "client_id": "ghe-app"}))?;
    script.answer(200, json!({"device_code": "d", "user_code": "AB-12", "verification_uri": "https://github.example.org/login/device", "expires_in": 900, "interval": 1}));
    script.answer(200, json!({"access_token": "ghe-secret", "token_type": "bearer", "scope": "repo"}));
    script.answer(200, json!({"login": "ada"}));
    s.execute("forge.sign_in", &json!({"service": "github", "instance": "https://github.example.org"}))?;
    let done = outcome(&mut s)?;
    assert_eq!(done["instance"], "https://github.example.org");
    let urls: Vec<String> = script.requests().iter().map(|r| r.url.clone()).collect();
    assert_eq!(urls[0], "https://github.example.org/login/device/code");
    assert_eq!(urls[2], "https://github.example.org/api/v3/user");
    assert_eq!(get(&script.form(0), "client_id"), "ghe-app");
    Ok(())
}

#[test]
fn the_organisation_s_servers_come_first_and_cannot_be_removed_or_changed() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    let provided = Server::new(SERVER, Service::GitLab, "org-app").map_err(|e| e.to_string())?;
    s.set_forge_provided_servers(vec![provided], vec!["server 2 (x): bad".to_owned()]);
    assert!(s.forge_servers()[0].provided);
    assert_eq!(s.forge_client_id(SERVER, Service::GitLab).as_deref(), Some("org-app"));
    assert!(s.execute("forge.remove_server", &json!({"address": SERVER})).is_err());
    assert!(add(&mut s).is_err(), "its application is the organisation's");
    assert!(s.forge_servers_saved().0.is_empty(), "nor is it kept in the user's preferences");
    let accounts = s.execute("forge.accounts", &json!({}))?;
    assert_eq!((accounts["servers"][0]["provided"].as_bool(), accounts["server_problems"][0].as_str()), (Some(true), Some("server 2 (x): bad")));
    Ok(())
}
