//! A scripted user of the formatted view in the window (docs/testing.md, "Testing the formatted
//! view"): on documents of the corpus, random keys, typed text, clicks at random places in the
//! text, drags, double and triple clicks, the toolbar's buttons, undo and redo, with the fonts the
//! build has. After each action: nothing panicked, the frame settles, the keyboard stays in the
//! text, the caret is in a line of the view inside the window, a letter typed (or composed with an
//! input method) shows just before the caret without moving its line by more than a line, one undo
//! takes a word typed letter by letter back whole, and undo then redo of an edit give back the
//! texts before and after it byte for byte.
//!
//! `TEXCRAFT_FUZZ_WINDOW_CASES` sets the number of cases (a few by default), `TEXCRAFT_FUZZ_SEED`
//! the first seed.

use std::panic::{AssertUnwindSafe, catch_unwind};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;

const SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);

const DOCS: &[(&str, &str)] = &[
    ("mixed", include_str!("../../../latex/tests/fixtures/formatted-mixed.tex")),
    ("lists", include_str!("../../../latex/tests/fixtures/formatted-lists.tex")),
    ("fragment", include_str!("../../../latex/tests/fixtures/formatted-fragment.tex")),
    ("structures", include_str!("../../../latex/tests/fixtures/formatted-structures.tex")),
    ("beamer", include_str!("../../../latex/tests/fixtures/formatted-beamer.tex")),
    ("frontmatter", include_str!("../../../latex/tests/fixtures/frontmatter.tex")),
    ("article", include_str!("../../../../templates/builtin/article/main.tex")),
    ("thesis/background", include_str!("../../../../templates/builtin/thesis/chapters/background.tex")),
    ("examples/intro", include_str!("../../../../examples/article/sections/intro.tex")),
];

/// SplitMix64.
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

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Debug, Clone)]
enum Act {
    Key(egui::Modifiers, egui::Key),
    Type(String),
    /// A click (or 2, 3) at a place of a line of the view: which line, and where across it.
    Click {
        line: usize,
        x: f32,
        times: usize,
        shift: bool,
    },
    Drag {
        from: (usize, f32),
        to: (usize, f32),
    },
    Button(&'static str),
    Undo,
    Redo,
    /// A word typed letter by letter, a frame each.
    Word(&'static str),
    /// Text composed with an input method: marked text, then committed.
    Compose(&'static str, &'static str),
}

const BUTTONS: &[&str] = &["Bold", "Italic", "Code", "Bulleted List", "Numbered List", "Outdent Item", "Indent Item"];
const TYPED: &[&str] = &["a", "x", "Word", " ", "{", "}", "$", "%", "&", "#", "_", "^", "~", "\\", "é", "中", "-", ".", "'"];

fn act(rng: &mut Rng) -> Act {
    use egui::Key as K;
    let none = egui::Modifiers::NONE;
    let keys = [
        (none, K::ArrowLeft),
        (none, K::ArrowRight),
        (none, K::ArrowUp),
        (none, K::ArrowDown),
        (egui::Modifiers::SHIFT, K::ArrowLeft),
        (egui::Modifiers::SHIFT, K::ArrowDown),
        (egui::Modifiers::COMMAND, K::ArrowRight),
        (none, K::Home),
        (none, K::End),
        (none, K::Backspace),
        (none, K::Backspace),
        (none, K::Delete),
        (none, K::Enter),
        (none, K::Tab),
        (egui::Modifiers::SHIFT, K::Tab),
        (egui::Modifiers::COMMAND, K::A),
    ];
    match rng.below(100) {
        0..=34 => {
            let (m, k) = keys[rng.below(keys.len())];
            Act::Key(m, k)
        }
        35..=59 => Act::Type(TYPED[rng.below(TYPED.len())].to_owned()),
        60..=74 => Act::Click { line: rng.below(1000), x: rng.unit(), times: 1, shift: rng.below(5) == 0 },
        75..=79 => Act::Click { line: rng.below(1000), x: rng.unit(), times: 2 + rng.below(2), shift: false },
        80..=84 => Act::Drag { from: (rng.below(1000), rng.unit()), to: (rng.below(1000), rng.unit()) },
        85..=90 => Act::Button(BUTTONS[rng.below(BUTTONS.len())]),
        91..=92 => Act::Word(["kiln", "Clay", "fire"][rng.below(3)]),
        93 => Act::Compose(["zhong", "nihon"][rng.below(2)], ["中文", "日本"][rng.below(2)]),
        94..=97 => Act::Undo,
        _ => Act::Redo,
    }
}

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn formatted(main: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": main})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let app = crate::support::app(s);
    // A character no font has (中 without craft-fonts, as on CI) is drawn as a box, as in the app:
    // this test checks editing, not which fonts are installed.
    let mut h = Harness::builder().with_size(SIZE).allow_missing_glyphs().build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    h
}

/// The view's lines (their text runs) on screen: their rects and texts.
fn lines(h: &Harness<'_, App>) -> Vec<(egui::accesskit::NodeId, egui::Rect, String)> {
    h.query_all(by().role(Role::TextRun))
        .filter(|n| n.accesskit_node().parent().is_some_and(|p| p.label().is_some_and(|l| l == "main.tex formatted")))
        .map(|n| (n.accesskit_node().locate().0, n.rect(), n.accesskit_node().value().unwrap_or_default()))
        .collect()
}

/// The caret: its line's rect and text, and where in it.
fn caret(h: &Harness<'_, App>) -> Option<(egui::Rect, String, usize)> {
    let view = h.query_by_label("main.tex formatted")?;
    let sel = view.accesskit_node().data().text_selection()?;
    let (_, rect, value) = lines(h).into_iter().find(|(id, ..)| *id == sel.focus.node)?;
    Some((rect, value, sel.focus.character_index))
}

fn point(h: &Harness<'_, App>, (line, x): (usize, f32)) -> Option<egui::Pos2> {
    let all = lines(h);
    let visible: Vec<_> = all.into_iter().filter(|(_, r, _)| r.max.y > 40.0 && r.min.y < SIZE.y - 10.0 && r.width() > 0.0).collect();
    let (_, r, _) = visible.get(line % visible.len().max(1))?;
    Some(egui::pos2(r.left() + x * r.width(), r.center().y))
}

fn press(h: &mut Harness<'_, App>, at: egui::Pos2, pressed: bool, modifiers: egui::Modifiers) {
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers });
    h.step();
}

