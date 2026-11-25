//! Requests and responses as plain values, and the transport that sends them: the desktop's HTTP
//! client, the browser's `fetch`, or a fake in tests. The API's logic sees only these, so it runs
//! the same on both targets and is tested without a network.

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::{Error, Service};

/// One HTTP request.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// `GET`, `POST` or `PATCH`.
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// The largest body to read; a bigger one is not read ([`Response::too_large`]).
    pub max_body: u64,
}

// The token is in the headers: a request is never printed with them.
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request").field("method", &self.method).field("url", &self.url).finish_non_exhaustive()
    }
}

/// The answer to a [`Request`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body was larger than the request's `max_body`, and was not read.
    pub too_large: bool,
}

impl Response {
    /// The value of header `name` (lower case), if the response has it.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// A request on its way: its response, or why there is none (the network, in words).
pub type Sending<'a> = Pin<Box<dyn Future<Output = Result<Response, String>> + 'a>>;

/// Sends requests. Any answer, an error status included, is a response; only a failure to get
/// one is an error.
pub trait Transport {
    fn send(&self, request: Request) -> Sending<'_>;
}

/// `response` when it is a success; otherwise the error its status means, about `what`, from
/// `forge`. A 403 or 429 that says no requests are left (GitHub's `X-RateLimit-Remaining: 0`) is
/// the rate limit, with the time until it resets.
pub(crate) fn check(forge: Service, response: Response, what: &str) -> crate::Result<Response> {
    let limited = response.header("x-ratelimit-remaining").is_some_and(|n| n.trim() == "0");
    let retry_after = || {
        let after = response.header("retry-after").and_then(|s| s.trim().parse().ok());
        let reset = response.header("x-ratelimit-reset").and_then(|s| s.trim().parse::<u64>().ok());
        after.or_else(|| Some(reset?.saturating_sub(now_secs()?)))
    };
    match response.status {
        200..=299 => Ok(response),
        401 => Err(Error::Unauthorized { forge }),
        403 | 429 if limited || response.header("retry-after").is_some() => Err(Error::RateLimited { forge, retry_after: retry_after() }),
        403 => Err(Error::Forbidden { forge, what: what.to_owned() }),
        404 => Err(Error::NotFound { forge, what: what.to_owned() }),
        429 => Err(Error::RateLimited { forge, retry_after: retry_after() }),
        status @ 400..=499 => Err(Error::Refused { forge, status, message: message_of(&response.body) }),
        status => Err(Error::Server { forge, status }),
    }
}

/// The forge's message in an error's body (`{"message": …}` or `{"error": …}`), or the body's
/// start.
fn message_of(body: &[u8]) -> String {
    let text = match serde_json::from_slice::<Value>(body) {
        Ok(v) => match v.get("message").or_else(|| v.get("error")) {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => v.to_string(),
        },
        Err(_) => String::from_utf8_lossy(body).into_owned(),
    };
    let text = text.trim();
    if text.chars().count() > 300 { format!("{}…", text.chars().take(300).collect::<String>()) } else { text.to_owned() }
}

/// Seconds since 1970 by this computer's clock, to tell how long until a rate limit resets.
fn now_secs() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
    #[cfg(target_arch = "wasm32")]
    Some((js_sys::Date::now() / 1000.0) as u64)
}

