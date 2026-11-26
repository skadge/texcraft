//! Save Project to GitHub… and Save Project to GitLab… (docs/forges.md, step 5): a dialog that
//! makes a new repository for the project, on the service's public server or one added, for the
//! account or one of its organisations (GitHub) or groups (GitLab), private by default; then, on
//! the desktop, sets it as the folder's `origin` and pushes the whole history to it, and in the
//! browser commits the files to it and links the project to it. A name taken is said in the
//! dialog, which stays open; once saved, the dialog links to the repository's page.
//!
//! Everything goes through `forge.owners` and `forge.create_repository`, in the background.

use egui::RichText;
use serde_json::{Value, json};
use texcraft_engine::forge::Service;

use crate::App;
use crate::dialogs::{Dialog, Outcome, actions, buttons, title};
use crate::forge::{SignInAction, field, say, sign_in_or_token, sign_in_view, token_hint, token_note};
use crate::theme::Tokens;

/// The Save Project to GitHub or GitLab dialog's fields and state.
pub(crate) struct SaveForm {
    pub service: Service,
    /// The server: the service's public one, or one added.
    pub instance: String,
    /// Where the account can make repositories, the account first; empty until looked up.
    pub owners: Vec<String>,
    /// The owner chosen (an organisation or group); none for the account itself.
    pub owner: Option<String>,
    /// The server and key the owners were last asked for.
    owners_for: Option<String>,
    /// Why the owners could not be listed.
    owners_error: Option<String>,
    pub name: String,
    pub private: bool,
    pub description: String,
    pub token: String,
    pub use_token: bool,
    /// The repository is being made (and the history pushed).
    pub running: bool,
    /// Why the last try did not work: the dialog stays open with it.
    pub error: Option<String>,
    /// The repository made.
    pub done: Option<Saved>,
    pub focus: bool,
}

/// A repository made: its path and page, and how the push went (the desktop).
pub(crate) struct Saved {
    pub project: String,
    pub web_url: String,
    /// Why the push did not work, when it did not (the repository is made and set as origin).
    pub push_error: Option<String>,
}

/// Opens the dialog for `service`, with a name made from the project's.
pub(crate) fn ask(app: &mut App, service: Service) {
    let instance = texcraft_engine::forge::oauth::instance(service).to_owned();
    let use_token = app.session.forge_sign_in_unavailable_at(service, &instance).is_some();
    app.view.dialog = Some(Dialog::ForgeSave(Box::new(SaveForm {
        service,
        instance,
        owners: Vec::new(),
        owner: None,
        owners_for: None,
        owners_error: None,
        name: texcraft_engine::forge::create::suggest_name(&app.project_name()),
        private: true,
        description: String::new(),
        token: String::new(),
        use_token,
        running: false,
        error: None,
        done: None,
        focus: true,
    })));
}

/// The servers of `service` the dialog offers: its public one, then those added.
fn servers(app: &App, service: Service) -> Vec<String> {
    let mut out = vec![texcraft_engine::forge::oauth::instance(service).to_owned()];
    out.extend(app.session.forge_servers().into_iter().filter(|s| s.service == service).map(|s| s.instance));
    out
}

/// Asks where the account can make repositories, once per server and key, when it can be asked.
fn look_up_owners(app: &mut App) {
    let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_ref() else { return };
    let token = if form.use_token { form.token.trim().to_owned() } else { String::new() };
    let reaches = !token.is_empty() || (!form.use_token && app.session.forge_reaches(&form.instance, form.service));
    let key = format!("{}\n{}\n{}", form.instance, form.use_token, token.len());
    if !reaches || form.owners_for.as_ref() == Some(&key) || app.session.git_busy().is_some() || form.running {
        return;
    }
    let mut params = json!({"service": form.service.id(), "instance": form.instance, "background": true});
    if !token.is_empty() {
        params["token"] = json!(token);
    }
    let started = app.session.execute("forge.owners", &params);
    if let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_mut() {
        form.owners_for = Some(key);
        form.owners_error = started.err();
    }
}

