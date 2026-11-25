//! Remotes in the configuration, ahead and behind, and moving the branch with the work tree.

use std::path::Path;

use texcraft_history::{ChangeKind, History, Merged};

use super::{R, Scratch, ada, now, snapshot};

fn write(dir: &Path, files: &[(&str, &str)]) -> R {
    for (path, text) in files {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(file, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Commits `files` as the project, written on disk too; the commit's id.
fn save(h: &History, dir: &Path, files: &[(&str, &str)], message: &str) -> Result<String, String> {
    write(dir, files)?;
    h.commit(&snapshot(files), message, Some(&ada()), now())?.ok_or_else(|| "nothing committed".to_owned())
}

fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).unwrap_or_default()
}

fn git_installed() -> bool {
    std::process::Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn a_remote_and_the_upstream_are_kept_in_the_configuration_with_what_was_there() -> R {
    let s = Scratch::new("remote-config")?;
    let mut h = History::open_or_init(&s.0)?;
    assert!(h.remotes()?.is_empty());
    h.set_remote("origin", "https://gitlab.com/ada/paper.git")?;
    h.set_upstream("origin", "main")?;
    assert_eq!(h.remote_url("origin")?.as_deref(), Some("https://gitlab.com/ada/paper.git"));
    assert_eq!(h.upstream()?, Some(("origin".to_owned(), "main".to_owned())));
    // Set again, the address changes and stays one.
    h.set_remote("origin", "https://github.com/ada/paper.git")?;
    assert_eq!(h.remotes()?.len(), 1);
    assert_eq!(h.remote_url("origin")?.as_deref(), Some("https://github.com/ada/paper.git"));
    // TeXCraft's mark is still there: the repository is still ours.
    assert_eq!(History::find(&s.0)?.map(|h| h.owner()), Some(texcraft_history::Owner::Ours));
    assert!(h.set_remote("-bad", "x").is_err());
    assert!(h.set_remote("origin", "one\ntwo").is_err());

    if git_installed() {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&s.0)
            .args(["config", "--get", "remote.origin.url"])
            .output()
            .map_err(|e| e.to_string())?;
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "https://github.com/ada/paper.git", "git sees the remote");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&s.0)
            .args(["config", "--get", "branch.main.merge"])
            .output()
            .map_err(|e| e.to_string())?;
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "refs/heads/main");
    }
    Ok(())
}

#[test]
fn a_fast_forward_moves_the_branch_and_the_files_on_disk() -> R {
    let s = Scratch::new("fast-forward")?;
    let h = History::open_or_init(&s.0)?;
    let first = save(&h, &s.0, &[("main.tex", "one"), ("old.tex", "old")], "first")?;
    let second = save(&h, &s.0, &[("main.tex", "two"), ("new/part.tex", "new")], "second")?;
    // Back to the first commit, as a clone that is behind.
    h.set_ref("refs/heads/main", &first, "test")?;
    std::fs::remove_dir_all(s.0.join("new")).map_err(|e| e.to_string())?;
    write(&s.0, &[("main.tex", "one"), ("old.tex", "old")])?;
    assert_eq!(h.ahead_behind(&first, &second)?, (0, 1));

    let changed = h.fast_forward(&second)?;
    assert_eq!(h.head()?.as_deref(), Some(second.as_str()));
    assert_eq!(read(&s.0, "main.tex"), "two");
    assert!(!s.0.join("old.tex").exists(), "a file the commit deleted goes");
    assert_eq!(read(&s.0, "new/part.tex"), "new");
    assert!(changed.contains(&("old.tex".to_owned(), ChangeKind::Deleted)));
    assert!(changed.contains(&("new/part.tex".to_owned(), ChangeKind::Added)));
    assert!(changed.contains(&("main.tex".to_owned(), ChangeKind::Modified)));
    if git_installed() {
        let out = std::process::Command::new("git").arg("-C").arg(&s.0).args(["status", "--porcelain"]).output().map_err(|e| e.to_string())?;
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "", "git sees a clean work tree");
    }
    Ok(())
}

