//! The Open from GitLab or GitHub dialog's project suggestions and branch list (docs/forges.md,
//! "Choosing a project and a branch"), through `forge.projects` and `forge.branches`.
//!
//! The project field is an editable combo box (WAI-ARIA's combobox with a list box): as the user
//! types, once typing pauses for a quarter of a second, the projects the account reaches whose
//! path holds the text are looked up in the background and listed under the field. Down opens
//! the list and moves through it, Up moves back, Enter chooses, Escape closes the list and keeps
//! the text; the keyboard stays in the field, the option chosen is its active descendant, and the
//! number of suggestions and the option chosen are announced through a live region
//! (standards/accessibility.md). A project chosen, or an address that reads as one, has its
//! branches listed in a drop-down with the default one chosen; a branch is typed by hand when the
//! list cannot be had, which the dialog says.

use std::hash::{Hash, Hasher};

use egui::{Key, Modifiers, RichText};
use serde_json::{Value, json};
use texcraft_engine::ForgeLookup;
use texcraft_engine::forge::{Address, Service};

use crate::App;
use crate::dialogs::Dialog;
use crate::forge::OpenForm;
use crate::i18n::trn;
use crate::theme::Tokens;

/// How long typing pauses before the projects are looked up, in seconds.
const PAUSE: f64 = 0.25;

/// A project suggested.
#[derive(Debug, Clone)]
pub(crate) struct Suggestion {
    pub path: String,
    pub web_url: String,
    pub visibility: String,
    pub default_branch: Option<String>,
    pub activity: Option<chrono::DateTime<chrono::Utc>>,
}

/// A branch of the list.
#[derive(Debug, Clone)]
pub(crate) struct BranchRow {
    pub name: String,
    pub protected: bool,
    pub default: bool,
}

/// What a lookup was for: the service, the instance, the query or project, and the key it was
/// asked with (a hash of the token typed, or none for the sign-in).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    service: Service,
    instance: String,
    text: String,
    key: Option<u64>,
}

/// The suggestions.
#[derive(Debug, Default)]
pub(crate) enum Projects {
    /// Nothing looked up yet.
    #[default]
    Idle,
    /// Neither signed in nor a token typed: nothing can be looked up.
    NeedsKey,
    Shown {
        items: Vec<Suggestion>,
        truncated: bool,
    },
    Failed(String),
}

/// The branch list.
#[derive(Debug, Default)]
pub(crate) enum Branches {
    /// No project to list the branches of yet.
    #[default]
    None,
    Loading,
    Listed {
        items: Vec<BranchRow>,
        truncated: bool,
    },
    Failed(String),
}

/// The project field's suggestions and the branch list of an Open dialog.
#[derive(Debug, Default)]
pub(crate) struct Picker {
    /// The suggestions are shown under the field.
    pub open: bool,
    pub selected: Option<usize>,
    pub projects: Projects,
    /// A lookup is on its way.
    pub loading: bool,
    /// What the suggestions shown are for.
    shown_for: Option<Asked>,
    /// The lookup of projects last sent.
    asked: Option<Asked>,
    /// Typing paused at this time (the window's clock): the projects are looked up once it has
    /// paused for [`PAUSE`].
    due: Option<f64>,
    pub branches: Branches,
    /// What the branch list is for.
    branches_for: Option<Asked>,
    /// The branches are to be listed (a project was chosen, or the address left).
    branches_due: bool,
    /// The branches are to be listed again once typing pauses (another key was typed).
    branches_later: bool,
    /// The branch is typed by hand, not chosen from the list.
    pub typing_branch: bool,
    /// The selection was moved by a key: scroll it into view.
    scroll: bool,
    /// The field lost the keyboard: once the pointer is up, the branches are listed. Not while
    /// it is down or just released, so that what is under it does not move under a click (egui
    /// would take the release for a click elsewhere and take the keyboard from the field
    /// clicked).
    left: bool,
    /// The server the projects are looked up on when the text names none (empty: chosen when
    /// the dialog is next drawn).
    pub server: String,
    /// The servers of the service to choose from, as the session has them.
    servers: Vec<String>,
}

