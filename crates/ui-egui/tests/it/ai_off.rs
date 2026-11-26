//! Issue #43: Settings › AI › Use AI features, turned off, hides every AI entry point (the menus
//! and the system's menu bar, the text's context menu, the build log's Explain, the rail's Ask AI,
//! All Commands, the tip of the day) and sends nothing; turned on again, they come back.

use std::sync::{Arc, Mutex};

use craft_menu::Node;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::{App, commands, ui_cmd};

use crate::ai::{harness, sent, session};

/// A connected, compiled session: every AI entry point applies.
fn connected(prompts: &Arc<Mutex<Vec<String>>>) -> Result<Session, String> {
    let mut s = session(prompts)?;
    s.execute("ai.connect", &json!({"endpoint": "https://api.example.com/v1", "token": "sk-off-test", "model": "small"}))?;
    s.execute("build.compile", &json!({}))?;
    s.wait();
    Ok(s)
}

fn press(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    h.run();
}

/// Every label (and its submenus') of the system's menu bar.
fn system_labels(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            Node::Item(i) => out.push(format!("{} {}", i.command.id, i.label)),
            Node::Submenu { label, children, .. } => {
                out.push(label.clone());
                system_labels(children, out);
            }
            _ => {}
        }
    }
}

/// Where AI shows in the window now, each place by name.
fn ai_shown(h: &mut Harness<'_, App>) -> Vec<String> {
    let mut found = Vec::new();
    let ai = |label: &str| label.contains("AI") || label.contains("ai.");
    // The rail.
    if h.query_by_label("Ask AI").is_some() {
        found.push("the rail's Ask AI".to_owned());
    }
    // The build log's cards.
    h.state_mut().control("engine.execute", &json!({"command": "view.logs"}));
    h.run();
    if h.query_by_label("Explain").is_some() {
        found.push("the build log's Explain".to_owned());
    }
    // The in-window menus, opened as the user would.
    for menu in ["Edit", "Insert"] {
        h.get_by_label(menu).click();
        h.run();
        for sub in ["Spelling and Grammar", "Review"].into_iter().filter(|_| menu == "Edit") {
            // The submenu, after the rail's section of the same name.
            if let Some(n) = h.query_all_by_label(sub).last() {
                n.hover();
            }
            h.run();
            found.extend(h.query_all_by_label_contains("with AI").map(|_| format!("{menu} › {sub}: with AI")));
        }
        found.extend(h.query_all_by_label_contains("with AI").map(|_| format!("the {menu} menu: with AI")));
        press(h, egui::Key::Escape);
        press(h, egui::Key::Escape);
    }
    // Every entry the menus could hold.
    for (_, entries) in ui_cmd::MENU_BAR {
        for id in ui_cmd::commands_in(entries) {
            if ui_cmd::is_ai(id) && ui_cmd::in_menus(h.state(), id) {
                found.push(format!("menu entry {id}"));
            }
        }
    }
    // The system's menu bar (macOS).
    let mut labels = Vec::new();
    for m in h.state().menu_tree().menus {
        system_labels(&m.children, &mut labels);
    }
    found.extend(labels.into_iter().filter(|l| ai(l)).map(|l| format!("the system menu: {l}")));
    // All Commands.
    found.extend(commands::listed(h.state()).into_iter().filter(|id| ui_cmd::is_ai(id)).map(|id| format!("All Commands: {id}")));
    // The text's context menu, on a selection.
    h.get_by_label("main.tex source").focus();
    h.run();
    ui_cmd::run(h.state_mut(), "edit.select_all");
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.run();
    found.extend(h.query_all_by_label_contains("AI").map(|_| "the text's context menu".to_owned()));
    press(h, egui::Key::Escape);
    found
}

/// Turns Use AI features on or off with its switch in Settings › AI, from the keyboard.
fn use_ai(h: &mut Harness<'_, App>) {
    ui_cmd::run(h.state_mut(), "app.settings");
    h.run();
    h.get_by_label("AI").click();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Use AI features").focus();
    h.run();
    press(h, egui::Key::Space);
}

#[test]
fn turned_off_every_ai_entry_point_is_hidden_and_turned_on_they_return() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = harness(connected(&prompts)?);
    h.run();
    let on = ai_shown(&mut h);
    for place in ["the rail's Ask AI", "the build log's Explain", "All Commands: edit.ai_make", "the text's context menu"] {
        assert!(on.iter().any(|p| p == place), "with AI on, {place} is missing: {on:?}");
    }
    assert!(on.iter().any(|p| p.starts_with("the system menu")), "with AI on, the system menu has no AI entry: {on:?}");

    use_ai(&mut h);
    // The tab keeps only the switch and why everything is hidden.
    assert!(h.query_by_label_contains("AI features are turned off").is_some(), "Settings › AI does not say AI is off");
    assert!(h.query_by_label("Endpoint").is_none(), "Settings › AI still shows the connection with AI off");
    let prefs = h.state_mut().take_changed_prefs().ok_or("turning AI off was not kept in the preferences")?;
    assert!(prefs.ai.disabled, "the preferences do not keep AI off");
    press(&mut h, egui::Key::Escape);
    let off = ai_shown(&mut h);
    assert!(off.is_empty(), "with AI off, AI still shows in: {off:?}");
    // A shortcut or an agent still reaching a hidden command sends nothing, and says why.
    for id in ui_cmd::AI_COMMANDS {
        let r = h.state_mut().control("engine.execute", &json!({"command": id, "params": {"how": "shorter"}}));
        h.run();
        let why = r["error"].as_str().unwrap_or_default().to_owned();
        assert!(why.contains("turned off"), "{id} with AI off: {r}");
    }
    assert_eq!(sent(&prompts), 0, "something was sent with AI off");

    // Restarted with the kept preferences: still off.
    let again = App::with_prefs(connected(&prompts)?, Prefs { auto_compile: false, ..prefs });
    assert!(!again.session().ai_enabled(), "AI off was not restored from the preferences");

    use_ai(&mut h);
    assert!(h.query_by_label("Endpoint").is_some(), "Settings › AI does not show the connection again");
    press(&mut h, egui::Key::Escape);
    let back = ai_shown(&mut h);
    assert_eq!(back.len(), on.len(), "turned on again, not every AI entry point came back: {back:?} (was {on:?})");
    Ok(())
}

/// The tip of the day about AI is in the tips only while AI features are on.
#[test]
fn turned_off_no_tip_of_the_day_mentions_ai() -> Result<(), String> {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let mut h = harness(session(&prompts)?);
    let tips_about_ai = |h: &mut Harness<'_, App>| {
        h.state_mut().show_start_page();
        h.run();
        let mut found = 0;
        // More presses than there are tips: each one is shown at least once.
        for _ in 0..40 {
            found += h.query_all_by_label_contains("AI model").count();
            h.get_by_label("Next Tip").click();
            h.run();
        }
        found
    };
    assert!(tips_about_ai(&mut h) > 0, "with AI on, no tip is about it");
    h.state_mut().control("engine.execute", &json!({"command": "ai.enabled", "params": {"on": false}}));
    h.run();
    assert_eq!(tips_about_ai(&mut h), 0, "with AI off, a tip is about it");
    Ok(())
}
