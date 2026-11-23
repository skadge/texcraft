use texcraft_review::{Anchor, Placement};

/// `text` with `start..end` replaced, as an edit: the new text and `(start, end, inserted)`.
fn edit(text: &str, start: usize, end: usize, with: &str) -> (String, [(usize, usize, usize); 1]) {
    let chars: Vec<char> = text.chars().collect();
    let new: String = chars[..start].iter().chain(with.chars().collect::<Vec<_>>().iter()).chain(chars[end..].iter()).collect();
    (new, [(start, end, with.chars().count())])
}

fn passage(text: &str, a: &Anchor) -> String {
    text.chars().skip(a.start).take(a.end - a.start).collect()
}

#[test]
fn an_anchor_keeps_its_passage_through_edits_before_inside_and_after_it() {
    let mut text = "Intro. The quick brown fox jumps. Outro.".to_owned();
    let mut a = Anchor::new("main.tex", &text, 7, 33);
    assert_eq!(passage(&text, &a), "The quick brown fox jumps.");
    for (start, end, with) in [(0, 0, "Très "), (0, 6, "Préface"), (10, 10, ">>"), (a.end + 2, a.end + 2, "<<")] {
        let (new, changes) = edit(&text, start, end, with);
        a.follow(&changes);
        text = new;
    }
    assert!(passage(&text, &a).starts_with("The quick"), "{}", passage(&text, &a));
    // Typed exactly at either end: outside the passage.
    let (new, changes) = edit(&text, a.start, a.start, "X");
    a.follow(&changes);
    text = new;
    let (new, changes) = edit(&text, a.end, a.end, "Y");
    a.follow(&changes);
    text = new;
    let inside = passage(&text, &a);
    assert!(inside.starts_with("The") && inside.ends_with("jumps."), "{inside}");
    // Inside: the passage grows and shrinks with it.
    let (new, changes) = edit(&text, a.start + 4, a.start + 9, "slow");
    a.follow(&changes);
    assert!(passage(&new, &a).starts_with("The slow brown"), "{}", passage(&new, &a));
    assert_eq!(a.placement, Placement::Anchored);
}

#[test]
fn a_deleted_passage_leaves_an_empty_anchor_marked_removed() {
    let text = "Keep this. Delete this sentence. Keep that.";
    let mut a = Anchor::new("main.tex", text, 11, 32);
    let (new, changes) = edit(text, 9, 33, "");
    a.follow(&changes);
    assert_eq!(a.placement, Placement::Removed);
    assert_eq!(a.start, a.end);
    assert!(a.start <= new.chars().count());
    assert_eq!(a.quote.exact, "Delete this sentence.", "the quote went with the text");
    a.requote(&new);
    assert_eq!(a.quote.exact, "Delete this sentence.", "saving forgot the quote of a removed passage");
}

#[test]
fn a_saved_anchor_is_placed_on_its_quote_or_marked_unanchored() {
    let saved = "line one\nthe passage\nline three\n";
    let anchor = Anchor::new("main.tex", saved, 9, 20);
    // The text did not change: the range stays.
    let mut a = anchor.clone();
    a.place(saved);
    assert_eq!((a.start, a.end, a.placement), (9, 20, Placement::Anchored));
    // Lines were added before it elsewhere: it is found by its quote, near its line.
    let moved = "added\nadded\nline one\nthe passage\nline three\n".to_owned() + &"x\n".repeat(20) + "the passage\n";
    let mut a = anchor.clone();
    a.place(&moved);
    assert_eq!(passage(&moved, &a), "the passage");
    assert_eq!(moved.chars().take(a.start).filter(|c| *c == '\n').count(), 3, "it took the occurrence far from its line");
    // The passage is gone.
    let mut a = anchor;
    a.place("something else entirely");
    assert_eq!(a.placement, Placement::Unanchored);
}