impl Picker {
    /// A picker that looks up the recent projects at once.
    pub(crate) fn new() -> Self {
        Self { due: Some(f64::NEG_INFINITY), ..Self::default() }
    }

    /// The items shown, if any.
    fn items(&self) -> &[Suggestion] {
        match &self.projects {
            Projects::Shown { items, .. } => items,
            _ => &[],
        }
    }
}

/// The instance and the query `text` (as typed) looks up: a host typed first names the instance
/// (`gitlab.example.org/lab/pa`, `https://github.com/ada/pa`); otherwise `server`.
fn target(text: &str, server: &str, service: Service) -> (String, String) {
    let text = text.trim();
    let named = text.contains("://") || text.split('/').next().is_some_and(|first| first.contains('.') && text.contains('/'));
    if named && let Ok(a) = Address::parse_as(text, Some(service)) {
        return (a.instance, a.path);
    }
    let (scheme, rest) = match (text.strip_prefix("https://"), text.strip_prefix("http://")) {
        (Some(rest), _) => ("https", rest),
        (_, Some(rest)) => ("http", rest),
        _ if text.split('/').next().is_some_and(|first| first.contains('.') && text.contains('/')) => ("https", text),
        _ => return (server.to_owned(), text.to_owned()),
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let instance = if host == "www.github.com" { texcraft_engine::forge::GITHUB_COM.to_owned() } else { format!("{scheme}://{host}") };
    let path = path.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    // A page under the project (`/-/tree/main`, `/tree/main`): the project's path.
    let path = path.split("/-/").next().unwrap_or(path);
    (instance, path.to_owned())
}

/// The instances of `service` the dialog can look projects up on: its public one and the
/// servers added.
pub(crate) fn servers(app: &App, service: Service) -> Vec<String> {
    let mut list = vec![texcraft_engine::forge::oauth::instance(service).to_owned()];
    for s in app.session.forge_servers().into_iter().filter(|s| s.service == service) {
        if !list.contains(&s.instance) {
            list.push(s.instance);
        }
    }
    list
}

/// The server first offered for `service`: the public one, unless only a server added is signed
/// in to.
fn default_server(app: &App, service: Service) -> String {
    let list = servers(app, service);
    let public = list[0].clone();
    if app.session.forge_reaches(&public, service) {
        return public;
    }
    list.into_iter().find(|i| app.session.forge_reaches(i, service)).unwrap_or(public)
}

/// The key a lookup goes with: the token typed (by its hash), or the session's (sign-in, or a
/// token given earlier); none when there is neither.
fn key(app: &App, form: &OpenForm, instance: &str) -> Option<Option<u64>> {
    let token = form.token.trim();
    if form.use_token {
        if token.is_empty() {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        token.hash(&mut h);
        return Some(Some(h.finish()));
    }
    app.session.forge_reaches(instance, form.service).then_some(None)
}

/// Sends the lookups due: the projects once typing has paused, the branches once a project was
/// chosen. Before the dialog is drawn, each frame it is open.
pub(crate) fn tick(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_ref() else { return };
    let service = form.service;
    let (list, server) = (servers(app, service), form.picker.server.is_empty().then(|| default_server(app, service)));
    if let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_mut() {
        form.picker.servers = list;
        if let Some(server) = server {
            form.picker.server = server;
        }
    }
    let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_ref() else { return };
    if let Some(at) = form.picker.due {
        let wait = PAUSE - (now - at);
        if wait > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        } else {
            look_up_projects(app);
            if let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_mut() {
                form.picker.branches_due |= std::mem::take(&mut form.picker.branches_later);
            }
        }
    }
    if let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_ref()
        && form.picker.branches_due
    {
        look_up_branches(app);
    }
}

fn token_param(form: &OpenForm, params: &mut Value) {
    if form.use_token {
        params["token"] = json!(form.token.trim());
    }
}

fn look_up_projects(app: &mut App) {
    let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_ref() else { return };
    let (instance, query) = target(&form.address, &form.picker.server, form.service);
    let Some(key) = key(app, form, &instance) else {
        if let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_mut() {
            form.picker.due = None;
            form.picker.projects = Projects::NeedsKey;
            form.picker.shown_for = None;
        }
        return;
    };
    let asked = Asked { service: form.service, instance: instance.clone(), text: query.clone(), key };
    let mut params = json!({"service": form.service.id(), "instance": instance, "query": query, "background": true});
    token_param(form, &mut params);
    let fresh = form.picker.shown_for.as_ref() == Some(&asked) && matches!(form.picker.projects, Projects::Shown { .. });
    let outcome = if fresh { None } else { Some(app.session.execute("forge.projects", &params)) };
    let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_mut() else { return };
    form.picker.due = None;
    if outcome.is_some() {
        form.picker.asked = Some(asked.clone());
    }
    match outcome {
        None => {}
        Some(Ok(v)) if v["started"] == true => form.picker.loading = true,
        Some(Ok(v)) => show_projects(form, &v),
        Some(Err(e)) => {
            form.picker.projects = Projects::Failed(e);
            form.picker.shown_for = Some(asked);
        }
    }
}

fn look_up_branches(app: &mut App) {
    let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_ref() else { return };
    let address = Address::parse_as(&qualified(&form.address, &form.picker.server, form.service), Some(form.service));
    let outcome = match address {
        Err(_) => Err(None),
        Ok(address) => match key(app, form, &address.instance) {
            None => Err(Some(tl!("Sign in or use a personal access token to list the branches.").to_owned())),
            Some(key) => {
                let asked = Asked { service: form.service, instance: address.instance.clone(), text: address.path.clone(), key };
                if form.picker.branches_for.as_ref() == Some(&asked) && matches!(form.picker.branches, Branches::Listed { .. } | Branches::Loading) {
                    Ok((asked, None))
                } else {
                    let mut params = json!({"service": form.service.id(), "project": address.web_url(), "background": true});
                    token_param(form, &mut params);
                    Ok((asked, Some(app.session.execute("forge.branches", &params))))
                }
            }
        },
    };
    let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_mut() else { return };
    form.picker.branches_due = false;
    match outcome {
        Err(None) => {}
        Err(Some(why)) => form.picker.branches = Branches::Failed(why),
        Ok((_, None)) => {}
        Ok((asked, Some(sent))) => {
            form.picker.branches_for = Some(asked);
            match sent {
                Ok(v) if v["started"] == true => form.picker.branches = Branches::Loading,
                Ok(v) => show_branches(form, &v),
                Err(e) => form.picker.branches = Branches::Failed(e),
            }
        }
    }
}

/// The address `text` (as typed) means: a path alone is on `server`.
fn qualified(text: &str, server: &str, service: Service) -> String {
    let text = text.trim();
    let bare = !text.contains("://") && !text.starts_with("git@") && !text.split('/').next().is_some_and(|first| first.contains('.'));
    if bare && server != texcraft_engine::forge::oauth::instance(service) && !server.is_empty() {
        format!("{server}/{text}")
    } else {
        text.to_owned()
    }
}

/// Takes in the lookups that ended, for the Open dialog if it is still open; an outcome for
/// another query or project than the dialog's is dropped (a newer one is on its way).
pub(crate) fn poll(app: &mut App) {
    while let Some((kind, outcome)) = app.session.poll_forge_lookup() {
        let Some(Dialog::ForgeOpen(form)) = app.view.dialog.as_mut() else { continue };
        match (kind, outcome) {
            (ForgeLookup::Projects, Ok(v)) => {
                form.picker.loading = false;
                show_projects(form, &v);
            }
            (ForgeLookup::Projects, Err(e)) => {
                form.picker.loading = false;
                form.picker.projects = Projects::Failed(e);
            }
            (ForgeLookup::Branches, Ok(v)) => {
                let current =
                    form.picker.branches_for.as_ref().is_some_and(|a| v["project"] == a.text.as_str() && v["instance"] == a.instance.as_str());
                if current {
                    show_branches(form, &v);
                }
            }
            (ForgeLookup::Branches, Err(e)) => form.picker.branches = Branches::Failed(e),
        }
    }
}

fn show_projects(form: &mut OpenForm, v: &Value) {
    let (instance, query) = target(&form.address, &form.picker.server, form.service);
    if v["instance"] != instance.as_str() || v["query"] != query.as_str() {
        return;
    }
    let items: Vec<Suggestion> = v["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Suggestion {
                path: p["path"].as_str()?.to_owned(),
                web_url: p["web_url"].as_str()?.to_owned(),
                visibility: p["visibility"].as_str().unwrap_or("private").to_owned(),
                default_branch: p["default_branch"].as_str().map(str::to_owned),
                activity: p["activity"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&chrono::Utc)),
            })
        })
        .collect();
    form.picker.selected = form.picker.selected.filter(|i| *i < items.len());
    form.picker.projects = Projects::Shown { items, truncated: v["truncated"] == true };
    form.picker.shown_for = form.picker.asked.clone().filter(|a| a.instance == instance && a.text == query);
}

