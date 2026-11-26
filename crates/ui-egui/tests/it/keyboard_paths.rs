//! Keyboard paths through every region, panel and dialog (docs/ROADMAP.md, A10). From a known
//! start, Tab, Shift+Tab, the arrows and F6 reach every control and move egui's focus, as the
//! accessibility tree reports it (not a painted ring); Escape leaves a dialog and gives the
//! keyboard back to the control that had it; no control keeps the keyboard; and every focusable
//! node has a role and a name that starts with its visible label (standards/accessibility.md).

use std::sync::Arc;

use egui::accesskit::{Action, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::{Backend, Diagnostic, Job, Outcome, Severity, Status, Unavailable};
use texcraft_engine::Session;
use texcraft_ui_egui::prefs::{Identity, Prefs};
use texcraft_ui_egui::{App, ui_cmd};

/// A control the keyboard stopped on.
#[derive(Debug, Clone, PartialEq)]
struct Stop {
    id: String,
    role: Role,
    name: String,
    /// Inside a dialog.
    in_dialog: bool,
}

impl Stop {
    fn is(&self, role: Role, name: &str) -> bool {
        self.role == role && self.name == name
    }
}

fn window(s: Session) -> Harness<'static, App> {
    let prefs = Prefs { auto_compile: false, identity: Identity::new("Ada").ok(), ..Prefs::default() };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.run();
    h
}

fn blank() -> Harness<'static, App> {
    window(Session::with_backend(Box::new(Unavailable("tests".into()))))
}

/// The node that has the keyboard, from the accessibility tree.
fn focused(h: &Harness<'_, App>) -> Option<Stop> {
    h.root().children_recursive().map(|n| n.accesskit_node()).find(|n| n.is_focused()).map(|n| Stop {
        id: format!("{:?}", n.id()),
        role: n.role(),
        name: n.label().unwrap_or_default(),
        in_dialog: std::iter::successors(n.parent(), |p| p.parent()).any(|p| p.role() == Role::Dialog),
    })
}

fn now(h: &Harness<'_, App>) -> Stop {
    let stop = focused(h);
    assert!(stop.is_some(), "nothing has the keyboard");
    stop.unwrap_or_else(|| Stop { id: String::new(), role: Role::Unknown, name: String::new(), in_dialog: false })
}

fn press(h: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) -> Stop {
    h.key_press_modifiers(modifiers, key);
    h.run();
    now(h)
}

fn tab(h: &mut Harness<'_, App>) -> Stop {
    press(h, egui::Modifiers::NONE, egui::Key::Tab)
}

fn back(h: &mut Harness<'_, App>) -> Stop {
    press(h, egui::Modifiers::SHIFT, egui::Key::Tab)
}

fn key(h: &mut Harness<'_, App>, key: egui::Key) -> Stop {
    press(h, egui::Modifiers::NONE, key)
}

/// Gives the keyboard to the control named `name` (and `role`); the one in a dialog, when one is
/// open.
fn focus(h: &mut Harness<'_, App>, role: Role, name: &str) -> Stop {
    let in_dialog = |n: &egui_kittest::Node<'_>| std::iter::successors(n.accesskit_node().parent(), |p| p.parent()).any(|p| p.role() == Role::Dialog);
    let all: Vec<_> = h.query_all_by_role_and_label(role, name).collect();
    let target = all.iter().find(|n| in_dialog(n)).or_else(|| all.first());
    assert!(target.is_some(), "no {name} ({role:?})");
    if let Some(node) = target {
        node.focus();
    }
    h.run();
    let stop = now(h);
    assert!(stop.is(role, name), "{name} did not take the keyboard: {stop:?}");
    stop
}

fn names(stops: &[Stop]) -> Vec<String> {
    stops.iter().map(|s| format!("{:?} {}", s.role, s.name)).collect()
}

/// Presses Tab until the keyboard is back where it started, at most `max` times: each press moves
/// it (so none is a trap), and the stops are returned in order, the start last.
fn tab_cycle(h: &mut Harness<'_, App>, max: usize) -> Vec<Stop> {
    let start = now(h);
    let mut stops: Vec<Stop> = Vec::new();
    while stops.len() < max {
        let stop = tab(h);
        // A dialog of one control keeps it: Tab has nowhere else to go.
        let done = stop.id == start.id;
        assert!(done || stops.last().is_none_or(|s| s.id != stop.id), "Tab did not move from {:?}; so far {:?}", stop.name, names(&stops));
        stops.push(stop);
        if done {
            break;
        }
    }
    assert!(stops.last().is_some_and(|s| s.id == start.id), "Tab did not come back to {:?} in {max} presses: {:?}", start.name, names(&stops));
    stops
}

