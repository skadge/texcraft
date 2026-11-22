use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;

fn app() -> App {
    App::new(Session::with_backend(Box::new(Unavailable("No engine in this test.".into()))))
}

#[test]
fn compiling_without_an_engine_says_why_instead_of_showing_success() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app());
    harness.get_by_label("Compile").click();
    // The build runs on a thread, and its spinner repaints until it ends: step until it has.
    for _ in 0..400 {
        harness.step();
        if harness.query_all_by_label_contains("No engine in this test.").next().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(harness.query_all_by_label_contains("No engine in this test.").next().is_some());
    assert!(harness.query_by_label_contains("Compiled in").is_none());
}