fn show_branches(form: &mut OpenForm, v: &Value) {
    let items: Vec<BranchRow> = v["branches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| Some(BranchRow { name: b["name"].as_str()?.to_owned(), protected: b["protected"] == true, default: b["default"] == true }))
        .collect();
    // The default branch chosen, unless one of the list was typed or chosen already.
    if !form.picker.typing_branch && !items.iter().any(|b| b.name == form.branch.trim()) {
        form.branch = items.iter().find(|b| b.default).map(|b| b.name.clone()).unwrap_or_default();
    }
    form.picker.branches = Branches::Listed { items, truncated: v["truncated"] == true };
}

/// The address field typed again, or the service or server changed: the suggestions follow.
fn typed(picker: &mut Picker, now: f64) {
    picker.due = Some(now);
    picker.open = true;
    picker.selected = None;
}

/// Chooses suggestion `s`: its address in the field, its default branch, and its branches
/// listed.
fn choose(form: &mut OpenForm, s: &Suggestion) {
    form.address = s.web_url.clone();
    form.branch = s.default_branch.clone().unwrap_or_default();
    form.picker.open = false;
    form.picker.selected = None;
    form.picker.typing_branch = false;
    form.picker.branches_due = true;
}

/// "Private", "Internal" or "Public": the marker is a word, not a colour.
fn visibility_word(v: &str) -> &'static str {
    match v {
        "public" => tl!("Public"),
        "internal" => tl!("Internal"),
        _ => tl!("Private"),
    }
}

