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
fn the_offer_to_download_every_template_is_answered_once() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(std::env::temp_dir().join(format!("texcraft-offer-{}", std::process::id())));
    let mut app = App::new(s);
    app.show_start_page();
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 820.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("Not Now").click();
    harness.run();
    assert!(harness.query_by_label("Download Templates").is_none(), "the offer stays after the answer");
    let prefs = harness.state_mut().take_changed_prefs();
    assert!(prefs.is_some_and(|p| p.templates_offered), "the answer is not kept for the next start");
}
