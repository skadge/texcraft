//! Signing in to a forge (docs/forges.md, step 4) instead of pasting a personal access token.
//!
//! - **GitLab**: OAuth 2's authorization code with PKCE (RFC 7636), as a non-confidential
//!   application (no client secret): the user is sent to [`authorize_url`], GitLab sends them back
//!   to the redirect URI with a code, and [`exchange`] trades it for an access token and a refresh
//!   token. The access token lasts two hours; [`fresh`] renews it with the refresh token when it
//!   has expired. Signing in asks for [`GITLAB_READ`] (`read_api read_user`); the first commit asks
//!   again for [`GITLAB_WRITE`] (`api`), which is broad, so it is asked for only when needed.
//!   [`revoke`] ends the grant.
//! - **GitHub**: the device flow (RFC 8628), desktop only: [`device_code`] gives a code the user
//!   enters on github.com, and [`device_flow`] polls until they have. GitHub's token endpoint does
//!   not answer web pages (no CORS), so the browser keeps the personal access token.
//!
//! Each build is registered as an application with each forge; the client ids are
//! [`GITLAB_CLIENT_ID`] and [`GITHUB_CLIENT_ID`]. A build whose id is still a placeholder cannot
//! sign in ([`Clients::ready`]), and says so.
//!
//! Tokens are [`Token`]s, which never print. A [`Grant`] (the tokens, their scopes and who they
//! are for) is written out only by [`Grant::stored`], for the secret store that keeps it.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::Digest as _;

use crate::{Error, GITHUB_COM, GITLAB_COM, Request, Response, Result, Service, Token, Transport, encode};

/// TeXCraft's application on gitlab.com: its client id (public; a non-confidential application
/// with PKCE has no secret). Its redirect URIs: `https://texcraft-893d51.gitlab.io/`,
/// `http://127.0.0.1:8768/` (the web build served locally) and `http://127.0.0.1:47321/callback`
/// (the desktop); its scopes `api`, `read_api` and `read_user`. A build without an application
/// uses an id ending in `_PENDING`, which turns signing in off.
pub const GITLAB_CLIENT_ID: &str = "ac60481471d63967c2fb87244d532e67b1d11914be011de848c50d5cabc1f9b8";
/// TeXCraft's OAuth application on GitHub, with the device flow enabled: its client id (public;
/// the device flow needs no secret). Its callback URL, unused by the device flow, is
/// `http://127.0.0.1:47321/callback`.
pub const GITHUB_CLIENT_ID: &str = "Ov23lizhMlYj5u42jscZ";

/// What a placeholder client id ends with.
pub const PLACEHOLDER: &str = "_PENDING";

/// Why signing out of GitHub cannot revoke the token.
pub const GITHUB_REVOKE: &str = "GitHub lets only the application's server revoke its tokens, and TeXCraft has none: remove TeXCraft's access on GitHub, under Settings › Applications › Authorized OAuth Apps.";

/// Where the desktop listens for GitLab's redirect: its redirect URI is
/// `http://127.0.0.1:47321/callback`, as registered.
pub const LOOPBACK: &str = "127.0.0.1:47321";

/// The scopes signing in to GitLab asks for: reading projects, and who the user is.
pub const GITLAB_READ: &[&str] = &["read_api", "read_user"];
/// The scopes asked for at the first commit: `api` (GitLab's `write_repository` is for git over
/// HTTP only), and who the user is.
pub const GITLAB_WRITE: &[&str] = &["api", "read_user"];
/// The scope the GitHub device flow asks for: repositories, to read and commit.
pub const GITHUB_SCOPE: &str = "repo";

/// GitHub's API, where the signed-in user is read.
const GITHUB_API: &str = "https://api.github.com";

/// Largest answer of a token endpoint or of `/user`.
const MAX_JSON: u64 = 1024 * 1024;

/// A token is renewed this many seconds before it expires.
const MARGIN: i64 = 60;

/// The applications this build signs in as, by client id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clients {
    pub gitlab: String,
    pub github: String,
}

