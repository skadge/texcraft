//! R2: suggested changes, how they follow the text, accepting and rejecting them, and Suggest
//! mode's reading of edits of the shown text.

use texcraft_review::view::{Caret, Op, View};
use texcraft_review::{Anchor, Author, Error, Filter, Id, Review, State, format};

use super::{ada, at};

type R = Result<(), Box<dyn std::error::Error>>;

fn bob() -> Author {
    Author::new("Bob", "b0b0b0b0b0b0b0b0")
}

/// `text` with `start..end` (characters) replaced by `with`.
fn replaced(text: &str, start: usize, end: usize, with: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars[..start].iter().collect::<String>() + with + &chars[end..].iter().collect::<String>()
}

/// Suggest mode, as the editor does it: each edit of the shown text becomes changes of the
/// suggestions, and the text is shown again.
struct Suggesting {
    source: String,
    review: Review,
    who: Author,
    caret: usize,
    next: u128,
}

impl Suggesting {
    fn new(source: &str, who: Author) -> Self {
        Self { source: source.to_owned(), review: Review::default(), who, caret: 0, next: 1 }
    }

    fn view(&self) -> View {
        View::new(&self.source, self.review.threads())
    }

    fn edit(&mut self, start: usize, end: usize, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        let view = self.view();
        let plan = view.interpret(&self.source, start, end, text, &self.who).map_err(|a| format!("refused: in {}'s suggestion", a.name))?;
        let mut made = None;
        for op in plan.ops {
            match op {
                Op::New { start, end, text } => {
                    let id = Id::new(1_000, self.next);
                    self.next += 1;
                    let anchor = Anchor::new("main.tex", &self.source, start, end);
                    self.review.suggest(id, anchor, &self.source, &text, self.who.clone(), at(1))?;
                    made = Some(id);
                }
                Op::Revise { id, start, end, text } => {
                    let anchor = Anchor::new("main.tex", &self.source, start, end);
                    self.review.revise(id, &self.who, anchor, &text, at(2))?;
                    made = Some(id);
                }
                Op::Remove { id } => {
                    self.review.remove(id, &self.who)?;
                }
            }
        }
        let after = self.view();
        self.caret = match (plan.caret, made) {
            (Caret::Inserted(k), Some(id)) => after.shown.iter().find(|s| s.id == id).map_or(0, |s| s.inserted_at + k),
            (Caret::Source(s), _) => after.to_view(s),
            (Caret::Inserted(_), None) => return Err("no suggestion for the caret".into()),
        };
        Ok(())
    }

    fn type_text(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        for c in text.chars() {
            self.edit(self.caret, self.caret, &c.to_string())?;
        }
        Ok(())
    }

    fn backspace(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.edit(self.caret - 1, self.caret, "")
    }

    /// Accepts every suggestion, as the engine does: one edit of the file.
    fn accept_all(&mut self) -> String {
        let list = self.review.to_accept_all(&Filter::default());
        let mut text = self.source.clone();
        for a in list.iter().rev() {
            text = replaced(&text, a.start, a.end, &a.replacement);
        }
        text
    }
}

#[test]
fn typing_over_a_word_in_suggest_mode_suggests_without_changing_the_file_and_accepting_types_it() -> R {
    let source = "The colour of the sky.";
    let mut s = Suggesting::new(source, ada());
    // Select "colour" and type "color" over it, one key at a time.
    s.edit(4, 10, "c")?;
    s.type_text("olor")?;
    assert_eq!(s.source, source, "suggesting changed the file");
    assert_eq!(s.review.threads().len(), 1, "consecutive typing made several suggestions");
    let view = s.view();
    assert!(view.text.contains("colour") && view.text.contains("color"), "the shown text lacks the passage or the change: {}", view.text);
    assert_eq!(s.accept_all(), "The color of the sky.");
    Ok(())
}

