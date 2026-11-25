//! Keeping up with the branch: its new commits taken in and merged, conflicts and their
//! resolutions, a new branch with a merge request, and the review threads on the branch.

use texcraft_forge::fake::Fake;
use texcraft_forge::{Choice, Choices, Conflict, ConflictKind, Error, Link, Loaded, Service, changes, commit, commit_to_new_branch, open, pull};

use crate::{block_on, both, client};

/// Paths and bytes.
type Files = Vec<(String, Vec<u8>)>;

const TEXT: &[u8] = b"\\documentclass{article}\n\\begin{document}\nOne.\n\nTwo.\n\nThree.\n\\end{document}\n";

/// The project on a fake GitLab and a fake GitHub.
fn fakes() -> [Fake; 2] {
    both(&[("main.tex", TEXT), ("intro.tex", b"Intro\n"), ("fig.png", b"\x89PNG\0\x01")])
}

fn opened(fake: &Fake) -> texcraft_forge::Result<Loaded> {
    block_on(open(&client(fake)?, None))
}

fn set(files: &mut Vec<(String, Vec<u8>)>, path: &str, bytes: &[u8]) {
    match files.iter_mut().find(|(p, _)| p == path) {
        Some(f) => f.1 = bytes.to_vec(),
        None => files.push((path.to_owned(), bytes.to_vec())),
    }
}

fn get<'a>(files: &'a [(String, Vec<u8>)], path: &str) -> Option<&'a [u8]> {
    files.iter().find(|(p, _)| p == path).map(|(_, b)| b.as_slice())
}

/// `files` with `updates` put in.
fn apply(files: &mut Vec<(String, Vec<u8>)>, updates: Vec<texcraft_forge::Update>) {
    for (path, bytes) in updates {
        match bytes {
            Some(b) => set(files, &path, &b),
            None => files.retain(|(p, _)| *p != path),
        }
    }
}

fn edit(text: &[u8], from: &str, to: &str) -> Vec<u8> {
    String::from_utf8_lossy(text).replace(from, to).into_bytes()
}

fn no_choices() -> Choices {
    Choices::new()
}

#[test]
fn a_pull_takes_in_what_changed_only_on_the_branch() {
    for fake in fakes() {
        let Loaded { link, mut files } = opened(&fake).unwrap();
        let nothing = block_on(pull(&client(&fake).unwrap(), &link, &files, &no_choices())).unwrap();
        assert!(!nothing.moved && nothing.updates.is_empty());

        fake.push("main", &[("intro.tex", Some(b"Their intro\n".as_slice())), ("fig.png", None), ("refs.bib", Some(b"@book{x}\n".as_slice()))]);
        set(&mut files, "main.tex", &edit(TEXT, "One.", "Mine."));
        let pulled = block_on(pull(&client(&fake).unwrap(), &link, &files, &no_choices())).unwrap();
        assert!(pulled.moved && pulled.merged.is_empty());
        assert_eq!(pulled.link.base, fake.head("main").unwrap());
        let paths: Vec<(&str, bool)> = pulled.updates.iter().map(|(p, b)| (p.as_str(), b.is_some())).collect();
        assert_eq!(paths, [("fig.png", false), ("intro.tex", true), ("refs.bib", true)]);
        apply(&mut files, pulled.updates);
        // Only the change made here is left to commit.
        let left: Vec<String> = changes(&pulled.link, &files).unwrap().into_iter().map(|c| c.path).collect();
        assert_eq!(left, ["main.tex"]);
        assert!(!fake.requests().iter().any(|r| r.starts_with("POST") || r.starts_with("PATCH")), "a pull sends nothing");
    }
}

#[test]
fn changes_to_different_parts_of_a_file_are_merged() {
    for fake in fakes() {
        let Loaded { link, mut files } = opened(&fake).unwrap();
        fake.push("main", &[("main.tex", Some(edit(TEXT, "Three.", "Their three.").as_slice()))]);
        set(&mut files, "main.tex", &edit(TEXT, "One.", "My one."));
        let pulled = block_on(pull(&client(&fake).unwrap(), &link, &files, &no_choices())).unwrap();
        assert_eq!(pulled.merged, ["main.tex"]);
        apply(&mut files, pulled.updates);
        let text = String::from_utf8_lossy(get(&files, "main.tex").unwrap()).into_owned();
        assert!(text.contains("My one.") && text.contains("Their three."), "{text}");

        // A commit merges the same way, and sends the merged file on top of the branch.
        let Loaded { link, mut files } = opened(&fake).unwrap();
        fake.push("main", &[("main.tex", Some(edit(&fake.files("main")["main.tex"], "Two.", "Their two.").as_slice()))]);
        let mine = edit(get(&files, "main.tex").unwrap(), "\\end{document}", "Mine at the end.\n\\end{document}");
        set(&mut files, "main.tex", &mine);
        let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine", &no_choices())).unwrap();
        assert_eq!(done.merged, ["main.tex"]);
        let on_branch = String::from_utf8_lossy(&fake.files("main")["main.tex"]).into_owned();
        assert!(on_branch.contains("Their two.") && on_branch.contains("Mine at the end."), "{on_branch}");
        apply(&mut files, done.updates);
        assert!(changes(&done.link, &files).unwrap().is_empty(), "the project is what was committed");
    }
}