impl Default for Clients {
    fn default() -> Self {
        Self { gitlab: GITLAB_CLIENT_ID.to_owned(), github: GITHUB_CLIENT_ID.to_owned() }
    }
}

impl Clients {
    /// `service`'s client id, whether or not it is ready.
    pub fn id(&self, service: Service) -> &str {
        match service {
            Service::GitLab => &self.gitlab,
            Service::GitHub => &self.github,
        }
    }

    /// Whether this build has an application on `service`: its client id is not the placeholder.
    pub fn has(&self, service: Service) -> bool {
        let id = self.id(service).trim();
        !id.is_empty() && !id.ends_with(PLACEHOLDER)
    }

    /// `service`'s client id, or why signing in to it cannot work in this build.
    pub fn ready(&self, service: Service) -> Result<&str> {
        if self.has(service) {
            Ok(self.id(service).trim())
        } else {
            Err(Error::SignIn(format!("This build has no {} application yet: use a personal access token instead.", service.name())))
        }
    }
}

/// The instance signing in reaches for `service`: gitlab.com or github.com (another instance
/// needs an application of its own, so it keeps the personal access token).
pub fn instance(service: Service) -> &'static str {
    match service {
        Service::GitLab => GITLAB_COM,
        Service::GitHub => GITHUB_COM,
    }
}

/// `bytes` as base64url without padding (RFC 7636's encoding).
fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `n` random bytes from the system.
fn random(n: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| Error::SignIn(format!("This computer gave no random numbers to sign in with: {e}")))?;
    Ok(bytes)
}

/// A PKCE pair (RFC 7636): the verifier kept here, and the challenge sent with the authorization
/// request (S256: the verifier's SHA-256, in base64url).
#[derive(Clone, PartialEq, Eq)]
pub struct Pkce {
    verifier: Token,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pkce(…)")
    }
}

impl Pkce {
    /// A new pair from 32 random bytes (a 43-character verifier).
    pub fn new() -> Result<Self> {
        Ok(Self::from_bytes(&random(32)?))
    }

    /// The pair whose verifier is `bytes` in base64url.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self { verifier: Token(base64url(bytes)) }
    }

    /// The pair of a verifier kept from before (the browser's round trip).
    pub fn from_verifier(verifier: &str) -> Result<Self> {
        let ok =
            (43..=128).contains(&verifier.len()) && verifier.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
        if !ok {
            return Err(Error::SignIn("The sign-in kept by this page is damaged: sign in again.".to_owned()));
        }
        Ok(Self { verifier: Token(verifier.to_owned()) })
    }

    /// The verifier, to keep until the code comes back (never to send anywhere but the token
    /// endpoint).
    pub fn verifier(&self) -> &str {
        &self.verifier.0
    }

    /// The challenge: `BASE64URL(SHA256(verifier))`.
    pub fn challenge(&self) -> String {
        base64url(&sha2::Sha256::digest(self.verifier.0.as_bytes()))
    }
}

/// A random `state` for an authorization request, which the redirect must bring back.
pub fn new_state() -> Result<String> {
    Ok(base64url(&random(16)?))
}

/// The page on `instance` where the user lets TeXCraft in (`/oauth/authorize`): the code comes
/// back to `redirect` with `state`.
pub fn authorize_url(instance: &str, client_id: &str, redirect: &str, scopes: &[&str], state: &str, pkce: &Pkce) -> String {
    format!(
        "{instance}/oauth/authorize?{}",
        query(&[
            ("client_id", client_id),
            ("redirect_uri", redirect),
            ("response_type", "code"),
            ("state", state),
            ("scope", &scopes.join(" ")),
            ("code_challenge", &pkce.challenge()),
            ("code_challenge_method", "S256"),
        ])
    )
}