#[test]
fn deleting_and_then_typing_makes_one_replacement_and_backspacing_takes_it_back() -> R {
    let source = "one two three";
    let mut s = Suggesting::new(source, ada());
    s.caret = 7;
    // Backspace over "two": each is marked deleted, and the caret moves before it.
    for _ in 0..3 {
        s.backspace()?;
    }
    assert_eq!(s.caret, 4, "the caret did not move back over the deleted text");
    assert_eq!(s.review.threads().len(), 1);
    assert_eq!(s.accept_all(), "one  three");
    // Typing at the deletion extends it into a replacement.
    s.type_text("2")?;
    assert_eq!(s.review.threads().len(), 1);
    assert_eq!(s.accept_all(), "one 2 three");
    // Backspacing what was typed leaves the deletion; typing the word back removes the suggestion.
    s.backspace()?;
    assert_eq!(s.accept_all(), "one  three");
    s.type_text("two")?;
    assert!(s.review.threads().is_empty(), "a suggestion that changes nothing was kept");
    assert_eq!(s.view().text, source);
    Ok(())
}

#[test]
fn an_edit_inside_another_persons_suggestion_is_refused_but_next_to_it_is_a_new_one() -> R {
    let source = "alpha beta";
    let mut s = Suggesting::new(source, ada());
    s.caret = 5;
    s.type_text("XY")?;
    s.who = bob();
    // Inside Ada's "XY".
    assert!(s.edit(6, 6, "z").is_err(), "Bob typed inside Ada's suggestion");
    // Just after it: Bob's own suggestion.
    s.caret = 7;
    s.type_text("!")?;
    assert_eq!(s.review.threads().len(), 2);
    assert_eq!(s.accept_all(), "alphaXY! beta");
    Ok(())
}

fn suggestion(r: &mut Review, text: &str, n: u128, start: usize, end: usize, with: &str) -> Result<Id, Error> {
    let id = Id::new(2_000, n);
    r.suggest(id, Anchor::new("main.tex", text, start, end), text, with, ada(), at(0))?;
    Ok(id)
}

#[test]
fn suggestions_follow_edits_and_an_edit_inside_one_makes_it_outdated() -> R {
    let text = "one two three four";
    let mut r = Review::default();
    let two = suggestion(&mut r, text, 1, 4, 7, "2")?;
    let four = suggestion(&mut r, text, 2, 14, 18, "4")?;
    // Typing before "two" and just after it moves both and outdates neither.
    let outdated = r.follow("main.tex", &[(0, 0, 3), (7, 7, 1)]);
    assert!(outdated.is_empty(), "an edit at the edges outdated a suggestion");
    let t = r.get(two).ok_or("gone")?;
    assert_eq!((t.anchor.start, t.anchor.end), (7, 10));
    assert!(r.to_accept(two).is_ok());
    // Typing inside "four" does.
    let f = r.get(four).ok_or("gone")?.anchor.clone();
    let outdated = r.follow("main.tex", &[(f.start + 2, f.start + 2, 1)]);
    assert_eq!(outdated, vec![four]);
    assert_eq!(r.to_accept(four), Err(Error::Outdated));
    // Outdated, it can still be rejected.
    r.reject(four, &ada(), at(5))?;
    assert!(matches!(r.get(four).map(|t| &t.state), Some(State::Rejected { .. })));
    Ok(())
}

