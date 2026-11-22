use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;

fn app() -> App {
    App::new(Session::with_backend(Box::new(Unavailable("tests".into()))))
}

#[test]
fn an_interface_command_sent_over_the_channel_changes_what_the_window_shows() {
    let mut app = app();
    let reply = app.control("engine.execute", &json!({"command": "view.layout.editor"}));
    assert_eq!(reply["ok"], true, "{reply}");
    let ui = app.control("ui.inspect", &json!({}));
    assert_eq!(ui["result"]["layout"], "editor");
}

#[test]
fn an_unknown_command_is_an_error_not_a_success() {
    let mut app = app();
    let reply = app.control("engine.execute", &json!({"command": "no.such.command"}));
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().is_some_and(|e| e.contains("no.such.command")));
}
