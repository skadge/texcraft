//! Import a Project's GitHub page: the project suggestions and branch list (docs/forges.md, "Choosing a project
//! and a branch"), against the fake GitHub: typing filters the suggestions, Down and Enter choose
//! one by keyboard and its branches fill the drop-down with the default one chosen, Escape closes
//! the list and keeps the text, and the list is a named list box whose count is announced.

use egui::accesskit::{Live, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use texcraft_engine::forge::fake::Fake;
use texcraft_ui_egui::{App, ui_cmd};

use crate::forge::{harness, quiet};

const TOKEN: &str = "github_pat_pick_test";

fn github() -> Fake {
    let fake = Fake::github("ada/paper", TOKEN, &[("main.tex", b"\\documentclass{article}\n")]);
    fake.push("draft", &[("main.tex", Some(b"x\n"))]);
    fake.list("ada/thesis", "public", "2026-01-01T00:00:00Z", Some("trunk"));
    fake
}

/// The Open dialog on GitHub with the token typed, and `text` typed into the project field.
fn dialog(fake: &Fake, text: &str) -> Harness<'static, App> {
    let mut h = harness(fake);
    ui_cmd::run_with(h.state_mut(), "project.import_dialog", &serde_json::json!({"source": "github"}));
    h.run();
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    h.get_by_label("Personal access token").click();
    h.run();
    h.get_by_label("Personal access token").type_text(TOKEN);
    quiet(&mut h);
    h.get_by_role_and_label(Role::EditableComboBox, "Project").click();
    h.run();
    h.get_by_role_and_label(Role::EditableComboBox, "Project").type_text(text);
    quiet(&mut h);
    h
}

fn options(h: &Harness<'_, App>) -> Vec<String> {
    h.query_all_by_role(Role::ListBoxOption).filter_map(|n| n.accesskit_node().label().map(|l| l.to_owned())).collect()
}

fn key(h: &mut Harness<'_, App>, key: egui::Key) {
    h.key_press(key);
    quiet(h);
}

#[test]
fn typing_filters_the_suggestions_in_a_named_list_whose_count_is_announced() {
    let fake = github();
    let mut h = dialog(&fake, "a");
    let all = options(&h);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all[0].starts_with("ada/paper, Private"), "the private marker is not in words: {all:?}");
    assert!(all[1].contains("Public") && all[1].contains("default branch trunk"), "{all:?}");
    let list = h.get_by_role(Role::ListBox);
    assert_eq!(list.accesskit_node().label().as_deref(), Some("Projects"));
    let field = h.get_by_role_and_label(Role::EditableComboBox, "Project");
    assert_eq!(field.accesskit_node().data().is_expanded(), Some(true), "the field does not say its list is open");
    let announced = |h: &Harness<'_, App>, text: &str| h.query_all_by_label_contains(text).any(|n| n.accesskit_node().live() != Live::Off);
    assert!(announced(&h, "2 suggestions"), "the count is not announced");

    h.get_by_role_and_label(Role::EditableComboBox, "Project").type_text("da/th");
    quiet(&mut h);
    let filtered = options(&h);
    assert_eq!(filtered.len(), 1, "typing did not filter: {filtered:?}");
    assert!(filtered[0].starts_with("ada/thesis"));
    assert!(announced(&h, "1 suggestion"), "the new count is not announced");
}

#[test]
fn down_and_enter_choose_a_project_and_its_branches_fill_the_list_with_the_default_chosen() {
    let fake = github();
    let mut h = dialog(&fake, "pap");
    key(&mut h, egui::Key::ArrowDown);
    let option = h.get_by_role(Role::ListBoxOption);
    assert_eq!(option.accesskit_node().is_selected(), Some(true), "Down did not choose the suggestion");
    // The option chosen is the field's active descendant: what assistive technology reads as
    // focused while the keyboard stays in the field.
    assert!(option.accesskit_node().is_focused(), "the option chosen is not the field's active descendant");
    assert!(h.query_all_by_label_contains("ada/paper, 1 of 1").count() > 0, "the option chosen is not said");

    key(&mut h, egui::Key::Enter);
    assert!(h.query_by_role(Role::ListBox).is_none(), "the list stayed open");
    let field = h.get_by_role_and_label(Role::EditableComboBox, "Project");
    assert_eq!(field.accesskit_node().value().as_deref(), Some("https://github.com/ada/paper"));
    assert!(h.query_by_label("Import from GitHub").is_some(), "Enter on a suggestion opened the project");
    // The branches, listed in a drop-down with the default one chosen.
    let branch = h.get_by_role_and_label(Role::ComboBox, "Branch");
    let shown = format!("{:?} {:?}", branch.accesskit_node().value(), branch.accesskit_node().label());
    assert!(h.query_all_by_label_contains("main (default)").count() > 0 || shown.contains("main (default)"), "{shown}");
    assert!(fake.requests().iter().any(|r| r.contains("/repos/ada/paper/branches")), "{:?}", fake.requests());
}