#[test]
fn accept_all_of_independent_suggestions_is_their_edits_in_any_order_and_overlaps_become_outdated() -> R {
    let text = "a quick brown fox";
    let mut r = Review::default();
    let quick = suggestion(&mut r, text, 1, 2, 7, "slow")?;
    let fox = suggestion(&mut r, text, 2, 14, 17, "dog")?;
    let insert = suggestion(&mut r, text, 3, 0, 0, "Once, ")?;
    let overlapping = suggestion(&mut r, text, 4, 4, 13, "ick red")?;
    let all = r.to_accept_all(&Filter::default());
    assert!(!all.iter().any(|a| a.id == overlapping), "an overlapping suggestion was taken too");
    // In the order of the text, from the end, and one by one in another order: the same text.
    let mut together = text.to_owned();
    for a in all.iter().rev() {
        together = replaced(&together, a.start, a.end, &a.replacement);
    }
    let mut one_by_one = Review::clone(&r);
    let mut by_one = text.to_owned();
    for id in [fox, insert, quick] {
        let a = one_by_one.to_accept(id)?;
        by_one = replaced(&by_one, a.start, a.end, &a.replacement);
        one_by_one.follow("main.tex", &[(a.start, a.end, a.replacement.chars().count())]);
        one_by_one.accepted(std::slice::from_ref(&a), &ada(), at(3));
        let t = one_by_one.get(id).ok_or("gone")?;
        assert_eq!(by_one.chars().skip(t.anchor.start).take(t.anchor.end - t.anchor.start).collect::<String>(), a.replacement);
    }
    assert_eq!(together, "Once, a slow brown dog");
    assert_eq!(by_one, together);
    // Made together, the overlapping one is reported outdated.
    let sizes: Vec<(usize, usize, usize)> = all.iter().map(|a| (a.start, a.end, a.replacement.chars().count())).collect();
    let outdated = r.follow("main.tex", &sizes);
    r.accepted(&all, &ada(), at(4));
    assert!(outdated.contains(&overlapping));
    assert_eq!(r.to_accept(overlapping), Err(Error::Outdated));
    for a in &all {
        let t = r.get(a.id).ok_or("gone")?;
        assert_eq!(together.chars().skip(t.anchor.start).take(t.anchor.end - t.anchor.start).collect::<String>(), a.replacement);
    }
    Ok(())
}

#[test]
fn undoing_an_accept_reopens_the_suggestion_and_redoing_it_accepts_it_again() -> R {
    let text = "The colour of the sky.";
    let mut r = Review::default();
    let id = suggestion(&mut r, text, 1, 4, 10, "color")?;
    let a = r.to_accept(id)?;
    r.follow("main.tex", &[(4, 10, 5)]);
    r.accepted(&[a], &bob(), at(1));
    assert!(matches!(r.get(id).map(|t| &t.state), Some(State::Accepted { .. })));
    assert!(r.reopen(id).is_err(), "an accepted suggestion was reopened while its text is in the file");
    // The undo puts "colour" back over "color".
    let changed = r.follow_undo("main.tex", &[(4, 9, "colour".to_owned())], Some(&bob()), at(2));
    assert_eq!(changed, vec![id]);
    let t = r.get(id).ok_or("gone")?;
    assert!(t.is_open());
    assert_eq!((t.anchor.start, t.anchor.end), (4, 10));
    assert!(r.to_accept(id).is_ok(), "the reopened suggestion cannot be accepted");
    // The redo makes it again.
    r.follow_undo("main.tex", &[(4, 10, "color".to_owned())], Some(&bob()), at(3));
    assert!(matches!(r.get(id).map(|t| &t.state), Some(State::Accepted { .. })));
    Ok(())
}

#[test]
fn a_suggestion_is_written_as_its_own_kind_and_read_back() -> R {
    let text = "one two";
    let mut r = Review::default();
    let id = suggestion(&mut r, text, 1, 4, 7, "2")?;
    let t = r.get(id).ok_or("gone")?;
    let file = format::write(t)?;
    assert!(file.contains("\"kind\": \"suggestion\""), "{file}");
    assert_eq!(&format::read(&file)?, t);
    // A thread's file that claims to be a suggestion is refused, not read partly.
    let wrong = file.replace("\"kind\": \"suggestion\"", "\"kind\": \"thread\"");
    assert!(format::read(&wrong).is_err());
    // A suggestion that changes nothing is refused.
    assert_eq!(suggestion(&mut r, text, 2, 0, 3, "one").err(), Some(Error::NoChange));
    Ok(())
}