/// `pairs` as a query string or a form's body, each part percent-encoded.
fn query(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

/// Reads a query string (`a=1&b=x%20y`, with or without its `?`) into its pairs, decoded.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    let query = query.strip_prefix('?').unwrap_or(query);
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// `s` with `+` as a space and `%XX` as its byte (an invalid escape is kept as it is).
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Who a grant signs in as.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct User {
    /// Their full name (their username when they gave none).
    pub name: String,
    /// Their username on the forge.
    pub username: String,
}

/// What signing in gives: the access token, the refresh token when the forge gives one, when the
/// access token expires, its scopes, and who it is for. Kept only in a secret store
/// ([`Grant::stored`]); it never prints its tokens.
#[derive(Clone, PartialEq, Eq)]
pub struct Grant {
    pub service: Service,
    /// The instance it is for: `https://gitlab.com`, `https://github.com`.
    pub instance: String,
    access: Token,
    refresh: Option<Token>,
    /// When the access token expires, in seconds since 1970 (none: when it is revoked).
    pub expires_at: Option<i64>,
    pub scopes: Vec<String>,
    pub user: Option<User>,
    /// The redirect URI and the PKCE verifier it was got with, which GitLab's refresh asks for.
    redirect: String,
    verifier: Option<Token>,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("service", &self.service)
            .field("instance", &self.instance)
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

/// A grant as its secret store keeps it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    service: Service,
    instance: String,
    access: String,
    #[serde(default)]
    refresh: Option<String>,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    user: Option<User>,
    #[serde(default)]
    redirect: String,
    #[serde(default)]
    verifier: Option<String>,
}

impl Grant {
    /// The access token sent with each request.
    pub fn access(&self) -> &Token {
        &self.access
    }

    /// Whether the token can commit: GitLab's `api` scope; on GitHub, `repo` (a token of a GitHub
    /// App has no scopes, and its permissions are the app's).
    pub fn can_commit(&self) -> bool {
        match self.service {
            Service::GitLab => self.scopes.iter().any(|s| s == "api"),
            Service::GitHub => self.scopes.is_empty() || self.scopes.iter().any(|s| s == "repo" || s == "public_repo"),
        }
    }

    /// Whether the access token has expired by `now` (seconds since 1970), or is about to.
    pub fn expired(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|at| now + MARGIN >= at)
    }

    /// The grant, tokens included, as JSON: for the secret store that keeps it, and nothing else.
    pub fn stored(&self) -> String {
        let stored = Stored {
            service: self.service,
            instance: self.instance.clone(),
            access: self.access.0.clone(),
            refresh: self.refresh.as_ref().map(|t| t.0.clone()),
            expires_at: self.expires_at,
            scopes: self.scopes.clone(),
            user: self.user.clone(),
            redirect: self.redirect.clone(),
            verifier: self.verifier.as_ref().map(|t| t.0.clone()),
        };
        serde_json::to_string(&stored).unwrap_or_default()
    }

    /// A grant [`Grant::stored`] wrote.
    pub fn from_stored(json: &str) -> Result<Self> {
        let s: Stored = serde_json::from_str(json).map_err(|e| Error::SignIn(format!("The kept sign-in cannot be read ({e}): sign in again.")))?;
        Ok(Self {
            service: s.service,
            instance: s.instance,
            access: Token::new(&s.access)?,
            refresh: s.refresh.as_deref().map(Token::new).transpose()?,
            expires_at: s.expires_at,
            scopes: s.scopes,
            user: s.user,
            redirect: s.redirect,
            verifier: s.verifier.as_deref().map(Token::new).transpose()?,
        })
    }

    /// A grant of a token answer from `service`'s token endpoint (`access_token`, `refresh_token`,
    /// `expires_in`, `created_at`, `scope`), at `now` when it does not say when it was made.
    fn of(service: Service, instance: &str, v: &Value, redirect: &str, verifier: Option<&Pkce>, now: i64) -> Result<Self> {
        let access = v["access_token"].as_str().ok_or_else(|| Error::Invalid("the token endpoint gave no access token".to_owned()))?;
        let made = v["created_at"].as_i64().unwrap_or(now);
        Ok(Self {
            service,
            instance: instance.to_owned(),
            access: Token::new(access)?,
            refresh: v["refresh_token"].as_str().filter(|t| !t.is_empty()).map(Token::new).transpose()?,
            expires_at: v["expires_in"].as_i64().map(|s| made + s),
            // GitLab separates scopes with spaces, GitHub with commas.
            scopes: v["scope"].as_str().unwrap_or_default().split([' ', ',']).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            user: None,
            redirect: redirect.to_owned(),
            verifier: verifier.map(|p| p.verifier.clone()),
        })
    }
}

