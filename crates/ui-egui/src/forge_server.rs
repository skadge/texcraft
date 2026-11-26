//! Adding a self-managed server (docs/forges.md, "Self-managed servers"): the Add a GitLab Server
//! dialog (and its GitHub Enterprise Server twin) asks for the server's address, checks it
//! (`forge.check_server`), shows what to register there with copy buttons, takes the application's
//! id and adds the server (`forge.add_server`). A link that proposes a server opens the same kind
//! of dialog, which asks before anything is added.

use egui::RichText;
use serde_json::json;
use texcraft_engine::forge::Service;
use texcraft_engine::forge::servers::{self, Registration, Server};

use crate::App;
use crate::dialogs::{Dialog, Outcome, buttons, title};
use crate::theme::Tokens;

/// Where the dialog goes back to when it closes.
pub(crate) enum Back {
    /// Settings, on Accounts.
    Settings,
    /// Import a Project, as it was (its GitLab Server page).
    Import(Box<crate::import::ImportForm>),
    /// Nowhere.
    Close,
}

/// Where the Add a Server dialog is.
pub(crate) enum Step {
    /// Asking for the address.
    Address,
    /// The server is being checked.
    Checking,
    /// The server answered: what to register on it, and its application's id.
    Register { instance: String, version: Option<String>, registration: Registration },
}

/// The Add a Server dialog.
pub(crate) struct ServerForm {
    pub service: Service,
    pub address: String,
    pub client_id: String,
    pub step: Step,
    /// Why the last check or add did not work.
    pub error: Option<String>,
    pub back: Back,
    pub focus: bool,
}

/// A server a link proposes, waiting for the user's yes.
pub(crate) struct ProposalForm {
    pub server: Server,
    /// The project the link points at, opened once the server is added.
    pub project: Option<String>,
    /// The application id kept for this server before, which the link's replaces.
    pub replaces: Option<String>,
}

/// Opens the Add a Server dialog for `service`, its address filled with `address`.
pub(crate) fn ask_add(app: &mut App, service: Service, address: &str, back: Back) {
    let address = servers::parse_instance(address).unwrap_or_else(|_| address.to_owned());
    app.view.dialog =
        Some(Dialog::ForgeServer(ServerForm { service, address, client_id: String::new(), step: Step::Address, error: None, back, focus: true }));
}

/// Whether the dialog waits for a check.
pub(crate) fn checking(app: &App) -> bool {
    matches!(&app.view.dialog, Some(Dialog::ForgeServer(ServerForm { step: Step::Checking, .. })))
}

/// The check ended: what to register, or why the server cannot be added.
pub(crate) fn checked(app: &mut App, outcome: Result<serde_json::Value, String>) {
    let registration = match &outcome {
        Ok(done) => {
            let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
            let instance = done["instance"].as_str().unwrap_or_default().to_owned();
            Some((instance.clone(), done["version"].as_str().map(str::to_owned), app.session.forge_registration(service, &instance)))
        }
        Err(_) => None,
    };
    let Some(Dialog::ForgeServer(form)) = app.view.dialog.as_mut() else { return };
    match (registration, outcome) {
        (Some((instance, version, registration)), _) => {
            form.step = Step::Register { instance, version, registration };
            form.error = None;
            form.focus = true;
        }
        (None, Err(e)) => {
            form.step = Step::Address;
            form.error = Some(e);
            form.focus = true;
        }
        (None, Ok(_)) => {}
    }
}

/// Reads a link's query (`?gitlab=…&client_id=…`): when it proposes a server, the dialog that asks
/// before adding it (or, when it is added already, the Open dialog on its project). Whether the
/// query proposed one.
pub(crate) fn propose(app: &mut App, query: &str) -> bool {
    let pairs = texcraft_engine::forge::oauth::parse_query(query);
    let Some(proposal) = servers::proposal(&pairs) else { return false };
    let proposal = match proposal {
        Ok(p) => p,
        Err(e) => {
            let why = tl!("This link proposes a server TeXCraft cannot add: {error}").replace("{error}", &crate::i18n::msg(&e.to_string()));
            app.view.status = Some((why, true));
            return true;
        }
    };
    let server = &proposal.server;
    let known = app.session.forge_servers().into_iter().find(|s| s.instance == server.instance);
    match known {
        Some(k) if k.client_id == server.client_id && k.service == server.service => open_project(app, server.service, proposal.project),
        Some(k) if k.provided => {
            let why = tl!("This link proposes another application for {host}, which your organisation provides: its own is kept.")
                .replace("{host}", server.host());
            app.view.status = Some((why, true));
        }
        known => {
            let replaces = known.map(|k| k.client_id);
            app.view.dialog = Some(Dialog::ForgeProposal(ProposalForm { server: proposal.server, project: proposal.project, replaces }));
        }
    }
    true
}

/// Import a Project on `project`, when there is one: at the GitLab Server card for a GitLab
/// server's.
fn open_project(app: &mut App, service: Service, project: Option<String>) {
    if let Some(project) = project {
        match service {
            Service::GitLab => crate::import::open_at(app, Some(crate::import::Source::GitLabServer), &project),
            Service::GitHub => crate::forge::ask_open_at(app, service, &project),
        }
    }
}

