//! Pushing one ref (`git-receive-pack`): the server's refs are read, the ref's update is checked
//! to be a fast-forward (the server's commit is in the local branch's history) unless forced, the
//! objects the server lacks are sent in a pack, and the server's report read: `unpack ok`, then
//! `ok <ref>` or `ng <ref> <why>`.

use std::path::Path;

use gix::ObjectId;

use crate::advertise::{Service, advertised};
use crate::pktline::{self, Reader};
use crate::{AGENT, Error, Http, Remote, Result, ZERO_ID, checked, local, pack};

/// What a push did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    /// The ref on the server (`refs/heads/main`).
    pub reference: String,
    /// What it was before; none when the push made it.
    pub old: Option<String>,
    /// What it is now.
    pub new: String,
    /// The objects sent (none when the server had the commit already).
    pub objects: usize,
    /// The server had the commit at the ref already: nothing was sent.
    pub up_to_date: bool,
    /// The server's progress messages, if it sent any.
    pub messages: String,
}

/// Pushes `commit` (a full id) of the repository at `git_dir` to ref `reference`
/// (`refs/heads/main`) of `remote`. Without `force`, a server's ref whose commit is not in the
/// history of `commit` is refused here ([`Error::NotFastForward`]) and nothing is sent.
pub fn push(http: &dyn Http, remote: &Remote, git_dir: &Path, commit: &str, reference: &str, force: bool) -> Result<Pushed> {
    let repo = gix::open(git_dir).map_err(|e| local("The repository cannot be opened", e))?;
    let new = ObjectId::from_hex(commit.as_bytes()).map_err(|_| Error::Local(format!("{commit:?} is not a commit id.")))?;
    if !pack::has_commit(&repo, new) {
        return Err(Error::Local(format!("The repository has no commit {commit}.")));
    }
    let advertised = advertised(http, remote, Service::ReceivePack)?;
    if !advertised.has("report-status") {
        return Err(Error::Protocol("the server does not report whether a push worked (no report-status)".to_owned()));
    }
    let old = advertised.get(reference);
    let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference).to_owned();
    if old == Some(new) {
        return Ok(Pushed {
            reference: reference.to_owned(),
            old: Some(commit.to_owned()),
            new: commit.to_owned(),
            objects: 0,
            up_to_date: true,
            messages: String::new(),
        });
    }
    if let Some(old) = old
        && !force
        && !pack::ancestors(&repo, &[new])?.contains(&old)
    {
        return Err(Error::NotFastForward { branch });
    }
    let theirs: Vec<ObjectId> = advertised.refs.iter().map(|(_, id)| *id).collect();
    let objects = pack::objects_to_send(&repo, new, &theirs)?;
    let pack = pack::write(&repo, &objects)?;

    let mut caps = vec!["report-status".to_owned()];
    let sideband = advertised.has("side-band-64k");
    if sideband {
        caps.push("side-band-64k".to_owned());
    }
    if advertised.has("agent") {
        caps.push(format!("agent={}", AGENT.trim_start_matches("git/")));
    }
    let old_hex = old.map_or_else(|| ZERO_ID.to_owned(), |id| id.to_string());
    let mut body = Vec::with_capacity(pack.len() + 256);
    pktline::write(&mut body, format!("{old_hex} {new} {reference}\0{}\n", caps.join(" ")).as_bytes());
    pktline::flush(&mut body);
    body.extend_from_slice(&pack);

    let mut headers = remote.headers();
    headers.push(("Content-Type".to_owned(), "application/x-git-receive-pack-request".to_owned()));
    headers.push(("Accept".to_owned(), "application/x-git-receive-pack-result".to_owned()));
    let url = format!("{}/git-receive-pack", remote.url);
    let reply = checked(http.post(&url, &headers, body).map_err(Error::Network)?)?;
    let (report, messages) = if sideband {
        let demuxed = pktline::demux(&mut Reader::new(&reply.body)).map_err(Error::Protocol)?;
        if let Some(error) = demuxed.error {
            return Err(Error::Server(error));
        }
        (demuxed.data, demuxed.progress)
    } else {
        (reply.body, String::new())
    };
    read_report(&report, reference)?;
    Ok(Pushed {
        reference: reference.to_owned(),
        old: old.map(|id| id.to_string()),
        new: commit.to_owned(),
        objects: objects.len(),
        up_to_date: false,
        messages,
    })
}

/// Reads a report-status: `unpack ok` (or why not), then a line per ref; `reference`'s must be
/// `ok`.
pub fn read_report(report: &[u8], reference: &str) -> Result<()> {
    let protocol = Error::Protocol;
    let mut reader = Reader::new(report);
    let first = reader.next_line().map_err(protocol)?.ok_or_else(|| Error::Protocol("the server's report is empty".to_owned()))?;
    let first = String::from_utf8_lossy(first);
    match first.strip_prefix("unpack ") {
        Some("ok") => {}
        Some(why) => return Err(Error::Unpack(why.to_owned())),
        None => return Err(Error::Protocol(format!("the report starts with {first:?}"))),
    }
    let mut found = false;
    while let Some(line) = reader.next_line().map_err(Error::Protocol)? {
        let line = String::from_utf8_lossy(line);
        if let Some(name) = line.strip_prefix("ok ") {
            found |= name == reference;
        } else if let Some(rest) = line.strip_prefix("ng ") {
            let (name, why) = rest.split_once(' ').unwrap_or((rest, "no reason given"));
            return Err(Error::Rejected { reference: name.to_owned(), why: why.to_owned() });
        }
    }
    if found { Ok(()) } else { Err(Error::Protocol(format!("the report does not say whether {reference} was updated"))) }
}
