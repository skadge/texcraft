//! Formulas typeset on their own, in process (no worker program in tests).

use texcraft_compile::formulas::{Done, Files, Formulas, State, Typeset, Wanted};

fn formulas(name: &str) -> (Formulas, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("texcraft-formulas-{name}-{}", std::process::id()));
    (Formulas::new(dir.clone()), dir)
}

fn no_files() -> Files {
    Files { files: Vec::new(), main_dir: None }
}

fn inline(s: &str) -> Wanted {
    Wanted { source: s.to_owned(), display: false }
}

fn done(states: &[State]) -> Vec<Done> {
    states
        .iter()
        .map(|s| match s {
            State::Done(d) => (**d).clone(),
            State::Pending => Done::Failed(vec!["still pending".to_owned()]),
        })
        .collect()
}

fn typeset(d: &Done) -> Option<&Typeset> {
    match d {
        Done::Typeset(t) => Some(t),
        Done::Failed(_) => None,
    }
}

const PREAMBLE: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n";

#[test]
fn a_batch_typesets_each_formula_on_its_page_and_an_invalid_one_says_why() {
    let (mut f, dir) = formulas("batch");
    let wanted = [inline("x^2 + y^2"), Wanted { source: "\\frac{a}{b}".to_owned(), display: true }, inline("\\nosuchmacro x")];
    let first = f.request(PREAMBLE, &wanted, no_files);
    assert!(first.iter().all(|s| matches!(s, State::Pending)), "nothing is typeset before the batch runs");
    f.wait();
    let states = done(&f.request(PREAMBLE, &wanted, no_files));
    let _ = std::fs::remove_dir_all(&dir);
    let (a, b) = (typeset(&states[0]), typeset(&states[1]));
    let (Some(a), Some(b)) = (a, b) else { panic!("the valid formulas were not typeset: {states:?}") };
    assert_ne!(a.page, b.page, "two formulas share a page");
    assert!(a.height_pt > 0.0 && b.height_pt > 0.0 && a.width_pt > 0.0, "a box is empty");
    assert!(b.height_pt + b.depth_pt > a.height_pt + a.depth_pt, "the display fraction is not taller than x^2 + y^2");
    assert!(texcraft_preview::Document::open(a.pdf.clone()).is_ok_and(|d| d.page_count() > b.page as usize), "the batch's PDF lacks the pages");
    match &states[2] {
        Done::Failed(messages) => assert!(!messages.is_empty(), "the invalid formula has no message"),
        Done::Typeset(_) => panic!("an undefined macro was typeset"),
    }
}

#[test]
fn asking_again_typesets_only_the_changed_formula() {
    let (mut f, dir) = formulas("cache");
    let _ = f.request(PREAMBLE, &[inline("a+b"), inline("c+d")], no_files);
    f.wait();
    let before = f.batches();
    let states = f.request(PREAMBLE, &[inline("a+b"), inline("c+e")], no_files);
    assert!(matches!(states[0], State::Done(_)), "the unchanged formula is typeset again");
    assert!(matches!(states[1], State::Pending), "the changed formula is not sent");
    f.wait();
    let after = done(&f.request(PREAMBLE, &[inline("a+b"), inline("c+e")], no_files));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(f.batches(), before + 1, "the change took more than one batch");
    assert!(after.iter().all(|d| typeset(d).is_some()), "a formula was not typeset: {after:?}");
}