/// The open Add a Server or proposal dialog, if any.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let mut outcome = Outcome::Open;
    let mut open_page = None;
    let modal = crate::sheet::show(ctx, &t, "texcraft-forge-server", 520.0, crate::sheet::Leave::Close, |ui| match app.view.dialog.as_mut() {
        Some(Dialog::ForgeServer(form)) => outcome = server_form(ui, &t, form, &mut open_page),
        Some(Dialog::ForgeProposal(form)) => outcome = proposal_form(ui, &t, form),
        _ => {}
    });
    if let Some(url) = open_page {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    if modal.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Open | Outcome::Discard => {}
        Outcome::Cancel => match app.view.dialog.take() {
            Some(Dialog::ForgeServer(form)) => go_back(app, form.back),
            _ => app.view.dialog = None,
        },
        Outcome::Run => match app.view.dialog.take() {
            Some(Dialog::ForgeServer(form)) => run_server_form(app, form),
            Some(Dialog::ForgeProposal(form)) => {
                let params = json!({"address": form.server.instance, "service": form.server.service.id(), "client_id": form.server.client_id});
                if app.run("forge.add_server", params).is_some() {
                    app.view.status = Some((added(form.server.host()), false));
                    open_project(app, form.server.service, form.project);
                }
            }
            other => app.view.dialog = other,
        },
    }
}

/// What the status line says once a server is added.
fn added(host: &str) -> String {
    tl!("Added {host}: sign in to it with its Sign In button.").replace("{host}", host)
}

/// The dialog's primary action: check the address, or add the server.
fn run_server_form(app: &mut App, mut form: ServerForm) {
    match &form.step {
        Step::Address | Step::Checking => {
            let params = json!({"address": form.address.trim(), "service": form.service.id(), "background": true});
            match app.session.execute("forge.check_server", &params) {
                Ok(_) => {
                    form.step = Step::Checking;
                    form.error = None;
                }
                Err(e) => form.error = Some(e),
            }
            app.view.dialog = Some(Dialog::ForgeServer(form));
        }
        Step::Register { instance, .. } => {
            let params = json!({"address": instance, "service": form.service.id(), "client_id": form.client_id.trim()});
            match app.session.execute("forge.add_server", &params) {
                Ok(_) => {
                    app.view.status = Some((added(servers::host_of(instance)), false));
                    go_back(app, form.back);
                }
                Err(e) => {
                    form.error = Some(e);
                    app.view.dialog = Some(Dialog::ForgeServer(form));
                }
            }
        }
    }
}

fn go_back(app: &mut App, back: Back) {
    app.view.dialog = match back {
        Back::Settings => Some(Dialog::Settings),
        Back::Import(form) => {
            crate::import::back_from_server(app, form);
            return;
        }
        Back::Close => None,
    };
}

/// A one-line field named by the label above it (AGENTS.md rule 21).
fn field(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, hint: &str) -> egui::Response {
    ui.add_space(t.unit * 2.0);
    let label = ui.label(RichText::new(label).color(t.text_muted));
    let edit = egui::TextEdit::singleline(value).hint_text(hint).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
    ui.add(edit).labelled_by(label.id)
}

/// A value to type on the server: its name, the value in code type, and a Copy button named after
/// it (AGENTS.md rule 21).
fn copy_row(ui: &mut egui::Ui, t: &Tokens, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).color(t.text_muted));
        let shown = ui.add(egui::Label::new(RichText::new(value).font(t.code_font()).color(t.text)).selectable(true));
        shown.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{name} {value}")));
        let r = ui.button(tl!("Copy"));
        let label = format!("{}: {name}", tl!("Copy"));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        if r.clicked() {
            ui.ctx().copy_text(value.to_owned());
        }
    });
}