/// Seconds since 1970.
pub fn now() -> i64 {
    crate::http::now_secs().and_then(|s| i64::try_from(s).ok()).unwrap_or_default()
}

/// A form POST to a token endpoint, answered in JSON.
fn form_post(url: String, pairs: &[(&str, &str)]) -> Request {
    Request {
        method: "POST",
        url,
        headers: vec![
            ("Content-Type".to_owned(), "application/x-www-form-urlencoded".to_owned()),
            ("Accept".to_owned(), "application/json".to_owned()),
        ],
        body: Some(query(pairs).into_bytes()),
        max_body: MAX_JSON,
    }
}

/// Sends a request to `service`'s OAuth endpoints and reads its JSON answer. OAuth's refusals
/// (`{"error": "invalid_grant", "error_description": …}`, a 400 or 401) are put in words.
async fn oauth_json(transport: &dyn Transport, service: Service, request: Request) -> Result<Value> {
    let response: Response = transport.send(request).await.map_err(|why| Error::Network { forge: service, why })?;
    let v: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    match response.status {
        200..=299 if !response.too_large => Ok(v),
        200..=299 => Err(Error::Invalid("the answer is too large".to_owned())),
        400 | 401 if v["error"].is_string() => Err(refusal(service, &v)),
        _ => crate::http::check(service, response, "signing in").map(|_| v),
    }
}

/// An OAuth error answer, in words.
fn refusal(service: Service, v: &Value) -> Error {
    let code = v["error"].as_str().unwrap_or_default();
    let why = v["error_description"].as_str().filter(|d| !d.is_empty()).unwrap_or(code);
    match code {
        "invalid_grant" => Error::SignIn(format!("{} did not accept the sign-in ({why}): sign in again.", service.name())),
        _ => Error::SignIn(format!("{} refused the sign-in: {why}", service.name())),
    }
}

/// Trades the `code` GitLab sent back to `redirect` for a grant (`grant_type=authorization_code`,
/// with the PKCE verifier and no client secret).
pub async fn exchange(transport: &dyn Transport, instance: &str, client_id: &str, code: &Token, redirect: &str, pkce: &Pkce) -> Result<Grant> {
    let request = form_post(
        format!("{instance}/oauth/token"),
        &[
            ("client_id", client_id),
            ("code", code.0.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect),
            ("code_verifier", pkce.verifier()),
        ],
    );
    let v = oauth_json(transport, Service::GitLab, request).await?;
    Grant::of(Service::GitLab, instance, &v, redirect, Some(pkce), now())
}

/// A new access token for `grant`, from its refresh token (`grant_type=refresh_token`). GitLab
/// gives a new refresh token too, and the old one stops working: keep the grant this returns.
pub async fn refresh(transport: &dyn Transport, client_id: &str, grant: &Grant) -> Result<Grant> {
    let name = grant.service.name();
    let Some(token) = &grant.refresh else {
        return Err(Error::SignIn(format!("The sign-in to {name} has expired: sign in again.")));
    };
    if grant.service == Service::GitHub {
        // Renewing a GitHub App's token needs its secret, which an application on a user's
        // computer cannot keep.
        return Err(Error::SignIn("The sign-in to GitHub has expired: sign in again.".to_owned()));
    }
    let mut pairs = vec![
        ("client_id", client_id),
        ("refresh_token", token.0.as_str()),
        ("grant_type", "refresh_token"),
        ("redirect_uri", grant.redirect.as_str()),
    ];
    if let Some(verifier) = &grant.verifier {
        pairs.push(("code_verifier", verifier.0.as_str()));
    }
    let request = form_post(format!("{}/oauth/token", grant.instance), &pairs);
    let v = oauth_json(transport, grant.service, request).await?;
    let mut renewed = Grant::of(grant.service, &grant.instance, &v, &grant.redirect, None, now())?;
    renewed.verifier = grant.verifier.clone();
    renewed.user = grant.user.clone();
    if renewed.refresh.is_none() {
        renewed.refresh = grant.refresh.clone();
    }
    if renewed.scopes.is_empty() {
        renewed.scopes = grant.scopes.clone();
    }
    Ok(renewed)
}