/// What a suggestion's row says after its path: its visibility, when it was last active, its
/// default branch.
fn details(s: &Suggestion, now: chrono::DateTime<chrono::Utc>) -> String {
    let mut parts = vec![visibility_word(&s.visibility).to_owned()];
    if let Some(t) = s.activity {
        parts.push(tl!("active {when}").replace("{when}", &crate::i18n::ago(t, now)));
    }
    if let Some(b) = &s.default_branch {
        parts.push(tl!("default branch {branch}").replace("{branch}", b));
    }
    parts.join(" · ")
}

/// The token typed or the choice of a token changed: the suggestions and the branches are
/// looked up again with it, once typing pauses.
pub(crate) fn key_changed(form: &mut OpenForm, now: f64) {
    form.picker.due = Some(now);
    form.picker.shown_for = None;
    form.picker.branches_for = None;
    form.picker.branches_later = !matches!(form.picker.branches, Branches::None);
}

/// The service changed: the suggestions and the branches start again, on its server.
pub(crate) fn service_changed(form: &mut OpenForm, now: f64) {
    form.picker = Picker { due: Some(now), ..Picker::default() };
}

/// The server drop-down, when `servers` has more than the public instance.
fn server_choice(ui: &mut egui::Ui, t: &Tokens, form: &mut OpenForm, now: f64) {
    let servers = form.picker.servers.clone();
    if servers.len() < 2 {
        return;
    }
    ui.add_space(t.unit * 2.0);
    let label = ui.label(RichText::new(tl!("Server")).color(t.text_muted));
    let shown = texcraft_engine::forge::servers::host_of(&form.picker.server).to_owned();
    let mut chosen = None;
    let combo = craft_ui::ComboBox::from_id_salt("forge-open-server").width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
        for s in &servers {
            if ui.selectable_label(*s == form.picker.server, texcraft_engine::forge::servers::host_of(s)).clicked() {
                chosen = Some(s.clone());
            }
        }
    });
    let _ = combo.response.labelled_by(label.id);
    if let Some(s) = chosen
        && s != form.picker.server
    {
        form.picker.server = s;
        form.picker.projects = Projects::Idle;
        form.picker.branches = Branches::None;
        typed(&mut form.picker, now - PAUSE);
    }
}

