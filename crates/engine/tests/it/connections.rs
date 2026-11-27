//! Connections (docs/design/connections.md): `accounts.list` follows sign-ins, tokens and servers
//! and never holds a token; Disconnect signs out and forgets; the default is kept, restored and
//! dropped with its server; a refused token is not kept; what earlier versions kept loads into
//! the list. The forges are a scripted transport and the fake GitLab; every token is a made-up
//! string and no test reaches the network.

use serde_json::{Value, json};
use texcraft_engine::Session;
use texcraft_engine::forge::fake::Script;

use crate::forge_sign_in::{ACCESS, Keychain, REFRESH, fake, outcome, session, sign_in_gitlab};

const GITHUB_TOKEN: &str = "ghp_fake_connection_token";
const OVERLEAF_TOKEN: &str = "olp_fake_git_token";
const SERVER: &str = "https://git.example.org";

fn list(s: &mut Session) -> Result<Value, String> {
    s.execute("accounts.list", &json!({}))
}

/// The row of `instance`, if listed.
fn row(s: &mut Session, instance: &str) -> Result<Option<Value>, String> {
    Ok(list(s)?["connections"].as_array().into_iter().flatten().find(|r| r["instance"] == instance).cloned())
}

#[test]
fn the_list_follows_sign_ins_tokens_and_servers_and_never_holds_a_token() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    assert_eq!(list(&mut s)?["connections"].as_array().map(Vec::len), Some(0), "nothing is connected yet");

    // Signed in to gitlab.com, to read only.
    sign_in_gitlab(&mut s, &script, "read_api read_user")?;
    let gitlab = row(&mut s, "https://gitlab.com")?.ok_or("gitlab.com is not listed once signed in")?;
    assert_eq!((gitlab["state"].as_str(), gitlab["user"]["username"].as_str()), (Some("signed_in"), Some("ada")));
    assert_eq!((gitlab["kept"].as_str(), gitlab["can_commit"].as_bool()), (Some("keychain"), Some(false)));

    // A GitHub token, checked with GitHub first, then kept.
    script.answer(200, json!({"login": "octo", "name": "Octo Cat"}));
    let added = s.execute("accounts.add", &json!({"kind": "github", "token": GITHUB_TOKEN}))?;
    assert_eq!(added["username"], "octo");
    let github = row(&mut s, "https://github.com")?.ok_or("github.com is not listed once its token is kept")?;
    assert_eq!((github["state"].as_str(), github["how"].as_str()), (Some("token"), Some("token")));
    assert_eq!(github["user"]["username"], "octo", "who the token is for is not said");

    // A server added, not signed in to; and Overleaf's git token.
    s.execute("forge.add_server", &json!({"address": "git.example.org", "client_id": "lab-app"}))?;
    let server = row(&mut s, SERVER)?.ok_or("the server added is not listed")?;
    assert_eq!((server["state"].as_str(), server["removable"].as_bool()), (Some("signed_out"), Some(true)));
    s.execute("accounts.add", &json!({"kind": "overleaf", "token": OVERLEAF_TOKEN}))?;
    let overleaf = row(&mut s, "https://git.overleaf.com")?.ok_or("Overleaf's token is not listed")?;
    assert_eq!((overleaf["kind"].as_str(), overleaf["can_default"].as_bool()), (Some("git"), Some(false)));

    // Forges first, then the git servers; no token anywhere in the list.
    let all = list(&mut s)?;
    let kinds: Vec<&str> = all["connections"].as_array().into_iter().flatten().filter_map(|r| r["kind"].as_str()).collect();
    assert!(kinds.windows(2).all(|w| !(w[0] == "git" && w[1] == "forge")), "forges are not first: {kinds:?}");
    let text = all.to_string();
    for secret in [ACCESS, REFRESH, GITHUB_TOKEN, OVERLEAF_TOKEN] {
        assert!(!text.contains(secret), "a token in accounts.list");
    }
    Ok(())
}