/// The owners listed: the account first, chosen unless an owner listed was.
pub(crate) fn owners_listed(app: &mut App, done: &Value) {
    let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_mut() else { return };
    if done["instance"].as_str() != Some(form.instance.as_str()) {
        return;
    }
    form.owners = done["owners"].as_array().into_iter().flatten().filter_map(|o| o["path"].as_str().map(str::to_owned)).collect();
    form.owners_error = None;
    if form.owner.as_ref().is_some_and(|o| !form.owners.contains(o)) {
        form.owner = None;
    }
}

/// The repository made: the dialog says so, with its page.
pub(crate) fn saved(app: &mut App, done: &Value) {
    let project = done["project"].as_str().unwrap_or_default().to_owned();
    let web_url = done["web_url"].as_str().unwrap_or_default().to_owned();
    let push_error = done["push_error"].as_str().map(str::to_owned);
    let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
    let said = say(service, tl!("Saved the project to {project} on GitLab."), tl!("Saved the project to {project} on GitHub."))
        .replace("{project}", &project);
    app.view.status = Some(match &push_error {
        Some(e) => (format!("{said} {}", tl!("The push did not work: {why}").replace("{why}", &crate::i18n::msg(e))), true),
        None => (said, false),
    });
    app.remember_project();
    match app.view.dialog.as_mut() {
        Some(Dialog::ForgeSave(form)) => {
            form.running = false;
            form.error = None;
            form.done = Some(Saved { project, web_url, push_error });
            form.focus = true;
        }
        _ => {
            app.view.dialog = None;
        }
    }
}

/// An operation of this dialog failed: said in it, which stays open. Whether it was one.
pub(crate) fn failed(app: &mut App, was: Option<&str>, e: &str) -> bool {
    let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_mut() else { return false };
    match was {
        Some("create_repository") => {
            form.running = false;
            form.error = Some(e.to_owned());
            true
        }
        Some("owners") => {
            form.owners_error = Some(e.to_owned());
            true
        }
        _ => false,
    }
}

/// What the status line says while the repository is made.
pub(crate) fn busy_text(app: &App, op: &str) -> Option<&'static str> {
    let service = match app.view.dialog.as_ref() {
        Some(Dialog::ForgeSave(form)) => form.service,
        _ => Service::GitLab,
    };
    Some(match op {
        "create_repository" => say(service, tl!("Saving the project to GitLab…"), tl!("Saving the project to GitHub…")),
        "git_push" => tl!("Pushing…"),
        "git_pull" => tl!("Pulling…"),
        "sync" => tl!("Syncing…"),
        _ => return None,
    })
}

/// The dialog, while it is open.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    look_up_owners(app);
    let t = app.tokens;
    let browser = app.session.library().is_some();
    let (service, instance) = match &app.view.dialog {
        Some(Dialog::ForgeSave(form)) => (form.service, form.instance.clone()),
        _ => return,
    };
    let view = sign_in_view(app, service, &instance);
    let servers = servers(app, service);
    let mut outcome = Outcome::Open;
    let mut action = None;
    let mut open_page = None;
    let modal = crate::sheet::show(ctx, &t, "texcraft-forge-save", 460.0, crate::sheet::Leave::Close, |ui| {
        if let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_mut() {
            outcome = match &form.done {
                Some(_) => done_form(ui, &t, form, &mut open_page),
                None => save_form(ui, &t, form, browser, &view, &servers, &mut action),
            };
        }
    });
    if modal.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    if let Some(url) = open_page {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    if let Some(action) = action {
        crate::forge::act(app, action);
    }
    match outcome {
        Outcome::Open | Outcome::Discard => {}
        // The operation goes on in the background: its outcome is said on the status line.
        Outcome::Cancel => app.view.dialog = None,
        Outcome::Run => start(app),
    }
}

/// Starts making the repository with the form's choices.
fn start(app: &mut App) {
    let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_ref() else { return };
    let mut params = json!({
        "service": form.service.id(),
        "instance": form.instance,
        "name": form.name.trim(),
        "private": form.private,
        "background": true,
    });
    if let Some(owner) = &form.owner {
        params["owner"] = json!(owner);
    }
    if !form.description.trim().is_empty() {
        params["description"] = json!(form.description.trim());
    }
    if form.use_token {
        params["token"] = json!(form.token.trim());
    }
    let started = app.session.execute("forge.create_repository", &params);
    if let Some(Dialog::ForgeSave(form)) = app.view.dialog.as_mut() {
        match started {
            Ok(_) => {
                form.running = true;
                form.error = None;
            }
            Err(e) => form.error = Some(e),
        }
    }
}

