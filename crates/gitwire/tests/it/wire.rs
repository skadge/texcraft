//! Packet lines, packs, pushing and fetching against the server in memory.

use texcraft_gitwire::pktline::{self, Packet, Reader};
use texcraft_gitwire::{Credentials, Error, Remote, fetch, push};

use crate::fake::Server;
use crate::repo::{Scratch, commit, head, init, init_bare, reachable, same_object};

const URL: &str = "https://git.example/owner/paper.git";
const MAIN: &str = "refs/heads/main";

fn basic(user: &str, password: &str) -> String {
    use base64::Engine as _;
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
}

#[test]
fn packet_lines_read_back_as_written_and_bad_lengths_are_refused() {
    let mut out = Vec::new();
    pktline::write(&mut out, b"hello\n");
    pktline::flush(&mut out);
    pktline::write(&mut out, &vec![b'x'; 70_000]);
    let mut reader = Reader::new(&out);
    assert_eq!(reader.next_line().unwrap(), Some(&b"hello"[..]));
    assert_eq!(reader.next_packet().unwrap(), Some(Packet::Flush));
    // A packet longer than git's limit is split, never written with a wrong length.
    let mut total = 0;
    while let Some(Packet::Data(d)) = reader.next_packet().unwrap() {
        assert!(d.len() + 4 <= pktline::MAX_PACKET);
        total += d.len();
    }
    assert_eq!(total, 70_000);
    assert!(Reader::new(b"zzzzabc").next_packet().is_err());
    assert!(Reader::new(b"0010abc").next_packet().is_err(), "a packet cut short");
}

#[test]
fn a_written_pack_is_read_back_by_gix_with_the_same_objects() {
    let scratch = Scratch::new("pack").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    commit(&git, &[("main.tex", "\\documentclass{article}\n"), ("figures/a.txt", "a")], "first").unwrap();
    let tip = commit(&git, &[("main.tex", "\\documentclass{book}\n"), ("figures/a.txt", "a")], "second").unwrap();
    let objects = reachable(&git, tip).unwrap();
    let repo = gix::open(&git).unwrap();
    let pack = texcraft_gitwire::pack::write(&repo, &objects).unwrap();
    let other = init_bare(&scratch.path("other.git")).unwrap();
    texcraft_gitwire::fetch::index(&other, &pack).unwrap();
    for id in &objects {
        assert!(same_object(&other, &git, *id), "object {id} read back the same");
    }
}

#[test]
fn a_push_into_an_empty_repository_sends_the_whole_history_and_a_second_only_what_is_new() {
    let scratch = Scratch::new("push").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    commit(&git, &[("main.tex", "one")], "first").unwrap();
    let tip = commit(&git, &[("main.tex", "two"), ("refs.bib", "@book{}")], "second").unwrap();
    let server = Server::new(init_bare(&scratch.path("server.git")).unwrap());
    let remote = Remote::new(URL, None);

    let pushed = push(&server, &remote, &git, &tip.to_string(), MAIN, false).unwrap();
    assert_eq!(pushed.old, None);
    assert_eq!(server.refs.borrow().get(MAIN), Some(&tip));
    for id in reachable(&git, tip).unwrap() {
        assert!(same_object(&server.dir, &git, id), "the server has {id}");
    }
    let first_count = pushed.objects;

    let next = commit(&git, &[("main.tex", "three"), ("refs.bib", "@book{}")], "third").unwrap();
    let pushed = push(&server, &remote, &git, &next.to_string(), MAIN, false).unwrap();
    assert_eq!(pushed.old.as_deref(), Some(tip.to_string().as_str()));
    assert!(pushed.objects < first_count, "an unchanged file is not sent again");
    assert_eq!(server.refs.borrow().get(MAIN), Some(&next));

    let again = push(&server, &remote, &git, &next.to_string(), MAIN, false).unwrap();
    assert!(again.up_to_date);
    let posts = server.requests.borrow().iter().filter(|(m, ..)| m == "POST").count();
    assert_eq!(posts, 2, "nothing is sent when the server is up to date");
}

#[test]
fn a_push_works_without_side_band_too() {
    let scratch = Scratch::new("push-plain").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    let tip = commit(&git, &[("main.tex", "one")], "first").unwrap();
    let mut server = Server::new(init_bare(&scratch.path("server.git")).unwrap());
    server.sideband = false;
    push(&server, &Remote::new(URL, None), &git, &tip.to_string(), MAIN, false).unwrap();
    assert_eq!(server.refs.borrow().get(MAIN), Some(&tip));
}