/// `grant`'s access token, renewed first when it has expired by `now`: the token, and the renewed
/// grant to keep in its place (its refresh token replaces the old one).
pub async fn fresh(transport: &dyn Transport, clients: &Clients, grant: &Grant, now: i64) -> Result<(Token, Option<Grant>)> {
    if !grant.expired(now) {
        return Ok((grant.access.clone(), None));
    }
    let renewed = refresh(transport, clients.id(grant.service), grant).await?;
    Ok((renewed.access.clone(), Some(renewed)))
}

/// Revokes `grant` on its forge (`/oauth/revoke`; GitLab revokes the refresh token with it).
/// GitHub lets only an application with its secret revoke a token: that is refused in words, and
/// the user removes TeXCraft on GitHub.
pub async fn revoke(transport: &dyn Transport, client_id: &str, grant: &Grant) -> Result<()> {
    if grant.service == Service::GitHub {
        return Err(Error::SignIn(GITHUB_REVOKE.to_owned()));
    }
    let request = form_post(format!("{}/oauth/revoke", grant.instance), &[("client_id", client_id), ("token", grant.access.0.as_str())]);
    oauth_json(transport, grant.service, request).await.map(|_| ())
}

/// Who `grant` signs in as: GitLab's `/api/v4/user`, GitHub's `/user`.
pub async fn user(transport: &dyn Transport, grant: &Grant) -> Result<User> {
    let (url, mut headers) = match grant.service {
        Service::GitLab => (format!("{}/api/v4/user", grant.instance), vec![("Accept".to_owned(), "application/json".to_owned())]),
        Service::GitHub => (
            format!("{GITHUB_API}/user"),
            vec![("Accept".to_owned(), "application/vnd.github+json".to_owned()), ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned())],
        ),
    };
    headers.push(("Authorization".to_owned(), grant.access.header()));
    let request = Request { method: "GET", url, headers, body: None, max_body: MAX_JSON };
    let response = transport.send(request).await.map_err(|why| Error::Network { forge: grant.service, why })?;
    let response = crate::http::check(grant.service, response, "who is signed in")?;
    let v: Value = serde_json::from_slice(&response.body).map_err(|e| Error::Invalid(e.to_string()))?;
    let username = v["username"].as_str().or_else(|| v["login"].as_str()).ok_or_else(|| Error::Invalid("the user has no username".to_owned()))?;
    let name = v["name"].as_str().filter(|n| !n.trim().is_empty()).unwrap_or(username);
    Ok(User { name: name.to_owned(), username: username.to_owned() })
}

/// The grant with who it is for.
pub async fn with_user(transport: &dyn Transport, mut grant: Grant) -> Result<Grant> {
    grant.user = Some(user(transport, &grant).await?);
    Ok(grant)
}

/// A device code (GitHub's device flow): the code the user enters at `verification_uri`.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceCode {
    device_code: Token,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds until the codes expire.
    pub expires_in: u64,
    /// Seconds to wait between polls.
    pub interval: u64,
}

impl std::fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode").field("user_code", &self.user_code).field("verification_uri", &self.verification_uri).finish_non_exhaustive()
    }
}