/// Makes `a`; whether the frame settled after it.
fn perform(h: &mut Harness<'_, App>, a: &Act) -> bool {
    match a {
        Act::Key(m, k) => h.key_press_modifiers(*m, *k),
        Act::Type(s) => h.event(egui::Event::Text(s.clone())),
        Act::Click { line, x, times, shift } => {
            let Some(at) = point(h, (*line, *x)) else { return true };
            let m = if *shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
            h.event(egui::Event::ModifiersChanged(m));
            h.event(egui::Event::PointerMoved(at));
            h.step();
            for _ in 0..*times {
                press(h, at, true, m);
                press(h, at, false, m);
            }
            h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        }
        Act::Drag { from, to } => {
            let (Some(p), Some(q)) = (point(h, *from), point(h, *to)) else { return true };
            h.event(egui::Event::PointerMoved(p));
            h.step();
            press(h, p, true, egui::Modifiers::NONE);
            h.event(egui::Event::PointerMoved(q));
            h.step();
            press(h, q, false, egui::Modifiers::NONE);
        }
        Act::Button(label) => {
            let Some(b) = h.query_by_role_and_label(Role::Button, label) else { return true };
            b.click();
        }
        Act::Word(w) => {
            for c in w.chars() {
                h.event(egui::Event::Text(c.to_string()));
                h.step();
            }
        }
        Act::Compose(marked, committed) => {
            h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: (*marked).to_owned(), active_range_chars: None }));
            h.step();
            h.event(egui::Event::Ime(egui::ImeEvent::Commit((*committed).to_owned())));
        }
        Act::Undo => h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z),
        Act::Redo => h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z),
    }
    h.try_run().is_ok()
}

/// What failed after an action.
#[derive(Debug)]
struct Failure {
    rule: &'static str,
    detail: String,
}

fn fail(rule: &'static str, detail: String) -> Result<(), Failure> {
    Err(Failure { rule, detail })
}

/// Runs the case's actions on a window; the first failure, at which action.
fn run(doc: &str, acts: &[Act]) -> Option<(usize, Failure)> {
    let out = catch_unwind(AssertUnwindSafe(|| {
        let mut h = formatted(doc);
        let mut history = vec![text(&mut h)];
        for (i, a) in acts.iter().enumerate() {
            if let Err(f) = step(&mut h, a, &mut history) {
                return Some((i, f));
            }
        }
        None
    }));
    match out {
        Ok(found) => found,
        Err(e) => {
            let message = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
            Some((usize::MAX, Failure { rule: "panic", detail: message }))
        }
    }
}

