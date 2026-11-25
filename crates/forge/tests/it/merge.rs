//! The three-way merge of a text file, line by line.

use texcraft_forge::merge::{is_text, merge3};

fn merged(base: &str, ours: &str, theirs: &str) -> Option<String> {
    merge3(base.as_bytes(), ours.as_bytes(), theirs.as_bytes()).map(|b| String::from_utf8_lossy(&b).into_owned())
}

const BASE: &str = "one\ntwo\nthree\nfour\nfive\nsix\n";

#[test]
fn changes_to_different_lines_are_both_kept() {
    let ours = "ONE\ntwo\nthree\nfour\nfive\nsix\n";
    let theirs = "one\ntwo\nthree\nfour\nFIVE\nsix\nseven\n";
    assert_eq!(merged(BASE, ours, theirs).as_deref(), Some("ONE\ntwo\nthree\nfour\nFIVE\nsix\nseven\n"));
    // The same from the other side.
    assert_eq!(merged(BASE, theirs, ours).as_deref(), Some("ONE\ntwo\nthree\nfour\nFIVE\nsix\nseven\n"));
}

#[test]
fn insertions_and_deletions_in_different_places_merge() {
    let ours = "zero\none\ntwo\nthree\nfour\nfive\nsix\n";
    let theirs = "one\ntwo\nfour\nfive\nsix\n";
    assert_eq!(merged(BASE, ours, theirs).as_deref(), Some("zero\none\ntwo\nfour\nfive\nsix\n"));
}

#[test]
fn a_change_made_on_one_side_only_is_that_side() {
    let theirs = "one\n2\nthree\nfour\nfive\nsix\n";
    assert_eq!(merged(BASE, BASE, theirs).as_deref(), Some(theirs));
    assert_eq!(merged(BASE, theirs, BASE).as_deref(), Some(theirs));
}

#[test]
fn the_same_change_on_both_sides_is_no_conflict() {
    let both = "one\ntwo\n3\nfour\nfive\nsix\n";
    assert_eq!(merged(BASE, both, both).as_deref(), Some(both));
    let ours = "one\ntwo\n3\nfour\nfive\nSIX\n";
    assert_eq!(merged(BASE, ours, both).as_deref(), Some(ours));
}

#[test]
fn overlapping_changes_are_a_conflict() {
    assert_eq!(merged(BASE, "one\ntwo\nmine\nfour\nfive\nsix\n", "one\ntwo\ntheirs\nfour\nfive\nsix\n"), None);
    // Changes to neighbouring lines overlap too, as in git.
    assert_eq!(merged(BASE, "one\ntwo\nmine\nfour\nfive\nsix\n", "one\ntwo\nthree\ntheirs\nfive\nsix\n"), None);
    // Different insertions at the same place.
    assert_eq!(merged(BASE, "one\nmine\ntwo\nthree\nfour\nfive\nsix\n", "one\ntheirs\ntwo\nthree\nfour\nfive\nsix\n"), None);
    // A deletion against an edit of the same line.
    assert_eq!(merged(BASE, "one\ntwo\nfour\nfive\nsix\n", "one\ntwo\nTHREE\nfour\nfive\nsix\n"), None);
}

#[test]
fn line_endings_are_kept() {
    let base = BASE.replace('\n', "\r\n");
    let ours = base.replace("one", "ONE");
    // Theirs comes with `\n`: the merged file keeps the local `\r\n` everywhere.
    let theirs = "one\ntwo\nthree\nfour\nFIVE\nsix\n";
    assert_eq!(merged(&base, &ours, theirs), Some("ONE\r\ntwo\r\nthree\r\nfour\r\nFIVE\r\nsix\r\n".to_owned()));
    // A file without a final line ending keeps none.
    assert_eq!(merged("a\nb\nc", "A\nb\nc", "a\nb\nC").as_deref(), Some("A\nb\nC"));
    // Adding the final line ending is a change.
    assert_eq!(merged("a\nb\nc", "A\nb\nc", "a\nb\nc\n").as_deref(), Some("A\nb\nc\n"));
}

#[test]
fn binary_files_do_not_merge() {
    assert!(!is_text(b"\x89PNG\0\x01") && !is_text(&[0xff, 0xfe]) && is_text("é\n".as_bytes()));
    assert_eq!(merge3(b"a\n", b"a\0\n", b"b\n"), None);
    assert_eq!(merge3(b"a\n", b"a\n", &[0xff, b'\n']), None);
}
