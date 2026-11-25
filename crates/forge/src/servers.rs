//! Self-managed servers (docs/forges.md, "Self-managed servers"): a GitLab of a company or a
//! university, or a GitHub Enterprise Server, on which the user (or its administrator) registers
//! TeXCraft as an application, so that signing in works there as on gitlab.com and github.com.
//!
//! - [`parse_instance`] reads the server's address as a user types it.
//! - [`probe`] checks that it is reachable and that it is the service said: GitLab's
//!   `/api/v4/version`, GitHub Enterprise Server's `/api/v3/meta`.
//! - [`registration`] says what to register there: where, under which name, the redirect URIs,
//!   the scopes.
//! - A [`Server`] is what is kept: its instance, service and the application's client id (public:
//!   a non-confidential application has no secret). Tokens are never part of it.
//! - [`proposal`] reads a shared link (`?gitlab=https://git.example.org/group/project&client_id=…`)
//!   into a server to propose; [`parse_config`] reads the file a managed machine provides
//!   (`texcraft-forges.toml`).

use serde_json::Value;

use crate::{Address, Error, GITHUB_COM, GITLAB_COM, Request, Result, Service, Transport};

/// The name to register the application under.
pub const APPLICATION_NAME: &str = "TeXCraft";

/// The scopes a GitLab application must allow: `api` to commit, `read_api` to read, `read_user`
/// for who is signed in.
pub const GITLAB_SCOPES: &str = "api read_api read_user";

/// The name of the file a managed machine lists its servers in.
pub const CONFIG_FILE: &str = "texcraft-forges.toml";

/// The largest answer of a probe.
const MAX_PROBE: u64 = 256 * 1024;

/// A self-managed server signing in reaches: its instance, its service and TeXCraft's application
/// on it. Kept in the preferences (the client id is not a secret); `provided` servers come from
/// the organisation's file and cannot be removed from the interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Server {
    /// `https://git.example.org`: scheme and host (and port), no `/` at the end.
    pub instance: String,
    pub service: Service,
    /// The application's client id (GitLab's Application ID, GitHub's Client ID).
    pub client_id: String,
    /// Listed by the organisation's file, not by the user.
    #[serde(skip)]
    pub provided: bool,
}

impl Server {
    /// A server of `service` at `address` (as typed) with `client_id`, both checked.
    pub fn new(address: &str, service: Service, client_id: &str) -> Result<Self> {
        Ok(Self { instance: parse_instance(address)?, service, client_id: check_client_id(client_id)?, provided: false })
    }

    /// The host, as users name the server.
    pub fn host(&self) -> &str {
        host_of(&self.instance)
    }
}

/// The host of `instance` (`https://git.example.org` → `git.example.org`).
pub fn host_of(instance: &str) -> &str {
    instance.split_once("://").map_or(instance, |(_, h)| h)
}

/// `client_id`, trimmed; refused when empty or not one (spaces, control characters, too long).
pub fn check_client_id(client_id: &str) -> Result<String> {
    let id = client_id.trim();
    if id.is_empty() {
        return Err(Error::SignIn("Give the application's ID, as the server shows it once the application is registered.".to_owned()));
    }
    if id.len() > 256 || !id.chars().all(|c| c.is_ascii_graphic()) {
        return Err(Error::SignIn("This is not an application's ID: it has spaces or characters an ID does not have.".to_owned()));
    }
    Ok(id.to_owned())
}

/// Whether `host` (with its port, if any) is this computer.
fn local(host: &str) -> bool {
    let name = if host.starts_with('[') {
        host.split(']').next().map_or(host, |h| h.trim_start_matches('['))
    } else {
        host.split(':').next().unwrap_or(host)
    };
    matches!(name, "localhost" | "127.0.0.1" | "::1")
}

