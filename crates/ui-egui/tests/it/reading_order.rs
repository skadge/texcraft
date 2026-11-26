//! Reading order (docs/ROADMAP.md, A13; WCAG 2.4.3): within each region of the main window, Tab
//! moves left to right along a row and top to bottom between rows, as the controls are shown. A
//! row built right to left (a layout switch, a panel header's buttons, the window's buttons) or a
//! column built bottom up fails here. The rail's Settings and Keyboard Shortcuts are buttons that
//! open dialogs, not tabs of its section list.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

#[derive(Debug, Clone)]
struct Stop {
    id: String,
    role: Role,
    name: String,
    rect: egui::Rect,
}

fn window(prefs: Prefs) -> Harness<'static, App> {
    let s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(s, Prefs { auto_compile: false, ..prefs }));
    h.run();
    h
}

fn focused(h: &Harness<'_, App>) -> Option<Stop> {
    h.root().children_recursive().find(|n| n.accesskit_node().is_focused()).map(|n| {
        let a = n.accesskit_node();
        Stop { id: format!("{:?}", a.id()), role: a.role(), name: a.label().unwrap_or_default(), rect: n.rect() }
    })
}

fn press(h: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) -> Option<Stop> {
    h.key_press_modifiers(modifiers, key);
    h.run();
    focused(h)
}

/// Whether going from `a` to `b` reads forward: right along the same row, down to a later row, or
/// up into a column that starts right of `a` (the next column of a region).
fn forward(a: &Stop, b: &Stop) -> bool {
    let same_row = (a.rect.center().y - b.rect.center().y).abs() < a.rect.height().min(b.rect.height()) / 2.0;
    if same_row { b.rect.left() > a.rect.left() } else { b.rect.top() >= a.rect.top() - 1.0 || b.rect.left() >= a.rect.right() - 1.0 }
}

#[test]
fn tab_follows_the_visual_order_in_every_region_of_the_main_window() {
    let mut h = window(Prefs { first_steps_seen: true, ..Prefs::default() });
    // The first control of each region, as F6 reaches them.
    let mut starts: Vec<Stop> = Vec::new();
    for _ in 0..8 {
        let Some(stop) = press(&mut h, egui::Modifiers::NONE, egui::Key::F6) else { break };
        if starts.first().is_some_and(|s| s.id == stop.id) {
            break;
        }
        starts.push(stop);
    }
    assert!(starts.len() >= 4, "F6 did not reach the regions: {starts:?}");
    let mut problems = Vec::new();
    let mut walked = 0;
    // Every control met so far: meeting one again means Tab left the region (it wrapped).
    let mut seen: Vec<String> = Vec::new();
    let bar_bottom = starts[0].rect.bottom();
    for start in &starts {
        // The editor keeps Tab for indenting: its region is a single stop.
        if matches!(start.role, Role::MultilineTextInput | Role::TextInput | Role::Document) {
            continue;
        }
        if let Some(node) = h.root().children_recursive().find(|n| format!("{:?}", n.accesskit_node().id()) == start.id) {
            node.focus();
        }
        h.run();
        let first = focused(&h);
        assert!(first.as_ref().is_some_and(|f| f.id == start.id), "F6 did not come back to {:?}", start.name);
        let next_region: Vec<&str> = starts.iter().map(|s| s.id.as_str()).collect();
        let mut last = start.clone();
        for _ in 0..60 {
            let Some(stop) = press(&mut h, egui::Modifiers::NONE, egui::Key::Tab) else { break };
            // Tab wraps from the last region to the top bar.
            let wrapped = start.rect.top() >= bar_bottom && stop.rect.top() < bar_bottom;
            if wrapped
                || next_region.contains(&stop.id.as_str())
                || seen.contains(&stop.id)
                || matches!(stop.role, Role::MultilineTextInput | Role::Document)
            {
                break;
            }
            seen.push(stop.id.clone());
            walked += 1;
            if !forward(&last, &stop) {
                problems
                    .push(format!("{:?} {:?} at {:?} → {:?} {:?} at {:?}", last.role, last.name, last.rect.min, stop.role, stop.name, stop.rect.min));
            }
            last = stop;
        }
    }
    assert!(walked > 10, "the walk saw too few controls ({walked})");
    assert!(problems.is_empty(), "Tab goes against the visual order:\n{}", problems.join("\n"));
}

#[test]
fn the_rail_s_settings_and_shortcuts_are_buttons_after_its_section_tabs() {
    let h = window(Prefs { first_steps_seen: true, ..Prefs::default() });
    for name in ["Settings", "Keyboard Shortcuts"] {
        assert!(h.query_by_role_and_label(Role::Tab, name).is_none(), "{name} is announced as a tab");
        assert!(h.query_by_role_and_label(Role::Button, name).is_some(), "{name} is not a button");
    }
    let list = h.get_by_role(Role::TabList);
    let tabs: Vec<String> = list.query_all_by_role(Role::Tab).map(|n| n.accesskit_node().label().unwrap_or_default()).collect();
    assert!(tabs.contains(&"Outline".to_owned()) && !tabs.contains(&"Settings".to_owned()), "the section list: {tabs:?}");
    // Shown top to bottom: Settings above Keyboard Shortcuts.
    let settings = h.get_by_role_and_label(Role::Button, "Settings").rect();
    let shortcuts = h.get_by_role_and_label(Role::Button, "Keyboard Shortcuts").rect();
    assert!(settings.top() < shortcuts.top());
}