/// Presses Tab until a stop is `role` and `name`, at most `max` times; the stops on the way.
fn tab_to(h: &mut Harness<'_, App>, role: Role, name: &str, max: usize) -> Vec<Stop> {
    let mut stops: Vec<Stop> = Vec::new();
    while !stops.last().is_some_and(|s| s.is(role, name)) {
        assert!(stops.len() < max, "Tab did not reach {name}: {:?}", names(&stops));
        let stop = tab(h);
        assert!(stops.last().is_none_or(|s| s.id != stop.id), "Tab did not move from {:?}", stop.name);
        stops.push(stop);
    }
    stops
}

/// Asserts `stops` reach each of `expected` in that order.
fn assert_order(stops: &[Stop], expected: &[(Role, &str)], what: &str) {
    let mut at = 0;
    for (role, name) in expected {
        let found = stops.iter().skip(at).position(|s| s.is(*role, name));
        assert!(found.is_some(), "{what}: {name} ({role:?}) is not reached in order: {:?}", names(stops));
        at += found.unwrap_or_default() + 1;
    }
}

/// Problems of the focusable nodes in the tree: no role, no name, or a name that does not start
/// with the text shown in the control.
fn audit(h: &Harness<'_, App>) -> Vec<String> {
    let mut problems = Vec::new();
    for node in h.root().children_recursive() {
        let n = node.accesskit_node();
        if n.is_hidden() || !n.data().supports_action(Action::Focus) {
            continue;
        }
        let name = n.label().unwrap_or_default();
        let named = !name.trim().is_empty() || n.data().placeholder().is_some() || !n.data().labelled_by().is_empty();
        if matches!(n.role(), Role::Unknown | Role::GenericContainer) {
            problems.push(format!("no role: {name:?}"));
        }
        if !named {
            problems.push(format!("no name: {:?} at {:?}", n.role(), n.raw_bounds()));
        }
        let shown = node.children().map(|c| c.accesskit_node()).find(|c| c.role() == Role::Label).and_then(|c| c.label());
        if let Some(shown) = shown.filter(|s| !s.is_empty())
            && !name.starts_with(shown.as_str())
        {
            problems.push(format!("{:?} {name:?} does not start with its label {shown:?}", n.role()));
        }
    }
    problems
}

fn assert_tree(h: &Harness<'_, App>, what: &str) {
    let problems = audit(h);
    assert!(problems.is_empty(), "{what}: {problems:#?}");
}

fn inspect(h: &mut Harness<'_, App>) -> serde_json::Value {
    h.state_mut().control("ui.inspect", &json!({}))["result"].clone()
}

fn run(h: &mut Harness<'_, App>, id: &str) {
    ui_cmd::run(h.state_mut(), id);
    h.run();
}

/// A dialog opened from the window: it takes the keyboard, Tab cycles inside it through `expected`
/// (in that order) and never reaches the window behind, Shift+Tab goes back, every control is
/// named, and Escape closes it and gives the keyboard back to `from`.
fn dialog_path(h: &mut Harness<'_, App>, from: &Stop, open: impl FnOnce(&mut Harness<'_, App>), expected: &[(Role, &str)]) {
    open(h);
    let dialog = inspect(h)["dialog"].clone();
    let first = now(h);
    assert!(first.in_dialog, "the {dialog} dialog did not take the keyboard: {first:?}");
    let cycle = tab_cycle(h, 80);
    assert!(cycle.iter().all(|s| s.in_dialog), "Tab left the {dialog} dialog: {:?}", names(&cycle));
    // From the start, once round: the expected controls in order.
    let mut round: Vec<Stop> = vec![first];
    round.extend(cycle.iter().take(cycle.len().saturating_sub(1)).cloned());
    assert_order(&round, expected, &format!("the {dialog} dialog"));
    let before = cycle.get(cycle.len().saturating_sub(2)).map(|s| s.id.clone());
    if cycle.len() > 1 {
        assert_eq!(Some(back(h).id), before, "Shift+Tab goes back in the {dialog} dialog");
    }
    assert_tree(h, &format!("the {dialog} dialog"));
    escape_closes(h, from);
}

