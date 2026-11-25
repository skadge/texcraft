//! Fetching one ref (`git-upload-pack`, protocol version 0, stateless): the server's refs are
//! read; when the repository lacks the ref's commit, it is asked for (`want`), with the commits
//! this repository has (`have`, newest first, at most [`MAX_HAVES`]) and `done` in one request,
//! so the server sends one pack of what is missing. The pack (side-band, no thin pack: every
//! delta's base is in it) is indexed into the repository's `objects/pack`.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use gix::ObjectId;

use crate::advertise::{Service, advertised};
use crate::pktline::{self, Packet, Reader};
use crate::{AGENT, Error, Http, Remote, Result, checked, local, pack};

/// Commits offered as had, at most: past them the server sends a little more than needed.
pub const MAX_HAVES: usize = 256;

/// What a fetch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The commit at the ref on the server; none when the server has no such ref (an empty
    /// repository, or another branch).
    pub tip: Option<String>,
    /// The branch the server's `HEAD` names, when it says.
    pub head: Option<String>,
    /// Whether a pack was received (false: the repository had the commit already).
    pub received: bool,
    /// The server's progress messages, if it sent any.
    pub messages: String,
}

/// Up to [`MAX_HAVES`] commits of the repository from `tips`, newest first by walk.
fn haves(repo: &gix::Repository, tips: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: VecDeque<ObjectId> = tips.iter().copied().filter(|id| pack::has_commit(repo, *id)).collect();
    while let Some(id) = queue.pop_front() {
        if out.len() >= MAX_HAVES {
            break;
        }
        if !seen.insert(id) {
            continue;
        }
        out.push(id);
        queue.extend(pack::parents(repo, id)?.into_iter().filter(|p| pack::has_commit(repo, *p)));
    }
    Ok(out)
}

/// Fetches ref `reference` (`refs/heads/main`) of `remote` into the repository at `git_dir`,
/// offering the history of `tips` (full commit ids: the local branch, its last-known remote
/// state) as what it has. The caller then moves its refs.
pub fn fetch(http: &dyn Http, remote: &Remote, git_dir: &Path, reference: &str, tips: &[String]) -> Result<Fetched> {
    let repo = gix::open(git_dir).map_err(|e| local("The repository cannot be opened", e))?;
    let advertised = advertised(http, remote, Service::UploadPack)?;
    let head = advertised.head().map(|h| h.strip_prefix("refs/heads/").unwrap_or(h).to_owned());
    let Some(tip) = advertised.get(reference) else {
        return Ok(Fetched { tip: None, head, received: false, messages: String::new() });
    };
    if pack::has_commit(&repo, tip) {
        return Ok(Fetched { tip: Some(tip.to_string()), head, received: false, messages: String::new() });
    }
    let tips: Vec<ObjectId> = tips.iter().filter_map(|t| ObjectId::from_hex(t.as_bytes()).ok()).collect();
    let haves = haves(&repo, &tips)?;

    let mut caps = Vec::new();
    let sideband = if advertised.has("side-band-64k") {
        caps.push("side-band-64k");
        true
    } else if advertised.has("side-band") {
        caps.push("side-band");
        true
    } else {
        false
    };
    if advertised.has("ofs-delta") {
        caps.push("ofs-delta");
    }
    if advertised.has("no-progress") {
        caps.push("no-progress");
    }
    let agent = format!("agent={}", AGENT.trim_start_matches("git/"));
    if advertised.has("agent") {
        caps.push(&agent);
    }
    let mut body = Vec::new();
    pktline::write(&mut body, format!("want {tip} {}\n", caps.join(" ")).as_bytes());
    pktline::flush(&mut body);
    for id in &haves {
        pktline::write(&mut body, format!("have {id}\n").as_bytes());
    }
    pktline::write(&mut body, b"done\n");

    let mut headers = remote.headers();
    headers.push(("Content-Type".to_owned(), "application/x-git-upload-pack-request".to_owned()));
    headers.push(("Accept".to_owned(), "application/x-git-upload-pack-result".to_owned()));
    let url = format!("{}/git-upload-pack", remote.url);
    let reply = checked(http.post(&url, &headers, body).map_err(Error::Network)?)?;
    let (pack, messages) = read_answer(&reply.body, sideband)?;
    index(git_dir, &pack)?;

    let repo = gix::open(git_dir).map_err(|e| local("The repository cannot be opened", e))?;
    if !pack::has_commit(&repo, tip) {
        return Err(Error::Protocol(format!("the server's pack does not have commit {tip}")));
    }
    Ok(Fetched { tip: Some(tip.to_string()), head, received: true, messages })
}

/// The pack in an upload-pack answer, and the server's messages: `NAK` or `ACK <id>` lines (and
/// `shallow` ones) come first, then the pack, in side-band packets or as it is.
pub fn read_answer(body: &[u8], sideband: bool) -> Result<(Vec<u8>, String)> {
    let mut reader = Reader::new(body);
    loop {
        let before = reader.rest();
        match reader.next_packet().map_err(Error::Protocol)? {
            None => return Err(Error::Protocol("the answer has no pack".to_owned())),
            Some(Packet::Flush) => continue,
            Some(Packet::Data(line)) => {
                if line.starts_with(b"NAK") || line.starts_with(b"ACK ") || line.starts_with(b"shallow ") || line.starts_with(b"unshallow ") {
                    continue;
                }
                if let Some(message) = line.strip_prefix(b"ERR ") {
                    return Err(Error::Server(String::from_utf8_lossy(message).trim().to_owned()));
                }
                if !sideband {
                    // The pack follows the negotiation's last line as it is.
                    return Ok((before.to_vec(), String::new()));
                }
                let mut rest = Reader::new(before);
                let demuxed = pktline::demux(&mut rest).map_err(Error::Protocol)?;
                if let Some(error) = demuxed.error {
                    return Err(Error::Server(error));
                }
                return Ok((demuxed.data, demuxed.progress));
            }
            Some(_) => return Err(Error::Protocol("an unexpected packet in the answer".to_owned())),
        }
    }
}

/// Writes `pack` and its index into the repository's `objects/pack`.
pub fn index(git_dir: &Path, pack: &[u8]) -> Result<()> {
    if !pack.starts_with(b"PACK") {
        return Err(Error::Protocol("the server's answer is not a pack".to_owned()));
    }
    let dir = git_dir.join("objects").join("pack");
    std::fs::create_dir_all(&dir).map_err(|e| local("The repository's pack folder cannot be made", e))?;
    let mut input = std::io::BufReader::new(pack);
    let interrupt = AtomicBool::new(false);
    let outcome = gix_pack::Bundle::write_to_directory(
        &mut input,
        Some(&dir),
        &mut gix::progress::Discard,
        &interrupt,
        None::<gix::objs::find::Never>,
        gix::hash::Kind::Sha1,
        gix_pack::bundle::write::Options::default(),
    )
    .map_err(|e| local("The commits received cannot be stored in the repository", e))?;
    // The pack is in place and indexed: nothing needs it kept from git's garbage collection.
    if let Some(keep) = outcome.keep_path {
        let _ = std::fs::remove_file(keep);
    }
    Ok(())
}