fn save_form(
    ui: &mut egui::Ui,
    t: &Tokens,
    form: &mut SaveForm,
    browser: bool,
    view: &crate::forge::SignInView,
    servers: &[String],
    action: &mut Option<SignInAction>,
) -> Outcome {
    title(ui, t, say(form.service, tl!("Save Project to GitLab"), tl!("Save Project to GitHub")));
    let what = if browser {
        say(
            form.service,
            tl!(
                "TeXCraft makes a new project on GitLab and commits this project's files to it, as one commit; the versions kept in this browser stay here. The project then commits to it."
            ),
            tl!(
                "TeXCraft makes a new repository on GitHub and commits this project's files to it, as one commit; the versions kept in this browser stay here. The project then commits to it."
            ),
        )
    } else {
        say(
            form.service,
            tl!(
                "TeXCraft makes a new project on GitLab, sets it as this folder's remote (origin), and pushes the project's whole history to it. Changes not committed yet are committed first."
            ),
            tl!(
                "TeXCraft makes a new repository on GitHub, sets it as this folder's remote (origin), and pushes the project's whole history to it. Changes not committed yet are committed first."
            ),
        )
    };
    ui.label(RichText::new(what).font(t.small()).color(t.text_muted));
    if servers.len() > 1 {
        ui.add_space(t.unit * 2.0);
        let label = ui.label(RichText::new(tl!("Server")).color(t.text_muted));
        let shown = texcraft_engine::forge::servers::host_of(&form.instance).to_owned();
        let mut chosen = None;
        let combo = craft_ui::ComboBox::from_id_salt("forge-save-server").width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
            for s in servers {
                if ui.selectable_label(*s == form.instance, texcraft_engine::forge::servers::host_of(s)).clicked() {
                    chosen = Some(s.clone());
                }
            }
        });
        let _ = combo.response.labelled_by(label.id);
        if let Some(s) = chosen.filter(|s| *s != form.instance) {
            form.instance = s;
            form.owners.clear();
            form.owner = None;
        }
    }
    owner_choice(ui, t, form);
    let name = field(ui, t, tl!("Name"), &mut form.name, "paper", false);
    if std::mem::take(&mut form.focus) {
        name.request_focus();
    }
    let mut responses = vec![name];
    let bad_name = texcraft_engine::forge::create::check_name(form.name.trim()).err().map(|e| e.to_string());
    if let Some(why) = &bad_name
        && !form.name.trim().is_empty()
    {
        ui.label(RichText::new(crate::i18n::msg(why)).font(t.small()).color(t.error));
    }
    ui.add_space(t.unit * 2.0);
    let group = tl!("Who can see it");
    ui.horizontal(|ui| {
        ui.label(RichText::new(group).color(t.text_muted));
        for (private, text) in [(true, tl!("Private")), (false, tl!("Public"))] {
            let on = form.private == private;
            let r = ui.radio(on, text);
            let name = format!("{text}: {group}");
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, &name));
            if r.clicked() {
                form.private = private;
            }
        }
    });
    let note = if form.private { tl!("Only you, and those you give access to, see it.") } else { tl!("Anyone can see it, and copy it.") };
    ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
    responses.push(field(ui, t, tl!("Description (optional)"), &mut form.description, "", false));
    let signed_in = sign_in_or_token(ui, t, view, &mut form.use_token, true, action);
    // Signed in to GitLab to read only: making a project needs permission to write.
    let mut needs_write = signed_in && view.user.as_ref().is_some_and(|u| !u.2);
    if needs_write {
        ui.label(RichText::new(tl!(
            "You signed in to GitLab to read only. To make a project, GitLab asks you to let TeXCraft write too (the api scope)."
        )));
        if ui.button(RichText::new(tl!("Allow Commits on GitLab")).font(t.strong())).clicked() {
            *action = Some(SignInAction::SignIn { service: form.service, instance: form.instance.clone(), write: true });
        }
        needs_write = !form.use_token;
    }
    if form.use_token {
        let token = field(ui, t, tl!("Personal access token"), &mut form.token, token_hint(form.service), true);
        if token.changed() {
            form.owners_for = None;
        }
        responses.push(token);
        ui.add_space(t.unit);
        let kept = if browser {
            tl!("Kept while this page is open; a token kept for this server (Settings › Accounts) is replaced by it.")
        } else {
            tl!("Kept while TeXCraft runs; a token kept for this server (Settings › Accounts) is replaced by it.")
        };
        token_note(ui, t, kept, form.service);
    }
    // Why the last try did not work, or what is going on: a live region says it as it changes.
    let (text, color) = match (&form.error, form.running) {
        (Some(e), _) => (crate::i18n::msg(e), t.error),
        (None, true) => (
            if browser { tl!("Making the repository and committing the files…") } else { tl!("Making the repository and pushing the history…") }
                .to_owned(),
            t.text_muted,
        ),
        (None, false) => (String::new(), t.text_muted),
    };
    if !text.is_empty() {
        ui.add_space(t.unit * 2.0);
        let said = ui.add(egui::Label::new(RichText::new(&text).color(color)).wrap());
        crate::status::announce(ui, &said, &text);
    }
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let has_key = signed_in || (form.use_token && !form.token.trim().is_empty());
    let ready = bad_name.is_none() && has_key && !needs_write && !form.running;
    let label = say(form.service, tl!("Save to GitLab"), tl!("Save to GitHub"));
    match buttons(ui, t, label, false, ready) {
        Outcome::Open if enter && ready => Outcome::Run,
        o => o,
    }
}

