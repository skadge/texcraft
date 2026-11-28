//! The browser's sign-ins across its tabs (docs/forges.md): kept in the storage every tab shares
//! while "Remember sign-ins in this browser" is on, in the tab's own when off; another tab's
//! sign-in, disconnection and renewal seen without a reload; GitLab's refresh token, which works
//! once, not spent twice. Two sessions over one shared storage stand for two tabs; the forge is a
//! scripted transport and the fake GitLab; tokens are made-up strings.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use texcraft_engine::Session;
use texcraft_engine::ai::Secrets;
use texcraft_engine::browser_secrets::{BrowserSecrets, KeyValue};
use texcraft_engine::forge::fake::{FakeGitLab, Script};
use texcraft_engine::forge::{Request, Sending, Service, Transport};

use crate::forge_sign_in::{ACCESS, Keychain, fake, session};

/// A storage in memory; clones share it, as a site's tabs share its local storage.
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<BTreeMap<String, String>>>);

impl Memory {
    fn dump(&self) -> String {
        format!("{:?}", self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl KeyValue for Memory {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn remove(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
        Ok(())
    }
    fn keys(&self) -> Result<Vec<String>, String> {
        Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).keys().cloned().collect())
    }
}

const HANDLE: &str = "texcraft.forge.gitlab.com";

/// A sign-in to gitlab.com as kept, with `access` and `refresh`, expiring at `expires_at`.
fn grant(access: &str, refresh: &str, expires_at: i64) -> String {
    json!({"service": "gitlab", "instance": "https://gitlab.com", "access": access, "refresh": refresh,
        "expires_at": expires_at, "scopes": ["read_api"], "user": {"name": "Ada", "username": "ada"},
        "redirect": "https://texcraft.example/"})
    .to_string()
}

/// Far ahead: an access token that has not expired.
const LATER: i64 = 4_000_000_000;

/// A tab: a session whose sign-ins are kept in `local`, shared, and a session storage of its own.
fn tab(script: &Script, fake: &FakeGitLab, local: &Memory) -> Session {
    let mut s = session(script, fake, &Keychain::default());
    s.set_forge_redirect("https://texcraft.example/");
    s.set_forge_secrets(Arc::new(BrowserSecrets::new(local.clone(), Memory::default())));
    s
}

/// The token requests the forge was sent (sign-ins and renewals).
fn renewals(script: &Script) -> usize {
    script.requests().iter().filter(|r| r.url.ends_with("/oauth/token")).count()
}

fn open(s: &mut Session) -> Result<Value, String> {
    s.execute("forge.open", &json!({"url": "https://gitlab.com/group/paper"}))
}

#[test]
fn where_sign_ins_are_kept_follows_the_setting() -> Result<(), String> {
    let (script, fake, local) = (Script::default(), fake(), Memory::default());
    let mut a = tab(&script, &fake, &local);
    let list = a.execute("accounts.list", &json!({}))?;
    assert_eq!((list["kept"].as_str(), list["remember_in_browser"].as_bool()), (Some("browser"), Some(true)));
    a.execute("accounts.remember_in_browser", &json!({"on": false}))?;
    let list = a.execute("accounts.list", &json!({}))?;
    assert_eq!((list["kept"].as_str(), list["remember_in_browser"].as_bool()), (Some("tab"), Some(false)));
    assert_eq!(a.remember_in_browser_saved().0, Some(false));

    // Where the store has no such choice (the desktop's keychain), it is not offered.
    let mut desktop = session(&script, &fake, &Keychain::default());
    assert_eq!(desktop.execute("accounts.list", &json!({}))?["remember_in_browser"], Value::Null);
    assert!(desktop.execute("accounts.remember_in_browser", &json!({"on": false})).is_err());
    Ok(())
}

#[test]
fn another_tab_sees_a_sign_in_and_a_disconnection_once_it_reads_again() -> Result<(), String> {
    let (script, fake, local) = (Script::default(), fake(), Memory::default());
    let mut a = tab(&script, &fake, &local);
    let mut b = tab(&script, &fake, &local);
    local.set(HANDLE, &grant(ACCESS, "gl-refresh-a", LATER))?;
    assert!(b.forge_signed_in(Service::GitLab).is_none());
    b.execute("accounts.reload", &json!({}))?;
    assert!(b.forge_signed_in(Service::GitLab).is_some(), "the other tab's sign-in is not seen");

    a.execute("accounts.reload", &json!({}))?;
    a.execute("accounts.disconnect", &json!({"instance": "https://gitlab.com"}))?;
    assert!(!local.dump().contains("gl-refresh-a"), "the disconnection left it in the shared storage");
    b.execute("accounts.reload", &json!({}))?;
    assert!(b.forge_signed_in(Service::GitLab).is_none(), "the other tab still has it");

    // Turned off in one tab: the other tab follows, and forgets what moved into the first.
    local.set(HANDLE, &grant(ACCESS, "gl-refresh-a", LATER))?;
    a.execute("accounts.reload", &json!({}))?;
    b.execute("accounts.reload", &json!({}))?;
    let (_, before) = b.remember_in_browser_saved();
    a.execute("accounts.remember_in_browser", &json!({"on": false}))?;
    b.execute("accounts.reload", &json!({}))?;
    let (on, after) = b.remember_in_browser_saved();
    assert!(on == Some(false) && after != before, "the other tab did not follow the setting");
    assert!(b.forge_signed_in(Service::GitLab).is_none() && a.forge_signed_in(Service::GitLab).is_some());
    Ok(())
}

#[test]
fn a_sign_in_another_tab_renewed_is_used_not_renewed_again() -> Result<(), String> {
    let (script, fake, local) = (Script::default(), fake(), Memory::default());
    local.set(HANDLE, &grant("gl-expired", "gl-old-refresh", 1))?;
    let mut a = tab(&script, &fake, &local);
    let mut b = tab(&script, &fake, &local);
    script.answer(200, json!({"access_token": ACCESS, "refresh_token": "gl-new-refresh", "expires_in": 7200, "scope": "read_api"}));
    assert_eq!(open(&mut a)?["files"], 1);
    assert_eq!(renewals(&script), 1);
    // B read the expired sign-in before A renewed it; it takes A's from the storage.
    assert_eq!(open(&mut b)?["files"], 1, "B did not open with A's renewed sign-in");
    assert_eq!(renewals(&script), 1, "B spent the refresh token A had already spent");
    Ok(())
}

/// The scripted forge, which, as the renewal is asked for, lets another tab's renewal land in
/// the shared storage first (the two tabs asked at once; the other's answer came first).
struct Racing {
    script: Script,
    fake: FakeGitLab,
    local: Memory,
}

impl Transport for Racing {
    fn send(&self, request: Request) -> Sending<'_> {
        if request.url.ends_with("/oauth/token") {
            let _ = self.local.set(HANDLE, &grant(ACCESS, "gl-refresh-other", LATER));
            return self.script.send(request);
        }
        Box::pin(async move { self.fake.transport().send(request).await })
    }
}

#[test]
fn a_refused_renewal_is_tried_again_with_what_the_storage_holds() -> Result<(), String> {
    let (script, fake, local) = (Script::default(), fake(), Memory::default());
    local.set(HANDLE, &grant("gl-expired", "gl-old-refresh", 1))?;
    let mut b = tab(&script, &fake, &local);
    let (sent, served, shared) = (script.clone(), fake.clone(), local.clone());
    b.set_forge_transport(move || Box::new(Racing { script: sent.clone(), fake: served.clone(), local: shared.clone() }));
    // GitLab refuses B's refresh token: the other tab spent it.
    script.answer(400, json!({"error": "invalid_grant", "error_description": "The provided authorization grant is invalid"}));
    assert_eq!(open(&mut b)?["files"], 1, "B did not fall back on the other tab's renewal");
    assert_eq!(renewals(&script), 1, "asked to renew more than once");
    let kept = BrowserSecrets::new(local.clone(), Memory::default()).get(HANDLE)?.map(|s| s.expose().to_owned()).unwrap_or_default();
    assert!(kept.contains("gl-refresh-other"), "the other tab's renewal was overwritten");
    Ok(())
}
