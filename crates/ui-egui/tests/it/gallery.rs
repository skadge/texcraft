use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::{App, ui_cmd};

#[test]
fn searching_the_gallery_narrows_the_templates_to_those_that_match() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(std::env::temp_dir().join(format!("texcraft-gallery-{}", std::process::id())));
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 820.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::new(s));
    harness.run();
    ui_cmd::run(harness.state_mut(), "project.from_template");
    harness.run();
    assert!(harness.query_by_label("IEEE journal article").is_some());
    assert!(harness.get_by_label("Search templates").is_focused(), "typing goes to the search");
    harness.get_by_label("Search templates").type_text("nature");
    harness.run();
    assert!(harness.query_by_label("Nature article").is_some(), "the match is shown");
    assert!(harness.query_by_label("IEEE journal article").is_none(), "the others are hidden");
    assert!(harness.query_by_label("Blank project").is_none(), "a search shows templates only");
}

#[test]
fn the_start_page_offers_each_template_without_downloading_any() {
    let dir = std::env::temp_dir().join(format!("texcraft-start-templates-{}", std::process::id()));
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(dir.clone());
    let mut app = App::new(s);
    app.show_start_page();
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 1400.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    assert!(harness.query_by_label("Nature article").is_some(), "a template's card is missing");
    assert!(harness.query_by_label("Blank project").is_some());
    assert!(!dir.exists(), "showing the cards downloaded something");
}