fn escape_closes(h: &mut Harness<'_, App>, from: &Stop) {
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(inspect(h)["dialog"], json!(null), "Escape did not close the dialog");
    let back = now(h);
    assert_eq!(back.id, from.id, "the keyboard did not go back to {:?} but to {back:?}", from.name);
}

#[test]
fn tab_crosses_the_window_in_reading_order_and_f6_takes_the_keyboard_out_of_the_editor() {
    let mut h = blank();
    let stops = tab_to(&mut h, Role::MultilineTextInput, "main.tex source", 60);
    // The top bar, the rail, the side panel (the files above the outline), the editor's tabs and
    // toolbar, then its text: in the order they read.
    let order = [
        (Role::Button, "Start Page"),
        (Role::Button, "File"),
        (Role::Button, "Help"),
        (Role::Button, "Untitled project: name and save the project"),
        (Role::Button, "Editor and PDF"),
        (Role::Tab, "Project Files"),
        (Role::Button, "Settings"),
        (Role::Button, "Add Files…"),
        (Role::TreeItem, "main.tex"),
        (Role::TreeItem, "Introduction"),
        (Role::Tab, "main.tex"),
        (Role::RadioButton, "Source"),
        (Role::Button, "Bold"),
        (Role::Button, "More formatting"),
    ];
    assert_order(&stops, &order, "the window");
    let splitters: Vec<&str> = stops.iter().filter(|s| s.role == Role::Splitter).map(|s| s.name.as_str()).collect();
    assert!(!splitters.is_empty() && splitters.iter().all(|s| *s != "Resize panel"), "each resize handle says what it resizes: {splitters:?}");
    assert_tree(&h, "the window");
    // The editor keeps Tab for indenting; F6 takes the keyboard on to the PDF's pane, whose
    // controls Tab goes through before coming back to the top.
    assert!(key(&mut h, egui::Key::F6).is(Role::Button, "Compile"), "F6 leaves the editor for the PDF pane");
    let after = tab_to(&mut h, Role::Button, "Start Page", 20);
    assert!(after.iter().any(|s| s.is(Role::Button, "Build log")), "{:?}", names(&after));
}

#[test]
fn f6_from_inside_a_region_goes_on_to_the_next_and_shift_f6_back_to_its_start() {
    let mut h = blank();
    focus(&mut h, Role::Button, "Bold");
    assert!(key(&mut h, egui::Key::F6).is(Role::Button, "Compile"), "F6 from the editor's toolbar goes to the PDF pane");
    focus(&mut h, Role::Button, "Bold");
    let start = press(&mut h, egui::Modifiers::SHIFT, egui::Key::F6);
    assert!(start.is(Role::MultilineTextInput, "main.tex source"), "Shift+F6 from the editor's toolbar goes to the editor's start: {start:?}");
}

#[test]
fn the_menu_bar_is_reached_with_f10_walked_with_the_arrows_and_left_with_escape() {
    let mut h = blank();
    let from = focus(&mut h, Role::Button, "Bold");
    assert!(key(&mut h, egui::Key::F10).is(Role::Button, "File"));
    assert!(key(&mut h, egui::Key::ArrowRight).is(Role::Button, "Edit"));
    assert!(key(&mut h, egui::Key::ArrowLeft).is(Role::Button, "File"));
    assert!(key(&mut h, egui::Key::ArrowLeft).is(Role::Button, "Help"), "Left from the first menu wraps to the last");
    let item = key(&mut h, egui::Key::ArrowDown);
    assert_ne!(item.name, "Help", "Down opens the menu with its first entry focused");
    let next = key(&mut h, egui::Key::ArrowDown);
    assert_ne!(next.id, item.id, "Down moves through the entries");
    assert!(key(&mut h, egui::Key::Escape).is(Role::Button, "Help"), "Escape closes the menu, back on its button");
    assert_eq!(key(&mut h, egui::Key::Escape).id, from.id, "Escape leaves the menu bar where the keyboard was");
}