/// The project field and its suggestions. Returns the field's response.
pub(crate) fn project_field(ui: &mut egui::Ui, t: &Tokens, form: &mut OpenForm) -> egui::Response {
    let now = ui.input(|i| i.time);
    ui.scope(|ui| server_choice(ui, t, form, now));
    let field_id = egui::Id::unique("texcraft-forge-project-address");
    let count = form.picker.items().len();
    let shown_before = form.picker.open && !matches!(form.picker.projects, Projects::Idle);
    // The keys the list takes while the field has the keyboard, before the field sees them.
    if ui.memory(|m| m.has_focus(field_id)) {
        let (down, up) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp)));
        if down && !form.picker.open {
            form.picker.open = true;
            if matches!(form.picker.projects, Projects::Idle) {
                form.picker.due = Some(now - PAUSE);
            }
        } else if count > 0 && (down || up) {
            form.picker.selected = Some(match (form.picker.selected, down) {
                (None, true) => 0,
                (None, false) => count - 1,
                (Some(i), true) => (i + 1) % count,
                (Some(i), false) => (i + count - 1) % count,
            });
            form.picker.scroll = true;
        }
        if form.picker.open
            && form.picker.selected.is_some()
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
            && let Some(s) = form.picker.selected.and_then(|i| form.picker.items().get(i)).cloned()
        {
            choose(form, &s);
        }
        if shown_before && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            form.picker.open = false;
            form.picker.selected = None;
        }
    }
    if form.picker.left && ui.input(|i| !i.pointer.any_down() && !i.pointer.any_released()) {
        form.picker.left = false;
        if Address::parse_as(&qualified(&form.address, &form.picker.server, form.service), Some(form.service)).is_ok() {
            form.picker.branches_due = true;
        }
    }
    ui.add_space(t.unit * 2.0);
    let label = ui.label(RichText::new(tl!("Project address")).color(t.text_muted));
    let example = match form.service {
        Service::GitLab => tl!("Type to find a project, or paste its address"),
        Service::GitHub => tl!("Type to find a repository, or paste its address"),
    };
    // While the list is open the field keeps the keyboard on Escape (which closes the list) and on
    // Up and Down (which move in it); otherwise Escape is the dialog's.
    let list_shown = form.picker.open && !matches!(form.picker.projects, Projects::Idle);
    let filter = egui::EventFilter { escape: list_shown, vertical_arrows: true, horizontal_arrows: true, tab: false };
    let edit = egui::TextEdit::singleline(&mut form.address)
        .id(field_id)
        .hint_text(example)
        .desired_width(f32::INFINITY)
        .margin(egui::vec2(8.0, 6.0))
        .event_filter(filter);
    let field = ui.add(edit).labelled_by(label.id);
    if field.changed() {
        typed(&mut form.picker, now);
        // A pasted address: its branches at once.
        if form.address.contains("://") && Address::parse_as(&form.address, Some(form.service)).is_ok() {
            form.picker.branches_due = true;
        }
    }
    if field.lost_focus() {
        form.picker.left = true;
    }
    let showing = form.picker.open && !matches!(form.picker.projects, Projects::Idle);
    let mut chosen: Option<Suggestion> = None;
    // A popup under the field, so that what follows does not move as suggestions come and go.
    let mut menu_open = showing;
    let summary_text = summary(&form.picker, form.service);
    let items = form.picker.items().to_vec();
    let (selected, scroll) = (form.picker.selected, form.picker.scroll);
    let truncated = matches!(form.picker.projects, Projects::Shown { truncated: true, .. });
    let utc = chrono::Utc::now();
    let mut active = None;
    let list_id = egui::Popup::default_response_id(&field);
    craft_ui::combo::Menu::new(ui, &field).height(260.0).open(&mut menu_open).show_ui_with_header(
        |ui| {
            let status = ui.label(RichText::new(&summary_text).font(t.small()).color(t.text_muted));
            crate::status::announce(ui, &status, &summary_text);
        },
        |ui, ()| {
            for (i, s) in items.iter().enumerate() {
                let on = selected == Some(i);
                let text = details(s, utc);
                let mut job = egui::text::LayoutJob::default();
                job.append(&s.path, 0.0, egui::TextFormat { font_id: t.strong(), color: t.text, ..Default::default() });
                job.append(&format!("\n{text}"), 0.0, egui::TextFormat { font_id: t.small(), color: t.text_muted, ..Default::default() });
                let row = ui
                    .push_id(list_id.with(i), |ui| ui.add(egui::Button::selectable(on, job).min_size(egui::vec2(ui.available_width(), 36.0))))
                    .inner;
                let name = format!("{}, {text}", s.path);
                row.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, true, on, &name));
                // egui says a selected row is toggled; a list box's option says it is selected.
                ui.ctx().accesskit_node_builder(row.id, |node| {
                    node.clear_toggled();
                    node.set_selected(on);
                });
                if on {
                    active = Some(row.id);
                    if scroll {
                        row.scroll_to_me(None);
                    }
                }
                if row.clicked() {
                    chosen = Some(s.clone());
                }
            }
            if truncated {
                let note = tl!("More projects than are listed: type more of the name to find the others.");
                ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
            }
        },
    );
    if showing {
        ui.ctx().accesskit_node_builder(list_id, |node| node.set_label(tl!("Projects")));
    }
    if showing && !menu_open && chosen.is_none() {
        // Closed by the popup: the field left for something else.
        form.picker.open = false;
        form.picker.selected = None;
    }
    ui.ctx().accesskit_node_builder(field.id, |node| {
        if showing {
            node.push_controlled(list_id.accesskit_id());
        }
        if let Some(row) = active {
            node.set_active_descendant(row.accesskit_id());
        }
    });
    form.picker.scroll = false;
    let expanded = showing;
    ui.ctx().accesskit_node_builder(field.id, |node| {
        node.set_role(egui::accesskit::Role::EditableComboBox);
        node.set_expanded(expanded);
    });
    if let Some(s) = chosen {
        choose(form, &s);
        field.request_focus();
    }
    field
}

