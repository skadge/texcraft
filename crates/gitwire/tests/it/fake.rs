//! A git server in memory: a bare repository answering the smart HTTP protocol's four requests,
//! built from this crate's own pieces (its refs advertised, packs indexed into it, packs written
//! from it), with the refusals a real server makes.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use gix::ObjectId;
use texcraft_gitwire::pktline::{self, Packet, Reader};
use texcraft_gitwire::{Http, Reply, ZERO_ID};

/// A request the server got: method, address, headers.
pub type Sent = (String, String, Vec<(String, String)>);

pub struct Server {
    /// The bare repository's folder.
    pub dir: PathBuf,
    /// Its refs; the bare repository holds the objects.
    pub refs: RefCell<BTreeMap<String, ObjectId>>,
    /// Whether requests must carry these credentials (else 401).
    pub auth: Option<String>,
    /// Refuses ref updates with this reason (`ng`), as a protected branch does.
    pub refuse: Option<String>,
    /// Whether it offers side-band-64k.
    pub sideband: bool,
    /// Every request: method, address, headers.
    pub requests: RefCell<Vec<Sent>>,
}

impl Server {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, refs: RefCell::default(), auth: None, refuse: None, sideband: true, requests: RefCell::default() }
    }

    /// Requires the authorization header `header`.
    pub fn with_auth(mut self, header: String) -> Self {
        self.auth = Some(header);
        self
    }

    fn authorized(&self, headers: &[(String, String)]) -> bool {
        match &self.auth {
            None => true,
            Some(expected) => headers.iter().any(|(k, v)| k == "Authorization" && v == expected),
        }
    }

    fn advertisement(&self, service: &str) -> Vec<u8> {
        let mut caps = vec!["report-status", "delete-refs", "ofs-delta", "agent=fake/1"];
        if self.sideband {
            caps.push("side-band-64k");
        }
        if service == "git-upload-pack" {
            caps.push("symref=HEAD:refs/heads/main");
            caps.push("no-progress");
        }
        let caps = caps.join(" ");
        let mut out = Vec::new();
        pktline::write(&mut out, format!("# service={service}\n").as_bytes());
        pktline::flush(&mut out);
        let refs = self.refs.borrow();
        if refs.is_empty() {
            pktline::write(&mut out, format!("{ZERO_ID} capabilities^{{}}\0{caps}\n").as_bytes());
        }
        for (i, (name, id)) in refs.iter().enumerate() {
            let line = if i == 0 { format!("{id} {name}\0{caps}\n") } else { format!("{id} {name}\n") };
            pktline::write(&mut out, line.as_bytes());
        }
        pktline::flush(&mut out);
        out
    }

    /// Wraps `data` in side-band packets on channel 1 (when offered), and a flush.
    fn banded(&self, data: &[u8]) -> Vec<u8> {
        if !self.sideband {
            return data.to_vec();
        }
        let mut out = Vec::new();
        for chunk in data.chunks(1000) {
            let mut packet = vec![1u8];
            packet.extend_from_slice(chunk);
            pktline::write(&mut out, &packet);
        }
        pktline::write(&mut out, b"\x02Resolving deltas: done.\n");
        pktline::flush(&mut out);
        out
    }

    fn receive(&self, body: &[u8]) -> Result<Reply, String> {
        let mut reader = Reader::new(body);
        let mut commands = Vec::new();
        while let Ok(Some(Packet::Data(line))) = reader.next_packet() {
            let line = line.split(|&b| b == 0).next().unwrap_or_default();
            let text = String::from_utf8_lossy(line).trim().to_owned();
            let parts: Vec<&str> = text.split(' ').collect();
            let [old, new, name] = parts[..] else { return Err(format!("{text:?} is no command")) };
            commands.push((old.to_owned(), new.to_owned(), name.to_owned()));
        }
        let pack = reader.rest();
        let count = u32::from_be_bytes(pack.get(8..12).and_then(|b| b.try_into().ok()).ok_or("no pack")?);
        if count > 0 {
            texcraft_gitwire::fetch::index(&self.dir, pack).map_err(|e| e.to_string())?;
        }
        let mut report = Vec::new();
        pktline::write(&mut report, b"unpack ok\n");
        for (old, new, name) in commands {
            let current = self.refs.borrow().get(&name).map(ObjectId::to_string).unwrap_or_else(|| ZERO_ID.to_owned());
            if let Some(why) = &self.refuse {
                pktline::write(&mut report, format!("ng {name} {why}\n").as_bytes());
            } else if current != old {
                pktline::write(&mut report, format!("ng {name} fetch first\n").as_bytes());
            } else {
                self.refs.borrow_mut().insert(name.clone(), ObjectId::from_hex(new.as_bytes()).map_err(|e| e.to_string())?);
                pktline::write(&mut report, format!("ok {name}\n").as_bytes());
            }
        }
        pktline::flush(&mut report);
        Ok(Reply { status: 200, content_type: Some("application/x-git-receive-pack-result".into()), body: self.banded(&report) })
    }

    fn upload(&self, body: &[u8]) -> Result<Reply, String> {
        let mut reader = Reader::new(body);
        let (mut wants, mut haves) = (Vec::new(), Vec::new());
        while let Ok(Some(packet)) = reader.next_packet() {
            let Packet::Data(line) = packet else { continue };
            let text = String::from_utf8_lossy(line).trim().to_owned();
            if let Some(rest) = text.strip_prefix("want ") {
                wants.push(ObjectId::from_hex(rest.split(' ').next().unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?);
            } else if let Some(rest) = text.strip_prefix("have ") {
                haves.push(ObjectId::from_hex(rest.as_bytes()).map_err(|e| e.to_string())?);
            }
        }
        let repo = gix::open(&self.dir).map_err(|e| e.to_string())?;
        let want = *wants.first().ok_or("no want")?;
        let objects = texcraft_gitwire::pack::objects_to_send(&repo, want, &haves).map_err(|e| e.to_string())?;
        let pack = texcraft_gitwire::pack::write(&repo, &objects).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        pktline::write(&mut out, b"NAK\n");
        out.extend_from_slice(&self.banded(&pack));
        Ok(Reply { status: 200, content_type: Some("application/x-git-upload-pack-result".into()), body: out })
    }
}

impl Http for Server {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Reply, String> {
        self.requests.borrow_mut().push(("GET".into(), url.into(), headers.to_vec()));
        if !self.authorized(headers) {
            return Ok(Reply { status: 401, content_type: None, body: b"HTTP Basic: Access denied".to_vec() });
        }
        let service = url.split("service=").nth(1).unwrap_or_default();
        Ok(Reply { status: 200, content_type: Some(format!("application/x-{service}-advertisement")), body: self.advertisement(service) })
    }

    fn post(&self, url: &str, headers: &[(String, String)], body: Vec<u8>) -> Result<Reply, String> {
        self.requests.borrow_mut().push(("POST".into(), url.into(), headers.to_vec()));
        if !self.authorized(headers) {
            return Ok(Reply { status: 401, content_type: None, body: Vec::new() });
        }
        if url.ends_with("/git-receive-pack") {
            self.receive(&body)
        } else if url.ends_with("/git-upload-pack") {
            self.upload(&body)
        } else {
            Ok(Reply { status: 404, content_type: None, body: Vec::new() })
        }
    }
}