#[test]
fn the_rail_and_the_side_panel_sections_are_walked_from_the_keyboard() {
    let mut h = blank();
    key(&mut h, egui::Key::F6);
    let rail = key(&mut h, egui::Key::F6);
    assert!(rail.is(Role::Tab, "Project Files"), "{rail:?}");
    assert!(key(&mut h, egui::Key::ArrowDown).is(Role::Tab, "Outline"), "Down moves along the rail");
    key(&mut h, egui::Key::Enter);
    assert_eq!(inspect(&mut h)["side"], "outline", "Enter opens the section");
    for (section, side) in [("Search", "search"), ("History", "history"), ("Review", "review")] {
        focus(&mut h, Role::Tab, section);
        key(&mut h, egui::Key::Space);
        assert_eq!(inspect(&mut h)["side"], side, "Space opens {section}");
        assert_tree(&h, section);
    }
}

#[test]
fn a_file_s_menu_opens_from_the_keyboard_and_delete_asks_in_a_dialog_that_gives_the_keyboard_back() {
    let mut h = blank();
    h.state_mut().control("engine.execute", &json!({"command": "file.new", "params": {"path": "other.tex"}}));
    h.run();
    let row = focus(&mut h, Role::TreeItem, "other.tex");
    let item = press(&mut h, egui::Modifiers::SHIFT, egui::Key::F10);
    assert!(item.is(Role::Button, "Open"), "Shift+F10 opens the file's menu with the keyboard on its first entry: {item:?}");
    assert!(key(&mut h, egui::Key::ArrowDown).name != "Open", "Down moves through the entries");
    assert!(key(&mut h, egui::Key::Escape).is(Role::TreeItem, "other.tex"), "Escape closes the menu, back on the file");
    let open = |h: &mut Harness<'_, App>| {
        press(h, egui::Modifiers::SHIFT, egui::Key::F10);
        h.get_by_label("Delete…").click();
        h.run();
    };
    dialog_path(&mut h, &row, open, &[(Role::Button, "Delete"), (Role::Button, "Cancel")]);
    let kept = texcraft_engine::project::ProjectPath::new("other.tex").is_ok_and(|p| h.state().session().project().get(&p).is_some());
    assert!(kept, "Escape deleted nothing");
}

#[test]
fn search_and_the_symbols_are_reached_from_their_fields() {
    let mut h = blank();
    run(&mut h, "view.find_in_project");
    let field = now(&h);
    assert!(field.is(Role::TextInput, "Search in the project"), "Find in Project puts the keyboard in its field: {field:?}");
    h.get_by_label("Search in the project").type_text("document");
    h.key_press(egui::Key::Enter);
    h.run();
    let stops: Vec<Stop> = (0..8).map(|_| tab(&mut h)).collect();
    assert!(stops.iter().any(|s| s.role == Role::TreeItem), "a result is reached by Tab: {:?}", names(&stops));
    assert_tree(&h, "Search");
    run(&mut h, "edit.symbols");
    let field = now(&h);
    assert!(field.is(Role::TextInput, "Search symbols"), "the palette puts the keyboard in its field: {field:?}");
    let first = tab(&mut h);
    assert!(first.is(Role::Button, "alpha \\alpha"), "Tab goes on to the symbols: {first:?}");
    let right = key(&mut h, egui::Key::ArrowRight);
    assert!(right.is(Role::Button, "beta \\beta"), "the arrows move through the grid: {right:?}");
    assert_tree(&h, "the symbols");
}

#[test]
fn the_editor_s_toolbar_menus_open_and_close_from_the_keyboard() {
    let mut h = blank();
    for (menu, first) in [("More formatting", "Inline Formula"), ("Heading", "Section"), ("Compile options", "Compile automatically")] {
        focus(&mut h, Role::Button, menu);
        let item = key(&mut h, egui::Key::Enter);
        assert_eq!(item.name, first, "Enter opens {menu} with the keyboard on its first entry");
        assert!(key(&mut h, egui::Key::Escape).is(Role::Button, menu), "Escape closes {menu}, back on its button");
    }
}

#[test]
fn the_formatted_view_s_figure_and_its_dialog_are_reached_by_tab() {
    let mut h = super::tables::formatted(super::tables::FIGURE);
    tab_to(&mut h, Role::Button, "Edit Figure…", 50);
    assert!(tab(&mut h).is(Role::MultilineTextInput, "main.tex formatted"), "then the text");
    let from = back(&mut h);
    assert_tree(&h, "the formatted view");
    let open = |h: &mut Harness<'_, App>| {
        h.key_press(egui::Key::Enter);
        h.run();
    };
    let expected =
        [(Role::ComboBox, "Image"), (Role::CheckBox, "Here, if it fits"), (Role::MultilineTextInput, "Caption (LaTeX)"), (Role::Button, "Apply")];
    dialog_path(&mut h, &from, open, &expected);
}