#[test]
fn a_change_on_disk_that_is_not_committed_stops_the_move_before_anything_is_written() -> R {
    let s = Scratch::new("ff-dirty")?;
    let h = History::open_or_init(&s.0)?;
    let first = save(&h, &s.0, &[("main.tex", "one")], "first")?;
    let second = save(&h, &s.0, &[("main.tex", "two")], "second")?;
    h.set_ref("refs/heads/main", &first, "test")?;
    write(&s.0, &[("main.tex", "edited outside")])?;
    let refused = h.fast_forward(&second).unwrap_err();
    assert!(refused.contains("main.tex"), "{refused}");
    assert_eq!(h.head()?.as_deref(), Some(first.as_str()), "the branch did not move");
    assert_eq!(read(&s.0, "main.tex"), "edited outside");
    Ok(())
}

#[test]
fn diverged_branches_merge_file_by_file_or_report_conflicts_and_change_nothing() -> R {
    let s = Scratch::new("merge")?;
    let h = History::open_or_init(&s.0)?;
    let base = save(&h, &s.0, &[("main.tex", "base"), ("a.tex", "a"), ("b.tex", "b")], "base")?;
    let theirs = save(&h, &s.0, &[("main.tex", "theirs"), ("a.tex", "a"), ("b.tex", "b2")], "theirs")?;
    h.set_ref("refs/heads/main", &base, "test")?;
    write(&s.0, &[("main.tex", "base"), ("b.tex", "b")])?;
    let ours = save(&h, &s.0, &[("main.tex", "ours"), ("a.tex", "a2"), ("b.tex", "b")], "ours")?;
    assert_eq!(h.ahead_behind(&ours, &theirs)?, (1, 1));
    assert!(!h.is_ancestor(&theirs, &ours)?);

    // A text merge that does not work: a conflict, nothing changed.
    let refuse = |_: &[u8], _: &[u8], _: &[u8]| None;
    let conflicts = h.merge(&theirs, "Merge", Some(&ada()), now(), &refuse)?;
    assert_eq!(conflicts, Merged::Conflicts(vec![("main.tex".to_owned(), "changed on both sides")]));
    assert_eq!(h.head()?.as_deref(), Some(ours.as_str()));
    assert_eq!(read(&s.0, "b.tex"), "b");

    // One that does: each side's changes are kept, and the merge has both parents.
    let join = |_: &[u8], o: &[u8], t: &[u8]| Some([o, b"+", t].concat());
    let Merged::Done { commit, changed } = h.merge(&theirs, "Merge", Some(&ada()), now(), &join)? else { return Err("not merged".into()) };
    assert_eq!(read(&s.0, "main.tex"), "ours+theirs");
    assert_eq!(read(&s.0, "a.tex"), "a2", "our change kept");
    assert_eq!(read(&s.0, "b.tex"), "b2", "theirs taken");
    assert!(changed.contains(&("b.tex".to_owned(), ChangeKind::Modified)));
    assert!(!changed.iter().any(|(p, _)| p == "a.tex"), "a file only we changed does not change on disk");
    assert!(h.is_ancestor(&theirs, &commit)? && h.is_ancestor(&ours, &commit)?);
    assert_eq!(h.ahead_behind(&commit, &theirs)?, (2, 0));
    Ok(())
}

#[test]
fn a_saved_project_has_nothing_uncommitted_and_an_edit_does() -> R {
    let s = Scratch::new("uncommitted")?;
    let h = History::open_or_init(&s.0)?;
    save(&h, &s.0, &[("main.tex", "one")], "first")?;
    assert!(!h.uncommitted(&snapshot(&[("main.tex", "one")]))?);
    assert!(h.uncommitted(&snapshot(&[("main.tex", "two")]))?);
    Ok(())
}
