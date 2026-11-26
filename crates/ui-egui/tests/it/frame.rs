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
    // Compiling automatically (the default), the pill's click still compiles now.
    harness.get_by_label("Auto-Compile: compile now").click();
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

#[test]
fn the_app_icon_goes_back_to_the_start_page() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app());
    harness.run();
    // Section titles are shown in capitals.
    assert!(harness.query_by_label("RECENT PROJECTS").is_none(), "the start page is already shown");
    harness.get_by_label("Start Page").click();
    harness.run();
    assert!(harness.query_by_label("RECENT PROJECTS").is_some(), "the start page is not shown");
}

fn maximize_asked(harness: &Harness<'_, App>) -> bool {
    harness.output().viewport_output.values().any(|v| v.commands.iter().any(|c| matches!(c, egui::ViewportCommand::Maximized(true))))
}

#[test]
fn a_desktop_window_has_its_controls_in_the_top_bar_unless_the_system_draws_them() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app());
    harness.run();
    assert!(harness.query_by_label("Minimize").is_none(), "the browser's page has window controls");

    let mut desktop = app();
    desktop.set_desktop_window(true);
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), desktop);
    harness.run();
    for label in ["Minimize", "Maximize", "Close"] {
        assert!(harness.query_by_label(label).is_some(), "no {label} button in the top bar");
    }
    harness.get_by_label("Maximize").click();
    harness.step();
    assert!(maximize_asked(&harness), "the Maximize button did not ask the window to maximize");

    let prefs = texcraft_ui_egui::prefs::Prefs { system_title_bar: true, ..Default::default() };
    let mut framed = App::with_prefs(Session::with_backend(Box::new(Unavailable("No engine in this test.".into()))), prefs);
    framed.set_desktop_window(true);
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), framed);
    harness.run();
    assert!(harness.query_by_label("Minimize").is_none(), "window controls next to the system's title bar");
}

#[test]
fn the_help_menu_opens_the_retex_manual_in_the_browser() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app());
    harness.run();
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label("retex User Manual").click();
    harness.step();
    let opened = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::OpenUrl(open) if open.url == texcraft_ui_egui::ui_cmd::RETEX_MANUAL));
    assert!(opened, "the manual's address was not opened");
}

fn opened(harness: &Harness<'_, App>, url: &str) -> bool {
    harness.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(open) if open.url == url))
}

#[test]
fn help_user_guide_and_f1_open_the_guide_and_a_page_parameter_cannot_leave_it() {
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app());
    harness.run();
    harness.get_by_label("Help").click();
    harness.run();
    // The menu's entry, with its shortcut (the first-steps hint names the guide too).
    harness.get_by_label("User Guide F1").click();
    harness.step();
    assert!(opened(&harness, texcraft_ui_egui::ui_cmd::USER_GUIDE), "Help › User Guide did not open the guide");

    // From the text: F1 is a window shortcut, not a key the editor keeps.
    let mut session = Session::with_backend(Box::new(Unavailable("No engine in this test.".into())));
    session.execute("file.write", &serde_json::json!({"path": "main.tex", "text": "Hello.\n"})).unwrap();
    let prefs = texcraft_ui_egui::prefs::Prefs { auto_compile: false, layout: "split".to_owned(), ..Default::default() };
    let mut harness =
        Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(session, prefs));
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    // One event per step: the press opens it (the release's step would replace its output).
    harness.key_down(egui::Key::F1);
    harness.step();
    let by_key = opened(&harness, texcraft_ui_egui::ui_cmd::USER_GUIDE);
    harness.key_up(egui::Key::F1);
    harness.step();
    assert!(by_key, "F1 did not open the guide");

    // A page of the guide opens there; anything else falls back to the guide's start.
    let guide = texcraft_ui_egui::ui_cmd::guide_page;
    assert!(guide("forges.html#servers").starts_with(texcraft_ui_egui::ui_cmd::USER_GUIDE));
    assert!(guide("forges.html#servers").ends_with("forges.html#servers"));
    for hostile in ["https://example.org/", "//example.org/x.html", "../x.html", "a.html?x=1", "javascript:alert(1)"] {
        assert_eq!(guide(hostile), texcraft_ui_egui::ui_cmd::USER_GUIDE, "{hostile} left the guide");
    }
}

#[test]
fn the_compile_pill_says_when_it_compiles_automatically_and_a_click_still_compiles_now() {
    let prefs = |auto| texcraft_ui_egui::prefs::Prefs { auto_compile: auto, ..texcraft_ui_egui::prefs::Prefs::default() };
    let session = || Session::with_backend(Box::new(Unavailable("No engine in this test.".into())));
    let mut off = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(session(), prefs(false)));
    off.run();
    assert!(off.query_by_role_and_label(egui::accesskit::Role::Button, "Compile").is_some(), "no Compile pill with compiling automatically off");
    let mut on = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, app: &mut App| app.frame(ui), App::with_prefs(session(), prefs(true)));
    on.run();
    // The pill says it compiles automatically; its name, what a click does.
    assert!(on.query_by_role_and_label(egui::accesskit::Role::Button, "Compile").is_none(), "the pill does not say it compiles automatically");
    on.get_by_role_and_label(egui::accesskit::Role::Button, "Auto-Compile: compile now").click();
    for _ in 0..400 {
        on.step();
        if on.query_all_by_label_contains("No engine in this test.").next().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(on.query_all_by_label_contains("No engine in this test.").next().is_some(), "clicking the pill did not compile");
}