#[test]
fn a_push_over_commits_the_project_lacks_is_refused_before_anything_is_sent() {
    let scratch = Scratch::new("push-diverged").unwrap();
    let ours = init(&scratch.path("ours")).unwrap();
    let theirs = init(&scratch.path("theirs")).unwrap();
    let server = Server::new(init_bare(&scratch.path("server.git")).unwrap());
    let remote = Remote::new(URL, None);
    let their_tip = commit(&theirs, &[("main.tex", "theirs")], "theirs").unwrap();
    push(&server, &remote, &theirs, &their_tip.to_string(), MAIN, false).unwrap();

    let our_tip = commit(&ours, &[("main.tex", "ours")], "ours").unwrap();
    let refused = push(&server, &remote, &ours, &our_tip.to_string(), MAIN, false).unwrap_err();
    assert_eq!(refused, Error::NotFastForward { branch: "main".into() });
    assert!(refused.to_string().contains("pull first"));
    assert_eq!(server.refs.borrow().get(MAIN), Some(&their_tip), "the server's branch is untouched");
    let posts = server.requests.borrow().iter().filter(|(m, ..)| m == "POST").count();
    assert_eq!(posts, 1, "only the first push posted");

    // Forced, it goes.
    push(&server, &remote, &ours, &our_tip.to_string(), MAIN, true).unwrap();
    assert_eq!(server.refs.borrow().get(MAIN), Some(&our_tip));
}

#[test]
fn a_ref_the_server_refuses_is_said_with_its_reason() {
    let scratch = Scratch::new("push-ng").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    let tip = commit(&git, &[("main.tex", "one")], "first").unwrap();
    let mut server = Server::new(init_bare(&scratch.path("server.git")).unwrap());
    server.refuse = Some("protected branch hook declined".into());
    let refused = push(&server, &Remote::new(URL, None), &git, &tip.to_string(), MAIN, false).unwrap_err();
    assert_eq!(refused, Error::Rejected { reference: MAIN.into(), why: "protected branch hook declined".into() });
    assert!(server.refs.borrow().is_empty());
}

#[test]
fn credentials_are_sent_as_basic_authentication_and_never_shown() {
    let scratch = Scratch::new("push-auth").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    let tip = commit(&git, &[("main.tex", "one")], "first").unwrap();
    let secret = "glpat-very-secret";
    let server = Server::new(init_bare(&scratch.path("server.git")).unwrap()).with_auth(basic("oauth2", secret));

    let refused = push(&server, &Remote::new(URL, None), &git, &tip.to_string(), MAIN, false).unwrap_err();
    assert_eq!(refused, Error::Unauthorized);
    let wrong = Remote::new(URL, Some(Credentials::new("oauth2", "wrong")));
    assert_eq!(push(&server, &wrong, &git, &tip.to_string(), MAIN, false).unwrap_err(), Error::Unauthorized);

    let remote = Remote::new(URL, Some(Credentials::new("oauth2", secret)));
    assert!(!format!("{remote:?}").contains(secret), "the password is not printed");
    push(&server, &remote, &git, &tip.to_string(), MAIN, false).unwrap();
    assert_eq!(server.refs.borrow().get(MAIN), Some(&tip));
    let unauthorized = Error::Unauthorized.to_string();
    assert!(!unauthorized.contains(secret));
}

#[test]
fn a_server_that_is_not_git_is_said_so() {
    struct Page;
    impl texcraft_gitwire::Http for Page {
        fn get(&self, _: &str, _: &[(String, String)]) -> Result<texcraft_gitwire::Reply, String> {
            Ok(texcraft_gitwire::Reply { status: 200, content_type: Some("text/html".into()), body: b"<html>".to_vec() })
        }
        fn post(&self, _: &str, _: &[(String, String)], _: Vec<u8>) -> Result<texcraft_gitwire::Reply, String> {
            Err("no".into())
        }
    }
    let scratch = Scratch::new("not-git").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    let tip = commit(&git, &[("main.tex", "one")], "first").unwrap();
    let e = push(&Page, &Remote::new(URL, None), &git, &tip.to_string(), MAIN, false).unwrap_err();
    assert!(matches!(e, Error::Protocol(_)), "{e:?}");
}

