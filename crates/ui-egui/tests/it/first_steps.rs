//! A new user's first steps (docs/ROADMAP.md, A14): the empty start page says what to do first, the
//! example project opens from it, the template cards say what each template is and whether it is
//! ready offline, the tip of the day, and the first-steps hint that stays dismissed.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::library::Library;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
}

fn start_page(s: Session, prefs: Prefs, today: chrono::NaiveDate) -> Harness<'static, App> {
    let mut app = App::with_prefs(s, Prefs { auto_compile: false, ..prefs });
    app.show_start_page();
    app.set_today(today);
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1600.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    run(&mut h);
    h
}

fn desktop() -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(std::env::temp_dir().join(format!("texcraft-first-steps-{}", std::process::id())));
    s
}

fn browser() -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.use_library(Library::default());
    s
}

fn run(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.run();
    }
}

fn status(h: &mut Harness<'_, App>) -> String {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["status"]["text"].as_str().unwrap_or_default().to_owned()
}

fn dialog(h: &mut Harness<'_, App>) -> String {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["dialog"].as_str().unwrap_or_default().to_owned()
}

/// The longest text inside the region named `name`.
fn region_text(h: &Harness<'_, App>, name: &str) -> Option<String> {
    let region = h.query_by_role_and_label(Role::Region, name)?;
    region
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == Role::Label)
        .filter_map(|n| n.accesskit_node().value())
        .max_by_key(String::len)
}

/// The tip shown.
fn tip(h: &Harness<'_, App>) -> Option<String> {
    region_text(h, "Tip of the day")
}

fn click(h: &mut Harness<'_, App>, name: &str) {
    h.get_by_role_and_label(Role::Button, name).click();
    run(h);
}

#[test]
fn an_empty_start_page_says_what_to_do_first_and_offers_the_example() {
    let h = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    assert!(h.query_by_label("No projects yet.").is_some(), "the empty start page does not say there are no projects");
    assert!(h.query_by_label_contains("To begin, try the example").is_some(), "it does not say what to do first");
    assert!(h.query_by_role_and_label(Role::Button, "Try an Example").is_some(), "the example is not offered");
    assert!(h.query_by_role_and_label(Role::Button, "Example project").is_some(), "the example's card is missing");
}

#[test]
fn the_example_opens_as_a_project_in_the_browser_under_a_name_to_accept() {
    let mut h = start_page(browser(), Prefs::default(), day(2026, 11, 26));
    h.get_by_role_and_label(Role::Button, "Try an Example").click();
    run(&mut h);
    assert_eq!(dialog(&mut h), "prompt", "no name was asked for");
    // The name is given: Create takes it as it is.
    click(&mut h, "Create");
    let s = h.state().session();
    assert_eq!(s.dir().map(|d| d.display().to_string()).as_deref(), Some("Example paper"));
    assert_eq!(s.project().main().map(|p| p.as_str()), Some("main.tex"));
    let paths: Vec<String> = s.project().paths().map(|p| p.as_str().to_owned()).collect();
    for file in ["sections/introduction.tex", "references.bib"] {
        assert!(paths.iter().any(|p| p == file), "{file} is missing: {paths:?}");
    }
    assert!(h.query_by_role_and_label(Role::Region, "Tip of the day").is_none(), "the start page is still shown");
}

#[test]
fn template_cards_say_what_each_is_for_who_it_is_from_its_licence_in_words_and_whether_it_is_ready_offline() {
    let h = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    let card = |name: &str| {
        let n = h.get_by_role_and_label(Role::Button, name);
        n.accesskit_node().description().unwrap_or_default()
    };
    let article = card("Article");
    assert!(article.contains("Built in") && article.contains("Free to use (public domain)") && article.contains("Ready offline"), "{article}");
    assert!(!article.contains("CC0"), "the licence is an SPDX id: {article}");
    let ieee = card("IEEE journal article");
    assert!(ieee.contains("IEEE") && ieee.contains("Free to use (LPPL)") && ieee.contains("Downloads when used"), "{ieee}");
    // A one-line description first, so the card says what the template is for.
    assert!(ieee.split(". ").next().is_some_and(|d| d.len() > 10), "{ieee}");
    assert!(h.query_by_label("Example paper: folding a sheet of paper").is_none(), "the example is listed as a template");
}