/// A project whose `main.tex` changed on both sides on the same line, and `intro.tex` on the
/// branch only.
fn in_conflict(fake: &Fake) -> texcraft_forge::Result<(Link, Files)> {
    let Loaded { link, mut files } = opened(fake)?;
    fake.push("main", &[("main.tex", Some(edit(TEXT, "Two.", "Their two.").as_slice())), ("intro.tex", Some(b"Their intro\n".as_slice()))]);
    set(&mut files, "main.tex", &edit(TEXT, "Two.", "My two."));
    Ok((link, files))
}

#[test]
fn overlapping_changes_are_conflicts_and_nothing_is_taken() {
    for fake in fakes() {
        let (link, mut files) = in_conflict(&fake).unwrap();
        // A binary file changed on both sides, a file deleted here and changed there.
        fake.push("main", &[("fig.png", Some(b"\x89PNG\0theirs".as_slice()))]);
        set(&mut files, "fig.png", b"\x89PNG\0mine");
        let err = block_on(pull(&client(&fake).unwrap(), &link, &files, &no_choices())).unwrap_err();
        let Error::Conflict { branch, files: conflicts, .. } = err else { panic!("not a conflict: {err:?}") };
        assert_eq!(branch, "main");
        assert_eq!(
            conflicts,
            [
                Conflict { path: "fig.png".to_owned(), kind: ConflictKind::Binary },
                Conflict { path: "main.tex".to_owned(), kind: ConflictKind::Overlap }
            ]
        );
    }
}

#[test]
fn a_file_deleted_on_one_side_and_changed_on_the_other_is_a_conflict() {
    for fake in fakes() {
        let Loaded { link, mut files } = opened(&fake).unwrap();
        fake.push("main", &[("intro.tex", None), ("main.tex", Some(edit(TEXT, "One.", "Their one.").as_slice()))]);
        set(&mut files, "intro.tex", b"My intro\n");
        files.retain(|(p, _)| p != "main.tex");
        let err = block_on(pull(&client(&fake).unwrap(), &link, &files, &no_choices())).unwrap_err();
        let Error::Conflict { files: conflicts, .. } = err else { panic!("not a conflict: {err:?}") };
        let kinds: Vec<(&str, ConflictKind)> = conflicts.iter().map(|c| (c.path.as_str(), c.kind)).collect();
        assert_eq!(kinds, [("intro.tex", ConflictKind::DeletedThere), ("main.tex", ConflictKind::DeletedHere)]);
    }
}

#[test]
fn keeping_mine_commits_it_over_the_branch() {
    for fake in fakes() {
        let (link, mut files) = in_conflict(&fake).unwrap();
        let mine = get(&files, "main.tex").unwrap().to_vec();
        let choices = Choices::from([("main.tex".to_owned(), Choice::Mine)]);
        let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine", &choices)).unwrap();
        assert!(done.commit.is_some());
        assert_eq!(fake.files("main")["main.tex"], mine);
        assert_eq!(fake.files("main")["intro.tex"], b"Their intro\n", "the other file of the branch stays");
        let paths: Vec<&str> = done.updates.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["intro.tex"], "only the file without conflict comes in");
        apply(&mut files, done.updates);
        assert!(changes(&done.link, &files).unwrap().is_empty());
    }
}

#[test]
fn taking_theirs_drops_the_change_made_here() {
    for fake in fakes() {
        let (link, mut files) = in_conflict(&fake).unwrap();
        let head = fake.head("main");
        let choices = Choices::from([("main.tex".to_owned(), Choice::Theirs)]);
        let pulled = block_on(pull(&client(&fake).unwrap(), &link, &files, &choices)).unwrap();
        apply(&mut files, pulled.updates);
        assert_eq!(get(&files, "main.tex").unwrap(), fake.files("main")["main.tex"].as_slice());
        assert!(changes(&pulled.link, &files).unwrap().is_empty());
        assert_eq!(fake.head("main"), head, "a pull commits nothing");
    }
}

