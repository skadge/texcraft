use texcraft_buffer::{Bias, Buffer, Change, EditKind, Selection, Transaction};

/// Types `text` one character at a time at the caret, `step` seconds apart from `t`.
fn type_text(b: &mut Buffer, caret: &mut usize, text: &str, t: &mut f64, step: f64) -> texcraft_buffer::Result<()> {
    for c in text.chars() {
        let before = Selection::point(*caret);
        let after = Selection::point(*caret + 1);
        b.edit(Transaction::insert(*caret, &c.to_string()), &before, &after, EditKind::Typing { at: *t })?;
        *caret += 1;
        *t += step;
    }
    Ok(())
}

#[test]
fn undo_takes_back_typing_a_word_at_a_time() {
    let mut b = Buffer::new("");
    let (mut caret, mut t) = (0, 0.0);
    type_text(&mut b, &mut caret, "hello world", &mut t, 0.1).unwrap();
    assert_eq!(b.undo(), Some(Selection::point(6)), "the caret goes back to where the word started");
    assert_eq!(b.text(), "hello ");
    b.undo();
    assert_eq!(b.text(), "");
    assert!(!b.can_undo());
}

#[test]
fn a_pause_or_a_caret_move_starts_a_new_undo_group() {
    let mut b = Buffer::new("");
    let (mut caret, mut t) = (0, 0.0);
    type_text(&mut b, &mut caret, "ab", &mut t, 0.1).unwrap();
    t += 5.0;
    type_text(&mut b, &mut caret, "cd", &mut t, 0.1).unwrap();
    b.undo();
    assert_eq!(b.text(), "ab", "a pause did not end the group");

    let mut b = Buffer::new("");
    let (mut caret, mut t) = (0, 0.0);
    type_text(&mut b, &mut caret, "ab", &mut t, 0.1).unwrap();
    caret = 0;
    type_text(&mut b, &mut caret, "x", &mut t, 0.1).unwrap();
    b.undo();
    assert_eq!(b.text(), "ab", "typing elsewhere joined the earlier group");
}

#[test]
fn a_run_of_backspaces_is_undone_at_once() {
    let mut b = Buffer::new("abcdef");
    for (i, at) in [(6, 0.0), (5, 0.1), (4, 0.2)] {
        b.edit(Transaction::delete(i - 1, i), &Selection::point(i), &Selection::point(i - 1), EditKind::Deleting { at }).unwrap();
    }
    assert_eq!(b.text(), "abc");
    assert_eq!(b.undo(), Some(Selection::point(6)));
    assert_eq!(b.text(), "abcdef");
}

#[test]
fn edits_inside_a_group_are_undone_as_one_and_redone_as_one() {
    let mut b = Buffer::new("one two");
    let s = Selection::point(0);
    b.begin_group();
    b.edit(Transaction::replace(0, 3, "ONE"), &s, &s, EditKind::Other).unwrap();
    b.edit(Transaction::insert(7, "!"), &s, &s, EditKind::Other).unwrap();
    b.end_group();
    b.edit(Transaction::insert(0, ">"), &s, &s, EditKind::Other).unwrap();
    b.undo();
    b.undo();
    assert_eq!(b.text(), "one two");
    b.redo();
    assert_eq!(b.text(), "ONE two!");
}

#[test]
fn a_new_edit_after_undo_drops_the_redo() {
    let mut b = Buffer::new("a");
    let s = Selection::point(0);
    b.edit(Transaction::insert(1, "b"), &s, &s, EditKind::Other).unwrap();
    b.undo();
    b.edit(Transaction::insert(1, "c"), &s, &s, EditKind::Other).unwrap();
    assert!(!b.can_redo());
    assert_eq!(b.redo(), None);
    assert_eq!(b.text(), "ac");
}

#[test]
fn marks_follow_edits_before_them_and_keep_their_side_of_an_insertion() {
    let mut b = Buffer::new("abc def");
    let left = b.add_mark(4, Bias::Left);
    let right = b.add_mark(4, Bias::Right);
    let s = Selection::point(0);
    b.edit(Transaction::insert(0, "xx"), &s, &s, EditKind::Other).unwrap();
    assert_eq!((b.mark(left), b.mark(right)), (Some(6), Some(6)));
    b.edit(Transaction::insert(6, "--"), &s, &s, EditKind::Other).unwrap();
    assert_eq!(b.mark(left), Some(6), "a left mark moved past text inserted at it");
    assert_eq!(b.mark(right), Some(8), "a right mark stayed before text inserted at it");
    b.undo();
    b.undo();
    assert_eq!((b.mark(left), b.mark(right)), (Some(4), Some(4)), "undo did not move the marks back");
}

#[test]
fn overlapping_or_out_of_bounds_changes_are_refused() {
    let overlap = vec![Change { start: 0, end: 3, text: String::new() }, Change { start: 2, end: 4, text: String::new() }];
    assert!(Transaction::new(overlap).is_err());
    let mut b = Buffer::new("abc");
    let s = Selection::point(0);
    assert!(b.edit(Transaction::delete(1, 9), &s, &s, EditKind::Other).is_err());
    assert_eq!(b.text(), "abc");
}

/// A small deterministic generator, so the test needs no dependency.
struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

#[test]
fn undoing_random_multi_change_edits_gives_back_every_earlier_text() {
    let mut rng = Lcg(7);
    let words = ["", "a", "é", "😀", "\n", "xyz\n", "\\begin{x}"];
    let mut b = Buffer::new("Le petit chat\nest là 😀.\n");
    let mut texts = vec![b.text()];
    let s = Selection::point(0);
    for _ in 0..200 {
        let len = b.len_chars();
        let mut changes = Vec::new();
        let mut at = 0;
        for _ in 0..3 {
            if at > len {
                break;
            }
            let start = at + rng.below(len - at + 1);
            let end = start + rng.below(len - start + 1).min(3);
            changes.push(Change { start, end, text: words[rng.below(words.len())].to_owned() });
            at = end + 1;
        }
        b.edit(Transaction::new(changes).unwrap(), &s, &s, EditKind::Other).unwrap();
        texts.push(b.text());
    }
    texts.pop();
    while let Some(expected) = texts.pop() {
        if b.undo().is_none() {
            break;
        }
        assert_eq!(b.text(), expected);
    }
}
