//! Signing in (docs/forges.md, step 4): PKCE, GitLab's authorization code exchange, refresh and
//! revocation, GitHub's device flow, and the desktop's loopback listener, against scripted
//! answers and a local client. No test reaches the network.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use texcraft_forge::fake::Script;
use texcraft_forge::oauth::{self, Clients, DevicePoll, Grant, Pkce};
use texcraft_forge::task::block_on;
use texcraft_forge::{Error, GITHUB_COM, GITLAB_COM, Service, Token};

fn field<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

const REDIRECT: &str = "http://127.0.0.1:47321/callback";

fn tokens(access: &str, refresh: &str, scope: &str) -> Value {
    json!({"access_token": access, "token_type": "Bearer", "expires_in": 7200, "refresh_token": refresh, "scope": scope, "created_at": 1_700_000_000})
}

/// A GitLab grant got by exchanging a code, with `scope`.
fn signed_in(scope: &str) -> Result<Grant, Error> {
    let script = Script::new(&[(200, tokens("gl-access-1", "gl-refresh-1", scope))]);
    block_on(oauth::exchange(&script, GITLAB_COM, "client-1", &Token::new("the-code")?, REDIRECT, &Pkce::from_bytes(&[7; 32])))
}

#[test]
fn the_pkce_challenge_is_rfc_7636s() {
    // RFC 7636, appendix B.
    let octets = [
        116, 24, 223, 180, 151, 153, 224, 37, 79, 250, 96, 125, 216, 173, 187, 186, 22, 212, 37, 77, 105, 214, 191, 240, 91, 88, 5, 88, 83, 132, 141,
        121,
    ];
    let pkce = Pkce::from_bytes(&octets);
    assert_eq!(pkce.verifier(), "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    assert_eq!(pkce.challenge(), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn new_verifiers_and_states_are_random_and_long_enough() -> Result<(), Error> {
    let (a, b) = (Pkce::new()?, Pkce::new()?);
    assert_ne!(a.verifier(), b.verifier());
    assert!(a.verifier().len() >= 43, "RFC 7636 asks for at least 43 characters");
    assert!(Pkce::from_verifier(a.verifier()).is_ok());
    assert!(Pkce::from_verifier("short").is_err());
    assert_ne!(oauth::new_state()?, oauth::new_state()?);
    Ok(())
}

#[test]
fn the_authorize_url_carries_the_request() {
    let pkce = Pkce::from_bytes(&[1; 32]);
    let url = oauth::authorize_url(GITLAB_COM, "client-1", REDIRECT, oauth::GITLAB_READ, "st&te", &pkce);
    let (page, query) = url.split_once('?').unwrap_or_default();
    assert_eq!(page, "https://gitlab.com/oauth/authorize");
    let q = oauth::parse_query(query);
    assert_eq!(field(&q, "client_id"), Some("client-1"));
    assert_eq!(field(&q, "redirect_uri"), Some(REDIRECT), "decoded back as it was");
    assert_eq!(field(&q, "response_type"), Some("code"));
    assert_eq!(field(&q, "state"), Some("st&te"));
    assert_eq!(field(&q, "scope"), Some("read_api read_user"), "reading only, at first");
    assert_eq!(field(&q, "code_challenge"), Some(pkce.challenge().as_str()));
    assert_eq!(field(&q, "code_challenge_method"), Some("S256"));
    assert!(!url.contains(pkce.verifier()), "the verifier stays here");
}

#[test]
fn a_code_is_exchanged_with_the_verifier_and_no_secret() -> Result<(), Error> {
    let script = Script::new(&[(200, tokens("gl-access-1", "gl-refresh-1", "read_api read_user"))]);
    let pkce = Pkce::from_bytes(&[7; 32]);
    let grant = block_on(oauth::exchange(&script, GITLAB_COM, "client-1", &Token::new("the-code")?, REDIRECT, &pkce))?;
    let sent = script.requests();
    assert_eq!((sent[0].method, sent[0].url.as_str()), ("POST", "https://gitlab.com/oauth/token"));
    let form = script.form(0);
    assert_eq!(field(&form, "grant_type"), Some("authorization_code"));
    assert_eq!(field(&form, "code"), Some("the-code"));
    assert_eq!(field(&form, "code_verifier"), Some(pkce.verifier()));
    assert_eq!(field(&form, "redirect_uri"), Some(REDIRECT));
    assert!(field(&form, "client_secret").is_none());
    assert_eq!(grant.access(), &Token::new("gl-access-1")?);
    assert_eq!(grant.scopes, ["read_api", "read_user"]);
    assert!(!grant.can_commit(), "read_api cannot commit");
    assert!(grant.expired(1_700_000_000 + 7200) && !grant.expired(1_700_000_000));
    Ok(())
}

#[test]
fn an_expired_token_is_renewed_with_the_refresh_token() -> Result<(), Error> {
    let grant = signed_in("api read_user")?;
    let clients = Clients { gitlab: "client-1".into(), github: "gh".into() };

    let idle = Script::new(&[]);
    let (token, renewed) = block_on(oauth::fresh(&idle, &clients, &grant, 1_700_000_000))?;
    assert!(renewed.is_none() && idle.requests().is_empty(), "a token that has not expired is sent as it is");
    assert_eq!(token, Token::new("gl-access-1")?);

    let script = Script::new(&[(
        200,
        json!({"access_token": "gl-access-2", "refresh_token": "gl-refresh-2", "expires_in": 7200, "created_at": 1_800_000_000}),
    )]);
    let (token, renewed) = block_on(oauth::fresh(&script, &clients, &grant, 1_800_000_000))?;
    let form = script.form(0);
    assert_eq!(field(&form, "grant_type"), Some("refresh_token"));
    assert_eq!(field(&form, "refresh_token"), Some("gl-refresh-1"));
    assert_eq!(field(&form, "client_id"), Some("client-1"));
    assert_eq!(token, Token::new("gl-access-2")?);
    let renewed = renewed.ok_or_else(|| Error::Invalid("not renewed".into()))?;
    assert!(renewed.can_commit(), "the scopes stay when the answer does not repeat them");
    assert!(!renewed.expired(1_800_000_000));

    // The next renewal sends the new refresh token: the old one no longer works.
    let next = Script::new(&[(200, tokens("gl-access-3", "gl-refresh-3", "api"))]);
    block_on(oauth::refresh(&next, "client-1", &renewed))?;
    assert_eq!(field(&next.form(0), "refresh_token"), Some("gl-refresh-2"));
    Ok(())
}

#[test]
fn a_refused_refresh_says_to_sign_in_again() -> Result<(), Error> {
    let grant = signed_in("read_api")?;
    let script = Script::new(&[(400, json!({"error": "invalid_grant", "error_description": "The provided authorization grant is invalid"}))]);
    let Err(Error::SignIn(why)) = block_on(oauth::refresh(&script, "client-1", &grant)) else { panic!("a refused refresh is a sign-in error") };
    assert!(why.contains("sign in again"), "{why}");
    Ok(())
}

#[test]
fn signing_out_revokes_the_token_on_gitlab_only() -> Result<(), Error> {
    let grant = signed_in("read_api")?;
    let script = Script::new(&[(200, json!({}))]);
    block_on(oauth::revoke(&script, "client-1", &grant))?;
    assert_eq!(script.requests()[0].url, "https://gitlab.com/oauth/revoke");
    assert_eq!(field(&script.form(0), "token"), Some("gl-access-1"));

    // GitHub revokes only for the application's server: said, and nothing sent.
    let device = Script::new(&[(200, json!({"access_token": "gho_x", "scope": "repo"}))]);
    let code = DeviceCodeFixture::code()?;
    let DevicePoll::Granted(github) = block_on(oauth::poll_device(&device, "gh", &code))? else { panic!("granted") };
    let none = Script::new(&[]);
    let Err(Error::SignIn(why)) = block_on(oauth::revoke(&none, "gh", &github)) else { panic!("GitHub cannot be revoked from here") };
    assert!(why.contains("Settings"), "{why}");
    assert!(none.requests().is_empty());
    Ok(())
}

#[test]
fn who_is_signed_in_is_read_from_the_forge() -> Result<(), Error> {
    let grant = signed_in("read_api read_user")?;
    let script = Script::new(&[(200, json!({"name": "Ada Lovelace", "username": "ada"}))]);
    let grant = block_on(oauth::with_user(&script, grant))?;
    let sent = &script.requests()[0];
    assert_eq!(sent.url, "https://gitlab.com/api/v4/user");
    assert!(sent.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer gl-access-1"));
    assert_eq!(grant.user.as_ref().map(|u| (u.name.as_str(), u.username.as_str())), Some(("Ada Lovelace", "ada")));

    let device = Script::new(&[(200, json!({"access_token": "gho_x", "scope": "repo"})), (200, json!({"name": null, "login": "octo"}))]);
    let DevicePoll::Granted(github) = block_on(oauth::poll_device(&device, "gh", &DeviceCodeFixture::code()?))? else { panic!("granted") };
    let github = block_on(oauth::with_user(&device, *github))?;
    assert_eq!(device.requests()[1].url, "https://api.github.com/user");
    assert_eq!(github.user.map(|u| u.name), Some("octo".to_owned()), "no name: the login");
    Ok(())
}

/// A device code from a scripted GitHub.
struct DeviceCodeFixture;

impl DeviceCodeFixture {
    fn code() -> Result<oauth::DeviceCode, Error> {
        let script = Script::new(&[(
            200,
            json!({"device_code": "dev-123", "user_code": "WDJB-MJHT", "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 5}),
        )]);
        let code = block_on(oauth::device_code(&script, "gh-client", oauth::GITHUB_SCOPE))?;
        let form = script.form(0);
        assert_eq!(script.requests()[0].url, format!("{GITHUB_COM}/login/device/code"));
        assert_eq!((field(&form, "client_id"), field(&form, "scope")), (Some("gh-client"), Some("repo")));
        Ok(code)
    }
}

/// Runs the device flow against `answers`; the waits it made, and its outcome.
/// The waits a device flow made, its outcome, and what it sent.
type Flow = (Vec<u64>, Result<Grant, Error>, Script);

fn device_flow(answers: &[(u16, Value)], stop: bool) -> Result<Flow, Error> {
    let code = DeviceCodeFixture::code()?;
    let script = Script::new(answers);
    let waits = Mutex::new(Vec::new());
    let stop = AtomicBool::new(stop);
    let sleep = |s: u64| {
        if let Ok(mut w) = waits.lock() {
            w.push(s);
        }
        std::future::ready(())
    };
    let outcome = block_on(oauth::device_flow(&script, "gh-client", &code, sleep, &stop));
    Ok((waits.into_inner().unwrap_or_default(), outcome, script))
}

#[test]
fn the_device_flow_polls_until_the_code_is_entered_and_slows_down_when_asked() -> Result<(), Error> {
    let (waits, outcome, script) = device_flow(
        &[
            (200, json!({"error": "authorization_pending"})),
            (200, json!({"error": "slow_down", "interval": 10})),
            (200, json!({"error": "authorization_pending"})),
            (200, json!({"access_token": "gho_secret", "token_type": "bearer", "scope": "repo"})),
        ],
        false,
    )?;
    let grant = outcome?;
    assert_eq!(grant.service, Service::GitHub);
    assert!(grant.can_commit());
    assert!(waits[0] >= 5, "never faster than the interval GitHub gave");
    assert!(waits[2] > waits[0] && waits[2] == waits[3], "slower from slow_down on, and staying so: {waits:?}");
    let form = script.form(0);
    assert_eq!(field(&form, "grant_type"), Some("urn:ietf:params:oauth:grant-type:device_code"));
    assert_eq!(field(&form, "device_code"), Some("dev-123"));
    Ok(())
}

#[test]
fn the_device_flow_ends_on_an_expired_code_a_refusal_or_a_cancel() -> Result<(), Error> {
    for (answer, words) in [("expired_token", "expired"), ("access_denied", "refused")] {
        let (_, outcome, _) = device_flow(&[(200, json!({"error": answer}))], false)?;
        match outcome {
            Err(Error::SignIn(why)) => assert!(why.contains(words), "{why}"),
            other => panic!("{answer}: {other:?}"),
        }
    }
    let (_, outcome, script) = device_flow(&[], true)?;
    assert!(matches!(outcome, Err(Error::SignIn(why)) if why.contains("cancelled")));
    assert!(script.requests().is_empty(), "a cancelled flow polls no more");
    // A code nobody enters: polling stops when it has expired.
    let pending = vec![(200, json!({"error": "authorization_pending"})); 400];
    let (waits, outcome, _) = device_flow(&pending, false)?;
    assert!(matches!(outcome, Err(Error::SignIn(_))));
    assert!(waits.iter().sum::<u64>() <= 900 + 5, "no polling past the code's life");
    Ok(())
}

#[test]
fn placeholder_client_ids_turn_signing_in_off() {
    let clients = Clients { gitlab: "TEXCRAFT_GITLAB_CLIENT_ID_PENDING".into(), github: "TEXCRAFT_GITHUB_CLIENT_ID_PENDING".into() };
    for service in [Service::GitLab, Service::GitHub] {
        assert!(!clients.has(service), "this build is not registered yet");
        let Err(Error::SignIn(why)) = clients.ready(service) else { panic!("refused") };
        assert!(why.contains("no") && why.contains(service.name()), "{why}");
    }
    let real = Clients { gitlab: "abc123".into(), github: "Iv1.def".into() };
    assert!(real.has(Service::GitLab) && real.ready(Service::GitHub).is_ok());
}

#[test]
fn tokens_never_print_and_are_written_out_only_for_the_store() -> Result<(), Error> {
    let grant = signed_in("api")?;
    let shown = format!("{grant:?} {:?} {:?}", Pkce::from_bytes(&[9; 32]), DeviceCodeFixture::code()?);
    for secret in ["gl-access-1", "gl-refresh-1", "dev-123", Pkce::from_bytes(&[9; 32]).verifier()] {
        assert!(!shown.contains(secret), "{secret} showed: {shown}");
    }
    let kept = grant.stored();
    assert!(kept.contains("gl-refresh-1"), "the store keeps the refresh token");
    assert_eq!(Grant::from_stored(&kept)?, grant);
    assert!(Grant::from_stored("{not json").is_err());
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
mod loopback {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use texcraft_forge::oauth::Loopback;
    use texcraft_forge::{Error, Token};

    /// Sends `GET target` to `port` as a browser would, and reads the answer.
    fn get(port: u16, target: &str) -> String {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else { return String::new() };
        let _ = write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/html\r\n\r\n");
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    }

    fn port(l: &Loopback) -> u16 {
        l.redirect_uri().trim_start_matches("http://127.0.0.1:").trim_end_matches("/callback").parse().unwrap_or_default()
    }

    #[test]
    fn the_redirect_brings_the_code_and_other_requests_are_ignored() -> Result<(), Error> {
        let listener = Loopback::bind("127.0.0.1:0")?;
        let port = port(&listener);
        assert!(listener.redirect_uri().starts_with("http://127.0.0.1:") && listener.redirect_uri().ends_with("/callback"));
        let browser = std::thread::spawn(move || {
            let favicon = get(port, "/favicon.ico");
            let forged = get(port, "/callback?code=evil&state=other");
            let real = get(port, "/callback?code=the%2Dcode&state=s1");
            (favicon, forged, real)
        });
        let code = listener.wait("s1", Duration::from_secs(10), &AtomicBool::new(false))?;
        let (favicon, forged, real) = browser.join().unwrap_or_default();
        assert_eq!(code, Token::new("the-code")?, "the code of the request with our state");
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(forged.contains("not the sign-in"), "{forged}");
        assert!(real.contains("You can close this tab"), "{real}");
        Ok(())
    }

    #[test]
    fn a_refusal_a_timeout_and_a_cancel_end_the_wait() -> Result<(), Error> {
        let listener = Loopback::bind("127.0.0.1:0")?;
        let port = port(&listener);
        let browser = std::thread::spawn(move || get(port, "/callback?error=access_denied&error_description=%3Cb%3Eno%3C%2Fb%3E&state=s2"));
        let refused = listener.wait("s2", Duration::from_secs(10), &AtomicBool::new(false));
        let page = browser.join().unwrap_or_default();
        assert!(matches!(&refused, Err(Error::SignIn(why)) if why.contains("<b>no</b>")), "{refused:?}");
        assert!(!page.contains("<b>no</b>") && page.contains("&lt;b&gt;"), "the reason is shown as text: {page}");

        assert!(listener.wait("s2", Duration::from_millis(100), &AtomicBool::new(false)).is_err());
        assert!(matches!(listener.wait("s2", Duration::from_secs(10), &AtomicBool::new(true)), Err(Error::SignIn(why)) if why.contains("cancelled")));
        Ok(())
    }

    #[test]
    fn a_port_in_use_is_said() -> Result<(), Error> {
        let first = Loopback::bind("127.0.0.1:0")?;
        let again = Loopback::bind(&format!("127.0.0.1:{}", port(&first)));
        assert!(matches!(again, Err(Error::SignIn(why)) if why.contains("personal access token")));
        Ok(())
    }
}
