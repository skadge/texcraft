//! Self-managed servers (docs/forges.md, "Self-managed servers"): their address, the probe that
//! checks a server is the service said, what to register on it, a shared link's proposal, the
//! organisation's file, and signing in to one with its own application. Scripted answers only: no
//! test reaches the network.

use serde_json::json;
use texcraft_forge::fake::Script;
use texcraft_forge::oauth::{self, Clients, Pkce};
use texcraft_forge::servers::{self, Server};
use texcraft_forge::task::block_on;
use texcraft_forge::{Error, Service, Token};

const SERVER: &str = "https://git.example.org";

fn pairs(query: &str) -> Vec<(String, String)> {
    oauth::parse_query(query)
}

#[test]
fn a_server_s_address_is_its_scheme_and_host_whatever_page_is_pasted() -> Result<(), Error> {
    for typed in ["git.example.org", "https://git.example.org/", "https://Git.Example.org/group/project/-/tree/main", " https://git.example.org?x=1 "]
    {
        assert_eq!(servers::parse_instance(typed)?, SERVER, "{typed}");
    }
    assert_eq!(servers::parse_instance("https://git.example.org:8443/x")?, "https://git.example.org:8443");
    assert_eq!(servers::parse_instance("http://localhost:3000")?, "http://localhost:3000", "plain HTTP on this computer, for a test server");
    for refused in ["", "http://git.example.org", "ftp://git.example.org", "https://gitlab.com", "github.com/owner/repo", "https://bad host/"] {
        assert!(servers::parse_instance(refused).is_err(), "{refused:?} is accepted");
    }
    Ok(())
}

#[test]
fn the_probe_knows_gitlab_by_its_version_or_its_401() -> Result<(), Error> {
    let script = Script::new(&[(200, json!({"version": "17.5.1-ee", "revision": "abc"}))]);
    let probed = block_on(servers::probe(&script, SERVER, Service::GitLab, false))?;
    assert_eq!(probed.version.as_deref(), Some("17.5.1-ee"));
    assert_eq!(script.requests()[0].url, "https://git.example.org/api/v4/version");
    assert!(script.requests()[0].headers.iter().all(|(k, _)| k != "Authorization"), "the probe sends no token");

    // Most servers ask for a token to say their version: GitLab's 401 proves GitLab.
    let script = Script::new(&[(401, json!({"message": "401 Unauthorized"}))]);
    assert_eq!(block_on(servers::probe(&script, SERVER, Service::GitLab, false))?.version, None);
    Ok(())
}

#[test]
fn the_probe_refuses_a_server_that_is_not_gitlab_in_words() {
    for (status, body) in
        [(200, json!("<!doctype html><title>Welcome</title>")), (404, json!({"error": "not found"})), (401, json!("Authorization Required"))]
    {
        let script = Script::new(&[(status, body)]);
        let Err(e) = block_on(servers::probe(&script, SERVER, Service::GitLab, false)) else { panic!("{status} is taken for GitLab") };
        assert!(e.to_string().contains("does not answer as a GitLab server"), "{e}");
    }
}

#[test]
fn a_server_that_does_not_answer_is_said_as_the_network_on_the_desktop_and_as_cors_in_the_browser() {
    let silent = Script::default();
    let Err(desktop) = block_on(servers::probe(&silent, SERVER, Service::GitLab, false)) else { panic!("no answer is no server") };
    assert!(matches!(desktop, Error::Network { .. }), "{desktop:?}");
    let Err(browser) = block_on(servers::probe(&silent, SERVER, Service::GitLab, true)) else { panic!("no answer is no server") };
    let words = browser.to_string();
    assert!(words.contains("does not answer web pages") && words.contains("use the desktop app"), "{words}");
}

#[test]
fn the_probe_knows_a_github_enterprise_server_by_its_meta() -> Result<(), Error> {
    let script = Script::new(&[(200, json!({"installed_version": "3.14.2", "verifiable_password_authentication": false}))]);
    let probed = block_on(servers::probe(&script, SERVER, Service::GitHub, false))?;
    assert_eq!((probed.service, probed.version.as_deref()), (Service::GitHub, Some("3.14.2")));
    assert_eq!(script.requests()[0].url, "https://git.example.org/api/v3/meta");
    let private = Script::new(&[(401, json!({"message": "Must authenticate", "documentation_url": "https://docs.github.com/enterprise-server"}))]);
    assert!(block_on(servers::probe(&private, SERVER, Service::GitHub, false)).is_ok(), "private mode answers 401 with GitHub's JSON");
    let gitlab = Script::new(&[(404, json!({"message": "404 Not Found"}))]);
    assert!(block_on(servers::probe(&gitlab, SERVER, Service::GitHub, false)).is_err(), "a GitLab is not a GitHub");
    Ok(())
}

#[test]
fn the_registration_lists_both_redirects_in_the_browser_and_the_scopes() {
    let desktop = servers::registration(Service::GitLab, SERVER, None);
    assert_eq!(desktop.redirect_uris, ["http://127.0.0.1:47321/callback"]);
    assert!(desktop.page.starts_with(SERVER) && desktop.admin_page.as_deref().is_some_and(|p| p.starts_with(SERVER)));
    let scopes: Vec<&str> = desktop.scopes.unwrap_or_default().split(' ').collect();
    for needed in oauth::GITLAB_READ.iter().chain(oauth::GITLAB_WRITE) {
        assert!(scopes.contains(needed), "signing in asks for {needed}, which the registration does not allow");
    }
    let browser = servers::registration(Service::GitLab, SERVER, Some("https://texcraft.example/"));
    assert!(browser.redirect_uris.contains(&"https://texcraft.example/".to_owned()) && browser.redirect_uris.len() == 2);
}

