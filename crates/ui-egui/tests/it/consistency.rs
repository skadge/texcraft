//! One way for each kind of thing (docs/ux-review-2.md): every dialog in a wide window has Close
//! at the end of its title's row and its primary action as the right-most of its footer; Settings
//! grows with the window; the side panel keeps its width from section to section; a quick menu's
//! last entry opens the section of Settings its choices come from.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::{App, ui_cmd};

fn harness(size: egui::Vec2) -> Harness<'static, App> {
    harness_in(size, "en")
}

fn harness_in(size: egui::Vec2, language: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let text = "\\documentclass{article}\n\\begin{document}\n\\section{One}\nHello.\n\\end{document}\n";
    let _ = s.execute("file.write", &json!({"path": "main.tex", "text": text}));
    let _ = s.execute("file.open", &json!({"path": "main.tex"}));
    let prefs = texcraft_ui_egui::prefs::Prefs { auto_compile: false, language: language.to_owned(), ..Default::default() };
    let app = App::with_prefs(s, prefs);
    let mut h = Harness::builder().with_size(size).with_max_steps(60).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.run();
    }
}

const WIDE: egui::Vec2 = egui::vec2(1280.0, 860.0);

/// The open dialog's box, as far as what is in it reaches (its node has no box of its own).
fn dialog(h: &Harness<'_, App>) -> Option<egui::Rect> {
    h.query_all_by(|n| n.bounding_box().is_some() && std::iter::successors(n.parent(), |p| p.parent()).any(|p| p.role() == Role::Dialog))
        .map(|n| n.rect())
        .filter(|r| r.width() > 0.0)
        .reduce(|a, b| a.union(b))
}

/// The buttons inside the open dialog, by name and place.
fn buttons(h: &Harness<'_, App>) -> Vec<(String, egui::Rect)> {
    h.query_all_by(|n| {
        n.role() == Role::Button && n.bounding_box().is_some() && std::iter::successors(n.parent(), |p| p.parent()).any(|p| p.role() == Role::Dialog)
    })
    .filter_map(|n| Some((n.accesskit_node().label()?, n.rect())))
    .filter(|(_, r)| r.width() > 0.0)
    .collect()
}

/// What is out of place in the open dialog `screen`: Close not at the end of the title's row,
/// `primary` (when given) not the right-most of the buttons in its row.
fn problems(h: &Harness<'_, App>, screen: &str, primary: Option<&str>) -> Vec<String> {
    let Some(frame) = dialog(h) else { return vec![format!("{screen}: no dialog is open")] };
    let all = buttons(h);
    let mut out = Vec::new();
    // The title's row: what lies within its first 40 points.
    let top: Vec<&(String, egui::Rect)> = all.iter().filter(|(_, r)| r.top() < frame.top() + 40.0).collect();
    match top.iter().max_by(|a, b| a.1.right().total_cmp(&b.1.right())) {
        Some((name, _)) if name == "Close" => {}
        Some((name, _)) => out.push(format!("{screen}: \"{name}\" ends the title's row, not Close")),
        None => out.push(format!("{screen}: no Close at the top ({:?})", all.iter().map(|(n, _)| n).collect::<Vec<_>>())),
    }
    if let Some(primary) = primary {
        match all.iter().find(|(n, _)| n == primary) {
            None => out.push(format!("{screen}: no \"{primary}\" among {:?}", all.iter().map(|(n, _)| n).collect::<Vec<_>>())),
            Some((_, at)) => {
                let mut row = all.iter().filter(|(n, r)| n != primary && (r.center().y - at.center().y).abs() < 6.0);
                if let Some((other, _)) = row.find(|(_, r)| r.right() > at.right()) {
                    out.push(format!("{screen}: \"{other}\" is right of the primary \"{primary}\""));
                }
            }
        }
    }
    out
}

/// A dialog: its name, how it is opened, and its primary action if it has one.
type Screen = (&'static str, fn(&mut Harness<'_, App>), Option<&'static str>);

#[test]
fn every_dialog_has_close_at_its_title_s_end_and_its_primary_action_right_most() {
    let mut found = Vec::new();
    let screens: [Screen; 6] = [
        ("New File", |h| ui_cmd::run(h.state_mut(), "file.new"), Some("Create")),
        ("About", |h| ui_cmd::run(h.state_mut(), "help.about"), None),
        (
            "Save the changes?",
            |h| {
                let _ =
                    h.state_mut().control("engine.execute", &json!({"command": "file.write", "params": {"path": "main.tex", "text": "Changed."}}));
                ui_cmd::run(h.state_mut(), "project.new");
            },
            Some("Save"),
        ),
        (
            "Import from GitLab",
            |h| {
                ui_cmd::run_with(h.state_mut(), "project.import_dialog", &json!({"source": "gitlab"}));
                // The project's changes are asked about first.
                settle(h);
                h.get_by_role_and_label(Role::Button, "Don't Save").click();
            },
            Some("Import"),
        ),
        ("Add a Connection", |h| ui_cmd::run(h.state_mut(), "app.add_connection"), None),
        ("Settings", |h| ui_cmd::run(h.state_mut(), "app.settings"), None),
    ];
    for (screen, open, primary) in screens {
        let mut h = harness(WIDE);
        open(&mut h);
        settle(&mut h);
        found.extend(problems(&h, screen, primary));
    }
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
}

