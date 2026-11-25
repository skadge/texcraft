//! The interface's languages: the Settings picker switches the language at once, and in the pseudo
//! locale (every string marked ⟦…⟧ and 40 % longer, as German or French run) the main window, the
//! start page, every Settings tab, About and Go to File keep every control
//! inside the window and apart from the others, and every name the screen reader hears went
//! through translation.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, ui_cmd};

const WINDOW: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn harness(language: &str) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n\\begin{document}\nHello.\n\\end{document}\n"}))?;
    s.execute("file.open", &json!({"path": "main.tex"}))?;
    let prefs =
        Prefs { auto_compile: false, language: language.to_owned(), layout: "split".to_owned(), side: "files".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(WINDOW).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
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

/// Controls someone clicks or types into.
fn control(role: Role) -> bool {
    matches!(role, Role::Button | Role::CheckBox | Role::RadioButton | Role::Tab | Role::ComboBox | Role::TextInput | Role::Slider | Role::MenuItem)
}

/// A control's name and place.
type Controls = Vec<(String, egui::Rect)>;

/// The controls on the screen.
fn controls(h: &Harness<'_, App>) -> Controls {
    h.query_all_by(|n| control(n.role()) && n.bounding_box().is_some())
        .map(|n| (label(&n), n.rect()))
        .filter(|(_, r)| r.width() > 0.0 && r.height() > 0.0)
        .collect()
}

/// What is wrong with the screen's layout: a control outside the window, two controls on top of
/// each other, a control whose name never went through translation. A dialog's screen leaves out
/// the window's own controls (`behind`), which it covers.
fn problems(h: &Harness<'_, App>, screen: &str, behind: &Controls) -> Vec<String> {
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW).expand(1.0);
    let controls: Controls = controls(h).into_iter().filter(|c| !behind.contains(c)).collect();
    let mut out = Vec::new();
    for (i, (name, rect)) in controls.iter().enumerate() {
        // Below the bottom is a scrolled area's rest; past the other edges is cut off.
        if rect.left() < window.left() || rect.right() > window.right() || rect.top() < window.top() {
            out.push(format!("{screen}: \"{name}\" reaches outside the window ({rect:?})"));
        }
        for (other, r) in controls.iter().skip(i + 1) {
            let overlap = rect.intersect(*r);
            // Shared edges and focus rings touch; a control drawn over another covers more.
            if overlap.width() > 2.0 && overlap.height() > 2.0 && !rect.contains_rect(*r) && !r.contains_rect(*rect) {
                out.push(format!("{screen}: \"{name}\" overlaps \"{other}\""));
            }
        }
        // A name made of words that the pseudo locale did not mark bypassed translation; file
        // names and the document's text are the user's.
        let words = name.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() > 2).count();
        if words > 0 && !name.contains('⟦') && !name.contains("main.tex") && !name.contains("Hello") {
            out.push(format!("{screen}: \"{name}\" is not translated"));
        }
    }
    out
}

/// A click with the pointer on the last control whose name starts with `name`, as a person makes
/// it (an open list's entry is drawn last).
fn tap(h: &mut Harness<'_, App>, name: &str) {
    let Some(at) = h.query_all_by(|n| n.label().is_some_and(|l| l.starts_with(name))).last().map(|n| n.rect().center()) else { return };
    h.event(egui::Event::PointerMoved(at));
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.run();
    }
    run(h);
}

#[test]
fn the_language_chosen_in_settings_applies_at_once() {
    let mut h = harness("en").unwrap();
    ui_cmd::run(h.state_mut(), "app.settings");
    run(&mut h);
    assert!(h.query_by_label("Appearance").is_some(), "Settings is not in English to begin with");
    h.get_by_role_and_label(Role::ComboBox, "Interface language").click();
    run(&mut h);
    tap(&mut h, "⟦Pseudo⟧");
    assert_eq!(h.state_mut().take_changed_prefs().map(|p| p.language).as_deref(), Some("pseudo"), "the choice is not kept in the preferences");
    assert!(h.query_by_label("Appearance").is_none(), "the open dialog still shows English");
    assert!(h.query_by_label_contains("⟦Appearance").is_some(), "Settings' tabs did not change language");
    h.get_by_role(Role::ComboBox).click();
    run(&mut h);
    tap(&mut h, "⟦System");
    run(&mut h);
    assert_eq!(h.state_mut().take_changed_prefs().map(|p| p.language).as_deref(), Some("auto"), "System is not kept as following the system");
}

#[test]
fn longer_languages_fit_the_main_window_settings_and_dialogs() {
    let mut h = harness(texcraft_ui_egui::i18n::PSEUDO).unwrap();
    let mut found = problems(&h, "main window", &Vec::new());
    let window = controls(&h);
    // Keyboard Shortcuts is craft-keymap's dialog, laid out and tested there.
    for (screen, command) in [("About", "help.about"), ("Go to File", "view.go_to_file")] {
        ui_cmd::run(h.state_mut(), command);
        run(&mut h);
        found.extend(problems(&h, screen, &window));
        h.key_press(egui::Key::Escape);
        run(&mut h);
    }
    ui_cmd::run(h.state_mut(), "app.settings");
    run(&mut h);
    let tabs: Vec<String> =
        h.query_all_by_role(Role::Tab).map(|n| label(&n)).filter(|l| l.starts_with('⟦') && !window.iter().any(|(w, _)| w == l)).collect();
    assert!(tabs.len() >= 5, "Settings' tabs are not shown in the pseudo locale: {tabs:?}");
    for tab in tabs {
        h.get_by_role_and_label(Role::Tab, &tab).click();
        run(&mut h);
        found.extend(problems(&h, &format!("Settings › {tab}"), &window));
    }
    h.key_press(egui::Key::Escape);
    run(&mut h);
    ui_cmd::run(h.state_mut(), "app.start");
    run(&mut h);
    found.extend(problems(&h, "start page", &Vec::new()));
    assert!(found.is_empty(), "{} layout problems:\n{}", found.len(), found.join("\n"));
}
