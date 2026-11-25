//! A narrow window (a phone, issue #12): the side panel starts hidden and opens from the rail as a
//! drawer over the panes, which closes on Escape, on a tap beside it, and once a file is chosen;
//! the editor and the PDF are two tabs.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

/// A phone held upright, in points.
const PHONE: egui::Vec2 = egui::vec2(400.0, 820.0);

fn harness(size: egui::Vec2) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\input{intro}\nHello.\n"}))?;
    s.execute("file.new", &json!({"path": "intro.tex", "text": "Intro.\n"}))?;
    s.execute("file.open", &json!({"path": "main.tex"}))?;
    // What a wide window was left with: the split layout and the files beside it.
    let prefs = Prefs { auto_compile: false, layout: "split".to_owned(), side: "files".to_owned(), ..Prefs::default() };
    let mut h = Harness::builder().with_size(size).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    run(&mut h);
    Ok(h)
}

fn run(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.run();
    }
}

fn files_shown(h: &Harness<'_, App>) -> bool {
    h.query_by_label("New Folder…").is_some()
}

/// The accessible name of `n`.
fn name(n: &egui_kittest::Node<'_>) -> String {
    n.accesskit_node().label().unwrap_or_default()
}

/// The pane tab whose name starts with `label` (the PDF's goes on with the build's state).
fn tab<'a>(h: &'a Harness<'_, App>, label: &str) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_role(Role::Tab).find(|n| name(n).starts_with(label) && !name(n).starts_with("Project"))
}

fn tab_selected(h: &Harness<'_, App>, label: &str) -> bool {
    tab(h, label).is_some_and(|n| n.accesskit_node().is_selected() == Some(true))
}

fn tap(h: &mut Harness<'_, App>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
}

#[test]
fn a_phone_shows_the_source_and_the_pdf_as_tabs_and_arrows_move_between_them() {
    let mut h = harness(PHONE).unwrap();
    assert!(tab_selected(&h, "Source"), "the source is not the tab shown first");
    assert!(h.query_by_label("main.tex source").is_some(), "the source's tab does not show the text");
    assert!(h.query_by_label("Compile").is_none(), "the PDF's pane is beside the text, not behind its tab");
    // The layout switch's buttons are the wide window's; the tabs replace them.
    assert!(h.query_by_label("Editor and PDF").is_none(), "the narrow top bar keeps the layout switch");

    tab(&h, "PDF").unwrap().click();
    run(&mut h);
    assert!(tab_selected(&h, "PDF"));
    assert!(h.query_by_label("Compile").is_some(), "the PDF's tab does not show the PDF's pane");
    assert!(h.query_by_label("main.tex source").is_none(), "the text stayed beside the PDF");

    tab(&h, "PDF").unwrap().focus();
    run(&mut h);
    h.key_press(egui::Key::ArrowLeft);
    run(&mut h);
    assert!(tab_selected(&h, "Source"), "Left from the PDF's tab did not show the source");
    assert!(tab(&h, "Source").unwrap().is_focused(), "the keyboard did not follow to the tab shown");

    // The commands the tabs run, for the automation interface.
    let reply = h.state_mut().control("engine.execute", &json!({"command": "view.pane.pdf"}));
    assert_eq!(reply["ok"], true, "{reply}");
    run(&mut h);
    assert!(tab_selected(&h, "PDF"));
    let ui = h.state_mut().control("ui.inspect", &json!({}));
    assert_eq!((ui["result"]["narrow"].as_bool(), ui["result"]["pane"].as_str()), (Some(true), Some("pdf")));
}

#[test]
fn a_phone_starts_without_the_side_panel_and_opens_it_as_a_drawer() {
    let mut h = harness(PHONE).unwrap();
    assert!(!files_shown(&h), "the files take room from the text on a phone");
    let rail = h.get_by_label("Project Files");
    rail.click();
    run(&mut h);
    assert!(files_shown(&h), "the rail's button did not open the files");
    let add = h.get_by_label("Add Files…");
    assert!(add.rect().left() >= h.get_by_label("Project Files").rect().right() - 1.0, "the drawer covers the rail");
    assert!(h.query_by_label("main.tex source").is_some(), "the drawer took the text's place instead of lying over it");
    let focused = h.query_all(egui_kittest::kittest::by()).find(|n| n.is_focused()).map(|n| n.rect());
    assert!(focused.is_some_and(|r| r.left() >= h.get_by_label("Project Files").rect().right() - 1.0), "the keyboard did not go into the drawer");

    h.key_press(egui::Key::Escape);
    run(&mut h);
    assert!(!files_shown(&h), "Escape did not close the drawer");
    assert!(h.get_by_label("Project Files").is_focused(), "the keyboard did not go back to the rail");

    // A tap beside it closes it too.
    h.get_by_label("Project Files").click();
    run(&mut h);
    assert!(files_shown(&h));
    tap(&mut h, egui::pos2(PHONE.x - 10.0, PHONE.y / 2.0));
    run(&mut h);
    assert!(!files_shown(&h), "a tap beside the drawer did not close it");
}

#[test]
fn choosing_a_file_in_the_drawer_closes_it_and_shows_the_file() {
    let mut h = harness(PHONE).unwrap();
    tab(&h, "PDF").unwrap().click();
    run(&mut h);
    h.get_by_label("Project Files").click();
    run(&mut h);
    h.get_by_label("intro.tex").click();
    run(&mut h);
    assert!(!files_shown(&h), "choosing a file left the drawer over it");
    assert!(tab_selected(&h, "Source"), "the chosen file is behind the PDF's tab");
    assert!(h.get_by_label("intro.tex source").is_focused(), "the chosen file's text did not take the keyboard");
}

#[test]
fn widening_the_window_brings_back_the_side_panel_and_the_split() {
    let mut h = harness(PHONE).unwrap();
    assert!(!files_shown(&h));
    h.set_size(egui::vec2(1400.0, 820.0));
    run(&mut h);
    assert!(files_shown(&h), "the wide window lost its side panel");
    assert!(h.query_by_label("Compile").is_some() && h.query_by_label("main.tex source").is_some(), "the wide window lost the split");
    assert!(h.query_all_by_role(Role::Tab).all(|n| !name(&n).starts_with("PDF")), "the wide window kept the tabs");
}

#[test]
fn a_phone_s_top_bar_holds_the_menus_in_one_with_the_title_beside_it() {
    let mut h = harness(PHONE).unwrap();
    assert!(h.query_by_label("File").is_none(), "the menus overflow a phone's top bar");
    let menu = h.get_by_label("Menu").rect();
    let title = h.query_all_by_role(Role::Button).find(|n| name(n).starts_with("Untitled project:")).map(|n| n.rect()).unwrap();
    assert!(title.left() >= menu.right() && title.right() <= PHONE.x, "the project's name lies over the menu or past the window");
    h.get_by_label("Menu").click();
    run(&mut h);
    assert!(["File", "Edit", "Insert", "Help"].iter().all(|m| h.query_by_label(m).is_some()), "a menu is missing from the phone's menu");
}
