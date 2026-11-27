//! Random edits of the formatted view's model, checked after every step (docs/testing.md,
//! "Testing the formatted view").
//!
//! Each case takes a document of the corpus (or a slice of one), then makes a random sequence of
//! what a user does in the view: clicks, selections, arrows, typing (LaTeX's special characters and
//! other scripts included), Backspace and Delete at every kind of edge, Enter, Tab and Shift+Tab,
//! pastes of lines and lists, and the toolbar's buttons and the table toolbar's operations, as the
//! window makes them ([`sim`]). After each step [`checks`] asserts that nothing panicked, that the
//! source edit is sound (changes in order and on the text, near the selection, nothing left
//! unclosed that was closed, no empty item, list, style or heading left behind, nothing turned into
//! source), that the caret stands where the view shows it, and that the view shows what the edit
//! meant (the typed text where it was typed, the deleted character gone, the words kept by a
//! change of structure). A failing case is shrunk to the fewest lines of its document and the
//! fewest steps, and printed.
//!
//! `TEXCRAFT_FUZZ_CASES` sets the number of cases (a few hundred by default), `TEXCRAFT_FUZZ_SEED`
//! the first seed, `TEXCRAFT_FUZZ_CORPUS` a folder whose `.tex` files join the corpus (read, never
//! copied).

mod checks;
mod corpus;
mod found;
mod lines;
mod sim;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use checks::{Before, Failure};
use sim::{Editor, Op};

/// SplitMix64: small, and the same sequence on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn u32(&mut self) -> u32 {
        (self.next() >> 32) as u32
    }

    fn pick<T: Clone>(&mut self, of: &[T]) -> T {
        of[self.below(of.len())].clone()
    }

    fn flip(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

const TYPED: &[&str] = &[
    "a",
    "x",
    "Word",
    "two words",
    " ",
    " ",
    "e",
    "1",
    "{",
    "}",
    "$",
    "%",
    "&",
    "#",
    "_",
    "^",
    "~",
    "\\",
    "é",
    "ü",
    "→",
    "中文",
    "😀",
    "-",
    "--",
    "'",
    "``",
    ".",
    "...",
    "\u{a0}",
    "a\u{301}",
];

const PASTED: &[&str] = &[
    "plain words",
    "line one\nline two",
    "- item a\n- item b",
    "1. first\n2) second",
    "para one\n\npara two",
    "x%y & z",
    "• bullet\n• another\nafter",
    "\\textbf{not a command}",
    "tab\there",
];

const BAR: &[&str] = &["bold", "italic", "code", "heading_1", "heading_2", "heading_3", "body", "itemize", "enumerate", "table"];

const TABLE_OPS: &[&str] = &[
    "insert_row_above",
    "insert_row_below",
    "delete_row",
    "insert_column_left",
    "insert_column_right",
    "delete_column",
    "align_column",
    "borders",
    "header",
    "merge",
    "split",
];

fn op(rng: &mut Rng) -> Op {
    let (l, s) = (rng.u32(), rng.u32());
    let shift = rng.below(3) == 0;
    let word = rng.below(4) == 0;
    match rng.below(100) {
        0..=11 => Op::Click(l, s),
        12..=15 => Op::Extend(l, s),
        16..=17 => Op::DoubleClick(l, s),
        18 => Op::TripleClick(l),
        19 => Op::SelectAll,
        20..=24 => Op::Left { shift, word },
        25..=29 => Op::Right { shift, word },
        30..=31 => Op::Up { shift },
        32..=33 => Op::Down { shift },
        34 => Op::Home { shift },
        35 => Op::End { shift },
        36..=55 => Op::Type(rng.pick(TYPED).to_owned()),
        56..=63 => Op::Backspace,
        64..=68 => Op::Delete,
        69..=74 => Op::Enter,
        75..=77 => Op::Tab,
        78..=79 => Op::ShiftTab,
        80..=81 => Op::Paste(rng.pick(PASTED).to_owned()),
        82 => Op::Cut,
        83..=90 => Op::Bar(rng.pick(BAR)),
        91 => Op::Cite("knuth84".to_owned()),
        92 => Op::Ref("sec:intro".to_owned()),
        93 => Op::Footnote("A note".to_owned()),
        94 => Op::Link("https://example.org".to_owned(), if rng.flip() { String::new() } else { "site".to_owned() }),
        95 => Op::Math(rng.pick(&["inline", "display", "equation"]), "x^2".to_owned()),
        _ => Op::Table(rng.pick(TABLE_OPS), rng.u32()),
    }
}

/// A part of a long document: its preamble (or the declarations that change the view) and a few
/// of its paragraphs.
fn slice(text: &str, rng: &mut Rng) -> String {
    if text.len() <= 3000 {
        return text.to_owned();
    }
    let begin = text.find("\\begin{document}").map(|i| i + "\\begin{document}".len());
    let (head, body) = match begin {
        Some(i) => text.split_at(i),
        None => ("", text),
    };
    let head = if head.len() > 4000 {
        let kept: Vec<&str> = head.lines().filter(|l| l.starts_with("\\documentclass") || l.contains("\\newtheorem")).collect();
        format!("{}\n\\begin{{document}}", kept.join("\n"))
    } else {
        head.to_owned()
    };
    let breaks: Vec<usize> = body.match_indices("\n\n").map(|(i, _)| i + 2).collect();
    let start = if breaks.is_empty() { 0 } else { breaks[rng.below(breaks.len())] };
    let mut end = (start + 1500 + rng.below(1500)).min(body.len());
    while !body.is_char_boundary(end) {
        end += 1;
    }
    let tail = if begin.is_some() { "\n\\end{document}\n" } else { "" };
    format!("{head}\n{}{tail}", body.get(start..end).unwrap_or_default())
}

#[derive(Debug, Clone)]
struct Case {
    name: String,
    text: String,
    ops: Vec<Op>,
}

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// Panics while a case runs are its failures, not noise on the test's output.
fn quiet_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                default(info);
            }
        }));
    });
}