/// What the line above the suggestions says, and the live region announces: how many there are
/// and which one is chosen, or why there are none.
fn summary(picker: &Picker, service: Service) -> String {
    match &picker.projects {
        Projects::Idle => String::new(),
        Projects::NeedsKey => match service {
            Service::GitLab => tl!("Sign in to see your projects, or use a personal access token.").to_owned(),
            Service::GitHub => tl!("Sign in to see your repositories, or use a personal access token.").to_owned(),
        },
        Projects::Failed(e) => e.clone(),
        Projects::Shown { items, .. } if items.is_empty() && picker.loading => tl!("Looking for projects…").to_owned(),
        Projects::Shown { items, .. } if items.is_empty() => tl!("No project matches: type its whole address to open it.").to_owned(),
        Projects::Shown { items, .. } => match picker.selected.and_then(|i| items.get(i).map(|s| (i, s))) {
            Some((i, s)) => tl!("{path}, {n} of {total}")
                .replace("{path}", &s.path)
                .replace("{n}", &(i + 1).to_string())
                .replace("{total}", &items.len().to_string()),
            None => trn(items.len(), "{n} suggestion: Down to choose it", "{n} suggestions: Down to choose one"),
        },
    }
}

/// A branch as the list says it: its name, and whether it is the default one or protected.
fn branch_text(b: &BranchRow) -> String {
    let mut text = b.name.clone();
    if b.default {
        text.push_str(&format!(" ({})", tl!("default")));
    }
    if b.protected {
        text.push_str(&format!(" ({})", tl!("protected")));
    }
    text
}

/// The branch: a drop-down of the project's branches once they are listed, else a field to type
/// it in, with why. Returns the field's response when it is the field.
pub(crate) fn branch_field(ui: &mut egui::Ui, t: &Tokens, form: &mut OpenForm) -> Option<egui::Response> {
    // In one scope, so that the widgets after it keep their ids whether it is a list or a field.
    ui.scope(|ui| branch_part(ui, t, form)).inner
}

