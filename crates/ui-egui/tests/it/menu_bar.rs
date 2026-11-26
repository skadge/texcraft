//! A9: F10, or Alt alone, takes the keyboard to the menu bar; the arrows move between the menus
//! and into one, and Escape goes back out, to where the keyboard was.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn harness() -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Hello.\n"}))?;
    let prefs = Prefs { auto_compile: false, ..Prefs::default() };
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    Ok(h)
}

fn press(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    for _ in 0..3 {
        h.run();
    }
}

fn focused(h: &Harness<'_, App>, label: &str) -> bool {
    h.query_by_label(label).is_some_and(|n| n.is_focused())
}

/// The File menu's first entry that can run now: the one the keyboard lands on.
fn first_file_entry(h: &mut Harness<'_, App>) -> Option<&'static str> {
    let session = h.state_mut().session();
    let file = texcraft_ui_egui::ui_cmd::commands_in(texcraft_ui_egui::ui_cmd::menu("File"));
    file.into_iter().filter_map(texcraft_engine::cmd::find).find(|c| (c.enabled)(session).is_ok()).map(|c| c.label)
}

#[test]
fn f10_reaches_the_menus_the_arrows_move_and_open_them_and_escape_goes_back_to_the_text() {
    let mut h = harness().unwrap();
    let entry = first_file_entry(&mut h).unwrap();
    press(&mut h, egui::Key::F10);
    assert!(focused(&h, "File"), "F10 did not focus the first menu");
    press(&mut h, egui::Key::ArrowRight);
    assert!(focused(&h, "Edit"), "Right did not move to the next menu");
    press(&mut h, egui::Key::ArrowLeft);
    press(&mut h, egui::Key::ArrowLeft);
    assert!(focused(&h, "Help"), "Left from the first menu did not wrap to the last");
    press(&mut h, egui::Key::ArrowRight);
    assert!(focused(&h, "File"));
    assert!(h.query_by_label(entry).is_none(), "the File menu is open before it was asked for");
    press(&mut h, egui::Key::ArrowDown);
    assert!(focused(&h, entry), "Down did not open the menu with its first entry focused");
    press(&mut h, egui::Key::Escape);
    assert!(h.query_by_label(entry).is_none(), "Escape did not close the menu");
    assert!(focused(&h, "File"), "Escape in a menu did not go back to its button");
    press(&mut h, egui::Key::Escape);
    assert!(focused(&h, "main.tex source"), "Escape on the menu bar did not give the keyboard back to the text");
}

#[test]
fn enter_opens_a_menu_with_its_first_entry_focused() {
    let mut h = harness().unwrap();
    let entry = first_file_entry(&mut h).unwrap();
    press(&mut h, egui::Key::F10);
    press(&mut h, egui::Key::Enter);
    assert!(focused(&h, entry), "Enter did not open the menu with its first entry focused");
}

#[test]
fn alt_alone_reaches_the_menu_bar_but_alt_with_a_key_does_not() {
    let mut h = harness().unwrap();
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::F);
    h.run();
    h.run();
    assert!(!focused(&h, "File"), "Alt with a key went to the menu bar");
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    h.run();
    assert!(!focused(&h, "File"), "pressing Alt went to the menu bar before it was released");
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
    h.run();
    assert!(focused(&h, "File"), "Alt pressed and released alone did not focus the first menu");
    press(&mut h, egui::Key::Escape);
    assert!(focused(&h, "main.tex source"), "Escape did not give the keyboard back to the text");
}

/// Issue #25: on a phone, each menu fits the screen: what is used less is a level down.
#[test]
fn each_menu_fits_on_a_phone_screen() {
    const PHONE: egui::Vec2 = egui::vec2(400.0, 820.0);
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let _ = s.execute("file.write", &json!({"path": "main.tex", "text": "Hello.\n"}));
    let mut h = Harness::builder().with_size(PHONE).build_ui_state(|ui, app: &mut App| app.frame(ui), crate::support::app(s));
    h.run();
    // Each menu's last entry here.
    for (menu, last) in [("File", "Save Project to GitLab or GitHub"), ("Edit", "Settings…"), ("Insert", "Footnote"), ("Help", "About TeXCraft")] {
        if h.query_by_label(menu).is_none() {
            h.get_by_label("Menu").click();
            h.run();
        }
        h.get_by_label(menu).click();
        h.run();
        // Labelled with their shortcut too: "Settings… Ctrl+,".
        let bottom = h.query_all_by_label_contains(last).next().map(|n| n.rect().bottom()).unwrap_or(f32::INFINITY);
        assert!(bottom <= PHONE.y, "the {menu} menu runs past the screen's bottom, to {bottom}");
        press(&mut h, egui::Key::Escape);
        press(&mut h, egui::Key::Escape);
    }
}