/// Runs a case; the first failure and the step it came at.
fn run(case: &Case) -> Option<(usize, Failure)> {
    QUIET.with(|q| q.set(true));
    let out = catch_unwind(AssertUnwindSafe(|| run_steps(case)));
    QUIET.with(|q| q.set(false));
    match out {
        Ok(found) => found,
        Err(e) => {
            let message = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
            Some((usize::MAX, Failure { rule: "panic", detail: message }))
        }
    }
}

fn run_steps(case: &Case) -> Option<(usize, Failure)> {
    let mut e = Editor::new(&case.text);
    for (i, op) in case.ops.iter().enumerate() {
        let before = e.clone();
        let shown = before.shown();
        let done = match e.run(op) {
            Ok(d) => d,
            Err(sim::Broken(why)) => return Some((i, Failure { rule: "bad-edit", detail: why })),
        };
        if let Err(f) = checks::after_step(&Before { editor: &before, shown: &shown }, op, &done, &e) {
            return Some((i, f));
        }
    }
    None
}

/// Whether `case` fails the same way, its document still balanced if it was (a reproducer stays
/// a document a user could have).
fn same(case: &Case, rule: &str, balanced: bool) -> bool {
    (!balanced || checks::balanced(&case.text)) && run(case).is_some_and(|(_, f)| f.rule == rule)
}

/// The case cut down while it fails by `rule`: its steps after the failing one dropped, then
/// chunks of its steps and of its document's lines, while it still fails so.
fn shrink(case: &Case, rule: &str, step: usize) -> Case {
    let balanced = checks::balanced(&case.text);
    let mut best = case.clone();
    if step < best.ops.len() {
        best.ops.truncate(step + 1);
    }
    let mut budget = 3000;
    loop {
        let start = (best.ops.len(), best.text.len());
        best = shrink_ops(best, (rule, balanced), &mut budget);
        best = shrink_nodes(best, (rule, balanced), &mut budget);
        best = shrink_lines(best, (rule, balanced), &mut budget);
        best = shrink_chars(best, (rule, balanced), &mut budget);
        if (best.ops.len(), best.text.len()) == start || budget == 0 {
            return best;
        }
    }
}

fn shrink_ops(mut best: Case, (rule, balanced): (&str, bool), budget: &mut usize) -> Case {
    let mut chunk = best.ops.len().div_ceil(2).max(1);
    while chunk >= 1 && *budget > 0 {
        let mut i = 0;
        while i < best.ops.len() && *budget > 0 {
            let mut c = best.clone();
            c.ops.drain(i..(i + chunk).min(c.ops.len()));
            *budget -= 1;
            if !c.ops.is_empty() && same(&c, rule, balanced) {
                best = c;
            } else {
                i += chunk;
            }
        }
        if chunk == 1 {
            break;
        }
        chunk /= 2;
    }
    best
}

fn shrink_lines(mut best: Case, (rule, balanced): (&str, bool), budget: &mut usize) -> Case {
    let mut chunk = best.text.lines().count().div_ceil(2).max(1);
    while *budget > 0 {
        let mut i = 0;
        loop {
            let lines: Vec<&str> = best.text.split_inclusive('\n').collect();
            if i >= lines.len() || *budget == 0 {
                break;
            }
            let mut kept = lines.clone();
            kept.drain(i..(i + chunk).min(lines.len()));
            let c = Case { text: kept.concat(), ..best.clone() };
            *budget -= 1;
            if same(&c, rule, balanced) {
                best = c;
            } else {
                i += chunk;
            }
        }
        if chunk == 1 {
            break;
        }
        chunk /= 2;
    }
    best
}