/// A build with a PDF, an error placed in the source and a raw log.
struct Failing;

impl Backend for Failing {
    fn name(&self) -> String {
        "failing".into()
    }
    fn compile(&mut self, job: &Job) -> Outcome {
        let mut outcome = Outcome::unavailable(job.id, "");
        outcome.status = Status::Built;
        outcome.log = "This is the log.\n! Undefined control sequence.\n".into();
        outcome.pdf = Some(Arc::new(b"%PDF".to_vec()));
        outcome.diagnostics = vec![Diagnostic {
            severity: Severity::Error,
            message: "Undefined control sequence".into(),
            origin: None,
            file: Some("main.tex".into()),
            line: Some(3),
            column: None,
            end_column: None,
            help: Vec::new(),
            fixes: Vec::new(),
            noticed: None,
        }];
        outcome
    }
}

#[test]
fn the_build_log_s_filters_messages_places_files_and_raw_log_are_reached_by_tab() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Failing));
    s.execute("build.compile", &json!({}))?;
    s.wait();
    let mut h = window(s);
    run(&mut h, "view.logs");
    focus(&mut h, Role::Button, "Back to PDF");
    let mut stops = tab_to(&mut h, Role::DisclosureTriangle, "Raw log", 40);
    key(&mut h, egui::Key::Enter);
    stops.extend(tab_to(&mut h, Role::Button, "Copy the Raw Log", 10));
    let expected = [
        (Role::RadioButton, "All 1"),
        (Role::RadioButton, "Errors 1"),
        (Role::Button, "Copy the messages shown"),
        (Role::Button, "Copy the message"),
        (Role::Link, "main.tex:3"),
        (Role::Button, "Save As… main.pdf"),
        (Role::TextInput, "Search the raw log"),
    ];
    assert_order(&stops, &expected, "the build log");
    assert_tree(&h, "the build log");
    Ok(())
}

#[test]
fn the_review_margin_s_thread_is_reached_and_its_reply_field_does_not_keep_the_keyboard() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "One two three.\n"}))?;
    s.set_author(Identity::new("Ada").ok().map(|i| i.author()));
    s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 7, "text": "Two?"}))?;
    let mut h = window(s);
    run(&mut h, "view.layout.editor");
    let stops: Vec<Stop> = (0..6).map(|_| key(&mut h, egui::Key::F6)).collect();
    let card = stops.iter().find(|s| s.name.starts_with("Comment by Ada")).cloned();
    assert!(card.is_some(), "F6 reaches the margin's thread: {:?}", names(&stops));
    let card = card.unwrap_or_else(|| now(&h));
    focus(&mut h, card.role, &card.name);
    let reply = key(&mut h, egui::Key::Enter);
    assert!(reply.is(Role::MultilineTextInput, "Reply"), "Enter chooses the thread and puts the keyboard in its reply: {reply:?}");
    let back_stops: Vec<Stop> = (0..2).map(|_| back(&mut h)).collect();
    assert_order(&back_stops, &[(Role::Button, "Resolve"), (card.role, &card.name)], "Shift+Tab from the reply");
    focus(&mut h, Role::MultilineTextInput, "Reply");
    assert_ne!(tab(&mut h).name, "Reply", "Tab leaves the reply field");
    assert_tree(&h, "the review margin");
    Ok(())
}

#[test]
fn every_settings_tab_is_reached_and_walked_from_the_keyboard() {
    let mut h = blank();
    let from = focus(&mut h, Role::Button, "Compile");
    run(&mut h, "app.settings");
    assert!(now(&h).in_dialog, "Settings takes the keyboard");
    for tab_name in ["Appearance", "Editor", "Building", "Keyboard", "Review", "AI", "Accounts"] {
        focus(&mut h, Role::Tab, tab_name);
        key(&mut h, egui::Key::Enter);
        let cycle = tab_cycle(&mut h, 60);
        assert!(cycle.iter().all(|s| s.in_dialog), "Tab left Settings › {tab_name}: {:?}", names(&cycle));
        let own = cycle.iter().any(|s| s.role != Role::Tab && s.name != "Close");
        assert!(own, "Settings › {tab_name} has controls of its own: {:?}", names(&cycle));
        assert_tree(&h, &format!("Settings › {tab_name}"));
    }
    escape_closes(&mut h, &from);
}

