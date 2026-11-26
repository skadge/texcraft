//! E11: spell checking a file's prose against a dictionary and the word lists, through the
//! `language.*` commands agents use.

use std::path::PathBuf;

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::spelling::Dictionary;

type R = Result<(), Box<dyn std::error::Error>>;

/// Knows a few English words; offers "word" for "wrod". No other language.
struct Fake;

const KNOWN: &[&str] = &["the", "a", "word", "is", "here", "and", "see", "text", "title"];

impl Dictionary for Fake {
    fn available(&self, lang: &str) -> Result<(), String> {
        if lang.starts_with("en") { Ok(()) } else { Err(format!("There is no spelling dictionary for {lang}.")) }
    }
    fn check(&self, word: &str, _: &str) -> bool {
        KNOWN.contains(&word.to_lowercase().as_str())
    }
    fn suggest(&self, word: &str, _: &str, max: usize) -> Vec<String> {
        if word == "wrod" { vec!["word".to_owned()].into_iter().take(max).collect() } else { Vec::new() }
    }
}

fn session(text: &str) -> Result<Session, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_dictionary(Box::new(Fake));
    s.execute("file.write", &json!({"path": "main.tex", "text": text}))?;
    Ok(s)
}

fn words(v: &Value) -> Vec<String> {
    v["misspellings"].as_array().map(|a| a.iter().filter_map(|m| m["word"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn check(s: &mut Session) -> Result<Value, String> {
    s.execute("language.spelling", &json!({"path": "main.tex"}))
}

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-spelling-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn only_the_prose_is_checked_and_each_misspelling_has_its_place_and_suggestions() -> R {
    let mut s = session("\\section{The wrod}\nSee \\ref{sec:zzkey} and $xyzzy$ here.\n\\label{qqq} text wrod\n")?;
    let v = check(&mut s)?;
    assert_eq!(words(&v), ["wrod", "wrod"]);
    let second = &v["misspellings"][1];
    assert_eq!((second["line"].as_u64(), second["column"].as_u64(), second["end_column"].as_u64()), (Some(3), Some(18), Some(22)));
    assert_eq!(second["suggestions"], json!(["word"]));
    // Lines limit the check.
    let first_line = s.execute("language.spelling", &json!({"path": "main.tex", "line": 1, "end_line": 1}))?;
    assert_eq!(words(&first_line), ["wrod"]);
    Ok(())
}

#[test]
fn added_and_ignored_words_are_no_longer_marked() -> R {
    let mut s = session("The wrod and the zorblax.\n")?;
    assert_eq!(words(&check(&mut s)?), ["wrod", "zorblax"]);
    s.execute("language.ignore_word", &json!({"word": "wrod"}))?;
    s.execute("language.add_word", &json!({"word": "Zorblax"}))?;
    assert!(words(&check(&mut s)?).is_empty());
    assert_eq!(s.user_words(), ["zorblax"], "the user's words are what the shell keeps");
    assert!(s.execute("language.add_word", &json!({"word": "two words"})).is_err(), "a list takes single words");
    Ok(())
}

#[test]
fn the_projects_words_are_kept_in_its_folder_and_read_back() -> R {
    let dir = temp("project")?;
    let mut s = session("A frobnicate here.\n")?;
    s.execute("project.save_as", &json!({"path": dir.to_string_lossy()}))?;
    let added = s.execute("language.add_word", &json!({"word": "frobnicate", "to": "project"}))?;
    assert_eq!(added["kept"], "project");
    assert!(std::fs::read_to_string(dir.join(".texcraft/words.txt"))?.lines().any(|l| l == "frobnicate"));

    let mut other = Session::with_backend(Box::new(Unavailable("tests".into())));
    other.set_dictionary(Box::new(Fake));
    other.execute("project.open", &json!({"path": dir.to_string_lossy()}))?;
    let main = other.project().main().map(|m| m.as_str().to_owned()).ok_or("no main file")?;
    let v = other.execute("language.spelling", &json!({"path": main}))?;
    assert!(words(&v).is_empty(), "the project's word was not read back: {v}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_language_without_a_dictionary_and_spelling_off_say_why_nothing_is_checked() -> R {
    let mut s = session("\\documentclass{article}\n\\usepackage[french]{babel}\n\\begin{document}\nUn mot.\n\\end{document}\n")?;
    let err = check(&mut s).err().unwrap_or_default();
    assert!(err.contains("no spelling dictionary for fr"), "{err}");
    let options = s.execute("language.spelling_options", &json!({"language": "en-GB"}))?;
    assert_eq!((options["checking"].as_str(), options["available"].as_bool()), (Some("en-GB"), Some(true)));
    assert_eq!(words(&check(&mut s)?), ["Un", "mot"]);
    s.execute("language.spelling_options", &json!({"on": false}))?;
    assert!(check(&mut s).err().unwrap_or_default().contains("Spelling is off"));
    assert!(s.execute("language.spelling_options", &json!({"language": "not a tag!"})).is_err());
    Ok(())
}

#[test]
fn the_suites_english_dictionary_marks_a_misspelling_and_offers_its_fix() -> R {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "This sentence has a mistaek in \\emph{it}.\n"}))?;
    let v = check(&mut s)?;
    assert_eq!(words(&v), ["mistaek"]);
    let offered: Vec<&str> = v["misspellings"][0]["suggestions"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    assert!(offered.contains(&"mistake"), "{offered:?}");
    Ok(())
}

#[test]
fn a_british_document_is_checked_with_british_spellings_and_an_american_one_with_american() -> R {
    // Issue #36: "labour" was flagged, with "labor" offered, in a British text.
    // Telling the two apart takes a British dictionary: SCOWL's embedded from craft-i18n, or one
    // installed on the system. The built-in word list alone has one English, so only the language
    // chosen is checked then.
    let dialects = craft_proof::english_source(craft_proof::ProofLang::EnGb).is_some_and(|s| s != craft_proof::EnglishSource::CoreList);
    let text = |lang: &str| {
        format!("\\documentclass{{article}}\n\\usepackage[{lang}]{{babel}}\n\\begin{{document}}\nThe labour of the neighbour.\n\\end{{document}}\n")
    };
    for british in ["british", "UKenglish"] {
        let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
        s.execute("file.write", &json!({"path": "main.tex", "text": text(british)}))?;
        let v = check(&mut s)?;
        assert_eq!(v["language"], "en-GB", "{v}");
        assert!(!dialects || words(&v).is_empty(), "British spellings flagged in a {british} document: {v}");
    }
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": text("american")}))?;
    let v = check(&mut s)?;
    assert_eq!(v["language"], "en-US", "{v}");
    assert!(!dialects || words(&v).iter().any(|w| w == "labour"), "an American document takes British spellings: {v}");
    // Chosen in the settings, British English is checked whatever the document says.
    s.execute("language.spelling_options", &json!({"language": "en-GB"}))?;
    let v = check(&mut s)?;
    assert_eq!(v["language"], "en-GB", "{v}");
    assert!(!dialects || words(&v).is_empty(), "{v}");
    Ok(())
}
