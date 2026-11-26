//! Dialogs on a phone (docs/ROADMAP.md, A12): below the narrow width every dialog is a sheet over
//! the window, its Close at the top and its actions in a bar at the bottom. In each family —
//! the one-line prompts and questions, About, Settings, the template gallery, Import a Project
//! (its sources and its GitLab page), Save to GitHub, Make with AI, Go to File — every control lies inside the window and
//! apart from the others (what scrolls under the bottom bar is reached by scrolling), and the
//! keyboard reaches Close and the actions. Settings' sections are a list; each opens as a page
//! whose Back returns to it.

use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

/// A phone held upright, in points.
const PHONE: egui::Vec2 = egui::vec2(400.0, 820.0);

fn phone() -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\begin{document}\nHello.\n\\end{document}\n"}))?;
    s.execute("file.new", &json!({"path": "intro.tex", "text": "Intro.\n"}))?;
    s.execute("file.open", &json!({"path": "main.tex"}))?;
    let prefs = Prefs { auto_compile: false, layout: "split".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(PHONE).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    run(&mut h);
    Ok(h)
}

fn run(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.run();
    }
}

fn label(n: &egui_kittest::Node<'_>) -> String {
    n.accesskit_node().label().unwrap_or_default()
}

fn control(role: Role) -> bool {
    matches!(role, Role::Button | Role::CheckBox | Role::RadioButton | Role::Tab | Role::ComboBox | Role::TextInput | Role::Slider | Role::Link)
}

/// The dialog's controls (those inside its node), with their names and places.
fn controls(h: &Harness<'_, App>) -> Vec<(String, egui::Rect)> {
    h.query_all_by(|n| {
        control(n.role()) && n.bounding_box().is_some() && std::iter::successors(n.parent(), |p| p.parent()).any(|p| p.role() == Role::Dialog)
    })
    .map(|n| (label(&n), n.rect()))
    .filter(|(_, r)| r.width() > 0.0 && r.height() > 0.0)
    .collect()
}

/// What is wrong with the open dialog at a phone's size: no Close, an action (`actions`, by the
/// start of its name) missing or outside the window, a control past the window's sides or top, two
/// controls on top of each other. What lies below the bottom bar's top scrolls under it.
fn problems(h: &Harness<'_, App>, screen: &str, actions: &[&str]) -> Vec<String> {
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, PHONE).expand(1.0);
    let all = controls(h);
    let mut out = Vec::new();
    if all.is_empty() {
        return vec![format!("{screen}: no dialog is open")];
    }
    let is_action = |name: &str| actions.iter().any(|a| name.starts_with(a));
    let leave = all.iter().find(|(n, _)| n == "Close" || n.starts_with("Back to"));
    match leave {
        Some((_, r)) if window.contains_rect(*r) => {}
        Some((n, r)) => out.push(format!("{screen}: \"{n}\" is outside the window ({r:?})")),
        None => out.push(format!("{screen}: no Close or Back")),
    }
    for action in actions {
        match all.iter().find(|(n, _)| n.starts_with(action)) {
            Some((_, r)) if window.contains_rect(*r) => {}
            Some((n, r)) => out.push(format!("{screen}: \"{n}\" is outside the window ({r:?})")),
            None => out.push(format!("{screen}: no \"{action}\" among {:?}", all.iter().map(|(n, _)| n).collect::<Vec<_>>())),
        }
    }
    // The bottom bar: what is above it is shown, what is below scrolls under it.
    let bar = all.iter().filter(|(n, _)| is_action(n)).map(|(_, r)| r.top()).fold(window.bottom(), f32::min);
    let shown: Vec<&(String, egui::Rect)> = all.iter().filter(|(n, r)| is_action(n) || r.bottom() <= bar + 1.0).collect();
    for (i, (name, rect)) in shown.iter().enumerate() {
        if rect.left() < window.left() || rect.right() > window.right() || rect.top() < window.top() {
            out.push(format!("{screen}: \"{name}\" reaches outside the window ({rect:?})"));
        }
        for (other, r) in shown.iter().skip(i + 1) {
            let overlap = rect.intersect(*r);
            if overlap.width() > 2.0 && overlap.height() > 2.0 && !rect.contains_rect(*r) && !r.contains_rect(*rect) {
                out.push(format!("{screen}: \"{name}\" overlaps \"{other}\""));
            }
        }
    }
    out
}

/// The names the keyboard stops on, Tab after Tab, until it comes round again.
fn tab_stops(h: &mut Harness<'_, App>) -> Vec<String> {
    let mut stops = Vec::new();
    for _ in 0..60 {
        h.key_press(egui::Key::Tab);
        h.run();
        let now = h.root().children_recursive().map(|n| n.accesskit_node()).find(|n| n.is_focused()).and_then(|n| n.label());
        if let Some(name) = now {
            if stops.first() == Some(&name) && stops.len() > 1 {
                break;
            }
            stops.push(name);
        }
    }
    stops
}

/// Whether the keyboard reaches Close (or Back) and each of `actions`.
fn unreachable(h: &mut Harness<'_, App>, screen: &str, actions: &[&str]) -> Vec<String> {
    let stops = tab_stops(h);
    let mut out = Vec::new();
    if !stops.iter().any(|s| s == "Close" || s.starts_with("Back to")) {
        out.push(format!("{screen}: the keyboard does not reach Close ({stops:?})"));
    }
    for action in actions {
        if !stops.iter().any(|s| s.starts_with(action)) {
            out.push(format!("{screen}: the keyboard does not reach \"{action}\" ({stops:?})"));
        }
    }
    out
}