/// Asks GitHub for a device code (`POST https://github.com/login/device/code`).
pub async fn device_code(transport: &dyn Transport, client_id: &str, scope: &str) -> Result<DeviceCode> {
    let request = form_post(format!("{GITHUB_COM}/login/device/code"), &[("client_id", client_id), ("scope", scope)]);
    let v = oauth_json(transport, Service::GitHub, request).await?;
    if v["error"].is_string() {
        return Err(refusal(Service::GitHub, &v));
    }
    let text = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).ok_or_else(|| Error::Invalid(format!("GitHub's device code has no {k}")));
    let verification_uri = text("verification_uri")?;
    if !verification_uri.starts_with("https://") {
        return Err(Error::Invalid("GitHub's verification page is not on HTTPS".to_owned()));
    }
    Ok(DeviceCode {
        device_code: Token::new(text("device_code")?)?,
        user_code: text("user_code")?.to_owned(),
        verification_uri: verification_uri.to_owned(),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
        interval: v["interval"].as_u64().unwrap_or(5).max(1),
    })
}

/// What one poll of the device flow says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    /// The user has not entered the code yet.
    Pending,
    /// Polling too fast: wait this many seconds between polls from now on.
    SlowDown(u64),
    /// The user let TeXCraft in.
    Granted(Box<Grant>),
}

