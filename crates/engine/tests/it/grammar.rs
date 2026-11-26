//! E12: English grammar checked offline by Harper on a file's prose, through the `grammar.*`
//! commands agents use, and in the background for the editor.

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;

type R = Result<(), Box<dyn std::error::Error>>;

fn session(text: &str) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    Ok(s)
}

fn check(s: &mut Session) -> Result<Value, String> {
    s.execute("grammar.check", &json!({"path": "main.tex"}))
}

fn problems(v: &Value) -> Vec<Value> {
    v["problems"].as_array().cloned().unwrap_or_default()
}

/// The words each problem flags, as the source has them.
fn flagged(text: &str, v: &Value) -> Vec<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    problems(v)
        .iter()
        .filter_map(|p| {
            let (l, c, e) = (p["line"].as_u64()? as usize - 1, p["column"].as_u64()? as usize - 1, p["end_column"].as_u64()? as usize - 1);
            (p["end_line"] == p["line"]).then(|| lines.get(l).map(|line| line.chars().skip(c).take(e - c).collect()))?
        })
        .collect()
}

fn text(s: &Session) -> String {
    ProjectPath::new("main.tex").ok().and_then(|p| s.project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn findings_land_on_the_source_words_and_commands_and_math_are_not_read() -> R {
    let text = "\\section{Results}\nWe has \\textbf{seen} $a$ result. It was \\emph{an} mistake.\n\\label{sec:has}\n";
    let mut s = session(text)?;
    let v = check(&mut s)?;
    let words = flagged(text, &v);
    assert!(words.iter().any(|w| w == "has"), "the agreement mistake was not found where it is: {v}");
    // Every finding is on prose: none on the label's key or inside the formula.
    for p in problems(&v) {
        assert_ne!(p["line"], 3, "a key was read as prose: {p}");
    }
    assert!(!words.iter().any(|w| w.contains('$')), "math was read as prose: {v}");
    Ok(())
}

#[test]
fn misspellings_are_left_to_the_spell_checker() -> R {
    let mut s = session("This sentense has a mispelled wurd in it.\n")?;
    let v = check(&mut s)?;
    let spelling: Vec<Value> = problems(&v).into_iter().filter(|p| p["message"].as_str().is_some_and(|m| m.contains("spell"))).collect();
    assert!(spelling.is_empty(), "Harper's spelling came through: {spelling:?}");
    Ok(())
}

#[test]
fn a_replacement_taken_changes_only_the_flagged_words_and_keeps_the_latex() -> R {
    let original = "It was a \\textbf{error} here. This is an mistake, see \\ref{sec:a}.\n";
    let mut s = session(original)?;
    let v = check(&mut s)?;
    let p = problems(&v).into_iter().find(|p| p["replacements"].as_array().is_some_and(|r| !r.is_empty())).ok_or("no problem with a replacement")?;
    s.execute("grammar.fix", &json!({"path": "main.tex", "id": p["id"]}))?;
    let after = text(&s);
    assert_ne!(after, original);
    for kept in ["\\textbf{error}", "\\ref{sec:a}"] {
        assert!(after.contains(kept), "{kept} was lost: {after}");
    }
    // A problem across a command offers no replacement: taking one would lose it.
    for p in problems(&check(&mut s)?) {
        if p["message"].as_str().is_some_and(|m| m.contains("article")) && p["line"] == 1 && p["column"] == 7 {
            assert_eq!(p["replacements"], json!([]), "{p}");
        }
    }
    Ok(())
}

#[test]
fn an_ignored_problem_and_a_rule_or_group_turned_off_are_not_reported() -> R {
    let mut s = session("This is an mistake. We has two.\n")?;
    let all = problems(&check(&mut s)?);
    let first = all.first().ok_or("nothing found")?.clone();
    s.execute("grammar.ignore", &json!({"id": first["id"]}))?;
    let after = problems(&check(&mut s)?);
    assert!(after.iter().all(|p| p["id"] != first["id"]) && after.len() < all.len());
    // Turning a rule off hides what it finds; turning it on shows it again.
    let other = after.first().ok_or("nothing else found")?.clone();
    s.execute("grammar.rule", &json!({"rule": other["rule"], "on": false}))?;
    assert!(problems(&check(&mut s)?).iter().all(|p| p["rule"] != other["rule"]));
    s.execute("grammar.rule", &json!({"rule": other["rule"], "on": true}))?;
    assert!(problems(&check(&mut s)?).iter().any(|p| p["rule"] == other["rule"]));
    s.execute("grammar.rule", &json!({"group": other["group"], "on": false}))?;
    assert!(problems(&check(&mut s)?).iter().all(|p| p["group"] != other["group"]));
    assert!(s.execute("grammar.rule", &json!({"rule": "NoSuchRule", "on": false})).is_err());
    Ok(())
}

#[test]
fn off_and_other_languages_say_why_nothing_is_checked() -> R {
    let mut s = session("\\documentclass{article}\n\\usepackage[french]{babel}\n\\begin{document}\nIl a un erreur.\n\\end{document}\n")?;
    s.execute("file.set_main", &json!({"path": "main.tex"}))?;
    let e = check(&mut s).err().ok_or("a French document was checked as English")?;
    assert!(e.contains("English") && e.contains("AI"), "{e}");
    let mut s = session("This is an mistake.\n")?;
    s.execute("grammar.options", &json!({"on": false}))?;
    assert!(check(&mut s).is_err());
    Ok(())
}

#[test]
fn the_editor_never_waits_for_the_checker_and_gets_the_findings_once_back() -> R {
    let mut s = session("This is an mistake.\n")?;
    let path = ProjectPath::new("main.tex")?;
    // Asked for each frame: the first answer comes at once, before the checker is done.
    let first = s.grammar_problems(&path, Some(0..10))?;
    assert!(first.is_empty() && s.grammar_busy(), "the first frame waited for the checker");
    let start = std::time::Instant::now();
    while s.grammar_busy() && start.elapsed() < std::time::Duration::from_secs(60) {
        s.grammar_problems(&path, Some(0..10))?;
        std::thread::yield_now();
    }
    assert!(!s.grammar_problems(&path, Some(0..10))?.is_empty(), "the findings never came back");
    Ok(())
}

/// A long document of varied paragraphs, sections, formulas and references.
fn fifty_pages() -> String {
    let nouns = ["method", "result", "model", "data set", "system", "approach", "analysis", "network", "value", "sample", "theory", "process"];
    let mut t = String::from("\\documentclass{article}\n\\begin{document}\n");
    for i in 0..400 {
        if i % 8 == 0 {
            t.push_str(&format!("\\section{{Part {i}}}\\label{{sec:{i}}}\n\n"));
        }
        let n = |k: usize| nouns[(i * 7 + k * 3) % nouns.len()];
        t.push_str(&format!(
            "The {} we propose for case {i} is evaluated on the {} of \\cite{{key{i}}}, and the {} shows that $x_{i}$ grows faster than the {} \
             in most cases (see Section~\\ref{{sec:{i}}}). However, there is still room for improvement, in particular when the {} is very large.{}\n\n",
            n(0),
            n(1),
            n(2),
            n(3),
            n(4),
            if i % 5 == 0 { " This is an mistake." } else { "" }
        ));
    }
    t.push_str("\\end{document}\n");
    t
}

#[test]
fn a_long_document_is_checked_in_the_background_its_findings_growing_as_they_come() -> R {
    let mut s = session(&fifty_pages())?;
    let path = ProjectPath::new("main.tex")?;
    let started = std::time::Instant::now();
    let first = s.grammar_problems(&path, Some(100..160))?.len();
    let mut slowest = started.elapsed();
    assert!(s.grammar_busy(), "the whole document was checked within the first frame");
    let mut frames = 0;
    while s.grammar_busy() && started.elapsed() < std::time::Duration::from_secs(600) {
        let t = std::time::Instant::now();
        s.grammar_problems(&path, None)?;
        slowest = slowest.max(t.elapsed());
        frames += 1;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let all = s.grammar_problems(&path, None)?.len();
    eprintln!("50 pages: checked in {:?} over {frames} frames, the slowest frame {slowest:?}; {all} problems", started.elapsed());
    assert!(all > first, "the findings did not grow as the checker went on");
    Ok(())
}
