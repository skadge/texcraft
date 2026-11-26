//! Issue #25: All Commands (Ctrl+Shift+P) lists every command, so the menus can keep only what
//! is used often; it is driven from the keyboard.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::{Session, cmd};
use texcraft_ui_egui::App;
use texcraft_ui_egui::ui_cmd::{self, MENU_BAR, UI_COMMANDS};

fn window() -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Hello.\n"}))?;
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    Ok(h)
}

fn theme(h: &mut Harness<'_, App>) -> String {
    h.state_mut().control("ui.inspect", &json!({}))["result"]["theme"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn the_shortcut_opens_the_list_and_enter_runs_the_command_typed() {
    let mut h = window().unwrap();
    let before = theme(&mut h);
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::P);
    h.run();
    h.get_by_label("Type part of a command's name").type_text("light dark");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.query_by_label("Type part of a command's name").is_none(), "the list stayed open");
    assert_ne!(theme(&mut h), before, "Switch Light and Dark did not run");
}

/// What the menus leave out is still a command away: every command with a menu path or a
/// shortcut, and every view command, is in the menus or in the list.
#[test]
fn every_command_is_in_the_menus_or_the_list() {
    let h = window().unwrap();
    let app = h.state();
    let listed = texcraft_ui_egui::commands::listed(app);
    let in_menus: Vec<&str> = MENU_BAR.iter().flat_map(|(_, entries)| ui_cmd::commands_in(entries)).collect();
    let reachable = |id: &str| listed.contains(&id) || in_menus.contains(&id);
    let engine = cmd::all().iter().filter(|c| !c.menu.is_empty() || c.shortcut.is_some()).map(|c| c.id);
    // Those that need what only their own control gives (a zoom, a way of rewriting, a symbol), or
    // act on the word or problem their menu was opened on.
    let own_control = ["view.zoom", "edit.rewrite", "edit.insert_symbol", "view.commands"];
    let own_menu = |id: &str| (id.starts_with("edit.spelling.") && id != "edit.spelling.next") || id.starts_with("edit.grammar.");
    let view = UI_COMMANDS.iter().map(|c| c.id).filter(|id| !own_control.contains(id) && !own_menu(id) && !id.starts_with("view.present."));
    // AI's are listed only while a model is connected and AI is on (ai_off.rs checks they are then).
    let lost: Vec<&str> = engine.chain(view).filter(|id| !reachable(id) && *id != "project.download_zip" && !ui_cmd::is_ai(id)).collect();
    assert!(lost.is_empty(), "not in the menus or All Commands: {lost:?}");
}