#[test]
fn settings_grows_with_the_window() {
    // The box is centred: its height is the window's less twice the room above its title's row.
    let height = |size: egui::Vec2| {
        let mut h = harness(size);
        ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": "accounts"}));
        settle(&mut h);
        h.query_by_label("Search settings").map_or(0.0, |n| size.y - 2.0 * (n.rect().top() - 20.0))
    };
    let (short, wide, tall) = (height(egui::vec2(1280.0, 640.0)), height(WIDE), height(egui::vec2(1280.0, 1000.0)));
    assert!(short > 0.0, "Settings did not open");
    assert!(tall > short + 200.0, "Settings is {short} tall in a short window and only {tall} in a tall one");
    assert!(wide > WIDE.y * 0.8, "Settings is {wide} tall in a window {} tall", WIDE.y);
}

#[test]
fn the_side_panel_keeps_its_width_from_section_to_section() {
    // In the pseudo locale (every text 40 % longer, as German): a long empty text or row must
    // not widen the panel.
    let mut h = harness_in(WIDE, "pseudo");
    let mut widths = Vec::new();
    for (id, name) in [
        ("view.side.files", "Files"),
        ("view.side.outline", "Outline"),
        ("view.side.review", "Review"),
        ("view.side.search", "Search"),
        ("view.side.history", "History"),
        ("view.side.ask", "Ask AI"),
    ] {
        ui_cmd::run(h.state_mut(), id);
        settle(&mut h);
        let x = h.query_by_label_contains("Resize the side panel").map(|n| n.rect().center().x);
        widths.push((name, x.unwrap_or(f32::NAN)));
    }
    let first = widths[0].1;
    assert!(widths.iter().all(|(_, x)| (x - first).abs() < 1.0), "the side panel changes width: {widths:?}");
}

#[test]
fn a_quick_menu_s_last_entry_opens_its_section_of_settings() {
    let mut h = harness(WIDE);
    h.get_by_role_and_label(Role::Button, "Compile options").click();
    settle(&mut h);
    h.get_by_label("Build Settings…").click();
    settle(&mut h);
    let building = h.query_by_role_and_label(Role::Tab, "Building").map(|n| n.accesskit_node().is_selected());
    assert_eq!(building, Some(Some(true)), "Build Settings… does not open Settings › Building");

    let mut h = harness(WIDE);
    h.get_by_label_contains("spelling language").click();
    settle(&mut h);
    h.get_by_label("Spelling Settings…").click();
    settle(&mut h);
    assert!(h.query_by_role_and_label(Role::CheckBox, "Check spelling").is_some(), "Spelling Settings… does not show the spelling settings");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Compile automatically").is_none(), "Spelling Settings… shows other settings too");
}

/// Every on-or-off setting, in every section of Settings, is a row: its name shown as the row's
/// title, with its help, and the checkbox named after it (`settings::setting`), as in each other
/// section, rather than a checkbox carrying its own words.
#[test]
fn every_switch_in_settings_is_a_row_with_its_name_shown() {
    let mut found = Vec::new();
    for tab in ["appearance", "editor", "building", "keyboard", "review", "ai", "accounts"] {
        let mut h = harness(egui::vec2(1280.0, 1400.0));
        ui_cmd::run_with(h.state_mut(), "app.settings", &json!({"tab": tab}));
        settle(&mut h);
        let in_dialog =
            |n: &egui_kittest::kittest::AccessKitNode<'_>| std::iter::successors(n.parent(), |p| p.parent()).any(|p| p.role() == Role::Dialog);
        let labels: Vec<String> = h
            .query_all_by(|n| n.role() == Role::Label && in_dialog(n))
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .collect();
        for b in h.query_all_by(|n| n.role() == Role::CheckBox && in_dialog(n)) {
            let name = b.accesskit_node().label().unwrap_or_default();
            // A checkbox that says what it does beside itself, not a setting of its own.
            if name == "Hide the token" {
                continue;
            }
            if !labels.contains(&name) {
                found.push(format!("{tab}: \"{name}\" is not a row with its name shown"));
            }
        }
    }
    assert!(found.is_empty(), "{} problems:\n{}", found.len(), found.join("\n"));
}