#[test]
fn a_fetch_receives_the_commits_the_project_lacks_and_only_those() {
    let scratch = Scratch::new("fetch").unwrap();
    let theirs = init(&scratch.path("theirs")).unwrap();
    let ours = init(&scratch.path("ours")).unwrap();
    let server = Server::new(init_bare(&scratch.path("server.git")).unwrap());
    let remote = Remote::new(URL, None);

    // Nothing on the server yet: no tip.
    let fetched = fetch(&server, &remote, &ours, MAIN, &[]).unwrap();
    assert_eq!(fetched.tip, None);

    let first = commit(&theirs, &[("main.tex", "one"), ("big.txt", "a large file")], "first").unwrap();
    push(&server, &remote, &theirs, &first.to_string(), MAIN, false).unwrap();
    let fetched = fetch(&server, &remote, &ours, MAIN, &[]).unwrap();
    assert_eq!(fetched.tip.as_deref(), Some(first.to_string().as_str()));
    assert_eq!(fetched.head.as_deref(), Some("main"));
    assert!(fetched.received);
    for id in reachable(&theirs, first).unwrap() {
        assert!(same_object(&ours, &theirs, id), "fetched {id}");
    }

    let second = commit(&theirs, &[("main.tex", "two"), ("big.txt", "a large file")], "second").unwrap();
    push(&server, &remote, &theirs, &second.to_string(), MAIN, false).unwrap();
    let before = server.requests.borrow().len();
    fetch(&server, &remote, &ours, MAIN, &[first.to_string()]).unwrap();
    for id in reachable(&theirs, second).unwrap() {
        assert!(same_object(&ours, &theirs, id), "fetched {id}");
    }
    // The second fetch said what it had.
    let requests = server.requests.borrow();
    assert!(requests[before..].iter().any(|(m, u, _)| m == "POST" && u.ends_with("/git-upload-pack")));

    // Had already: nothing is asked.
    drop(requests);
    let again = fetch(&server, &remote, &ours, MAIN, &[second.to_string()]).unwrap();
    assert!(!again.received);
    assert_eq!(head(&ours, MAIN), None, "fetching moves no ref: the caller does");
}

#[test]
fn an_upload_pack_answer_with_an_error_is_said() {
    let mut body = Vec::new();
    pktline::write(&mut body, b"ERR upload-pack: not our ref\n");
    let e = texcraft_gitwire::fetch::read_answer(&body, true).unwrap_err();
    assert_eq!(e, Error::Server("upload-pack: not our ref".into()));
    let mut body = Vec::new();
    pktline::write(&mut body, b"NAK\n");
    pktline::write(&mut body, b"\x03repository disabled\n");
    pktline::flush(&mut body);
    assert_eq!(texcraft_gitwire::fetch::read_answer(&body, true).unwrap_err(), Error::Server("repository disabled".into()));
}

#[test]
fn reports_say_unpack_failures_and_refusals() {
    let mut report = Vec::new();
    pktline::write(&mut report, b"unpack index-pack failed\n");
    assert_eq!(texcraft_gitwire::push::read_report(&report, MAIN).unwrap_err(), Error::Unpack("index-pack failed".into()));
    let mut report = Vec::new();
    pktline::write(&mut report, b"unpack ok\n");
    pktline::flush(&mut report);
    assert!(matches!(texcraft_gitwire::push::read_report(&report, MAIN), Err(Error::Protocol(_))), "no line for the ref");
}

/// With git installed (opt-in: skipped without it), the server's repository after a push passes
/// `git fsck`.
#[test]
fn a_pushed_repository_passes_git_fsck_when_git_is_installed() {
    let Ok(version) = std::process::Command::new("git").arg("--version").output() else { return };
    if !version.status.success() {
        return;
    }
    let scratch = Scratch::new("fsck").unwrap();
    let git = init(&scratch.path("work")).unwrap();
    commit(&git, &[("main.tex", "one"), ("figures/a.txt", "a")], "first").unwrap();
    let tip = commit(&git, &[("main.tex", "two"), ("figures/a.txt", "a"), ("figures/b.txt", "b")], "second").unwrap();
    let server = Server::new(init_bare(&scratch.path("server.git")).unwrap());
    push(&server, &Remote::new(URL, None), &git, &tip.to_string(), MAIN, false).unwrap();
    std::fs::write(server.dir.join("refs/heads/main"), format!("{tip}\n")).unwrap();
    let fsck = std::process::Command::new("git").args(["-C"]).arg(&server.dir).args(["fsck", "--strict"]).output().unwrap();
    assert!(fsck.status.success(), "{}", String::from_utf8_lossy(&fsck.stderr));
}