fn server_form(ui: &mut egui::Ui, t: &Tokens, form: &mut ServerForm, open_page: &mut Option<String>) -> Outcome {
    let gitlab = form.service == Service::GitLab;
    title(ui, t, if gitlab { tl!("Add a GitLab Server") } else { tl!("Add a GitHub Enterprise Server") });
    let error = |ui: &mut egui::Ui, error: &Option<String>| {
        if let Some(e) = error {
            ui.add_space(t.unit * 2.0);
            ui.label(RichText::new(crate::i18n::msg(e)).color(t.error));
        }
    };
    match &form.step {
        Step::Address | Step::Checking => {
            let intro = if gitlab {
                tl!(
                    "Sign in to your organisation's GitLab as to gitlab.com: TeXCraft checks the server, then says what to register there, once (you, or its administrator for everyone)."
                )
            } else {
                tl!(
                    "Sign in to your organisation's GitHub Enterprise Server as to github.com: TeXCraft checks the server, then says what to register there, once."
                )
            };
            ui.label(intro);
            let checking = matches!(form.step, Step::Checking);
            let address = ui.add_enabled_ui(!checking, |ui| field(ui, t, tl!("Server address"), &mut form.address, "https://git.example.org")).inner;
            if std::mem::take(&mut form.focus) {
                address.request_focus();
            }
            if checking {
                ui.add_space(t.unit * 2.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Checking the server…"));
                });
            }
            error(ui, &form.error);
            let ready = !checking && !form.address.trim().is_empty();
            let enter = address.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            match buttons(ui, t, tl!("Check Server"), false, ready) {
                Outcome::Open if enter && ready => Outcome::Run,
                o => o,
            }
        }
        Step::Register { instance, version, registration } => {
            let host = servers::host_of(instance);
            let found = match (gitlab, version) {
                (true, Some(v)) => tl!("{host} is a GitLab server (version {version}).").replace("{version}", v),
                (true, None) => tl!("{host} is a GitLab server.").to_owned(),
                (false, Some(v)) => tl!("{host} is a GitHub Enterprise Server (version {version}).").replace("{version}", v),
                (false, None) => tl!("{host} is a GitHub Enterprise Server.").to_owned(),
            };
            ui.label(found.replace("{host}", host));
            ui.add_space(t.unit * 2.0);
            if gitlab {
                ui.label(tl!(
                    "Register TeXCraft there as an application: under your avatar, Edit profile › Applications, for yourself; or Admin › Applications, for everyone on the server. Give it:"
                ));
            } else {
                ui.label(tl!(
                    "Register TeXCraft there as an OAuth app: Settings › Developer settings › OAuth Apps › New OAuth App, for yourself; or in an organisation's settings, for its members. Give it:"
                ));
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(tl!("Open the Applications Page")).clicked() {
                    *open_page = Some(registration.page.clone());
                }
                if let Some(admin) = &registration.admin_page
                    && ui.button(tl!("Open the Admin Applications Page")).clicked()
                {
                    *open_page = Some(admin.clone());
                }
            });
            ui.add_space(t.unit);
            copy_row(ui, t, tl!("Name"), registration.name);
            if let Some(homepage) = registration.homepage {
                copy_row(ui, t, tl!("Homepage URL"), homepage);
            }
            let redirect_name = if gitlab { tl!("Redirect URI") } else { tl!("Authorization callback URL") };
            for (i, uri) in registration.redirect_uris.iter().enumerate() {
                let name = if registration.redirect_uris.len() > 1 { format!("{redirect_name} {}", i + 1) } else { redirect_name.to_owned() };
                copy_row(ui, t, &name, uri);
            }
            if let Some(scopes) = registration.scopes {
                copy_row(ui, t, tl!("Scopes"), scopes);
                let note = if registration.redirect_uris.len() > 1 {
                    tl!(
                        "Put each redirect URI on a line of its own. Leave Confidential unticked: TeXCraft keeps no secret. Tick the scopes api, read_api and read_user."
                    )
                } else {
                    tl!("Leave Confidential unticked: TeXCraft keeps no secret. Tick the scopes api, read_api and read_user.")
                };
                ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
            } else {
                let note = tl!("Then tick Enable Device Flow in the app's settings: TeXCraft signs in with a code to enter on the server's page.");
                ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
            }
            let id_label = if gitlab { tl!("Application ID") } else { tl!("Client ID") };
            let id = field(ui, t, id_label, &mut form.client_id, "");
            if std::mem::take(&mut form.focus) {
                id.request_focus();
            }
            ui.label(
                RichText::new(tl!("The ID is not a secret: it is kept in the preferences. Sign-ins stay in the keychain."))
                    .font(t.small())
                    .color(t.text_muted),
            );
            error(ui, &form.error);
            let ready = !form.client_id.trim().is_empty();
            let enter = id.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            match buttons(ui, t, tl!("Add Server"), false, ready) {
                Outcome::Open if enter && ready => Outcome::Run,
                o => o,
            }
        }
    }
}

fn proposal_form(ui: &mut egui::Ui, t: &Tokens, form: &ProposalForm) -> Outcome {
    title(ui, t, tl!("Add a Server from a Link?"));
    let server = &form.server;
    let text = match server.service {
        Service::GitLab => {
            tl!("This link proposes to add the GitLab server {host}, with TeXCraft's application there ({id}), so that you can sign in to it.")
        }
        Service::GitHub => {
            tl!(
                "This link proposes to add the GitHub Enterprise Server {host}, with TeXCraft's application there ({id}), so that you can sign in to it."
            )
        }
    };
    ui.label(text.replace("{host}", server.host()).replace("{id}", &server.client_id));
    if let Some(old) = &form.replaces {
        ui.label(tl!("It replaces the application kept for this server until now ({id}).").replace("{id}", old));
    }
    ui.add_space(t.unit * 2.0);
    ui.label(
        RichText::new(tl!(
            "Add it only if you trust where the link comes from: signing in sends you to that server's page, and TeXCraft to its API."
        ))
        .font(t.small())
        .color(t.text_muted),
    );
    buttons(ui, t, tl!("Add Server"), false, true)
}
