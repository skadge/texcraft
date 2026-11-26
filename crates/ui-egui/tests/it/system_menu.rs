//! A9: the menus for the system's menu bar (macOS) are the in-window menus, entry for entry, with
//! the same shortcuts and enabled states, and an entry chosen there runs the same command. The
//! AppKit backend itself is tried only on a Mac; here a stand-in backend reports the choices.

use std::cell::RefCell;
use std::rc::Rc;

use craft_keymap::{Binding, Chord};
use craft_menu::{Backend, CommandRef, Dispatch, Event, MenuBar, MenuRole, NativeMenu, Node};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::{Session, cmd};
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;
use texcraft_ui_egui::ui_cmd::{self, MENU_BAR, MENUS, UI_COMMANDS};

fn app(prefs: Prefs) -> Result<App, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "Hello.\n"}))?;
    Ok(App::with_prefs(s, Prefs { auto_compile: false, ..prefs }))
}

/// A backend standing in for AppKit: what the app synced, and the choices to report.
#[derive(Default)]
struct Fake {
    synced: Rc<RefCell<Vec<MenuBar>>>,
    events: Rc<RefCell<Vec<Event>>>,
}

impl Backend for Fake {
    fn sync(&mut self, bar: &MenuBar) {
        self.synced.borrow_mut().push(bar.clone());
    }
    fn drain(&mut self) -> Vec<Event> {
        self.events.borrow_mut().drain(..).collect()
    }
    fn attach(&mut self, _dispatch: Dispatch) {}
}

type Shared<T> = Rc<RefCell<Vec<T>>>;

/// A window whose menus are in a stand-in system menu bar.
fn with_system_menu(app: App) -> (Harness<'static, App>, Shared<MenuBar>, Shared<Event>) {
    let fake = Fake::default();
    let (synced, events) = (fake.synced.clone(), fake.events.clone());
    let mut app = app;
    app.set_system_menu(NativeMenu::new(Box::new(fake)));
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run_steps(10);
    (h, synced, events)
}

/// The system menu reports `command` chosen; the next frames run it.
fn choose(h: &mut Harness<'_, App>, events: &Shared<Event>, command: CommandRef) {
    events.borrow_mut().push(Event::Click(command));
    let mut raw = egui::RawInput::default();
    h.state_mut().system_menu_input(&mut raw);
    // Frames by number, not until settled: opening a project starts a build, whose spinner
    // keeps asking for frames for as long as it lasts.
    h.run_steps(10);
}

fn default_chord(shortcut: Option<&str>) -> Option<Chord> {
    shortcut.and_then(|s| Binding::parse(s).ok()).map(|b| b.trigger.chord)
}

/// The tree is the menus' one definition ([`MENU_BAR`]): each command that applies here is in
/// it once, with the shortcut and enabled state the window's menus show.
#[test]
fn the_tree_has_each_menu_entry_once_with_its_shortcut_and_enabled_state() {
    let app = app(Prefs::default()).unwrap();
    let tree = app.menu_tree();
    let titles: Vec<&str> = tree.menus.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, MENUS);
    let roles: Vec<MenuRole> = tree.menus.iter().map(|m| m.role).collect();
    assert_eq!(roles, [MenuRole::File, MenuRole::Edit, MenuRole::Other, MenuRole::Help]);

    let session = app.session();
    let mut seen = std::collections::HashSet::new();
    for (_, entries) in MENU_BAR {
        for id in ui_cmd::commands_in(entries) {
            assert!(seen.insert(id), "{id} is in the menus twice");
            let spec = cmd::find(id);
            let shortcut = match spec {
                Some(c) => c.shortcut,
                None => UI_COMMANDS.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("{id} is not a command")).shortcut,
            };
            // The browser's own entries (Download Project as Zip), the forge's for a project on
            // one, AI's without a model: not in this window's menus.
            if !ui_cmd::in_menus(&app, id) {
                assert!(tree.find(&id.into()).is_none(), "{id} is in the menus where it cannot apply");
                continue;
            }
            let item = tree.find(&id.into()).unwrap_or_else(|| panic!("{id} is missing"));
            assert_eq!(item.shortcut, default_chord(shortcut), "{id} shortcut");
            if let Some(spec) = spec {
                assert_eq!(item.enabled, (spec.enabled)(session).is_ok(), "{id} enabled");
            }
        }
    }
    // Nothing is selected yet: Copy can't run, Paste can.
    assert!(tree.find(&"edit.copy".into()).is_some_and(|i| !i.enabled));
    assert!(tree.find(&"edit.paste".into()).is_some_and(|i| i.enabled));
}

/// On a Mac, About and Settings leave Help and Edit for the app menu, as in other Mac apps.
#[test]
fn about_and_settings_go_to_the_app_menu() {
    let bar = app(Prefs::default()).unwrap().system_menu_layout().bar;
    let in_menu =
        |role: MenuRole, id: &str| bar.menu(role).is_some_and(|m| m.children.iter().any(|n| matches!(n, Node::Item(i) if i.command.id == id)));
    assert!(in_menu(MenuRole::App, "help.about"));
    assert!(in_menu(MenuRole::App, "app.settings"));
    assert!(!in_menu(MenuRole::Help, "help.about"));
    assert!(!in_menu(MenuRole::Edit, "app.settings"));
}

#[test]
fn with_the_system_menu_the_window_has_no_menu_bar_and_a_choice_runs_its_command() {
    let (mut h, synced, events) = with_system_menu(app(Prefs::default()).unwrap());
    assert!(h.query_by_label("Insert").is_none(), "the in-window menu bar is still drawn");
    assert!(synced.borrow().last().is_some_and(|bar| bar.menu(MenuRole::App).is_some()), "the system menus were not synced");
    choose(&mut h, &events, CommandRef::new("help.about"));
    let ui = h.state_mut().control("ui.inspect", &json!({}));
    assert_eq!(ui["result"]["dialog"], "about");
}

/// A dialog greys the menus: a choice that arrives behind it does nothing.
#[test]
fn nothing_chosen_in_the_system_menu_runs_behind_a_dialog() {
    let (mut h, synced, events) = with_system_menu(app(Prefs::default()).unwrap());
    choose(&mut h, &events, CommandRef::new("help.about"));
    h.run();
    assert!(synced.borrow().last().and_then(|bar| bar.find(&"help.shortcuts".into())).is_some_and(|i| !i.enabled), "the menus are not greyed");
    choose(&mut h, &events, CommandRef::new("help.shortcuts"));
    let ui = h.state_mut().control("ui.inspect", &json!({}));
    assert_eq!(ui["result"]["dialog"], "about");
}

#[test]
fn open_recent_in_the_system_menu_opens_that_project() -> std::io::Result<()> {
    let dir = std::env::temp_dir().join(format!("texcraft-system-menu-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("main.tex"), "\\documentclass{article}")?;
    let path = dir.display().to_string();
    // Nothing unsaved, so the project opens without asking first.
    let app = App::with_prefs(Session::with_backend(Box::new(Unavailable("tests".into()))), Prefs { recent: vec![path.clone()], ..Prefs::default() });
    let tree = app.menu_tree();
    let open = CommandRef::with_params("project.open", json!({ "path": path }));
    let listed = tree.find(&open).is_some_and(|i| i.enabled);
    let (mut h, _, events) = with_system_menu(app);
    choose(&mut h, &events, open);
    let opened = h.state().session().dir().map(|d| d.display().to_string());
    std::fs::remove_dir_all(&dir)?;
    assert!(listed, "the recent project is not in Open Recent");
    assert_eq!(opened.as_deref(), Some(path.as_str()));
    Ok(())
}