/// Asks GitHub once whether the user has entered the code
/// (`grant_type=urn:ietf:params:oauth:grant-type:device_code`). An expired code and a refusal are
/// errors, in words.
pub async fn poll_device(transport: &dyn Transport, client_id: &str, device: &DeviceCode) -> Result<DevicePoll> {
    let request = form_post(
        format!("{GITHUB_COM}/login/oauth/access_token"),
        &[("client_id", client_id), ("device_code", device.device_code.0.as_str()), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
    );
    let v = oauth_json(transport, Service::GitHub, request).await?;
    match v["error"].as_str() {
        None => Ok(DevicePoll::Granted(Box::new(Grant::of(Service::GitHub, GITHUB_COM, &v, "", None, now())?))),
        Some("authorization_pending") => Ok(DevicePoll::Pending),
        Some("slow_down") => Ok(DevicePoll::SlowDown(v["interval"].as_u64().unwrap_or(device.interval + 5))),
        Some("expired_token") => Err(Error::SignIn("The code expired before it was entered on GitHub: sign in again.".to_owned())),
        Some("access_denied") => Err(Error::SignIn("Signing in to GitHub was refused on GitHub's page.".to_owned())),
        Some(_) => Err(refusal(Service::GitHub, &v)),
    }
}

/// Why a sign-in stopped before it ended.
fn cancelled() -> Error {
    Error::SignIn("Signing in was cancelled.".to_owned())
}

/// Polls GitHub every `interval` (slower when it says so) until the user has entered the code,
/// refused, or the code expired, or until `stop` is set. `sleep(seconds)` waits.
pub async fn device_flow<S, F>(transport: &dyn Transport, client_id: &str, device: &DeviceCode, sleep: S, stop: &AtomicBool) -> Result<Grant>
where
    S: Fn(u64) -> F,
    F: Future<Output = ()>,
{
    let mut interval = device.interval.max(1);
    let mut waited = 0;
    loop {
        if waited >= device.expires_in {
            return Err(Error::SignIn("The code expired before it was entered on GitHub: sign in again.".to_owned()));
        }
        sleep(interval).await;
        waited += interval;
        if stop.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        match poll_device(transport, client_id, device).await? {
            DevicePoll::Pending => {}
            DevicePoll::SlowDown(n) => interval = n.max(interval + 5),
            DevicePoll::Granted(grant) => return Ok(*grant),
        }
    }
}

/// The JSON of a pending GitLab sign-in (the browser keeps it across the round trip): its state,
/// verifier, redirect URI and whether it asks to commit.
pub fn pending_json(state: &str, pkce: &Pkce, redirect: &str, write: bool) -> String {
    json!({"state": state, "verifier": pkce.verifier(), "redirect": redirect, "write": write}).to_string()
}

#[cfg(not(target_arch = "wasm32"))]
pub use loopback::Loopback;

#[cfg(not(target_arch = "wasm32"))]
mod loopback {
    //! The desktop's end of GitLab's redirect: a listener on the loopback address that takes the
    //! one request the browser makes to the redirect URI, reads the code from it, and answers with
    //! a page that says the user can go back to TeXCraft (RFC 8252, loopback redirection).

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::{cancelled, parse_query};
    use crate::{Error, Result, Token};

    /// Listening for the redirect.
    pub struct Loopback {
        listener: TcpListener,
        port: u16,
    }

    impl Loopback {
        /// Listens on `addr` (`127.0.0.1:47321`; port 0 for any, in tests).
        pub fn bind(addr: &str) -> Result<Self> {
            let listener = TcpListener::bind(addr).map_err(|e| {
                Error::SignIn(format!(
                    "TeXCraft cannot wait for GitLab's answer on {addr} ({e}): another program, or another TeXCraft signing in, uses it. Try again, or use a personal access token."
                ))
            })?;
            let port = listener.local_addr().map_err(|e| Error::SignIn(e.to_string()))?.port();
            listener.set_nonblocking(true).map_err(|e| Error::SignIn(e.to_string()))?;
            Ok(Self { listener, port })
        }

        /// The redirect URI the forge sends the browser back to: `http://127.0.0.1:<port>/callback`.
        pub fn redirect_uri(&self) -> String {
            format!("http://127.0.0.1:{}/callback", self.port)
        }

        /// Waits for the browser's request to `/callback` with `state`, at most `timeout` or until
        /// `stop` is set, and gives its code. Requests to other paths, or with another state, are
        /// answered and ignored; a redirect that says the user refused is an error.
        pub fn wait(&self, state: &str, timeout: Duration, stop: &AtomicBool) -> Result<Token> {
            let end = Instant::now() + timeout;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                if Instant::now() >= end {
                    return Err(Error::SignIn("Signing in took too long: the browser did not come back from GitLab. Sign in again.".to_owned()));
                }
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        if let Some(outcome) = answer(stream, state) {
                            return outcome;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
                    Err(e) => return Err(Error::SignIn(format!("TeXCraft stopped waiting for GitLab's answer: {e}"))),
                }
            }
        }
    }

    /// The page shown in the browser.
    fn page(title: &str, text: &str) -> String {
        let body = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title></head><body style=\"font-family:sans-serif;max-width:32em;margin:4em auto\"><h1>{title}</h1><p>{text}</p></body></html>"
        );
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
            body.len()
        )
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    /// Reads one request and answers it: the code (or the refusal) when it is the redirect, none
    /// when it is another request.
    fn answer(mut stream: TcpStream, state: &str) -> Option<Result<Token>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while head.len() < 16 * 1024 && !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
        let head = String::from_utf8_lossy(&head);
        let target = head.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or_default();
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if path != "/callback" {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return None;
        }
        let pairs = parse_query(query);
        let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        if get("state") != Some(state) {
            let reply = page("TeXCraft", "This page is not the sign-in TeXCraft is waiting for. You can close it.");
            let _ = stream.write_all(reply.as_bytes());
            return None;
        }
        let outcome = match (get("code"), get("error")) {
            (Some(code), None) if !code.is_empty() => Token::new(code),
            (_, error) => {
                let why = get("error_description").or(error).unwrap_or("no code came back");
                Err(Error::SignIn(format!("GitLab did not let TeXCraft in: {why}")))
            }
        };
        let reply = match &outcome {
            Ok(_) => page("Signed in", "TeXCraft is signed in to GitLab. You can close this tab and go back to TeXCraft."),
            Err(e) => page("Not signed in", &format!("{}. You can close this tab.", escape(&e.to_string()))),
        };
        let _ = stream.write_all(reply.as_bytes());
        Some(outcome)
    }
}