/// The owner drop-down: the account, then its organisations or groups, once listed.
fn owner_choice(ui: &mut egui::Ui, t: &Tokens, form: &mut SaveForm) {
    ui.add_space(t.unit * 2.0);
    let label =
        ui.label(RichText::new(say(form.service, tl!("Owner (you, or a group)"), tl!("Owner (you, or an organisation)"))).color(t.text_muted));
    let Some(account) = form.owners.first().cloned() else {
        let text = match &form.owners_error {
            Some(e) => crate::i18n::msg(e),
            None => tl!("You: sign in, or give a token, to choose another owner.").to_owned(),
        };
        let color = if form.owners_error.is_some() { t.error } else { t.text_muted };
        ui.label(RichText::new(text).font(t.small()).color(color));
        return;
    };
    let shown = form.owner.clone().unwrap_or_else(|| account.clone());
    let mut chosen = None;
    let combo = craft_ui::ComboBox::from_id_salt("forge-save-owner").width(ui.available_width()).selected_text(&shown).show_ui(ui, |ui| {
        for (i, o) in form.owners.iter().enumerate() {
            if ui.selectable_label(*o == shown, o).clicked() {
                chosen = Some(if i == 0 { None } else { Some(o.clone()) });
            }
        }
    });
    let _ = combo.response.labelled_by(label.id);
    if let Some(c) = chosen {
        form.owner = c;
    }
}

fn done_form(ui: &mut egui::Ui, t: &Tokens, form: &mut SaveForm, open_page: &mut Option<String>) -> Outcome {
    let Some(saved) = &form.done else { return Outcome::Cancel };
    title(ui, t, say(form.service, tl!("Saved to GitLab"), tl!("Saved to GitHub")));
    let said = match &saved.push_error {
        None => tl!("The project is in {project}: its remote, origin, from now on. Push and Pull are in the History panel.")
            .replace("{project}", &saved.project),
        Some(e) => {
            tl!("{project} was made and set as the folder's remote (origin), but the push did not work: {why} Push again from the History panel.")
                .replace("{project}", &saved.project)
                .replace("{why}", &crate::i18n::msg(e))
        }
    };
    let color = if saved.push_error.is_some() { t.error } else { t.text };
    let label = ui.add(egui::Label::new(RichText::new(&said).color(color)).wrap());
    crate::status::announce(ui, &label, &said);
    ui.add_space(t.unit * 2.0);
    let open = ui.button(tl!("Open in Browser"));
    if std::mem::take(&mut form.focus) {
        open.request_focus();
    }
    if open.clicked() {
        *open_page = Some(saved.web_url.clone());
    }
    let mut outcome = Outcome::Open;
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        if ui.add(egui::Button::new(tl!("Close")).min_size(egui::vec2(80.0, 30.0))).clicked() {
            outcome = Outcome::Cancel;
        }
    });
    outcome
}
