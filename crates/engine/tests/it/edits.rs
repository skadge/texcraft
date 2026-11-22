//! F1: edits, not texts (docs/collaboration-roadmap.md).

use serde_json::{Value, json};
use texcraft_engine::Origin;
use texcraft_engine::project::ProjectPath;

fn session() -> texcraft_engine::Session {
    texcraft_engine::Session::with_backend(Box::new(texcraft_compile::Unavailable("tests".into())))
}

/// A small deterministic generator, so the test needs no dependency.
struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

fn read(s: &mut texcraft_engine::Session) -> String {
    let v = s.execute("file.read", &json!({"path": "main.tex"})).unwrap_or_default();
    v["text"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn a_sequence_of_edits_gives_the_text_of_making_them_on_a_string() {
    let mut s = session();
    let mut expected: Vec<char> = "Le chat 😀 est là.\n\\section{x}\n".chars().collect();
    s.execute("file.write", &json!({"path": "main.tex", "text": expected.iter().collect::<String>()})).unwrap();
    let mut rng = Lcg(3);
    let words = ["", "é", "😀", "\n", "ab", "\\end{x}"];
    for _ in 0..300 {
        // Up to three non-overlapping changes, sent in reverse order to show order does not matter.
        let mut edits = Vec::new();
        let mut at = 0;
        for _ in 0..3 {
            if at > expected.len() {
                break;
            }
            let start = at + rng.below(expected.len() - at + 1);
            let end = start + rng.below(expected.len() - start + 1).min(4);
            edits.push((start, end, words[rng.below(words.len())]));
            at = end + 1;
        }
        let list: Vec<Value> = edits.iter().rev().map(|(a, b, t)| json!({"start": a, "end": b, "text": t})).collect();
        s.execute("file.edit", &json!({"path": "main.tex", "edits": list})).unwrap();
        for (a, b, t) in edits.iter().rev() {
            expected.splice(a..b, t.chars());
        }
        assert_eq!(read(&mut s), expected.iter().collect::<String>());
    }
}

#[test]
fn overlapping_edits_are_refused_and_change_nothing() {
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "abcdef"})).unwrap();
    let r = s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": 0, "end": 3, "text": ""}, {"start": 2, "end": 4, "text": "x"}]}));
    assert!(r.is_err());
    assert_eq!(read(&mut s), "abcdef");
}

#[test]
fn writing_a_slightly_changed_text_records_a_small_edit() {
    let mut s = session();
    let long = "\\documentclass{article}\n".to_owned() + &"Some text of the paper.\n".repeat(200);
    s.execute("file.write", &json!({"path": "main.tex", "text": long})).unwrap();
    let before = s.revision();
    s.execute("file.write", &json!({"path": "main.tex", "text": long.replacen("Some", "Same", 1)})).unwrap();
    let path = ProjectPath::new("main.tex").unwrap();
    let edits = s.edits_since(&path, before).unwrap();
    let [edit] = edits.as_slice() else { panic!("not one edit: {edits:?}") };
    let [change] = edit.changes.changes() else { panic!("not one change") };
    assert!(change.end - change.start < 5 && change.text.len() < 5, "{change:?}");
    assert_eq!(edit.origin, Origin::Local);
}

#[test]
fn the_log_keeps_each_edit_with_its_origin_and_file() {
    let mut s = session();
    s.execute("file.new", &json!({"path": "other.tex", "text": "x"})).unwrap();
    let before = s.revision();
    s.execute("file.edit", &json!({"path": "main.tex", "edits": [{"start": 0, "end": 0, "text": "a"}], "origin": "remote"})).unwrap();
    s.execute("file.edit", &json!({"path": "other.tex", "edits": [{"start": 0, "end": 0, "text": "b"}]})).unwrap();
    s.execute("file.write", &json!({"path": "main.tex", "text": "changed on disk", "origin": "external"})).unwrap();
    let path = ProjectPath::new("main.tex").unwrap();
    let origins: Vec<Origin> = s.edits_since(&path, before).unwrap().iter().map(|e| e.origin).collect();
    assert_eq!(origins, [Origin::Remote, Origin::External]);
    assert!(s.execute("file.edit", &json!({"path": "main.tex", "edits": [], "origin": "someone"})).is_err());
}

#[test]
fn replace_all_changes_only_the_matches() {
    let mut s = session();
    s.execute("file.write", &json!({"path": "main.tex", "text": "colour, then a long middle, then colour"})).unwrap();
    let before = s.revision();
    let r = s.execute("file.replace", &json!({"path": "main.tex", "pattern": "colour", "replacement": "color"})).unwrap();
    assert_eq!(r["replaced"], 2);
    let path = ProjectPath::new("main.tex").unwrap();
    let edits = s.edits_since(&path, before).unwrap();
    let changed: usize = edits.iter().flat_map(|e| e.changes.changes()).map(|c| c.end - c.start).sum();
    assert!(changed < 15, "the middle was replaced too");
    assert_eq!(read(&mut s), "color, then a long middle, then color");
}