#[test]
fn a_link_proposes_a_server_and_the_project_it_names() -> Result<(), Error> {
    let q = pairs("gitlab=https%3A%2F%2Fgit.example.org%2Fgroup%2Fpaper&client_id=abc123");
    let proposal = servers::proposal(&q).ok_or(Error::Nothing)??;
    assert_eq!((proposal.server.instance.as_str(), proposal.server.service, proposal.server.client_id.as_str()), (SERVER, Service::GitLab, "abc123"));
    assert_eq!(proposal.project.as_deref(), Some("https://git.example.org/group/paper"));
    let bare = servers::proposal(&pairs("github=git.example.org&client_id=Iv1.x")).ok_or(Error::Nothing)??;
    assert_eq!((bare.server.service, bare.project), (Service::GitHub, None));
    assert!(servers::proposal(&pairs("gitlab=https://git.example.org/g/p")).is_some_and(|p| p.is_err()), "no client id: nothing to add");
    assert!(servers::proposal(&pairs("code=1&state=2")).is_none(), "a query that proposes nothing");
    Ok(())
}

#[test]
fn the_organisation_s_file_lists_provided_servers_and_says_what_it_leaves_out() -> Result<(), String> {
    let text = r#"
[[server]]
address = "https://git.example.org"
client_id = "gl-app"

[[server]]
address = "github.example.org"
service = "github"
client_id = "Iv1.app"

[[server]]
address = "http://insecure.example.org"
client_id = "x"
"#;
    let (servers, problems) = servers::parse_config(text)?;
    assert_eq!(servers.len(), 2);
    assert!(servers.iter().all(|s| s.provided));
    assert_eq!((servers[0].service, servers[1].service), (Service::GitLab, Service::GitHub));
    assert_eq!(servers[1].instance, "https://github.example.org");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("insecure.example.org"));
    assert!(servers::parse_config("[[server]\naddress=").is_err(), "a file that is not TOML");
    Ok(())
}

#[test]
fn a_kept_server_holds_no_secret_and_is_not_provided_when_read_back() -> Result<(), String> {
    let mut server = Server::new("git.example.org", Service::GitLab, " app-id ").map_err(|e| e.to_string())?;
    server.provided = true;
    let json = serde_json::to_string(&server).map_err(|e| e.to_string())?;
    assert_eq!(json, r#"{"instance":"https://git.example.org","service":"gitlab","client_id":"app-id"}"#);
    let back: Server = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    assert!(!back.provided, "only the organisation's file provides a server");
    Ok(())
}

#[test]
fn a_grant_from_a_server_is_renewed_and_revoked_with_that_server_s_application() -> Result<(), Error> {
    let script = Script::new(&[(
        200,
        json!({"access_token": "a1", "refresh_token": "r1", "expires_in": 7200, "scope": "read_api read_user", "created_at": 1_700_000_000}),
    )]);
    let pkce = Pkce::from_bytes(&[3; 32]);
    let grant = block_on(oauth::exchange(&script, SERVER, "server-app", &Token::new("code")?, "http://127.0.0.1:47321/callback", &pkce))?;
    assert_eq!(script.requests()[0].url, "https://git.example.org/oauth/token");
    let kept = oauth::Grant::from_stored(&grant.stored())?;

    let renew = Script::new(&[(200, json!({"access_token": "a2", "refresh_token": "r2", "expires_in": 7200}))]);
    let clients = Clients { gitlab: "gitlab-com-app".into(), github: "gh".into() };
    let (_, renewed) = block_on(oauth::fresh(&renew, &clients, &kept, 1_800_000_000))?;
    assert_eq!(renew.requests()[0].url, "https://git.example.org/oauth/token");
    assert_eq!(renew.form(0).iter().find(|(k, _)| k == "client_id").map(|(_, v)| v.as_str()), Some("server-app"), "gitlab.com's application");
    let renewed = renewed.ok_or(Error::Nothing)?;
    let revoke = Script::new(&[(200, json!({}))]);
    block_on(oauth::revoke(&revoke, "gitlab-com-app", &renewed))?;
    assert_eq!(revoke.form(0).iter().find(|(k, _)| k == "client_id").map(|(_, v)| v.as_str()), Some("server-app"));
    Ok(())
}

#[test]
fn github_enterprise_s_device_flow_and_user_are_on_its_own_host() -> Result<(), Error> {
    let script = Script::new(&[
        (
            200,
            json!({"device_code": "d", "user_code": "AB-12", "verification_uri": "https://git.example.org/login/device", "expires_in": 900, "interval": 5}),
        ),
        (200, json!({"access_token": "ghe-token", "token_type": "bearer", "scope": "repo"})),
        (200, json!({"login": "ada", "name": "Ada"})),
    ]);
    let device = block_on(oauth::device_code_at(&script, SERVER, "ghe-app", oauth::GITHUB_SCOPE))?;
    let oauth::DevicePoll::Granted(grant) = block_on(oauth::poll_device(&script, "ghe-app", &device))? else { panic!("granted") };
    let grant = block_on(oauth::with_user(&script, *grant))?;
    let urls: Vec<String> = script.requests().iter().map(|r| r.url.clone()).collect();
    assert_eq!(
        urls,
        ["https://git.example.org/login/device/code", "https://git.example.org/login/oauth/access_token", "https://git.example.org/api/v3/user"]
    );
    assert_eq!(grant.instance, SERVER);
    Ok(())
}