#[test]
fn the_small_dialogs_take_the_keyboard_keep_it_and_give_it_back() {
    let mut h = blank();
    let from = focus(&mut h, Role::Button, "Compile");
    dialog_path(&mut h, &from, |h| run(h, "help.about"), &[(Role::Button, "Close")]);
    let prompt = [(Role::TextInput, "Its path in the project, such as chapters/intro.tex"), (Role::Button, "Cancel")];
    dialog_path(&mut h, &from, |h| run(h, "file.new"), &prompt);
    let open_forge = [(Role::EditableComboBox, "Project address"), (Role::Button, "Cancel"), (Role::RadioButton, "GitLab: Service")];
    dialog_path(&mut h, &from, |h| run(h, "forge.open"), &open_forge);
    // The gallery is no dialog of the control channel's, but a modal all the same.
    run(&mut h, "project.from_template");
    assert!(now(&h).in_dialog, "the gallery takes the keyboard");
    let cycle = tab_cycle(&mut h, 120);
    assert!(cycle.iter().all(|s| s.in_dialog), "Tab left the gallery: {:?}", names(&cycle));
    assert_order(&cycle, &[(Role::TreeItem, "All"), (Role::Button, "Blank project")], "the gallery");
    assert_tree(&h, "the gallery");
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(now(&h).id, from.id, "closing the gallery gives the keyboard back");
    // Unsaved changes, before a new project.
    h.state_mut().control("engine.execute", &json!({"command": "file.write", "params": {"path": "main.tex", "text": "changed"}}));
    h.run();
    let unsaved = [(Role::Button, "Save"), (Role::Button, "Cancel"), (Role::Button, "Don't Save")];
    dialog_path(&mut h, &from, |h| run(h, "project.new"), &unsaved);
}

#[test]
fn the_keyboard_shortcuts_dialog_keeps_the_keyboard_gives_it_back_and_opens_again() {
    let mut h = blank();
    let from = focus(&mut h, Role::Button, "Compile");
    run(&mut h, "help.shortcuts");
    let first = now(&h);
    assert!(first.in_dialog, "the shortcuts dialog takes the keyboard: {first:?}");
    let cycle = tab_cycle(&mut h, 60);
    assert!(cycle.iter().all(|s| s.in_dialog), "Tab left the shortcuts dialog: {:?}", names(&cycle));
    assert!(cycle.iter().any(|s| s.is(Role::Button, "OK")), "{:?}", names(&cycle));
    escape_closes(&mut h, &from);
    run(&mut h, "help.shortcuts");
    assert!(now(&h).in_dialog, "the dialog opens again");
}

#[test]
fn the_forge_commit_dialog_is_walked_and_left_from_the_keyboard() {
    let fake = super::forge::paper();
    let mut h = super::forge::opened(&fake);
    assert!(super::forge::settle(&mut h), "the project did not open");
    super::forge::write(&mut h, "main.tex", "changed\n");
    let from = focus(&mut h, Role::Button, "Compile");
    dialog_path(&mut h, &from, |h| run(h, "forge.commit"), &[(Role::Button, "Cancel")]);
}

#[test]
fn a_folder_says_whether_it_is_open_and_enter_opens_and_closes_it() {
    let mut h = blank();
    h.state_mut().control("engine.execute", &json!({"command": "file.new", "params": {"path": "chapters/one.tex"}}));
    h.run();
    let expanded = |h: &Harness<'_, App>| h.get_by_role_and_label(Role::TreeItem, "chapters").accesskit_node().data().is_expanded();
    assert_eq!(expanded(&h), Some(true), "an open folder says so");
    focus(&mut h, Role::TreeItem, "chapters");
    key(&mut h, egui::Key::Enter);
    assert_eq!(expanded(&h), Some(false), "Enter closes it, and it says so");
    assert!(h.query_by_role_and_label(Role::TreeItem, "one.tex").is_none(), "its files are hidden");
    assert!(now(&h).is(Role::TreeItem, "chapters"), "the folder keeps the keyboard");
}

