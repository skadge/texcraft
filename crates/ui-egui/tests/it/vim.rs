use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

#[test]
fn vim_keys_typed_into_the_editor_edit_the_file_and_its_ctrl_keys_reach_it() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": ""})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim: true, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    harness.get_by_label("main.tex source").type_text("ihello");
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.get_by_label("main.tex source").type_text("x");
    harness.run();
    assert_eq!(text(&mut harness), "hell", "Insert mode, Escape and x");
    harness.get_by_label("main.tex source").type_text("u");
    harness.run();
    assert_eq!(text(&mut harness), "hello");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::R);
    harness.run();
    assert_eq!(text(&mut harness), "hell", "Ctrl-R did not reach Vim");
}

#[test]
fn a_click_moves_vims_caret_at_once() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "abc\ndef"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim: true, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    // The second line's first character: the source's top left is laid out as the window draws it.
    let line_one = harness.get_by_label("main.tex source").rect();
    let p = egui::pos2(line_one.left() + 60.0, line_one.top() + 8.0 + 21.0 + 10.0);
    harness.event(egui::Event::PointerMoved(p));
    harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    harness.run();
    // Before any key: the caret shown (and the status bar's place) is where the click was.
    assert!(harness.query_by_label_contains("Ln 2,").is_some(), "Vim's caret stayed on line 1 after the click");
}

#[test]
fn ctrl_arrows_move_vims_caret_by_words() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "one two three"})).unwrap();
    let app = App::with_prefs(s, Prefs { auto_compile: false, vim: true, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("main.tex source").focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowRight);
    harness.run();
    harness.get_by_label("main.tex source").type_text("x");
    harness.run();
    assert_eq!(text(&mut harness), "one wo three", "Ctrl+Right in Normal mode");
    harness.get_by_label("main.tex source").type_text("i");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::ArrowRight);
    harness.get_by_label("main.tex source").type_text("X");
    harness.run();
    assert_eq!(text(&mut harness), "one wo Xthree", "Ctrl+Right in Insert mode");
}

#[test]
fn colon_w_saves_the_file_and_compiles() -> std::io::Result<()> {
    let dir = std::env::temp_dir().join(format!("texcraft-vim-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("main.tex"), "")?;
    let mut app = App::with_prefs(
        Session::with_backend(Box::new(Unavailable("No engine in this test.".into()))),
        Prefs { auto_compile: false, vim: true, ..Prefs::default() },
    );
    app.open_project(&dir.display().to_string());
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("main.tex source").focus();
    h.run();
    h.get_by_label("main.tex source").type_text("ihello");
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label("main.tex source").type_text(":w");
    h.key_press(egui::Key::Enter);
    // The build runs on a thread, and its spinner repaints until it ends.
    for _ in 0..400 {
        h.step();
        if h.query_all_by_label_contains("No engine in this test.").next().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let written = std::fs::read_to_string(dir.join("main.tex"));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(written?, "hello", ":w did not save the file");
    assert!(h.query_all_by_label_contains("No engine in this test.").next().is_some(), ":w did not compile");
    Ok(())
}