/// The transport this build uses to reach a forge: the desktop's HTTP client, or the browser's
/// `fetch`.
pub fn transport() -> Box<dyn Transport> {
    #[cfg(not(target_arch = "wasm32"))]
    return Box::new(native::Ureq::new());
    #[cfg(target_arch = "wasm32")]
    Box::new(web::Fetch)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{Request, Response, Sending, Transport};

    /// HTTPS with rustls and the bundled root certificates, blocking: it runs on its own thread.
    pub struct Ureq {
        agent: ureq::Agent,
    }

    impl Ureq {
        pub fn new() -> Self {
            let config = ureq::Agent::config_builder()
                // Error statuses are answers, read like any other.
                .http_status_as_error(false)
                .timeout_global(Some(std::time::Duration::from_secs(300)))
                .user_agent("TeXCraft")
                .build();
            Self { agent: config.into() }
        }

        fn send_now(&self, request: &Request) -> Result<Response, String> {
            let sent = match request.method {
                "POST" | "PATCH" => {
                    let mut builder = if request.method == "PATCH" { self.agent.patch(&request.url) } else { self.agent.post(&request.url) };
                    for (k, v) in &request.headers {
                        builder = builder.header(k, v);
                    }
                    builder.send(request.body.as_deref().unwrap_or_default())
                }
                _ => {
                    let mut builder = self.agent.get(&request.url);
                    for (k, v) in &request.headers {
                        builder = builder.header(k, v);
                    }
                    builder.call()
                }
            };
            let mut response = sent.map_err(|e| e.to_string())?;
            let status = response.status().as_u16();
            let headers =
                response.headers().iter().filter_map(|(k, v)| Some((k.as_str().to_ascii_lowercase(), v.to_str().ok()?.to_owned()))).collect();
            let (body, too_large) = match response.body_mut().with_config().limit(request.max_body).read_to_vec() {
                Ok(body) => (body, false),
                Err(ureq::Error::BodyExceedsLimit(_)) => (Vec::new(), true),
                Err(e) => return Err(e.to_string()),
            };
            Ok(Response { status, headers, body, too_large })
        }
    }

    impl Transport for Ureq {
        fn send(&self, request: Request) -> Sending<'_> {
            let response = self.send_now(&request);
            Box::pin(std::future::ready(response))
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    use super::{Request, Response, Sending, Transport};

    /// The headers the API's logic reads; the browser lists only those the server exposes.
    const HEADERS: &[&str] = &["link", "x-next-page", "retry-after", "x-ratelimit-remaining", "x-ratelimit-reset", "content-length", "content-type"];

    /// The browser's `fetch`, as a CORS request (GitLab's and GitHub's APIs allow any origin).
    pub struct Fetch;

    fn why(e: wasm_bindgen::JsValue) -> String {
        e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message())).or_else(|| e.as_string()).unwrap_or_else(|| format!("{e:?}"))
    }

    async fn fetch(request: Request) -> Result<Response, String> {
        let window = web_sys::window().ok_or("no window")?;
        let init = web_sys::RequestInit::new();
        init.set_method(request.method);
        init.set_mode(web_sys::RequestMode::Cors);
        let headers = web_sys::Headers::new().map_err(why)?;
        for (k, v) in &request.headers {
            headers.set(k, v).map_err(why)?;
        }
        init.set_headers(&headers);
        if let Some(body) = &request.body {
            init.set_body(&js_sys::Uint8Array::from(body.as_slice()));
        }
        let req = web_sys::Request::new_with_str_and_init(&request.url, &init).map_err(why)?;
        let response: web_sys::Response =
            JsFuture::from(window.fetch_with_request(&req)).await.map_err(why)?.dyn_into().map_err(|_| "no response".to_owned())?;
        let got = response.headers();
        let headers: Vec<(String, String)> = HEADERS.iter().filter_map(|name| Some(((*name).to_owned(), got.get(name).ok().flatten()?))).collect();
        let declared = headers.iter().find(|(n, _)| n == "content-length").and_then(|(_, v)| v.parse::<u64>().ok());
        let status = response.status();
        if declared.is_some_and(|n| n > request.max_body) {
            return Ok(Response { status, headers, body: Vec::new(), too_large: true });
        }
        let buffer = JsFuture::from(response.array_buffer().map_err(why)?).await.map_err(why)?;
        let body = js_sys::Uint8Array::new(&buffer).to_vec();
        if body.len() as u64 > request.max_body {
            return Ok(Response { status, headers, body: Vec::new(), too_large: true });
        }
        Ok(Response { status, headers, body, too_large: false })
    }

    impl Transport for Fetch {
        fn send(&self, request: Request) -> Sending<'_> {
            Box::pin(fetch(request))
        }
    }
}
