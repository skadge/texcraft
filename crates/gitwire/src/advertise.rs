//! The refs a server advertises for a service (protocol version 0): `GET <url>/info/refs?service=…`
//! answers `# service=…`, a flush, then one ref per line (`<id> <name>`), the first with the
//! server's capabilities after a NUL. An empty repository advertises `capabilities^{}` at the
//! zero id.

use gix::ObjectId;

use crate::pktline::{Packet, Reader};
use crate::{Error, Http, Remote, Result, checked};

/// The two services of git's smart HTTP protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// Fetching: `git-upload-pack`.
    UploadPack,
    /// Pushing: `git-receive-pack`.
    ReceivePack,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Service::UploadPack => "git-upload-pack",
            Service::ReceivePack => "git-receive-pack",
        }
    }
}

/// What a server advertised.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Advertised {
    /// Its refs and the objects they point at, as listed (peeled tags, `^{}`, left out).
    pub refs: Vec<(String, ObjectId)>,
    pub capabilities: Vec<String>,
}

impl Advertised {
    /// The object ref `name` (`refs/heads/main`) points at, if the server has it.
    pub fn get(&self, name: &str) -> Option<ObjectId> {
        self.refs.iter().find(|(n, _)| n == name).map(|(_, id)| *id)
    }

    /// Whether the server offers capability `name` (with or without a value).
    pub fn has(&self, name: &str) -> bool {
        self.capabilities.iter().any(|c| c == name || c.strip_prefix(name).is_some_and(|r| r.starts_with('=')))
    }

    /// The branch the server's `HEAD` names (its `symref=HEAD:refs/heads/main`), if it says.
    pub fn head(&self) -> Option<&str> {
        self.capabilities.iter().find_map(|c| c.strip_prefix("symref=HEAD:"))
    }

    /// Reads an advertisement's body for `service`.
    pub fn parse(body: &[u8], service: Service) -> Result<Self> {
        let protocol = |why: String| Error::Protocol(why);
        let mut reader = Reader::new(body);
        let first = reader.next_line().map_err(protocol)?.ok_or_else(|| protocol("the advertisement is empty".to_owned()))?;
        let expected = format!("# service={}", service.name());
        if first != expected.as_bytes() {
            return Err(protocol(format!("it does not start with {expected:?}")));
        }
        // The flush after the service line.
        match reader.next_packet().map_err(protocol)? {
            Some(Packet::Flush) => {}
            _ => return Err(protocol("no flush after the service line".to_owned())),
        }
        let mut out = Advertised::default();
        let mut first = true;
        while let Some(line) = reader.next_line().map_err(protocol)? {
            let (line, caps) = match line.iter().position(|&b| b == 0) {
                Some(at) => (&line[..at], Some(&line[at + 1..])),
                None => (line, None),
            };
            if let Some(caps) = caps.filter(|_| first) {
                out.capabilities = String::from_utf8_lossy(caps).split_ascii_whitespace().map(str::to_owned).collect();
            }
            first = false;
            let text = std::str::from_utf8(line).map_err(|_| protocol("a ref's line is not text".to_owned()))?;
            let (id, name) = text.split_once(' ').ok_or_else(|| protocol(format!("{text:?} is not a ref's line")))?;
            let id = ObjectId::from_hex(id.as_bytes()).map_err(|_| protocol(format!("{id:?} is not an object id")))?;
            if name == "capabilities^{}" || name.ends_with("^{}") {
                continue;
            }
            out.refs.push((name.to_owned(), id));
        }
        Ok(out)
    }
}

/// The refs `remote` advertises for `service`.
pub fn advertised(http: &dyn Http, remote: &Remote, service: Service) -> Result<Advertised> {
    let url = format!("{}/info/refs?service={}", remote.url, service.name());
    let reply = checked(http.get(&url, &remote.headers()).map_err(Error::Network)?)?;
    let expected = format!("application/x-{}-advertisement", service.name());
    if reply.content_type.as_deref().map(|c| c.split(';').next().unwrap_or_default().trim()) != Some(expected.as_str()) {
        return Err(Error::Protocol(format!(
            "it is not a git server that speaks the smart HTTP protocol (its answer is {}, not {expected})",
            reply.content_type.as_deref().unwrap_or("of no type")
        )));
    }
    Advertised::parse(&reply.body, service)
}
