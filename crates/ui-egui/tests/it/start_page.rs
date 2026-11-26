//! The start page at a phone's width and a desktop's: its buttons whole and in reading order, and
//! no build reported while no project is open.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

/// A phone held upright, in points.
const PHONE: egui::Vec2 = egui::vec2(400.0, 820.0);

fn harness(size: egui::Vec2) -> Result<Harness<'static, App>, String> {
    let s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let prefs = Prefs { auto_compile: false, ..Prefs::default() };
    let mut h = Harness::builder().with_size(size).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, prefs));
    h.state_mut().show_start_page();
    run(&mut h);
    Ok(h)
}

fn run(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.run();
    }
}

/// The accessible name of `n`.
fn name(n: &egui_kittest::Node<'_>) -> String {
    n.accesskit_node().label().unwrap_or_default()
}

#[test]
fn a_phone_s_start_page_shows_its_buttons_whole_the_main_one_first() {
    let h = harness(PHONE).unwrap();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, PHONE);
    let mut tops = Vec::new();
    for label in ["New Project", "Open Folder…", "Open from GitLab or GitHub…"] {
        let rect = h.get_by_role_and_label(Role::Button, label).rect();
        assert!(screen.contains_rect(rect), "{label} is cut off at the window's edge: {rect:?}");
        tops.push(rect.top());
    }
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "the buttons are not one under the other, the main one first");
    // The keyboard meets them in that order too.
    let order: Vec<String> = h.query_all_by_role(Role::Button).map(|n| name(&n)).filter(|n| n == "New Project" || n.starts_with("Open ")).collect();
    assert_eq!(order.first().map(String::as_str), Some("New Project"), "the main button is not first in reading order: {order:?}");
}

#[test]
fn a_wide_start_page_reads_its_buttons_in_order_and_reports_no_build() {
    let h = harness(egui::vec2(1400.0, 900.0)).unwrap();
    let buttons: Vec<(String, f32)> = h
        .query_all_by_role(Role::Button)
        .map(|n| (name(&n), n.rect().left()))
        .filter(|(n, _)| n == "New Project" || n.starts_with("Open "))
        .collect();
    assert_eq!(buttons.len(), 3, "{buttons:?}");
    assert!(buttons.windows(2).all(|w| w[0].1 < w[1].1), "the reading order is not left to right: {buttons:?}");
    // No project, no build: the status bar does not speak of one.
    assert!(h.query_by_label_contains("compiled").is_none() && h.query_by_label_contains("Compiled").is_none(), "the start page reports a build");
}
