//! What changed since the base, and committing it back as one commit.

use serde_json::Value;
use texcraft_forge::{ChangeKind, Error, Loaded, changes, commit, open};

use crate::{block_on, client, paper};

fn opened(fake: &texcraft_forge::fake::FakeGitLab) -> texcraft_forge::Result<Loaded> {
    block_on(open(&client(fake)?, None))
}

fn set(files: &mut Vec<(String, Vec<u8>)>, path: &str, bytes: &[u8]) {
    match files.iter_mut().find(|(p, _)| p == path) {
        Some(f) => f.1 = bytes.to_vec(),
        None => files.push((path.to_owned(), bytes.to_vec())),
    }
}

fn remove(files: &mut Vec<(String, Vec<u8>)>, path: &str) -> Vec<u8> {
    let i = files.iter().position(|(p, _)| p == path);
    i.map(|i| files.remove(i).1).unwrap_or_default()
}

#[test]
fn changes_are_found_by_blob_id_and_a_rename_is_a_move() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    assert!(changes(&link, &files).unwrap().is_empty(), "files as loaded count as changed");
    set(&mut files, "main.tex", b"changed\n");
    set(&mut files, "refs.bib", b"@book{x}\n");
    let png = remove(&mut files, "fig/plot.png");
    set(&mut files, "figures/plot.png", &png);
    remove(&mut files, "chapters/intro.tex");
    let found: Vec<(String, ChangeKind, Option<String>)> = changes(&link, &files).unwrap().into_iter().map(|c| (c.path, c.kind, c.from)).collect();
    assert_eq!(
        found,
        [
            ("chapters/intro.tex".to_owned(), ChangeKind::Deleted, None),
            ("figures/plot.png".to_owned(), ChangeKind::Moved, Some("fig/plot.png".to_owned())),
            ("main.tex".to_owned(), ChangeKind::Modified, None),
            ("refs.bib".to_owned(), ChangeKind::Added, None),
        ]
    );
}

#[test]
fn a_commit_sends_each_change_as_an_action_and_moves_the_base() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    set(&mut files, "main.tex", b"changed\n");
    set(&mut files, "img/logo.bin", b"\0\x01\x02binary");
    remove(&mut files, "chapters/intro.tex");
    let png = remove(&mut files, "fig/plot.png");
    set(&mut files, "plot.png", &png);
    let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Edit the paper")).unwrap();

    let actions = fake.last_actions();
    let of = |path: &str| actions.iter().find(|a| a["file_path"] == path).cloned().unwrap_or(Value::Null);
    assert_eq!(of("main.tex")["action"], "update");
    assert_eq!(of("img/logo.bin")["action"], "create");
    assert_eq!(of("img/logo.bin")["encoding"], "base64");
    assert_eq!(of("chapters/intro.tex")["action"], "delete");
    assert_eq!(of("plot.png")["action"], "move");
    assert_eq!(of("plot.png")["previous_path"], "fig/plot.png");
    assert!(of("plot.png").get("content").is_none(), "a move without changes sends the contents again");
    // What the branch holds is what the project holds, binary bytes included.
    let on_branch: Vec<(String, Vec<u8>)> = fake.files("main").into_iter().collect();
    let mut ours = files.clone();
    ours.sort();
    assert_eq!(on_branch, ours);
    // One commit, on the old head; it is the new base and nothing is left to commit.
    assert_eq!(done.commit, fake.head("main"));
    assert_eq!(fake.parent(done.commit.as_deref().unwrap()), Some(link.base.clone()));
    assert!(changes(&done.link, &files).unwrap().is_empty());
    assert!(done.taken.is_empty() && done.removed.is_empty() && !done.raced);
}

#[test]
fn nothing_to_commit_makes_no_commit() {
    let fake = paper();
    let Loaded { link, files } = opened(&fake).unwrap();
    let before = fake.head("main");
    let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Nothing")).unwrap();
    assert_eq!(done.commit, None);
    assert_eq!(fake.head("main"), before);
}

#[test]
fn new_commits_to_other_files_are_kept_and_taken_in() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    fake.push("main", &[("chapters/intro.tex", Some(b"Their intro\n".as_slice())), ("theirs.tex", Some(b"new\n".as_slice()))]);
    set(&mut files, "main.tex", b"mine\n");
    let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine")).unwrap();
    let branch = fake.files("main");
    assert_eq!(branch["main.tex"], b"mine\n");
    assert_eq!(branch["chapters/intro.tex"], b"Their intro\n", "their change was undone");
    let mut taken: Vec<&str> = done.taken.iter().map(|(p, _)| p.as_str()).collect();
    taken.sort();
    assert_eq!(taken, ["chapters/intro.tex", "theirs.tex"]);
    // With what was taken in, the project is the branch.
    for (path, bytes) in done.taken {
        set(&mut files, &path, &bytes);
    }
    assert!(changes(&done.link, &files).unwrap().is_empty());
}

#[test]
fn new_commits_to_the_same_files_refuse_the_commit() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    fake.push("main", &[("main.tex", Some(b"theirs\n".as_slice()))]);
    let head = fake.head("main");
    set(&mut files, "main.tex", b"mine\n");
    let err = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine")).unwrap_err();
    assert_eq!(err, Error::Conflict { branch: "main".to_owned(), files: vec!["main.tex".to_owned()] });
    assert_eq!(fake.head("main"), head, "something was committed");
}

#[test]
fn the_same_change_made_on_the_branch_is_no_conflict() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    fake.push("main", &[("main.tex", Some(b"same\n".as_slice()))]);
    set(&mut files, "main.tex", b"same\n");
    set(&mut files, "extra.tex", b"x\n");
    let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine")).unwrap();
    assert!(done.commit.is_some());
    assert!(fake.last_actions().iter().all(|a| a["file_path"] != "main.tex"), "the branch's own change was sent again");
}

#[test]
fn a_commit_made_meanwhile_is_said_and_taken_in() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    fake.race("main", "late.tex", b"late\n");
    set(&mut files, "main.tex", b"mine\n");
    let done = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine")).unwrap();
    assert!(done.raced);
    assert!(done.taken.iter().any(|(p, _)| p == "late.tex"));
}

#[test]
fn a_protected_branch_is_refused_before_anything_is_sent() {
    let fake = paper();
    let Loaded { link, mut files } = opened(&fake).unwrap();
    fake.set_can_push(false);
    set(&mut files, "main.tex", b"mine\n");
    let err = block_on(commit(&client(&fake).unwrap(), &link, &files, "Mine")).unwrap_err();
    assert!(matches!(&err, Error::Forbidden { what } if what.contains("protected")), "{err:?}");
    assert!(!fake.requests().iter().any(|r| r.starts_with("POST")));
}