#[test]
fn disconnect_signs_out_revokes_and_forgets_the_kept_sign_in() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    sign_in_gitlab(&mut s, &script, "api read_user")?;
    assert!(keychain.dump().contains(ACCESS));
    script.answer(200, json!({}));
    let done = s.execute("accounts.disconnect", &json!({"host": "gitlab.com"}))?;
    assert!(done["projects"].is_string(), "what happens to linked projects is not said");
    assert!(!keychain.dump().contains(ACCESS), "the sign-in is still in the keychain");
    assert!(row(&mut s, "https://gitlab.com")?.is_none(), "still listed once disconnected");
    assert_eq!(outcome(&mut s)?["revoked"], true, "GitLab was not asked to revoke it");

    // A git server's token: gone from the store and the list; nothing left to disconnect.
    s.execute("accounts.add", &json!({"kind": "git", "host": "git.example.net", "token": "fake-git-token"}))?;
    assert!(row(&mut s, "https://git.example.net")?.is_some());
    s.execute("accounts.disconnect", &json!({"instance": "https://git.example.net"}))?;
    assert!(row(&mut s, "https://git.example.net")?.is_none() && !keychain.dump().contains("fake-git-token"));
    assert!(s.execute("accounts.disconnect", &json!({"host": "git.example.net"})).is_err(), "disconnected twice");
    Ok(())
}

#[test]
fn the_default_is_kept_restored_and_dropped_with_its_server() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    let (_, before) = s.default_connection_saved();
    s.execute("accounts.set_default", &json!({"instance": "https://github.com"}))?;
    let (default, after) = s.default_connection_saved();
    assert_eq!(default, Some("https://github.com"));
    assert!(after > before, "the shell is not told to keep it");
    assert_eq!(row(&mut s, "https://github.com")?.map(|r| r["default"].clone()), Some(json!(true)), "the default's row is not listed as such");
    assert!(s.execute("accounts.set_default", &json!({"instance": "https://git.overleaf.com"})).is_err(), "a git server cannot be the default");

    // A restart: the preferences give it back.
    let mut again = session(&script, &fake, &keychain);
    again.restore_default_connection(Some("https://github.com".to_owned()));
    assert_eq!(again.default_connection().map(|(i, _)| i).as_deref(), Some("https://github.com"));

    // A server as the default, then removed: the default goes with it.
    s.execute("forge.add_server", &json!({"address": "git.example.org", "client_id": "lab-app"}))?;
    s.execute("accounts.set_default", &json!({"instance": SERVER}))?;
    s.execute("forge.remove_server", &json!({"address": SERVER}))?;
    assert_eq!(s.default_connection_saved().0, None, "the default outlived its server");
    // One the preferences name that is not known here is dropped.
    again.restore_default_connection(Some(SERVER.to_owned()));
    assert!(again.default_connection().is_none());
    Ok(())
}

#[test]
fn a_token_the_forge_refuses_is_not_kept() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut s = session(&script, &fake, &keychain);
    script.answer(401, json!({"message": "Bad credentials"}));
    let why = s.execute("accounts.add", &json!({"kind": "github", "token": GITHUB_TOKEN})).err().unwrap_or_default();
    assert!(why.contains("did not accept"), "{why}");
    assert!(!keychain.dump().contains(GITHUB_TOKEN), "a refused token was kept");
    assert!(row(&mut s, "https://github.com")?.is_none(), "a refused token is listed");
    Ok(())
}

#[test]
fn what_an_earlier_version_kept_loads_into_the_list_with_no_new_sign_in() -> Result<(), String> {
    let (script, fake, keychain) = (Script::default(), fake(), Keychain::default());
    let mut first = session(&script, &fake, &keychain);
    sign_in_gitlab(&mut first, &script, "api read_user")?;
    first.execute("forge.add_server", &json!({"address": "git.example.org", "client_id": "lab-app"}))?;
    first.execute("git.keep_credential", &json!({"host": "git.overleaf.com", "token": OVERLEAF_TOKEN}))?;
    let servers = first.forge_servers_saved().0.to_vec();
    let hosts = first.git_hosts_saved().0.to_vec();
    let sent = script.requests().len();

    // As the window starts: the keychain, then what the preferences kept.
    let mut s = session(&script, &fake, &keychain);
    s.restore_forge_servers(servers);
    s.restore_git_hosts(hosts);
    s.restore_default_connection(None);
    let gitlab = row(&mut s, "https://gitlab.com")?.ok_or("the kept sign-in is not listed")?;
    assert_eq!((gitlab["state"].as_str(), gitlab["user"]["username"].as_str()), (Some("signed_in"), Some("ada")));
    assert!(row(&mut s, SERVER)?.is_some(), "the server added is not listed");
    assert!(row(&mut s, "https://git.overleaf.com")?.is_some(), "the token kept is not listed");
    assert_eq!(script.requests().len(), sent, "listing asked a forge something");
    Ok(())
}
