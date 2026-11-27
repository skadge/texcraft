//! Missing TeX Live packages on the desktop (B13): the setting reaches the engine, and only a
//! window asks about the first missing package, once a session.

use std::sync::{Arc, Mutex};

use serde_json::json;
use texcraft_compile::missing::{Lack, MissingPackages};
use texcraft_compile::{Backend, Job, Outcome, Status};
use texcraft_engine::Session;

/// The setting each job carried.
type Seen = Arc<Mutex<Vec<MissingPackages>>>;

/// A build that lacks the package `epigraph`, recording the setting each job carries.
struct Lacking(Seen);

impl Backend for Lacking {
    fn name(&self) -> String {
        "test".to_owned()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(job.missing_packages);
        }
        let mut o = Outcome::unavailable(job.id, "");
        o.status = Status::Failed;
        o.missing = vec![Lack::File { file: "epigraph.sty".into(), package: Some("epigraph".into()) }];
        o
    }
}

fn session() -> Result<(Session, Seen), String> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_backend(Box::new(Lacking(seen.clone())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\\usepackage{epigraph}"}))?;
    Ok((s, seen))
}

#[test]
fn the_command_sets_what_builds_do_and_a_change_builds_again() -> Result<(), String> {
    let (mut s, seen) = session()?;
    s.execute("build.compile", &json!({"wait": true}))?;
    assert!(!s.is_stale());
    let set = s.execute("build.missing_packages", &json!({"value": "auto"}))?;
    assert_eq!(set["downloads"], json!(true), "{set}");
    assert!(s.is_stale(), "turning downloads on does not build again");
    s.execute("build.compile", &json!({"wait": true}))?;
    let jobs = seen.lock().map_err(|e| e.to_string())?.clone();
    assert_eq!(jobs, [MissingPackages::Ask, MissingPackages::Auto], "the jobs do not carry the setting");
    assert!(s.execute("build.missing_packages", &json!({"value": "always"})).is_err(), "a value that is no choice was taken");
    Ok(())
}

#[test]
fn without_a_window_nothing_asks_and_the_result_says_how_to_download() -> Result<(), String> {
    let (mut s, _) = session()?;
    let result = s.execute("build.compile", &json!({"wait": true}))?;
    assert_eq!(s.take_package_question(), None, "a session without a window asked");
    let missing = &result["missingPackages"];
    assert_eq!(missing["lacked"][0]["package"], json!("epigraph"), "{result}");
    assert_eq!(missing["setting"], json!("ask"));
    assert!(missing["how"].as_str().is_some_and(|h| h.contains("build.missing_packages")), "{missing}");
    Ok(())
}

#[test]
fn a_window_is_asked_once_and_only_while_the_setting_is_ask() -> Result<(), String> {
    let (mut s, _) = session()?;
    s.set_asks_about_packages(true);
    s.execute("build.compile", &json!({"wait": true}))?;
    let asked = s.take_package_question();
    assert_eq!(asked.as_ref().and_then(Lack::name), Some("epigraph"));
    s.execute("build.compile", &json!({"wait": true}))?;
    assert_eq!(s.take_package_question(), None, "the next build of the session asked again");
    for value in ["auto", "manual", "off"] {
        let (mut s, _) = session()?;
        s.set_asks_about_packages(true);
        s.execute("build.missing_packages", &json!({"value": value}))?;
        s.execute("build.compile", &json!({"wait": true}))?;
        assert_eq!(s.take_package_question(), None, "asked with the setting {value}");
    }
    Ok(())
}
