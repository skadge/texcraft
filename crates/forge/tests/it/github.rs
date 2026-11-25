//! What is GitHub's own: its addresses, commits through the Git Data API, a tree it lists only in
//! part, its rate limit, and branch rules it does not show.

use std::collections::BTreeMap;

use texcraft_forge::fake::Fake;
use texcraft_forge::{Address, ChangeKind, Choices, Error, GITHUB_COM, GITLAB_COM, Link, Loaded, Service, changes, commit, open};

use crate::{PROJECT, TOKEN, block_on, client};

fn paper() -> Fake {
    Fake::github(PROJECT, TOKEN, &[("main.tex", b"\\documentclass{article}\n"), ("intro.tex", b"Intro\n"), ("fig/plot.png", b"\x89PNG\0\x01")])
}

#[test]
fn github_addresses_name_the_repository_on_github() {
    for input in [
        "https://github.com/owner/paper",
        "https://github.com/owner/paper.git",
        "https://www.github.com/owner/paper/",
        "https://github.com/owner/paper/tree/main/chapters?x=1",
        "git@github.com:owner/paper.git",
        "github.com/owner/paper",
    ] {
        let a = Address::parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!((a.service, a.instance.as_str(), a.path.as_str()), (Service::GitHub, GITHUB_COM, "owner/paper"), "{input}");
        assert_eq!(a.api(), "https://api.github.com");
    }
    // A bare owner/repository is GitHub's only when GitHub is chosen.
    assert_eq!(Address::parse("owner/paper").unwrap().service, Service::GitLab);
    let chosen = Address::parse_as("owner/paper", Some(Service::GitHub)).unwrap();
    assert_eq!((chosen.instance.as_str(), chosen.path.as_str()), (GITHUB_COM, "owner/paper"));
    // Another host, chosen as GitHub, is a GitHub Enterprise Server.
    let server = Address::parse_as("https://git.example.org/team/thesis", Some(Service::GitHub)).unwrap();
    assert_eq!(server.api(), "https://git.example.org/api/v3");
    // The well-known hosts are not taken for the other service.
    assert!(Address::parse_as("https://github.com/owner/paper", Some(Service::GitLab)).is_err());
    assert!(Address::parse_as("https://gitlab.com/group/paper", Some(Service::GitHub)).is_err());
    assert_eq!(Address::parse_as("https://gitlab.com/group/paper", Some(Service::GitLab)).unwrap().instance, GITLAB_COM);
    for bad in ["https://github.com/owner", "https://github.com/-owner/paper", "https://github.com/owner/pa per", "https://github.com/owner/.."] {
        assert!(Address::parse(bad).is_err(), "{bad:?} was accepted");
    }
}

#[test]
fn a_link_saved_before_github_is_a_gitlab_one() {
    let json = r#"{"address":{"instance":"https://gitlab.com","path":"group/paper"},"project_id":1,"web_url":"https://gitlab.com/group/paper","branch":"main","base":"0000000000000000000000000000000000000000","files":{}}"#;
    let link: Link = serde_json::from_str(json).unwrap();
    assert_eq!(link.address.service, Service::GitLab);
}

#[test]
fn a_commit_goes_through_blobs_a_tree_a_commit_and_the_branch_moved() {
    let fake = paper();
    let Loaded { link, mut files } = block_on(open(&client(&fake).unwrap(), None)).unwrap();
    files.retain(|(p, _)| p != "intro.tex");
    let png = files.iter().position(|(p, _)| p == "fig/plot.png").map(|i| files.remove(i).1).unwrap();
    files.push(("plot.png".to_owned(), png));
    files.iter_mut().filter(|(p, _)| p == "main.tex").for_each(|f| f.1 = b"changed\n".to_vec());
    files.push(("refs.bib".to_owned(), b"@book{x}\n".to_vec()));
    let found: Vec<ChangeKind> = changes(&link, &files).unwrap().into_iter().map(|c| c.kind).collect();
    assert_eq!(found.len(), 4, "{found:?}");

    let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Edit", &Choices::new())).unwrap();
    // Only new contents are sent as blobs: a moved file's is already there.
    let blobs = fake.requests().iter().filter(|r| r.contains("/git/blobs") && r.starts_with("POST")).count();
    assert_eq!(blobs, 2, "{:?}", fake.requests());
    let on_branch: BTreeMap<String, Vec<u8>> = fake.files("main");
    let ours: BTreeMap<String, Vec<u8>> = files.iter().cloned().collect();
    assert_eq!(on_branch, ours);
    let id = done.commit.unwrap();
    assert_eq!(fake.head("main"), Some(id.clone()));
    assert_eq!(fake.parent(&id), Some(link.base.clone()));
    assert!(changes(&done.link, &files).unwrap().is_empty());
}