/// Reads a server's address as a user types it: `git.example.org`, `https://git.example.org/`,
/// or any page on it (`https://git.example.org/group/project`): its scheme and host. HTTPS only,
/// but on this computer (`localhost`, a test server). gitlab.com and github.com are known already,
/// and refused.
pub fn parse_instance(input: &str) -> Result<String> {
    let input = input.trim();
    let bad = |why: &str| Error::Address(format!("{input}: {why}"));
    if input.is_empty() {
        return Err(Error::Address("Give the server's address, such as https://git.example.org.".to_owned()));
    }
    let (scheme, rest) = match input.split_once("://") {
        Some(("https", rest)) => ("https", rest),
        Some(("http", rest)) => ("http", rest),
        Some(_) => return Err(bad("the address must start with https://.")),
        None => ("https", input),
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    let host = host.rsplit('@').next().unwrap_or_default().to_owned();
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']')) {
        return Err(bad("the host is not a valid name."));
    }
    if scheme == "http" && !local(&host) {
        return Err(bad("the address must start with https://, so that sign-ins are not sent in the clear."));
    }
    let instance = format!("{scheme}://{host}");
    if matches!(instance.as_str(), GITLAB_COM | GITHUB_COM | "https://www.github.com" | "https://www.gitlab.com") {
        return Err(bad("TeXCraft signs in to gitlab.com and github.com already: there is nothing to add."));
    }
    Ok(instance)
}

/// What a probe found: the server answers as `service`, and its version when it says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub instance: String,
    pub service: Service,
    pub version: Option<String>,
}

/// Why a probe's server is not `service`'s, in words.
fn not_service(instance: &str, service: Service, why: &str) -> Error {
    let what = match service {
        Service::GitLab => "a GitLab server",
        Service::GitHub => "a GitHub Enterprise Server",
    };
    Error::SignIn(format!("{} does not answer as {what} ({why}): check the address.", host_of(instance)))
}

/// Checks that `instance` is reachable and answers as `service`: GitLab's `GET /api/v4/version`
/// (a 401 with GitLab's JSON proves GitLab too: the version needs a token on most servers), or a
/// GitHub Enterprise Server's `GET /api/v3/meta` (its `installed_version`; a 401 with GitHub's
/// JSON in private mode). In `browser`, a request that gets no answer is most often a server that
/// does not answer web pages (CORS), and is said so.
pub async fn probe(transport: &dyn Transport, instance: &str, service: Service, browser: bool) -> Result<Probed> {
    let url = match service {
        Service::GitLab => format!("{instance}/api/v4/version"),
        Service::GitHub => format!("{instance}/api/v3/meta"),
    };
    let request =
        Request { method: "GET", url, headers: vec![("Accept".to_owned(), "application/json".to_owned())], body: None, max_body: MAX_PROBE };
    let response = match transport.send(request).await {
        Ok(r) => r,
        Err(_) if browser => {
            return Err(Error::SignIn(format!(
                "{} did not answer this page: it cannot be reached from here, or it does not answer web pages (it does not let other sites' pages use its API). Ask its administrator, or use the desktop app.",
                host_of(instance)
            )));
        }
        Err(why) => return Err(Error::Network { forge: service, why }),
    };
    let v: Value = if response.too_large { Value::Null } else { serde_json::from_slice(&response.body).unwrap_or(Value::Null) };
    let version = match (service, response.status) {
        (Service::GitLab, 200) => match v["version"].as_str() {
            Some(version) => Some(version.to_owned()),
            None => return Err(not_service(instance, service, "its answer is not GitLab's")),
        },
        (Service::GitLab, 401) if v["message"].is_string() => None,
        (Service::GitHub, 200) if v["installed_version"].is_string() => v["installed_version"].as_str().map(str::to_owned),
        (Service::GitHub, 200) if v.get("verifiable_password_authentication").is_some() => None,
        (Service::GitHub, 200) => return Err(not_service(instance, service, "its answer is not GitHub's")),
        (Service::GitHub, 401) if v["documentation_url"].is_string() => None,
        (_, status) => return Err(not_service(instance, service, &format!("it answered with the status {status}"))),
    };
    Ok(Probed { instance: instance.to_owned(), service, version })
}