#[test]
fn a_conflict_can_go_to_a_new_branch_with_a_merge_request() {
    for fake in fakes() {
        let (link, files) = in_conflict(&fake).unwrap();
        let main_head = fake.head("main");
        let done = block_on(commit_to_new_branch(&client(&fake).unwrap(), &link, &files, "My two\n\nDetails.", "my-two")).unwrap();
        // The new branch starts from the old base, with the changes made here.
        let id = done.commit.clone().unwrap();
        assert_eq!(fake.head("my-two"), Some(id.clone()));
        assert_eq!(fake.parent(&id), Some(link.base.clone()));
        assert_eq!(fake.files("my-two")["main.tex"], get(&files, "main.tex").unwrap());
        assert_eq!(fake.head("main"), main_head, "the branch is left alone");
        let mrs = fake.merge_requests();
        assert_eq!(mrs.len(), 1);
        assert_eq!((mrs[0].source.as_str(), mrs[0].target.as_str(), mrs[0].title.as_str()), ("my-two", "main", "My two"));
        let mr = done.merge_request.unwrap();
        let page = match fake.address().service {
            Service::GitLab => "merge_requests",
            Service::GitHub => "pull",
        };
        assert!(mr.web_url.starts_with(&fake.address().instance) && mr.web_url.ends_with(&format!("/{page}/{}", mr.iid)), "{}", mr.web_url);
        // The project follows the new branch, and nothing is left to commit.
        assert_eq!(done.link.branch, "my-two");
        assert!(changes(&done.link, &files).unwrap().is_empty());
    }
}

#[test]
fn a_protected_branch_takes_its_changes_through_a_merge_request() {
    for fake in fakes() {
        let Loaded { link, mut files } = opened(&fake).unwrap();
        fake.protect("main");
        set(&mut files, "intro.tex", b"Mine\n");
        let err = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine", &no_choices())).unwrap_err();
        assert_eq!(err, Error::CannotPush { forge: fake.address().service, branch: "main".to_owned() });
        let done = block_on(commit_to_new_branch(&client(&fake).unwrap(), &link, &files, "Mine", "mine")).unwrap();
        assert!(done.merge_request.is_some());
        assert_eq!(fake.files("mine")["intro.tex"], b"Mine\n");
    }
}

#[test]
fn a_new_branch_needs_a_new_valid_name() {
    for fake in fakes() {
        let Loaded { link, mut files } = opened(&fake).unwrap();
        set(&mut files, "intro.tex", b"Mine\n");
        for bad in ["", "main", "with space", "a..b", "-x", "x.lock"] {
            let err = block_on(commit_to_new_branch(&client(&fake).unwrap(), &link, &files, "Mine", bad)).unwrap_err();
            assert!(matches!(err, Error::BranchName(_)), "{bad:?}: {err:?}");
        }
        fake.push("taken", &[]);
        let err = block_on(commit_to_new_branch(&client(&fake).unwrap(), &link, &files, "Mine", "taken")).unwrap_err();
        assert!(matches!(&err, Error::Refused { status: 400 | 422, message, .. } if message.contains("already exists")), "{err:?}");
        assert!(fake.merge_requests().is_empty());
    }
}

#[test]
fn review_threads_are_loaded_committed_and_pulled_but_no_other_hidden_file() {
    for fake in fakes() {
        fake.push(
            "main",
            &[
                (".texcraft/review/a.json", Some(b"{\"thread\": \"a\"}\n".as_slice())),
                (".texcraft/settings.json", Some(b"{}\n".as_slice())),
                (".texcraft/review/.hidden", Some(b"x\n".as_slice())),
                (".gitignore", Some(b"*.aux\n".as_slice())),
            ],
        );
        let Loaded { link, mut files } = opened(&fake).unwrap();
        let loaded: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).filter(|p| p.starts_with('.')).collect();
        assert_eq!(loaded, [".texcraft/review/a.json"]);

        // A new thread here is committed; a thread changed there is pulled.
        set(&mut files, ".texcraft/review/b.json", b"{\"thread\": \"b\"}\n");
        let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Thread", &no_choices())).unwrap();
        assert_eq!(fake.files("main")[".texcraft/review/b.json"], b"{\"thread\": \"b\"}\n");
        fake.push("main", &[(".texcraft/review/a.json", Some(b"{\"thread\": \"a2\"}\n".as_slice()))]);
        let pulled = block_on(pull(&client(&fake).unwrap(), &done.link, &files, &no_choices())).unwrap();
        assert_eq!(pulled.updates, [(".texcraft/review/a.json".to_owned(), Some(b"{\"thread\": \"a2\"}\n".to_vec()))]);
        // The other hidden files stayed on the branch as they were.
        assert!(fake.files("main").contains_key(".texcraft/settings.json") && fake.files("main").contains_key(".gitignore"));
    }
}