#[test]
fn the_history_s_versions_files_and_changes_are_reached_from_the_keyboard() -> Result<(), String> {
    let dir = super::history::temp("keyboard").map_err(|e| e.to_string())?;
    let mut h = window(super::history::saved_twice(&dir)?);
    run(&mut h, "view.side.history");
    let side: Vec<Stop> = (0..3).map(|_| key(&mut h, egui::Key::F6)).collect();
    let version = side.last().cloned().unwrap_or_else(|| now(&h));
    assert!(version.name.contains("Edited main.tex"), "F6 reaches the newest version: {:?}", names(&side));
    let older = key(&mut h, egui::Key::ArrowDown);
    assert!(older.name.contains("Added main.tex") && older.id != version.id, "Down goes to the older version: {older:?}");
    key(&mut h, egui::Key::Enter);
    tab_to(&mut h, Role::TreeItem, "main.tex", 4);
    key(&mut h, egui::Key::Enter);
    let after = tab_to(&mut h, Role::Button, "Restore This File…", 10);
    assert!(after.iter().all(|s| s.name != "Resize panel"), "{:?}", names(&after));
    assert_tree(&h, "the history");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_pdf_pane_s_toolbar_and_menus_are_walked_from_the_keyboard() {
    // More steps than the PDF tests' harness: the page thread asks for frames while it draws.
    let mut h = window(Session::with_backend(Box::new(super::pdf_preview::Sample)));
    run(&mut h, "build.compile");
    for _ in 0..400 {
        h.step();
        if h.query_by_label("1 / 2").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
    focus(&mut h, Role::Button, "Compile");
    let stops = tab_to(&mut h, Role::Button, "Zoom In", 20);
    // In the order they read, left to right (Previous page is disabled on the first page).
    let expected = [(Role::Button, "Build log"), (Role::Button, "Export"), (Role::Button, "Next page"), (Role::Button, "Zoom Out")];
    assert_order(&stops, &expected, "the PDF pane's toolbar");
    // Then the pages and their links.
    let pages: Vec<Stop> = (0..3).map(|_| tab(&mut h)).collect();
    assert_order(&pages, &[(Role::Document, "Page 1 of 2"), (Role::Link, "Link to page 2")], "the PDF's pages");
    focus(&mut h, Role::Button, "Zoom In");
    let zoom = stops.iter().find(|s| s.name.ends_with('%') || s.name.starts_with("Fit")).cloned();
    assert!(zoom.is_some(), "the zoom menu's button is reached: {:?}", names(&stops));
    let zoom = zoom.unwrap_or_else(|| now(&h));
    focus(&mut h, zoom.role, &zoom.name);
    let entry = key(&mut h, egui::Key::Enter);
    assert!(entry.name.starts_with("Fit width"), "Enter opens the zoom menu on its first entry: {entry:?}");
    assert_eq!(key(&mut h, egui::Key::Escape).id, zoom.id, "Escape closes it, back on its button");
    let export = focus(&mut h, Role::Button, "Export");
    assert_eq!(key(&mut h, egui::Key::Enter).name, "PDF", "Enter opens the export menu on its first format");
    assert_eq!(key(&mut h, egui::Key::Escape).id, export.id);
    assert_tree(&h, "the PDF pane");
}

#[test]
fn the_status_bar_s_controls_come_last_as_they_read() -> Result<(), String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "One two three.\n"}))?;
    s.set_author(Identity::new("Ada").ok().map(|i| i.author()));
    s.execute("review.comment", &json!({"path": "main.tex", "start": 4, "end": 7, "text": "Two?"}))?;
    let mut h = window(s);
    run(&mut h, "view.focus_mode");
    focus(&mut h, Role::Button, "Compile");
    let stops = tab_to(&mut h, Role::Button, "Leave Focus Mode", 20);
    assert_order(&stops, &[(Role::Button, "Build log"), (Role::Button, "1 comment")], "the PDF pane, then the status bar left to right");
    let text = key(&mut h, egui::Key::Enter);
    assert_eq!(inspect(&mut h)["focus_mode"], false, "Enter leaves focus mode");
    assert!(text.is(Role::MultilineTextInput, "main.tex source"), "and the keyboard goes to the text: {text:?}");
    Ok(())
}