/// What to register on a server for TeXCraft to sign in there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub service: Service,
    /// Where a user registers an application of their own: GitLab's Edit profile › Applications,
    /// GitHub's Settings › Developer settings › OAuth Apps › New.
    pub page: String,
    /// Where an administrator registers one for everyone on the instance (GitLab's Admin ›
    /// Applications); none on GitHub, where an organisation's owner does it in its settings.
    pub admin_page: Option<String>,
    pub name: &'static str,
    /// The redirect URIs (GitLab), or the callback URL (GitHub, unused by the device flow).
    pub redirect_uris: Vec<String>,
    /// The scopes to tick (GitLab); none on GitHub, whose OAuth apps ask for them when signing in.
    pub scopes: Option<&'static str>,
    /// GitHub's Homepage URL.
    pub homepage: Option<&'static str>,
}

/// What to register on `instance` for `service`. `page` is the browser build's own page, which
/// GitLab sends back to; the desktop's loopback redirect is always listed.
pub fn registration(service: Service, instance: &str, page: Option<&str>) -> Registration {
    let desktop = format!("http://{}/callback", crate::oauth::LOOPBACK);
    match service {
        Service::GitLab => {
            let mut redirect_uris = vec![desktop];
            redirect_uris.extend(page.map(str::to_owned));
            Registration {
                service,
                page: format!("{instance}/-/user_settings/applications"),
                admin_page: Some(format!("{instance}/admin/applications/new")),
                name: APPLICATION_NAME,
                redirect_uris,
                scopes: Some(GITLAB_SCOPES),
                homepage: None,
            }
        }
        Service::GitHub => Registration {
            service,
            page: format!("{instance}/settings/applications/new"),
            admin_page: None,
            name: APPLICATION_NAME,
            redirect_uris: vec![desktop],
            scopes: None,
            homepage: Some("https://gitlab.com/severin-lemaignan/texcraft"),
        },
    }
}

/// A server a link proposes to add, and the project it points at, if it names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub server: Server,
    /// The project's address, to open once the server is added.
    pub project: Option<String>,
}

/// Reads a link's query into the server it proposes: `gitlab=` (or `github=`) a project's or the
/// server's address, and `client_id=` the application's id. None when the query proposes nothing;
/// an error when it does, but not in a way that can be added.
pub fn proposal(pairs: &[(String, String)]) -> Option<Result<Proposal>> {
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let (service, address) = match (get("gitlab"), get("github")) {
        (Some(a), _) => (Service::GitLab, a),
        (None, Some(a)) => (Service::GitHub, a),
        (None, None) => return None,
    };
    let client_id = get("client_id").unwrap_or_default();
    Some(Server::new(address, service, client_id).map(|server| {
        let project = Address::parse_as(address, Some(service)).ok().filter(|a| a.instance == server.instance).map(|a| a.web_url());
        Proposal { server, project }
    }))
}

/// One `[[server]]` of the organisation's file.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    address: String,
    #[serde(default)]
    service: Option<String>,
    client_id: String,
}

#[derive(serde::Deserialize)]
struct File {
    #[serde(default)]
    server: Vec<Entry>,
}

/// Reads the organisation's file (`texcraft-forges.toml`):
///
/// ```toml
/// [[server]]
/// address = "https://git.example.org"
/// service = "gitlab"            # or "github"; gitlab when left out
/// client_id = "…"
/// ```
///
/// Its servers are `provided`. A file that cannot be read gives an error; an entry that cannot be
/// a server is left out, and said in the list of problems.
pub fn parse_config(text: &str) -> std::result::Result<(Vec<Server>, Vec<String>), String> {
    let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut servers = Vec::new();
    let mut problems = Vec::new();
    for (i, entry) in file.server.into_iter().enumerate() {
        let service = match entry.service.as_deref() {
            None => Ok(Service::GitLab),
            Some(s) => Service::parse(s).ok_or_else(|| format!("service is \"gitlab\" or \"github\", not {s:?}")),
        };
        match service.and_then(|service| Server::new(&entry.address, service, &entry.client_id).map_err(|e| e.to_string())) {
            Ok(mut server) => {
                server.provided = true;
                servers.push(server);
            }
            Err(why) => problems.push(format!("server {} ({}): {why}", i + 1, entry.address)),
        }
    }
    Ok((servers, problems))
}