/// The ranges of the nodes of the tree of `text`, outermost first.
fn node_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    use texcraft_latex::tree::{Kind, Node};
    fn walk(nodes: &[Node], out: &mut Vec<std::ops::Range<usize>>) {
        for n in nodes {
            out.push(n.range.clone());
            match &n.kind {
                Kind::Group { body, .. } | Kind::Environment { body, .. } => walk(body, out),
                Kind::Command { args, .. } => args.iter().for_each(|a| walk(&a.body, out)),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&texcraft_latex::tree::parse(text), &mut out);
    out
}

/// Whole nodes of the document (an environment, a command, a group, a text) taken out.
fn shrink_nodes(mut best: Case, (rule, balanced): (&str, bool), budget: &mut usize) -> Case {
    let mut i = 0;
    while *budget > 0 {
        let ranges = node_ranges(&best.text);
        let Some(r) = ranges.get(i) else { break };
        let mut c = best.clone();
        c.text.replace_range(r.clone(), "");
        *budget -= 1;
        if same(&c, rule, balanced) {
            best = c;
        } else {
            i += 1;
        }
    }
    best
}

/// Words, then single characters, taken out of the document.
fn shrink_chars(mut best: Case, (rule, balanced): (&str, bool), budget: &mut usize) -> Case {
    for word in [true, false] {
        let mut i = 0;
        while i < best.text.len() && *budget > 0 {
            if !best.text.is_char_boundary(i) {
                i += 1;
                continue;
            }
            let rest = best.text.get(i..).unwrap_or_default();
            let len = if word {
                rest.find(|c: char| !c.is_alphanumeric()).unwrap_or(rest.len()).max(rest.chars().next().map_or(1, char::len_utf8))
            } else {
                rest.chars().next().map_or(1, char::len_utf8)
            };
            let mut c = best.clone();
            c.text.replace_range(i..i + len, "");
            *budget -= 1;
            if same(&c, rule, balanced) {
                best = c;
            } else {
                i += len;
            }
        }
    }
    best
}

fn report(case: &Case, f: &Failure, step: usize) -> String {
    let ops: Vec<String> = case.ops.iter().map(|o| format!("{o:?}")).collect();
    format!(
        "[{}] at step {step} of {} (from {}): {}\n  document: {:?}\n  steps: [{}]",
        f.rule,
        case.ops.len(),
        case.name,
        f.detail,
        case.text,
        ops.join(", ")
    )
}

fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// Runs `cases` random cases from `seed`; the shrunk reproducer of each kind of failure found.
fn search(docs: &[corpus::Doc], seed: u64, cases: u64) -> BTreeMap<String, String> {
    quiet_panics();
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut rng = Rng(seed);
    for _ in 0..cases {
        let doc = &docs[rng.below(docs.len())];
        let text = slice(&doc.text, &mut rng);
        let n = 1 + rng.below(30);
        let case = Case { name: doc.name.clone(), text, ops: (0..n).map(|_| op(&mut rng)).collect() };
        let Some((step, f)) = run(&case) else { continue };
        let kind = case.ops.get(step).map_or("?".to_owned(), |o| format!("{o:?}").split(['(', ' ']).next().unwrap_or_default().to_owned());
        let key = format!("{} after {kind}", f.rule);
        if found.contains_key(&key) {
            continue;
        }
        let small = shrink(&case, f.rule, step);
        let (step, f) = run(&small).unwrap_or((step, f));
        found.insert(key, report(&small, &f, step));
    }
    found
}

#[test]
fn random_edits_in_the_formatted_view_keep_the_source_sound_and_the_view_as_meant() {
    let mut docs = corpus::committed();
    if let Some(dir) = std::env::var_os("TEXCRAFT_FUZZ_CORPUS").filter(|d| !d.is_empty()) {
        docs.extend(corpus::local(std::path::Path::new(&dir), env_number("TEXCRAFT_FUZZ_FILES", 5000) as usize));
    }
    let found = search(&docs, env_number("TEXCRAFT_FUZZ_SEED", 1), env_number("TEXCRAFT_FUZZ_CASES", 300));
    let all: Vec<&String> = found.values().collect();
    assert!(all.is_empty(), "{} kinds of failure:\n\n{}", all.len(), all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n\n"));
}