#[test]
fn escape_closes_the_list_and_keeps_the_text_and_the_dialog() {
    let fake = github();
    let mut h = dialog(&fake, "the");
    assert!(h.query_by_role(Role::ListBox).is_some(), "no list to close");
    key(&mut h, egui::Key::Escape);
    assert!(h.query_by_role(Role::ListBox).is_none(), "Escape did not close the list");
    let field = h.get_by_role_and_label(Role::EditableComboBox, "Project");
    assert_eq!(field.accesskit_node().value().as_deref(), Some("the"));
    assert_eq!(field.accesskit_node().data().is_expanded(), Some(false));
    assert!(h.query_by_label("Import from GitHub").is_some(), "Escape closed the dialog with the list");
    // Down opens it again.
    key(&mut h, egui::Key::ArrowDown);
    assert!(h.query_by_role(Role::ListBox).is_some(), "Down did not open the list again");
}

#[test]
fn without_a_sign_in_or_a_token_the_list_says_to_sign_in() {
    let fake = github();
    let mut h = harness(&fake);
    ui_cmd::run_with(h.state_mut(), "project.import_dialog", &serde_json::json!({"source": "github"}));
    h.run();
    h.get_by_role_and_label(Role::EditableComboBox, "Project").click();
    h.run();
    h.get_by_role_and_label(Role::EditableComboBox, "Project").type_text("pap");
    quiet(&mut h);
    assert!(h.query_by_label_contains("Sign in to see your repositories").is_some(), "the list does not say why it is empty");
    assert!(fake.requests().is_empty(), "GitHub was asked with no key");
}

/// A transport that holds every request until `open` is set: the account's list stays on its way.
struct Gated {
    inner: Box<dyn texcraft_engine::forge::Transport>,
    open: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl texcraft_engine::forge::Transport for Gated {
    fn send(&self, request: texcraft_engine::forge::Request) -> texcraft_engine::forge::Sending<'_> {
        for _ in 0..2000 {
            if self.open.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.inner.send(request)
    }
}

/// The window, its forge's answers held until `open` is set.
pub(crate) fn gated(fake: &Fake, open: &std::sync::Arc<std::sync::atomic::AtomicBool>, browser: bool) -> Harness<'static, App> {
    let mut s = texcraft_engine::Session::with_backend(Box::new(texcraft_compile::Unavailable("tests".into())));
    if browser {
        s.use_library(texcraft_engine::library::Library::default());
    }
    let (fake, open) = (fake.clone(), open.clone());
    s.set_forge_transport(move || Box::new(Gated { inner: fake.transport(), open: open.clone() }));
    let app = crate::support::app(s);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h
}

#[test]
fn while_the_list_is_fetched_the_field_says_so_and_stays_usable_then_counts_it() {
    let fake = github();
    let open = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut h = gated(&fake, &open, false);
    ui_cmd::run_with(h.state_mut(), "project.import_dialog", &serde_json::json!({"source": "github"}));
    h.run();
    if h.query_by_label("Personal access token").is_none() {
        h.get_by_label("Use a personal access token instead").click();
        h.run();
    }
    h.get_by_label("Personal access token").click();
    h.run();
    h.get_by_label("Personal access token").type_text(TOKEN);
    for _ in 0..40 {
        h.step();
    }
    let announced = |h: &Harness<'_, App>, text: &str| h.query_all_by_label_contains(text).any(|n| n.accesskit_node().live() != Live::Off);
    assert!(h.state().session().forge_lookup_busy(), "the list is not being fetched");
    assert!(announced(&h, "Fetching your repositories…"), "nothing says the list is on its way");
    // The field is usable meanwhile; its suggestions say the list is still coming.
    h.get_by_role_and_label(Role::EditableComboBox, "Project").click();
    h.step();
    h.get_by_role_and_label(Role::EditableComboBox, "Project").type_text("pap");
    for _ in 0..30 {
        h.step();
    }
    assert_eq!(h.get_by_role_and_label(Role::EditableComboBox, "Project").accesskit_node().value().as_deref(), Some("pap"));
    assert!(announced(&h, "Still fetching your repositories…"), "the suggestions do not say the list is still coming");
    open.store(true, std::sync::atomic::Ordering::Relaxed);
    quiet(&mut h);
    assert!(h.query_by_label_contains("Fetching your repositories").is_none(), "still says it is fetching");
    assert!(announced(&h, "2 repositories"), "the count is not said once the list came");
    assert!(options(&h).iter().any(|o| o.starts_with("ada/paper")), "the suggestions did not follow: {:?}", options(&h));
}

#[test]
fn a_list_that_cannot_be_fetched_says_why_in_words() {
    let fake = github();
    fake.fail_with(500, &[], "{\"message\": \"Server Error\"}");
    let h = dialog(&fake, "");
    assert!(h.query_by_label_contains("Your list could not be fetched:").is_some(), "the failure is not said");
    assert!(h.query_by_label_contains("Fetching your repositories").is_none());
}