#[test]
fn a_tree_github_lists_only_in_part_is_refused() {
    let fake = paper();
    fake.set_truncated(true);
    let err = block_on(open(&client(&fake).unwrap(), None)).unwrap_err();
    assert_eq!(err, Error::TooManyFiles { forge: Service::GitHub });
}

#[test]
fn a_rule_the_branch_does_not_show_refuses_the_commit_without_retrying() {
    let fake = paper();
    let Loaded { link, mut files } = block_on(open(&client(&fake).unwrap(), None)).unwrap();
    fake.rule("main");
    let head = fake.head("main");
    files.push(("new.tex".to_owned(), b"new\n".to_vec()));
    let err = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine", &Choices::new())).unwrap_err();
    assert_eq!(err, Error::CannotPush { forge: Service::GitHub, branch: "main".to_owned() });
    assert_eq!(fake.head("main"), head);
    assert!(err.to_string().contains("pull request"), "{err}");
}

#[test]
fn no_requests_left_is_the_rate_limit_with_its_reset_time() {
    let fake = paper();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let reset = (now + 1790).to_string();
    fake.fail_with(403, &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", &reset)], r#"{"message":"API rate limit exceeded"}"#);
    let err = block_on(open(&client(&fake).unwrap(), None)).unwrap_err();
    assert!(matches!(err, Error::RateLimited { forge: Service::GitHub, retry_after: Some(_) }), "{err:?}");
    assert!(err.to_string().contains("30 minutes"), "{err}");
    // Another 403 is the token's permissions, said in GitHub's terms.
    fake.fail_with(403, &[("x-ratelimit-remaining", "4999")], r#"{"message":"Resource not accessible by personal access token"}"#);
    let err = block_on(open(&client(&fake).unwrap(), None)).unwrap_err();
    assert!(matches!(err, Error::Forbidden { forge: Service::GitHub, .. }), "{err:?}");
    assert!(err.to_string().contains("Contents"), "{err}");
}

#[test]
fn a_wrong_token_is_said_to_be_github_s_refusal() {
    let fake = paper();
    let wrong = texcraft_forge::Client::new(fake.transport(), fake.address(), texcraft_forge::Token::new("github_pat_wrong").unwrap());
    let err = block_on(open(&wrong, None)).unwrap_err();
    assert_eq!(err, Error::Unauthorized { forge: Service::GitHub });
    assert!(err.to_string().starts_with("GitHub"), "{err}");
}

#[test]
fn a_changed_or_moved_executable_stays_executable() {
    let fake = paper();
    fake.push("main", &[("build.sh", Some(b"latexmk\n".as_slice())), ("run.sh", Some(b"run\n".as_slice()))]);
    fake.chmod("main", "build.sh", "100755");
    fake.chmod("main", "run.sh", "100755");
    let Loaded { link, mut files } = block_on(open(&client(&fake).unwrap(), None)).unwrap();
    files.iter_mut().filter(|(p, _)| p == "build.sh").for_each(|f| f.1 = b"latexmk -pdf\n".to_vec());
    files.iter_mut().filter(|(p, _)| p == "run.sh").for_each(|f| f.0 = "bin/run.sh".to_owned());
    block_on(commit(&client(&fake).unwrap(), &link, &files, "Build", &Choices::new())).unwrap();
    assert_eq!(fake.files("main")["build.sh"], b"latexmk -pdf\n");
    assert_eq!(fake.mode("main", "build.sh").as_deref(), Some("100755"));
    assert_eq!(fake.mode("main", "bin/run.sh").as_deref(), Some("100755"));
}