fn branch_part(ui: &mut egui::Ui, t: &Tokens, form: &mut OpenForm) -> Option<egui::Response> {
    ui.add_space(t.unit * 2.0);
    let listed = match &form.picker.branches {
        Branches::Listed { items, truncated } if !form.picker.typing_branch => Some((items.clone(), *truncated)),
        _ => None,
    };
    let mut field = None;
    let control_height = 28.0;
    if let Some((items, truncated)) = &listed {
        let label = ui.label(RichText::new(tl!("Branch")).color(t.text_muted));
        let shown = items.iter().find(|b| b.name == form.branch).map_or_else(|| form.branch.clone(), branch_text);
        let mut chosen = None;
        let mut typing = false;
        let combo = ui.allocate_ui(egui::vec2(ui.available_width(), control_height), |ui| {
            craft_ui::ComboBox::from_id_salt("forge-open-branch").width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
                for b in items {
                    if ui.selectable_label(b.name == form.branch, branch_text(b)).clicked() {
                        chosen = Some(b.name.clone());
                    }
                }
                if ui.selectable_label(false, tl!("Another branch…")).clicked() {
                    typing = true;
                }
            })
        });
        let _ = combo.inner.response.labelled_by(label.id);
        if let Some(name) = chosen {
            form.branch = name;
        }
        if typing {
            form.picker.typing_branch = true;
        }
        let note = if *truncated { tl!("Only the first branches are listed: choose Another branch… to type one.") } else { "" };
        status_line(ui, t, note, None);
    } else {
        let label = ui.label(RichText::new(tl!("Branch (the default branch if left empty)")).color(t.text_muted));
        let hint = if matches!(form.picker.branches, Branches::Loading) { tl!("Listing the branches…") } else { "main" };
        let edit = egui::TextEdit::singleline(&mut form.branch).hint_text(hint).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
        field = Some(ui.add_sized(egui::vec2(ui.available_width(), control_height), edit).labelled_by(label.id));
        let note = match &form.picker.branches {
            Branches::Failed(e) => {
                format!(
                    "{} {}",
                    tl!("The branches could not be listed: type the branch, or leave it empty for the default one."),
                    crate::i18n::msg(e)
                )
            }
            Branches::Loading => tl!("Listing the branches…").to_owned(),
            _ => String::new(),
        };
        let back = matches!(form.picker.branches, Branches::Listed { .. }).then_some(tl!("Choose from the List"));
        if status_line(ui, t, &note, back) {
            form.picker.typing_branch = false;
        }
    }
    field
}

/// One line under the branch, always there so that nothing below moves when it changes: `text`
/// cut to the line (whole in its tooltip and its accessible name, and announced), and a button
/// `back` at its end. Whether the button was clicked.
fn status_line(ui: &mut egui::Ui, t: &Tokens, text: &str, back: Option<&str>) -> bool {
    let mut clicked = false;
    let height = ui.text_style_height(&egui::TextStyle::Button) + 6.0;
    ui.horizontal(|ui| {
        ui.set_min_height(height);
        let reserve = if back.is_some() { 160.0 } else { 0.0 };
        if !text.is_empty() {
            ui.scope(|ui| {
                ui.set_max_width((ui.available_width() - reserve).max(40.0));
                let label = ui.add(egui::Label::new(RichText::new(text).font(t.small()).color(t.text_muted)).truncate());
                let label = label.on_hover_text(text);
                crate::status::announce(ui, &label, text);
            });
        }
        if let Some(back) = back {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| clicked = ui.small_button(back).clicked());
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::target;
    use texcraft_engine::forge::Service;

    #[test]
    fn a_host_typed_first_names_the_instance_and_the_rest_is_the_query() {
        let gitlab = "https://gitlab.com";
        assert_eq!(target("pap", gitlab, Service::GitLab), (gitlab.to_owned(), "pap".to_owned()));
        assert_eq!(target("git.example.org/lab/pa", gitlab, Service::GitLab), ("https://git.example.org".to_owned(), "lab/pa".to_owned()));
        // A page under a repository: the repository.
        assert_eq!(target("https://github.com/ada/paper/tree/main", "x", Service::GitHub), ("https://github.com".to_owned(), "ada/paper".to_owned()));
        assert_eq!(target("https://git.example.org/lab", gitlab, Service::GitLab), ("https://git.example.org".to_owned(), "lab".to_owned()));
    }
}
