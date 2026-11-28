//! Link to a Repository (docs/design/add-remote.md): Import's dialog for a repository that
//! exists, by keyboard alone, What happens before anything changes, the choice per file, the name
//! asked when `origin` is taken, and Unlink… asking first. Against the git server in memory.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::gitwire::fake::Server;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::{App, ui_cmd};

use crate::import::{published, temp, window};

const NOTES: &str = "https://example.org/lab/notes.git";
/// Where the other history was pushed from: the user name in it, as git keeps it.
const AS_ADA: &str = "https://ada@example.org/lab/notes.git";
const WIDE: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// A window on a project of `files` in its own folder, reaching `server`.
fn linked_window(name: &str, server: &Server, files: &[(&str, &str)]) -> (Harness<'static, App>, std::path::PathBuf) {
    linked_window_with(name, server, files, None)
}

/// [`linked_window`], with a remote set first.
fn linked_window_with(name: &str, server: &Server, files: &[(&str, &str)], origin: Option<&str>) -> (Harness<'static, App>, std::path::PathBuf) {
    let dir = temp(&format!("link-{name}"));
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let sent = server.clone();
    s.set_git_http(move || Box::new(sent.clone()));
    for (path, text) in files {
        if s.execute("file.write", &json!({"path": path, "text": text})).is_err() {
            let _ = s.execute("file.new", &json!({"path": path, "text": text}));
        }
    }
    let folder = dir.join("mine");
    let _ = s.execute("project.save_as", &json!({"path": folder.display().to_string()}));
    if let Some(url) = origin {
        let _ = s.execute("git.set_remote", &json!({"name": "origin", "url": url}));
    }
    (window(s, &dir, WIDE), folder)
}

/// Runs frames until the checks and the link have ended.
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..3 {
        for _ in 0..45 {
            h.step();
        }
        for _ in 0..600 {
            h.step();
            let s = h.state().session();
            if s.import_busy().is_none() && s.git_busy().is_none() && s.forge_busy().is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    h.run();
}

fn text(h: &Harness<'_, App>, path: &str) -> Option<String> {
    let path = ProjectPath::new(path).ok()?;
    h.state().session().project().text(&path).ok().map(str::to_owned)
}

fn said(h: &Harness<'_, App>, text: &str) -> bool {
    h.query_all_by_label_contains(text).count() > 0
}

fn focused(h: &Harness<'_, App>) -> String {
    h.query_all_by(|n| n.is_focused()).last().and_then(|n| n.accesskit_node().label()).unwrap_or_default()
}

fn type_text(h: &mut Harness<'_, App>, text: &str) {
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
}

/// The dialog's Link (the editor's toolbar has a Link of its own, before it).
fn link_button<'a>(h: &'a Harness<'_, App>) -> Result<egui_kittest::Node<'a>, String> {
    h.query_all_by_role_and_label(Role::Button, "Link").last().ok_or_else(|| "no Link button".to_owned())
}

fn type_into(h: &mut Harness<'_, App>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
}

/// Tab until the control named `name` has the keyboard.
fn tab_to(h: &mut Harness<'_, App>, name: &str) {
    for _ in 0..20 {
        if focused(h) == name {
            return;
        }
        key(h, egui::Key::Tab);
    }
    assert_eq!(focused(h), name, "Tab never reached {name}");
}

fn key(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    h.run();
}

#[test]
fn by_keyboard_alone_a_git_address_is_checked_said_and_linked_with_both_histories() -> Result<(), String> {
    let dir = temp("link-keys-server");
    let server = published(&dir, AS_ADA, Some(("ada", "tok-link")))?;
    let (mut h, folder) = linked_window("keys", &server, &[("notes.tex", "Mine\n")]);
    ui_cmd::run_with(h.state_mut(), "git.add_remote", &json!({"source": "git"}));
    h.run();
    h.run();
    assert!(h.query_by_label("Link to a Git Address").is_some(), "the page did not open");
    assert!(h.query_by_label("Into").is_none(), "a link asks for no folder");
    // The keyboard starts in the address; the sign-in fields come when the server asks.
    type_text(&mut h, NOTES);
    settle(&mut h);
    assert!(said(&h, "needs signing in"), "the server's question is not said");
    key(&mut h, egui::Key::Tab);
    assert!(focused(&h).starts_with("User name"), "Tab went to {}", focused(&h));
    type_text(&mut h, "ada");
    key(&mut h, egui::Key::Tab);
    assert!(focused(&h).starts_with("Password or token"), "Tab went to {}", focused(&h));
    type_text(&mut h, "tok-link");
    settle(&mut h);
    assert!(said(&h, "they will be merged with your project's"), "What happens does not say the merge");
    let before = text(&h, "main.tex");
    // Both start with a main.tex: kept from this project, by keyboard.
    tab_to(&mut h, "Keep mine: main.tex");
    key(&mut h, egui::Key::Space);
    tab_to(&mut h, "Link");
    key(&mut h, egui::Key::Enter);
    settle(&mut h);
    assert!(h.query_by_label("Link to a Git Address").is_none(), "the dialog did not close");
    assert!(text(&h, "notes.tex").is_some(), "this project's file stays");
    assert_eq!(text(&h, "main.tex"), before, "the file kept is this project's");
    assert!(folder.join("main.tex").is_file());
    assert!(said(&h, "Linked to"), "the status line does not say it");
    let config = std::fs::read_to_string(folder.join(".git/config")).map_err(|e| e.to_string())?;
    assert!(config.contains("example.org/lab/notes.git") && !config.contains("tok-link"), "the remote, without its token: {config}");
    let head = std::fs::read_to_string(folder.join(".git/refs/heads/main")).map_err(|e| e.to_string())?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head.trim().to_owned()), "the merge was sent");
    Ok(())
}