fn step(h: &mut Harness<'_, App>, a: &Act, history: &mut Vec<String>) -> Result<(), Failure> {
    let before = text(h);
    let caret_before = caret(h);
    if !perform(h, a) {
        return fail("unsettled", "the frames did not settle".to_owned());
    }
    let after = text(h);
    if h.query_by_role(Role::Dialog).is_some() {
        h.key_press(egui::Key::Escape);
        h.run();
    }
    if !h.query_by_label("main.tex formatted").is_some_and(|n| n.accesskit_node().is_focused()) {
        // Tab outside a list or a table goes on to the next control: no trap.
        if matches!(a, Act::Key(_, egui::Key::Tab)) {
            h.get_by_label("main.tex formatted").focus();
            h.run();
            return Ok(());
        }
        return fail("focus-lost", "the keyboard left the text".to_owned());
    }
    let Some((rect, value, at)) = caret(h) else {
        return if lines(h).is_empty() { Ok(()) } else { fail("no-caret", "the caret is in no line of the view".to_owned()) };
    };
    if rect.max.y < 0.0 || rect.min.y > SIZE.y {
        return fail("caret-offscreen", format!("the caret's line is at {rect:?}, outside the window"));
    }
    let typed = match a {
        Act::Type(s) => Some(s.as_str()),
        Act::Word(s) | Act::Compose(_, s) => Some(*s),
        _ => None,
    };
    if let Some(s) = typed
        && s.chars().all(char::is_alphabetic)
        && after != before
    {
        let shown: String = value.chars().take(at).collect();
        if !shown.ends_with(s) {
            return fail("caret-misplaced", format!("typed {s:?}, the caret's line reads {:?} before the caret", shown));
        }
        if let Some((r0, ..)) = caret_before
            && (r0.min.y - rect.min.y).abs() > 40.0
        {
            return fail("layout-jump", format!("the caret's line moved from y {} to {} on typing a letter", r0.min.y, rect.min.y));
        }
    }
    if let Act::Word(w) = a
        && after != before
    {
        one_undo_per_word(h, w, (&before, &after))?;
    }
    if after != before && !matches!(a, Act::Undo | Act::Redo) {
        undo_redo(h, (&before, &after), history)?;
        history.push(after);
    }
    Ok(())
}

/// Undo after an edit gives back an earlier text exactly (typing goes back in one step with the
/// typing before it); redo gives back the text after the edit.
fn undo_redo(h: &mut Harness<'_, App>, (before, after): (&str, &str), history: &[String]) -> Result<(), Failure> {
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    let undone = text(h);
    if undone != before && !history.contains(&undone) {
        return fail("undo", format!("undo gave a text that never was: {undone:?}, before the edit {before:?}"));
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    h.run();
    let redone = text(h);
    if redone != after {
        return fail("redo", format!("redo gave {redone:?}, the edit made {after:?}"));
    }
    Ok(())
}

/// One undo after a word typed letter by letter takes the whole word back, not its last letter
/// (it may take more: the typing before it, when the word goes on from it). Redo puts it back.
fn one_undo_per_word(h: &mut Harness<'_, App>, word: &str, (before, after): (&str, &str)) -> Result<(), Failure> {
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    let undone = text(h);
    // Some of the word left: undo went back a letter, not a word.
    let left = (1..word.len()).any(|k| after.replacen(word, &word[..k], 1) == undone);
    if left || undone == after {
        return fail("undo-word", format!("one undo after typing {word:?} gave {undone:?}, before the word {before:?}"));
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    h.run();
    Ok(())
}

/// Fewer actions while the case fails the same way.
fn shrink(doc: &str, acts: &[Act], rule: &str, budget: usize) -> Vec<Act> {
    let mut best = acts.to_vec();
    let mut tries = 0;
    let mut i = 0;
    while i < best.len() && tries < budget {
        let mut c = best.clone();
        c.remove(i);
        tries += 1;
        if !c.is_empty() && run(doc, &c).is_some_and(|(_, f)| f.rule == rule) {
            best = c;
        } else {
            i += 1;
        }
    }
    best
}

fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

#[test]
fn a_scripted_user_keeps_the_keyboard_the_caret_and_the_history_of_the_formatted_view() {
    let cases = env_number("TEXCRAFT_FUZZ_WINDOW_CASES", 8);
    let mut rng = Rng(env_number("TEXCRAFT_FUZZ_SEED", 1));
    let mut found: Vec<String> = Vec::new();
    for _ in 0..cases {
        let (name, doc) = DOCS[rng.below(DOCS.len())];
        let acts: Vec<Act> = (0..8 + rng.below(16)).map(|_| act(&mut rng)).collect();
        let Some((at, f)) = run(doc, &acts) else { continue };
        if found.iter().any(|s| s.starts_with(&format!("[{}]", f.rule))) {
            continue;
        }
        let cut = if at < acts.len() { &acts[..=at] } else { &acts[..] };
        let small = shrink(doc, cut, f.rule, 40);
        found.push(format!("[{}] after {} actions on {name}: {}\n  actions: {small:?}", f.rule, small.len(), f.detail));
    }
    assert!(found.is_empty(), "{} kinds of failure:\n\n{}", found.len(), found.join("\n\n"));
}