/// The dialog's layout, and the keyboard's way to Close and to `enabled` (the actions that can be
/// used now; one waiting for a field to be filled in takes no keyboard).
fn check(h: &mut Harness<'_, App>, screen: &str, actions: &[&str], enabled: &[&str]) -> Vec<String> {
    let mut found = problems(h, screen, actions);
    found.extend(unreachable(h, screen, enabled));
    found
}

fn close(h: &mut Harness<'_, App>) {
    h.key_press(egui::Key::Escape);
    run(h);
}

#[test]
fn prompts_questions_and_about_fit_a_phone() -> Result<(), String> {
    let mut h = phone()?;
    let mut found = Vec::new();
    ui_cmd::run(h.state_mut(), "file.new");
    run(&mut h);
    found.extend(check(&mut h, "New File", &["Create", "Cancel"], &["Cancel"]));
    close(&mut h);
    ui_cmd::run(h.state_mut(), "help.about");
    run(&mut h);
    found.extend(check(&mut h, "About", &[], &[]));
    close(&mut h);
    // Unsaved changes, then a new project: the question comes first.
    h.state_mut().control("engine.execute", &json!({"command": "file.write", "params": {"path": "main.tex", "text": "Changed."}}));
    ui_cmd::run(h.state_mut(), "project.new");
    run(&mut h);
    found.extend(check(&mut h, "Save the changes?", &["Save", "Cancel", "Don't Save"], &["Save", "Cancel", "Don't Save"]));
    close(&mut h);
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
    Ok(())
}

#[test]
fn settings_on_a_phone_is_a_list_of_sections_each_a_page_with_back() -> Result<(), String> {
    let mut h = phone()?;
    ui_cmd::run(h.state_mut(), "app.settings");
    run(&mut h);
    let sections: Vec<String> = controls(&h).into_iter().map(|(n, _)| n).filter(|n| n != "Close").collect();
    assert!(sections.len() >= 6, "Settings does not list its sections: {sections:?}");
    assert!(h.query_all_by_role(Role::Tab).all(|n| !label(&n).starts_with("Appearance")), "the sections are still tabs");
    let mut found = check(&mut h, "Settings", &[], &[]);
    for section in &sections {
        h.get_by_role_and_label(Role::Button, section).click();
        run(&mut h);
        assert!(h.query_by_label("Back to Settings").is_some(), "{section} does not open as a page with Back");
        found.extend(problems(&h, &format!("Settings › {section}"), &[]));
        h.get_by_label("Back to Settings").click();
        run(&mut h);
        let listed = h.query_all_by(|n| n.role() == Role::Button && n.label().as_deref() == Some(section.as_str())).next().is_some();
        assert!(h.query_by_label("Back to Settings").is_none() && listed, "Back did not return to the list");
    }
    // By the keyboard too: a section's row opens it, and its Back has the keyboard.
    h.get_by_role_and_label(Role::Button, "Editor").focus();
    h.run();
    h.key_press(egui::Key::Enter);
    run(&mut h);
    let focused = h.root().children_recursive().map(|n| n.accesskit_node()).find(|n| n.is_focused()).and_then(|n| n.label());
    assert_eq!(focused.as_deref(), Some("Back to Settings"), "the page's Back does not have the keyboard");
    found.extend(unreachable(&mut h, "Settings › Editor", &[]));
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
    Ok(())
}

#[test]
fn the_template_gallery_on_a_phone_is_one_column_with_create_in_its_bottom_bar() -> Result<(), String> {
    let mut h = phone()?;
    ui_cmd::run(h.state_mut(), "project.from_template");
    run(&mut h);
    // The cards: the dialog's tall buttons.
    let cards: Vec<egui::Rect> = controls(&h).into_iter().map(|(_, r)| r).filter(|r| r.height() > 100.0).collect();
    assert!(cards.len() > 2, "the gallery shows no cards");
    for pair in cards.windows(2) {
        assert!(pair[1].top() >= pair[0].bottom(), "two cards side by side: {pair:?}");
    }
    let found = check(&mut h, "New Project", &["Create Project"], &[]);
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
    Ok(())
}

#[test]
fn opening_from_and_saving_to_a_forge_fit_a_phone() -> Result<(), String> {
    let mut h = phone()?;
    let mut found = Vec::new();
    // The project has unsaved changes: the question comes first, as on the desktop.
    ui_cmd::run(h.state_mut(), "forge.open");
    run(&mut h);
    h.get_by_label("Don't Save").click();
    run(&mut h);
    found.extend(check(&mut h, "Import from GitLab", &["Import", "Cancel"], &["Cancel"]));
    close(&mut h);
    ui_cmd::run(h.state_mut(), "project.import_dialog");
    run(&mut h);
    found.extend(problems(&h, "Import a Project", &[]));
    ui_cmd::run(h.state_mut(), "forge.save_to_github");
    run(&mut h);
    found.extend(problems(&h, "Save Project to GitHub", &["Cancel"]));
    found.extend(unreachable(&mut h, "Save Project to GitHub", &["Cancel"]));
    close(&mut h);
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
    Ok(())
}

#[test]
fn make_with_ai_and_go_to_file_fit_a_phone() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = crate::ai::editor("<<<LATEX\nx\nLATEX>>>", "Hello.\n", &prompts, false, false)?;
    h.set_size(PHONE);
    run(&mut h);
    let mut found = Vec::new();
    ui_cmd::run(h.state_mut(), "edit.ai_make");
    run(&mut h);
    found.extend(check(&mut h, "Make with AI", &["Make", "Cancel"], &["Cancel"]));
    close(&mut h);
    ui_cmd::run(h.state_mut(), "view.go_to_file");
    run(&mut h);
    found.extend(problems(&h, "Go to File", &[]));
    close(&mut h);
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
    Ok(())
}
