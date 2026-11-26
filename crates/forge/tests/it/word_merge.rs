//! The three-way merge word by word, where both sides changed the same lines: a LaTeX paragraph
//! is often one line. The fixtures are written for these tests.

use texcraft_forge::merge::{MAX_PASSAGE, Merge, merge_text, merge3};

fn merged(base: &str, ours: &str, theirs: &str) -> Option<String> {
    merge3(base.as_bytes(), ours.as_bytes(), theirs.as_bytes()).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// A file whose second line is a paragraph of three sentences.
fn file(paragraph: &str) -> String {
    format!("\\section{{Method}}\n{paragraph}\n\nThe next paragraph.\n")
}

const PARAGRAPH: &str = "Diffusion models learn to reverse a noising process. They are trained on large corproa \\cite{ho2020}. Sampling is slow, but distillation helps.";

#[test]
fn edits_to_different_sentences_of_one_line_merge() {
    let base = file(PARAGRAPH);
    let ours = file(&PARAGRAPH.replace("learn to reverse", "learn to invert"));
    let theirs = file(&PARAGRAPH.replace("is slow,", "is still slow,"));
    let both = file(&PARAGRAPH.replace("learn to reverse", "learn to invert").replace("is slow,", "is still slow,"));
    assert_eq!(merged(&base, &ours, &theirs), Some(both.clone()));
    assert_eq!(merged(&base, &theirs, &ours), Some(both));
}

#[test]
fn edits_to_different_words_of_one_sentence_merge() {
    let base = file(PARAGRAPH);
    let ours = file(&PARAGRAPH.replace("learn to", "learned to"));
    let theirs = file(&PARAGRAPH.replace("a noising", "a gradual noising"));
    assert_eq!(merged(&base, &ours, &theirs), Some(file(&PARAGRAPH.replace("learn to", "learned to").replace("a noising", "a gradual noising"))));
}

#[test]
fn the_same_word_changed_differently_is_a_conflict_of_that_passage_only() {
    let base = file(PARAGRAPH);
    let ours = file(&PARAGRAPH.replace("slow", "costly"));
    let theirs = file(&PARAGRAPH.replace("slow", "expensive"));
    assert_eq!(merged(&base, &ours, &theirs), None);
    let Merge::Conflicts(passages) = merge_text(base.as_bytes(), ours.as_bytes(), theirs.as_bytes()) else {
        panic!("a conflict");
    };
    assert_eq!(passages.len(), 1, "{passages:?}");
    let p = &passages[0];
    // Each side's passage is where it is in its file, and holds that side's change.
    assert_eq!(
        (&base[p.base.clone()], &ours[p.ours.clone()], &theirs[p.theirs.clone()]),
        (p.base_text.as_str(), p.ours_text.as_str(), p.theirs_text.as_str())
    );
    assert!(p.base_text.contains("slow") && p.ours_text.contains("costly") && p.theirs_text.contains("expensive"), "{p:?}");
    // Narrower than the line it is in.
    assert!(p.ours_text.len() < PARAGRAPH.len() / 4, "{p:?}");
}

#[test]
fn a_citation_added_merges_with_a_typo_fixed_elsewhere_in_the_line() {
    let base = file(PARAGRAPH);
    let ours = file(&PARAGRAPH.replace("process.", "process \\cite{song2021}."));
    let theirs = file(&PARAGRAPH.replace("corproa", "corpora"));
    let both = file(&PARAGRAPH.replace("process.", "process \\cite{song2021}.").replace("corproa", "corpora"));
    assert_eq!(merged(&base, &ours, &theirs), Some(both));
    // A changed key of the citation that is there is a change to its argument, beside the typo.
    let ours = file(&PARAGRAPH.replace("\\cite{ho2020}", "\\cite{ho2020,song2021}"));
    let both = file(&PARAGRAPH.replace("\\cite{ho2020}", "\\cite{ho2020,song2021}").replace("corproa", "corpora"));
    assert_eq!(merged(&base, &ours, &theirs), Some(both));
}

#[test]
fn an_edit_inside_math_merges_with_prose_edited_elsewhere() {
    let line = "The forward step is $x_t = \\sqrt{\\alpha_t} x_0 + \\epsilon$, and the noise is small.";
    let ours = line.replace("\\sqrt{\\alpha_t}", "\\sqrt{\\bar{\\alpha}_t}");
    let theirs = line.replace("is small", "is Gaussian");
    let both = ours.replace("is small", "is Gaussian");
    assert_eq!(merged(&file(line), &file(&ours), &file(&theirs)), Some(file(&both)));
    // Two different edits of the formula are a conflict.
    let theirs = line.replace("\\sqrt{\\alpha_t}", "\\alpha_t");
    assert_eq!(merged(&file(line), &file(&ours), &file(&theirs)), None);
}

#[test]
fn the_same_edit_on_both_sides_counts_once() {
    let base = file(PARAGRAPH);
    let ours = file(&PARAGRAPH.replace("slow", "fast").replace("Diffusion models", "Score models"));
    let theirs = file(&PARAGRAPH.replace("slow", "fast"));
    let out = merged(&base, &ours, &theirs).expect("merges");
    assert_eq!(out, ours);
    assert_eq!(out.matches("fast").count(), 1);
}

#[test]
fn a_sentence_deleted_merges_with_another_edited() {
    let base = file(PARAGRAPH);
    let ours = file(&PARAGRAPH.replace(" They are trained on large corproa \\cite{ho2020}.", ""));
    let theirs = file(&PARAGRAPH.replace("distillation helps", "distillation helps a lot"));
    let out = merged(&base, &ours, &theirs).expect("merges");
    assert!(!out.contains("trained") && out.contains("helps a lot") && out.contains("Diffusion models learn"), "{out}");
    assert_eq!(merged(&base, &theirs, &ours), Some(out));
    // A deletion against an edit of the deleted sentence is a conflict.
    let theirs = file(&PARAGRAPH.replace("large", "huge"));
    assert_eq!(merged(&base, &ours, &theirs), None);
}

#[test]
fn crlf_files_keep_their_line_endings() {
    let crlf = |s: &str| s.replace('\n', "\r\n");
    let base = crlf(&format!("{PARAGRAPH}\nSecond line.\n"));
    let ours = crlf(&format!("{}\nSecond line.\n", PARAGRAPH.replace("slow", "fast")));
    // Theirs comes with `\n`, and splits a sentence of the paragraph onto a line of its own.
    let theirs = format!("{}\nSecond line.\n", PARAGRAPH.replace("process. They", "process.\nThey"));
    let out = merged(&base, &ours, &theirs).expect("merges");
    assert!(out.contains("fast") && out.contains("process.\r\nThey"), "{out:?}");
    assert_eq!(out.matches('\n').count(), out.matches("\r\n").count(), "only CRLF: {out:?}");
}

#[test]
fn non_utf8_is_not_text() {
    let base = file(PARAGRAPH);
    let mut ours = base.clone().into_bytes();
    ours[3] = 0xff;
    assert_eq!(merge_text(base.as_bytes(), &ours, base.as_bytes()), Merge::NotText);
    assert_eq!(merge3(base.as_bytes(), base.as_bytes(), &ours), None);
}

/// A small deterministic generator (an LCG), so the sweep is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

const WORDS: &[&str] =
    &["the", "model", "noise", "is", "small,", "and", "\\cite{a}", "$x_t$", "{b}", "we", "train", "it.", "\\emph{fast}", "\\%", "data;"];

/// Random paragraphs (one line) edited by each side in word ranges apart from each other: they
/// always merge, into the paragraph with both edits, each once.
#[test]
fn edits_to_separate_words_of_random_paragraphs_always_merge() {
    let mut rng = Lcg(2026);
    let mut fresh = 0;
    for case in 0..400 {
        let len = 6 + rng.below(40);
        // No word repeats one of the five before it: a word repeated next to an edit makes where
        // the edit is ambiguous ("the the"), and the diff may put it beside the other side's edit,
        // a conflict (safe, but not what this test is about).
        let mut words: Vec<String> = Vec::new();
        while words.len() < len {
            let w = WORDS[rng.below(WORDS.len())];
            if !words.iter().rev().take(5).any(|x| x == w) {
                words.push(w.to_owned());
            }
        }
        // Two ranges with at least one word left alone between them.
        let a0 = rng.below(len - 3);
        let a1 = a0 + 1 + rng.below((len - a0 - 2).min(4));
        let b0 = a1 + 1 + rng.below((len - a1 - 1).min(3));
        let b1 = (b0 + 1 + rng.below(4)).min(len);
        let mut new = |n: usize| -> Vec<String> {
            (0..n)
                .map(|_| {
                    fresh += 1;
                    if fresh % 3 == 0 { format!("\\new{{w{fresh}}}") } else { format!("w{fresh}") }
                })
                .collect()
        };
        let (na, nb) = (new(rng.below(3)), new(rng.below(3)));
        let edit = |w: &[String], r: std::ops::Range<usize>, with: &[String]| -> Vec<String> { [&w[..r.start], with, &w[r.end..]].concat() };
        let line = |w: &[String]| format!("Intro.\n{}\nEnd.\n", w.join(" "));
        let ours = edit(&words, a0..a1, &na);
        let theirs = edit(&words, b0..b1, &nb);
        // Both, the later range first so the earlier's indices still hold.
        let both = edit(&edit(&words, b0..b1, &nb), a0..a1, &na);
        let (base, ours, theirs, both) = (line(&words), line(&ours), line(&theirs), line(&both));
        let (o, t) = if case % 2 == 0 { (&ours, &theirs) } else { (&theirs, &ours) };
        assert_eq!(merged(&base, o, t).as_deref(), Some(both.as_str()), "case {case}:\n{base}{ours}{theirs}");
    }
}

/// Inputs that try the merge's limits: it ends, without panicking, and what it cannot merge by
/// word in bounded time is a conflict.
#[test]
fn hostile_inputs_end_and_stay_bounded() {
    // A line longer than a passage merged by word: a conflict, as by line.
    let long = "word ".repeat(MAX_PASSAGE / 4);
    let (ours, theirs) = (format!("A{long}"), format!("{long}Z"));
    assert_eq!(merged(&long, &ours, &theirs), None);
    // Many tokens, all changed on both sides: past the bounded number of edits.
    let many = "a b ".repeat(30_000);
    assert_eq!(merged(&many, &many.replace('a', "c"), &many.replace('b', "d")), None);
    // Within the bounds, the same shape merges.
    let few = "a b ".repeat(100);
    assert_eq!(merged(&few, &few.replace('a', "c"), &few.replace('b', "d")), Some("c d ".repeat(100)));
    let odd = [
        "",
        "\n",
        "\r",
        "\r\n\r",
        "{{{{",
        "}}}",
        "\\",
        "\\\\\\",
        "%",
        "% only a comment\r\n",
        "$$ $",
        "é\u{301}\u{200b}\u{a0} ",
        "\\cmd{\\other{",
        "a\rb\r\nc\n",
    ];
    for base in odd {
        for ours in odd {
            for theirs in odd {
                let out = merge_text(base.as_bytes(), ours.as_bytes(), theirs.as_bytes());
                if let Merge::Conflicts(passages) = &out {
                    for p in passages {
                        assert_eq!(
                            (&base[p.base.clone()], &ours[p.ours.clone()], &theirs[p.theirs.clone()]),
                            (p.base_text.as_str(), p.ours_text.as_str(), p.theirs_text.as_str())
                        );
                    }
                }
                if ours == theirs {
                    assert_eq!(out, Merge::Clean(ours.as_bytes().to_vec()), "{base:?} {ours:?}");
                }
            }
        }
    }
}