#[test]
fn a_file_on_both_sides_is_chosen_before_linking() -> Result<(), String> {
    let dir = temp("link-clash-server");
    let server = published(&dir, AS_ADA, Some(("ada", "tok-link")))?;
    let (mut h, _) = linked_window("clash", &server, &[("main.tex", "Mine\n")]);
    ui_cmd::run_with(h.state_mut(), "git.add_remote", &json!({"url": NOTES}));
    settle(&mut h);
    type_into(&mut h, "User name", "ada");
    type_into(&mut h, "Password or token", "tok-link");
    settle(&mut h);
    assert!(said(&h, "Both have files with the same names"), "the clash is not said");
    assert!(link_button(&h)?.accesskit_node().is_disabled(), "Link before each file is chosen");
    h.get_by_role_and_label(Role::RadioButton, "Take theirs: main.tex").click();
    h.run();
    link_button(&h)?.click();
    settle(&mut h);
    let theirs = text(&h, "main.tex").unwrap_or_default();
    assert!(theirs.contains("Two"), "the repository's version was not taken: {theirs}");
    assert!(h.state().session().forge_conflicts().is_empty());
    Ok(())
}

#[test]
fn origin_taken_asks_for_a_name_and_unlink_asks_first() -> Result<(), String> {
    let dir = temp("link-name-server");
    let server = published(&dir, AS_ADA, Some(("ada", "tok-link")))?;
    let (mut h, folder) = linked_window_with("name", &server, &[("notes.tex", "Mine\n")], Some("https://example.org/lab/other.git"));
    let remotes = || std::fs::read_to_string(folder.join(".git/config")).map(|c| c.matches("[remote ").count()).unwrap_or(0);
    ui_cmd::run_with(h.state_mut(), "git.add_remote", &json!({"url": NOTES}));
    settle(&mut h);
    type_into(&mut h, "User name", "ada");
    type_into(&mut h, "Password or token", "tok-link");
    settle(&mut h);
    let name = h.get_by_role_and_label(Role::TextInput, "Name");
    let suggested = name.accesskit_node().value().unwrap_or_default();
    assert!(!suggested.is_empty() && suggested != "origin", "no other name suggested: {suggested:?}");
    assert!(said(&h, "stays as it is"), "what happens to origin is not said");
    h.get_by_role_and_label(Role::RadioButton, "Keep mine: main.tex").click();
    h.run();
    link_button(&h)?.click();
    settle(&mut h);
    assert_eq!(remotes(), 2, "both remotes");

    ui_cmd::run(h.state_mut(), "git.remove_remote");
    h.run();
    assert!(said(&h, "Neither the project nor the repository changes"), "Unlink did not ask first");
    h.get_by_role_and_label(Role::Button, "Unlink").click();
    h.run();
    assert_eq!(remotes(), 1, "origin stays");
    Ok(())
}

#[test]
fn the_dialog_offers_the_online_places_only_the_default_first() {
    let dir = temp("link-cards-server");
    let server = Server::new(&dir);
    let (mut h, _) = linked_window("cards", &server, &[("notes.tex", "x\n")]);
    ui_cmd::run(h.state_mut(), "git.add_remote");
    h.run();
    h.run();
    assert!(h.query_by_label("Link to a Repository").is_some());
    assert!(h.query_by_role_and_label(Role::Group, "Where the repository is").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Git Address, Any HTTPS address").is_some());
    assert!(h.query_all_by_label_contains("Folder,").count() == 0, "a folder cannot be linked to");
    assert!(focused(&h).starts_with("GitLab"), "the first card has the keyboard: {}", focused(&h));
}