#[test]
fn the_tip_of_the_day_stays_all_day_changes_the_next_and_steps_with_next_and_previous() {
    let first = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    let today = tip(&first);
    assert!(today.is_some(), "no tip on the start page");
    let again = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    assert_eq!(tip(&again), today, "the tip changed within the day");
    let tomorrow = start_page(desktop(), Prefs::default(), day(2026, 11, 27));
    assert_ne!(tip(&tomorrow), today, "the next day has the same tip");

    let mut h = first;
    click(&mut h, "Next Tip");
    let next = tip(&h);
    assert_ne!(next, today, "Next Tip did not change the tip");
    click(&mut h, "Previous Tip");
    assert_eq!(tip(&h), today, "Previous Tip did not go back");
}

#[test]
fn turning_tips_off_hides_the_card_and_is_kept() {
    let mut h = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    h.get_by_label("Show tips at startup").click();
    run(&mut h);
    assert!(h.query_by_role_and_label(Role::Region, "Tip of the day").is_none(), "the card is still shown");
    assert!(status(&mut h).contains("Settings"), "where to turn them on again is not said");
    let prefs = h.state_mut().take_changed_prefs();
    assert!(prefs.as_ref().is_some_and(|p| !p.show_tips), "the choice is not kept");
    let next = start_page(desktop(), prefs.unwrap_or_default(), day(2026, 11, 26));
    assert!(next.query_by_role_and_label(Role::Region, "Tip of the day").is_none(), "the next start shows tips again");
}

/// Every tip shown on a whole round of Next Tip.
fn all_tips(h: &mut Harness<'_, App>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..60 {
        let Some(t) = tip(h) else { break };
        if seen.contains(&t) {
            break;
        }
        seen.push(t);
        click(h, "Next Tip");
    }
    seen
}

#[test]
fn tips_for_a_feature_this_build_lacks_are_not_shown() {
    let mut desk = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    let on_desktop = all_tips(&mut desk);
    let mut web = start_page(browser(), Prefs::default(), day(2026, 11, 26));
    let in_browser = all_tips(&mut web);
    assert!(on_desktop.len() >= 15 && in_browser.len() >= 15, "{} and {} tips", on_desktop.len(), in_browser.len());
    assert!(on_desktop.iter().any(|t| t.contains("AI model")) && !in_browser.iter().any(|t| t.contains("AI model")), "the AI tip");
    assert!(in_browser.iter().any(|t| t.contains("Download Project as Zip")) && !on_desktop.iter().any(|t| t.contains("Download Project as Zip")));
}

#[test]
fn a_tip_s_button_runs_its_command() {
    let mut h = start_page(desktop(), Prefs::default(), day(2026, 11, 26));
    for _ in 0..40 {
        if h.query_by_role_and_label(Role::Button, "Open Settings").is_some() {
            break;
        }
        click(&mut h, "Next Tip");
    }
    click(&mut h, "Open Settings");
    assert_eq!(dialog(&mut h), "settings", "the tip's button did not open Settings");
}

#[test]
fn the_first_steps_hint_shows_with_a_project_and_stays_dismissed() {
    let s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    run(&mut h);
    let hint = h.query_by_role_and_label(Role::Region, "First steps");
    assert!(hint.is_some(), "no first-steps hint");
    let text = region_text(&h, "First steps").unwrap_or_default();
    assert!(text.contains("Build log") && text.contains("User Guide (F1)"), "{text}");
    click(&mut h, "Dismiss");
    assert!(h.query_by_role_and_label(Role::Region, "First steps").is_none(), "Dismiss did not hide it");
    assert!(status(&mut h).contains("First Steps"), "where to find it again is not said");
    let prefs = h.state_mut().take_changed_prefs().unwrap_or_default();
    assert!(prefs.first_steps_seen, "dismissing it is not kept");
    let again = App::with_prefs(Session::with_backend(Box::new(Unavailable("tests".into()))), prefs);
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), again);
    run(&mut h);
    assert!(h.query_by_role_and_label(Role::Region, "First steps").is_none(), "it came back at the next start");
    texcraft_ui_egui::ui_cmd::run(h.state_mut(), "help.first_steps");
    run(&mut h);
    assert!(h.query_by_role_and_label(Role::Region, "First steps").is_some(), "Help › First Steps does not show it again");
}
