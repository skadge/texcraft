//! Missing packages on the desktop (B13): the first build that lacks a TeX Live package while
//! Settings › Building › Missing packages is still Ask asks what to do; the answer is kept in the
//! setting, which Settings shows.

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::missing::{Lack, MissingPackages};
use texcraft_compile::{Backend, Job, Outcome, Status};
use texcraft_engine::Session;
use texcraft_ui_egui::{App, ui_cmd};

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

const QUESTION: &str = "epigraph is not installed";

/// A window over a project that lacks `epigraph`, with the setting `setting`, and the jobs' record.
fn window(setting: MissingPackages) -> Result<(Harness<'static, App>, Seen), String> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = Session::with_backend(Box::new(Lacking(seen.clone())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\\usepackage{epigraph}"}))?;
    let prefs = texcraft_ui_egui::prefs::Prefs { auto_compile: false, missing_packages: setting, ..Default::default() };
    let app = App::with_prefs(s, prefs);
    let h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    Ok((h, seen))
}

/// Compiles and lets the build end.
fn compile(h: &mut Harness<'_, App>, seen: &Seen) {
    let before = seen.lock().map_or(0, |s| s.len());
    ui_cmd::run(h.state_mut(), "build.compile");
    for _ in 0..40 {
        h.step();
        if seen.lock().map_or(0, |s| s.len()) > before && !h.state().session().is_building() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.run();
    h.run();
}

fn dialog(h: &mut Harness<'_, App>) -> String {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["dialog"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn the_first_missing_package_asks_once_and_not_now_keeps_ask() -> Result<(), String> {
    let (mut h, seen) = window(MissingPackages::Ask)?;
    compile(&mut h, &seen);
    assert_eq!(dialog(&mut h), "missing_packages", "the first missing package did not ask");
    assert!(h.query_by_label(QUESTION).is_some(), "the question does not name the package");
    // The keyboard is in the question: its first answer.
    let first = h.get_by_label("Download Automatically");
    assert!(first.is_focused(), "the question does not take the keyboard");
    h.get_by_label("Not Now").click();
    h.run();
    assert_eq!(dialog(&mut h), "", "Not Now did not close the question");
    assert_eq!(h.state().session().settings.missing_packages, MissingPackages::Ask, "Not Now chose");
    compile(&mut h, &seen);
    assert_eq!(dialog(&mut h), "", "the next build of the session asked again");
    Ok(())
}

#[test]
fn other_settings_do_not_ask() -> Result<(), String> {
    for setting in [MissingPackages::Auto, MissingPackages::Manual, MissingPackages::Off] {
        let (mut h, seen) = window(setting)?;
        compile(&mut h, &seen);
        assert_eq!(dialog(&mut h), "", "asked with {setting:?}");
    }
    Ok(())
}

#[test]
fn each_answer_is_kept_and_settings_shows_it() -> Result<(), String> {
    for (answer, kept, shown) in [
        ("Download Automatically", MissingPackages::Auto, "Download automatically"),
        ("Install Them Myself", MissingPackages::Manual, "Install them myself"),
        ("Don't Download", MissingPackages::Off, "Don't download"),
    ] {
        let (mut h, seen) = window(MissingPackages::Ask)?;
        compile(&mut h, &seen);
        h.get_by_label(answer).click();
        // Steps, not `run`: Download Automatically starts a build at once, whose spinner keeps
        // repainting (under load, past `run`'s few frames).
        for _ in 0..4 {
            h.step();
        }
        assert_eq!(h.state().session().settings.missing_packages, kept, "{answer} was not kept");
        let prefs = h.state_mut().take_changed_prefs().ok_or("the answer is not saved in the preferences")?;
        assert_eq!(prefs.missing_packages, kept, "{answer}: the preferences keep another value");
        if kept == MissingPackages::Auto {
            // At once: a build that downloads.
            for _ in 0..40 {
                h.step();
                if seen.lock().map_or(0, |s| s.len()) > 1 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(seen.lock().map_err(|e| e.to_string())?.last(), Some(&MissingPackages::Auto), "Download did not build again with downloads");
        }
        ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": "building"}));
        h.run();
        h.run();
        let valued = h.query_all_by_label("Missing packages").any(|n| n.accesskit_node().value().is_some_and(|v| v.contains(shown)));
        assert!(valued || h.query_all_by_label(shown).next().is_some(), "Settings does not show {shown} for the answer {answer}");
    }
    Ok(())
}

#[test]
fn open_settings_goes_to_the_setting() -> Result<(), String> {
    let (mut h, seen) = window(MissingPackages::Ask)?;
    compile(&mut h, &seen);
    h.get_by_label("Open Settings").click();
    h.run();
    h.run();
    assert_eq!(dialog(&mut h), "settings", "Open Settings did not open Settings");
    assert!(h.query_all_by_label("Missing packages").next().is_some(), "Settings does not show the setting");
    assert_eq!(h.state().session().settings.missing_packages, MissingPackages::Ask, "Open Settings chose");
    Ok(())
}
