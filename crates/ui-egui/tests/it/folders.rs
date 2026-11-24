//! New folders in the file tree, and moving a file into one: by the keyboard (the row's menu,
//! Move to…) and by dragging it onto the folder.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn harness(files: &[&str]) -> Result<Harness<'static, App>, String> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for f in files {
        s.execute("file.import", &json!({"path": f, "data": "iVBORw0KGgo="}))?;
    }
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h =
        Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run_steps(10);
    Ok(h)
}

fn has(h: &mut Harness<'_, App>, path: &str) -> bool {
    h.state_mut().session().project().paths().any(|p| p.as_str() == path)
}

#[test]
fn a_folder_made_from_the_files_header_is_shown_in_the_tree() -> Result<(), String> {
    let mut h = harness(&[])?;
    h.get_by_label("New Folder…").click();
    h.run_steps(10);
    h.get_by_label("Its path in the project, such as chapters/figures").type_text("chapters");
    h.run_steps(10);
    h.key_press(egui::Key::Enter);
    h.run_steps(10);
    assert!(h.query_by_role_and_label(Role::TreeItem, "chapters").is_some(), "the empty folder is not in the tree");
    let folders: Vec<String> = h.state_mut().session().empty_folders().map(|f| f.as_str().to_owned()).collect();
    assert_eq!(folders, ["chapters"]);
    Ok(())
}

#[test]
fn the_keyboard_moves_a_file_into_a_folder_from_its_menu() -> Result<(), String> {
    let mut h = harness(&["plot.png", "figures/old.png"])?;
    h.get_by_role_and_label(Role::TreeItem, "plot.png").focus();
    h.run_steps(10);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    h.run_steps(10);
    for _ in 0..10 {
        if h.query_by_label("Move to…").is_some_and(|n| n.is_focused()) {
            break;
        }
        h.key_press(egui::Key::Tab);
        h.run_steps(10);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(10);
    h.get_by_label("The folder to move it to (empty for the top level)").type_text("figures");
    h.run_steps(10);
    h.key_press(egui::Key::Enter);
    h.run_steps(10);
    assert!(has(&mut h, "figures/plot.png") && !has(&mut h, "plot.png"), "the file was not moved");
    Ok(())
}

#[test]
fn a_file_dragged_onto_a_folder_moves_there_but_not_over_a_file_of_its_name() -> Result<(), String> {
    let mut h = harness(&["plot.png", "figures/old.png", "other/plot.png"])?;
    let drag = |h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2| {
        let button =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        h.event(egui::Event::PointerMoved(from));
        h.step();
        h.event(button(from, true));
        h.step();
        for i in 1..=10 {
            h.event(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
            h.step();
        }
        h.event(button(to, false));
        h.run_steps(3);
    };
    let center = |h: &mut Harness<'_, App>, label: &str| h.get_by_role_and_label(Role::TreeItem, label).rect().center();

    let (from, to) = (center(&mut h, "old.png"), center(&mut h, "other"));
    drag(&mut h, from, to);
    assert!(has(&mut h, "other/old.png") && !has(&mut h, "figures/old.png"), "not moved onto the folder");

    let top = h
        .query_all_by_role_and_label(Role::TreeItem, "plot.png")
        .map(|n| n.rect().center())
        .fold(None, |a: Option<egui::Pos2>, p| Some(a.map_or(p, |a| if p.y > a.y { p } else { a })));
    let from = top.ok_or("no plot.png row")?;
    let to = center(&mut h, "other");
    drag(&mut h, from, to);
    assert!(has(&mut h, "plot.png") && has(&mut h, "other/plot.png"), "moved over a file of the same name");
    Ok(())
}
