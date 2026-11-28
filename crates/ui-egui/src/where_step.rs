//! New Project's second step, "Where to Keep It" (docs/design/new-project.md): after the template,
//! the place the project lives, as a row of Import's cards. This Computer (a folder) or This
//! Browser, then one card per connection that can make repositories (Settings › Accounts), then
//! Add a Connection…. A forge's card asks for the repository's name (checked on the forge as
//! typed), its owner and who can see it, and on the desktop the folder of its clone.
//!
//! Everything goes through commands: `accounts.list` (the cards), `forge.owners`,
//! `forge.name_available`, and `project.from_template` (with `forge`: the repository made first,
//! then the template written, committed and pushed to it), in the background; the footer shows
//! its phases, and a push that fails after the repository was made keeps the project and offers
//! Try Again.

use std::path::PathBuf;

use egui::{Key, Modifiers, RichText, vec2};
use serde_json::{Value, json};
use texcraft_engine::forge::Service;

use crate::App;
use crate::forge::{SignInAction, say};
use crate::import::{Line, Look, Tone, hint};
use crate::theme::Tokens;
use crate::{icons, widgets};

/// How long typing pauses before the name is looked up on the forge, in seconds.
const PAUSE: f64 = 0.5;
/// The height of a forge's fields on the desktop: the others take as much room.
const FIELDS_HEIGHT: f32 = 272.0;

/// Where the project lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    /// A folder on this computer, or the browser's library.
    Local,
    /// A new repository on this connection (its address).
    Forge(String),
}

impl Place {
    /// As the preferences keep it: `local`, or the connection's address.
    fn id(&self) -> &str {
        match self {
            Place::Local => "local",
            Place::Forge(i) => i,
        }
    }
}

/// The name's check on the forge.
#[derive(Default, PartialEq)]
enum Check {
    #[default]
    None,
    /// Asked; for this owner and name.
    Running(String),
    Free(String),
    Taken(String),
    Failed(String),
}

/// A repository made whose push did not work: the project is open, and kept.
pub(crate) struct Unsent {
    project: String,
    service: Service,
    folder: String,
    why: String,
}

/// The step's fields and state.
pub(crate) struct WhereStep {
    /// The template's id and title.
    pub template: String,
    title: String,
    place: Place,
    /// The chosen card gets the keyboard next frame.
    focus_card: bool,
    /// The repository's name, or the browser project's.
    name: String,
    name_edited: bool,
    /// The folder of the project (the desktop).
    folder: String,
    folder_edited: bool,
    owners: Vec<String>,
    owner: Option<String>,
    owners_for: Option<String>,
    owners_error: Option<String>,
    visibility: &'static str,
    check: Check,
    /// When the name or owner was last typed.
    due: Option<f64>,
    /// The project is being made.
    pub running: bool,
    error: Option<String>,
    unsent: Option<Unsent>,
    /// The connections there were when Add a Connection… opened: the new one is chosen after.
    adding: Option<Vec<String>>,
}

/// A connection that can make repositories, as its card shows it.
struct Connection {
    instance: String,
    service: Service,
    host: String,
    /// A self-managed server.
    server: bool,
    /// Signed in, or a token: it can be used.
    connected: bool,
    /// Signed in to GitLab to read only.
    read_only: bool,
    default: bool,
    line: String,
}

fn connections(app: &mut App) -> Vec<Connection> {
    let listed = app.session.execute("accounts.list", &json!({})).unwrap_or_default();
    listed["connections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["can_create"] == true)
        .filter_map(|r| {
            let service = Service::parse(r["service"].as_str()?)?;
            let state = r["state"].as_str().unwrap_or_default();
            let user = r["user"]["username"].as_str();
            let connected = r["connected"] == true && state != "expired";
            let mut line = match (state, user) {
                ("expired", _) => tl!("Sign in again").to_owned(),
                ("signed_in" | "token", Some(u)) => format!("@{u}"),
                ("token", None) => tl!("Token kept").to_owned(),
                ("signing_in", _) => tl!("Signing in…").to_owned(),
                _ => tl!("Not signed in").to_owned(),
            };
            let default = r["default"] == true;
            if default {
                line = format!("{line} · {}", tl!("Default"));
            }
            Some(Connection {
                instance: r["instance"].as_str()?.to_owned(),
                service,
                host: r["host"].as_str().unwrap_or_default().to_owned(),
                server: r["server"] == true,
                connected,
                read_only: r["can_commit"] == false,
                default,
                line,
            })
        })
        .collect()
}

/// The template's title made a name the forges and folders take: `ieee-journal-article`.
fn slug(title: &str) -> String {
    texcraft_engine::forge::create::suggest_name(title).to_lowercase()
}

fn in_browser(app: &App) -> bool {
    app.session.library().is_some() || cfg!(target_arch = "wasm32")
}

/// Opens the step for template `id`, at the default connection, else the last place, else here.
pub(crate) fn open(app: &mut App, id: String) {
    if let Some(mut step) = app.gallery.left_step.take().filter(|s| s.template == id) {
        step.focus_card = true;
        app.gallery.place_step = Some(step);
        app.gallery.open = true;
        app.gallery.error = None;
        return;
    }
    let title =
        texcraft_templates::Catalog::builtin().ok().and_then(|c| c.find(&id).ok().map(|t| crate::i18n::msg(&t.title))).unwrap_or_else(|| id.clone());
    let conns = connections(app);
    let default = conns.iter().find(|c| c.default).map(|c| Place::Forge(c.instance.clone()));
    let last = app.prefs.new_project_place.as_deref().and_then(|p| match p {
        "local" => Some(Place::Local),
        i => conns.iter().find(|c| c.instance == i).map(|c| Place::Forge(c.instance.clone())),
    });
    let place = default.or(last).unwrap_or(Place::Local);
    let name = if in_browser(app) && place == Place::Local { title.clone() } else { slug(&title) };
    app.gallery.place_step = Some(Box::new(WhereStep {
        template: id,
        title,
        place,
        focus_card: true,
        name,
        name_edited: false,
        folder: String::new(),
        folder_edited: false,
        owners: Vec::new(),
        owner: None,
        owners_for: None,
        owners_error: None,
        visibility: "private",
        check: Check::None,
        due: Some(f64::NEG_INFINITY),
        running: false,
        error: None,
        unsent: None,
        adding: None,
    }));
    app.gallery.open = true;
    app.gallery.error = None;
}

/// The folder new projects go in: the one Import's Change… chose, else the documents folder.
fn parent(app: &App) -> Option<PathBuf> {
    app.prefs.clone_folder.as_ref().map(PathBuf::from).or_else(|| app.documents.clone())
}

/// What the user did this frame.
enum Act {
    Choose(Place),
    Add,
    Back,
    Close,
    Create,
    ChangeFolder,
    Sign(SignInAction),
    TryAgain,
}

/// Draws the step in the gallery's dialog; the gallery handles closing.
pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let browser = in_browser(app);
    let conns = connections(app);
    let now = ctx.input(|i| i.time);
    let parent = parent(app);
    let phase = app.session.new_project_phase();
    let downloading = app.gallery.job.as_ref().map(|j| j.label.clone());
    let narrow = crate::sheet::narrow(ctx, &t);
    if let Some(step) = app.gallery.place_step.as_mut() {
        follow(step, &conns, browser, parent.as_ref());
    }
    look_up(app, &conns, now);
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
    let running = app.gallery.place_step.as_ref().is_some_and(|s| s.running) || downloading.is_some();
    let leave = crate::sheet::Leave::Back(tl!("Back to templates"));
    let mut acts = Vec::new();
    let shown = crate::sheet::show(ctx, &t, "texcraft-gallery", 866.0, leave, |ui| {
        let Some(step) = app.gallery.place_step.as_mut() else { return };
        let info = Info { browser, narrow, now, parent: parent.as_ref(), phase, downloading: downloading.as_deref(), running };
        draw(ui, &t, step, &conns, &info, &mut acts);
    });
    if shown.back {
        acts.push(Act::Back);
    }
    let popup = egui::Popup::is_any_open(ctx);
    if !running && (shown.close || (ctx.input(|i| i.key_pressed(Key::Escape)) && !popup)) {
        acts.push(Act::Close);
    }
    if !running && ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowLeft) || i.consume_key(Modifiers::COMMAND, Key::OpenBracket)) {
        acts.push(Act::Back);
    }
    for act in acts {
        perform(app, act, &conns);
    }
}

/// What the step shows that comes from the window, gathered before it is drawn.
struct Info<'a> {
    browser: bool,
    narrow: bool,
    now: f64,
    parent: Option<&'a PathBuf>,
    phase: Option<&'static str>,
    downloading: Option<&'a str>,
    running: bool,
}

/// The defaults that follow the place and the name until edited.
fn follow(step: &mut WhereStep, conns: &[Connection], browser: bool, parent: Option<&PathBuf>) {
    if let Place::Forge(i) = &step.place
        && !conns.iter().any(|c| c.instance == *i)
    {
        step.place = Place::Local;
    }
    if !step.name_edited {
        step.name = if browser && step.place == Place::Local { step.title.clone() } else { slug(&step.title) };
    }
    if !step.folder_edited {
        step.folder = match parent {
            Some(p) => {
                let name = if step.name.trim().is_empty() { slug(&step.title) } else { step.name.trim().to_owned() };
                texcraft_engine::free_folder(p, &name).display().to_string()
            }
            None => String::new(),
        };
    }
}

/// Asks for the owners once per connection, and the name's check once typing paused.
fn look_up(app: &mut App, conns: &[Connection], now: f64) {
    let busy = app.session.git_busy().is_some();
    let Some(step) = app.gallery.place_step.as_mut() else { return };
    let Place::Forge(instance) = step.place.clone() else { return };
    let Some(c) = conns.iter().find(|c| c.instance == instance) else { return };
    if !c.connected || busy || step.running || step.unsent.is_some() {
        return;
    }
    let service = c.service.id();
    if step.owners_for.as_deref() != Some(instance.as_str()) {
        step.owners_for = Some(instance.clone());
        step.owners.clear();
        step.owner = None;
        let started = app.session.execute("forge.owners", &json!({"service": service, "instance": instance, "background": true}));
        if let (Err(e), Some(step)) = (started, app.gallery.place_step.as_mut()) {
            step.owners_error = Some(e);
        }
        return;
    }
    let Some(at) = step.due else { return };
    if now - at < PAUSE {
        return;
    }
    step.due = None;
    let name = step.name.trim().to_owned();
    if texcraft_engine::forge::create::check_name(&name).is_err() {
        step.check = Check::None;
        return;
    }
    let asked = format!("{}/{name}", step.owner.clone().unwrap_or_default());
    step.check = Check::Running(asked);
    let mut params = json!({"service": service, "instance": instance, "name": name, "background": true});
    if let Some(owner) = &step.owner {
        params["owner"] = json!(owner);
    }
    match app.session.execute("forge.name_available", &params) {
        Ok(v) if v["started"] == true => {}
        Ok(v) => name_checked(app, Ok(&v)),
        Err(e) => name_checked(app, Err(&e)),
    }
}

/// The owners listed (`forge.owners`): the account first.
pub(crate) fn owners_listed(app: &mut App, done: &Value) {
    let Some(step) = app.gallery.place_step.as_mut() else { return };
    if step.owners_for.as_deref() != done["instance"].as_str() {
        return;
    }
    step.owners = done["owners"].as_array().into_iter().flatten().filter_map(|o| o["path"].as_str().map(str::to_owned)).collect();
    step.owners_error = None;
    // The name is checked for the account now known.
    step.due = Some(f64::NEG_INFINITY);
}

/// The name's check answered.
pub(crate) fn name_checked(app: &mut App, outcome: Result<&Value, &str>) {
    let Some(step) = app.gallery.place_step.as_mut() else { return };
    // Typed again since: its own check follows.
    if step.due.is_some() || !matches!(step.check, Check::Running(_)) {
        return;
    }
    step.check = match outcome {
        Ok(v) => {
            let project = v["project"].as_str().unwrap_or_default().to_owned();
            if v["available"] == true { Check::Free(project) } else { Check::Taken(project) }
        }
        Err(e) => Check::Failed(e.to_owned()),
    };
}

fn perform(app: &mut App, act: Act, conns: &[Connection]) {
    match act {
        Act::Choose(place) => {
            if let Some(step) = app.gallery.place_step.as_mut()
                && step.place != place
            {
                step.place = place;
                step.error = None;
                step.check = Check::None;
                step.due = Some(f64::NEG_INFINITY);
                step.owners_error = None;
            }
        }
        Act::Add => {
            if let Some(step) = app.gallery.place_step.as_mut() {
                step.adding = Some(conns.iter().map(|c| c.instance.clone()).collect());
            }
            app.gallery.open = false;
            crate::accounts::open_add(app, None);
        }
        Act::Back => {
            if let Some(step) = app.gallery.place_step.take() {
                app.gallery.selected = Some(step.template.clone());
                app.gallery.left_step = Some(step);
            }
            app.gallery.focus_create = true;
        }
        Act::Close => {
            app.gallery.place_step = None;
            app.gallery.left_step = None;
            app.gallery.open = false;
        }
        Act::Create => create(app, conns),
        Act::ChangeFolder => app.request_pick(crate::picks::Pick::CloneFolder),
        Act::Sign(a) => crate::forge::act(app, a),
        Act::TryAgain => {
            if let Some(step) = app.gallery.place_step.as_mut() {
                step.running = true;
                step.error = None;
            }
            match app.session.execute("git.push", &json!({"background": true})) {
                Ok(v) if v["started"] == true => {}
                Ok(v) => resent(app, &v),
                Err(e) => push_failed(app, &e),
            }
        }
    }
}

/// Create Project: the project, here or on the forge (the template downloaded first when it is
/// not kept).
fn create(app: &mut App, conns: &[Connection]) {
    let browser = in_browser(app);
    let Some(step) = app.gallery.place_step.as_mut() else { return };
    step.error = None;
    let folder = if browser { step.name.trim().to_owned() } else { step.folder.trim().to_owned() };
    let mut params = json!({"template": step.template, "folder": folder});
    if let Place::Forge(instance) = &step.place {
        let Some(c) = conns.iter().find(|c| c.instance == *instance) else { return };
        let mut forge = json!({"service": c.service.id(), "instance": instance, "name": step.name.trim(), "visibility": step.visibility});
        if let Some(owner) = &step.owner {
            forge["owner"] = json!(owner);
        }
        params["forge"] = forge;
        params["background"] = json!(true);
    }
    let place = step.place.id().to_owned();
    let id = step.template.clone();
    app.prefs.new_project_place = Some(place);
    app.prefs_changed = true;
    let kept = texcraft_templates::Catalog::builtin().ok().and_then(|c| c.find(&id).ok().map(|t| app.template_kept(t))).unwrap_or(false);
    if kept {
        app.make_from_template(params);
    } else {
        let label = if browser { tl!("Fetching the template…") } else { tl!("Downloading the template from TeX Live…") };
        app.start_download(vec![id], Some(params), label.to_owned());
    }
}

/// `project.from_template` with the step's choices: on a forge it runs in the background.
pub(crate) fn run(app: &mut App, params: Value) {
    if params.get("forge").is_none() {
        if app.run("project.from_template", params).is_some() {
            app.gallery.place_step = None;
            app.template_project_opened();
        } else if let Some(step) = app.gallery.place_step.as_mut() {
            step.error = app.view.status.take().map(|(m, _)| m);
        }
        return;
    }
    let started = app.session.execute("project.from_template", &params);
    match started {
        Ok(v) if v["started"] == true => {
            if let Some(step) = app.gallery.place_step.as_mut() {
                step.running = true;
            }
        }
        Ok(v) => created(app, &v),
        Err(e) => {
            failed(app, Some("new_project"), &e);
        }
    }
}

/// The project was made on the forge: it opens; a push that did not work keeps the dialog, saying
/// so, with Try Again.
pub(crate) fn created(app: &mut App, done: &Value) {
    let project = done["project"].as_str().unwrap_or_default().to_owned();
    let service = done["service"].as_str().and_then(Service::parse).unwrap_or_default();
    let said = say(service, tl!("Created {project} on GitLab."), tl!("Created {project} on GitHub.")).replace("{project}", &project);
    let keep = app.gallery.place_step.take();
    app.template_project_opened();
    match done["push_error"].as_str() {
        Some(why) => {
            let why = crate::i18n::msg(why);
            let folder = done["folder"].as_str().unwrap_or_default().to_owned();
            app.view.status = Some((format!("{said} {}", tl!("Sending it did not work: {why}").replace("{why}", &why)), true));
            if let Some(mut step) = keep {
                step.running = false;
                step.unsent = Some(Unsent { project, service, folder, why });
                app.gallery.place_step = Some(step);
                app.gallery.open = true;
            }
        }
        None => app.view.status = Some((said, false)),
    }
}

/// Try Again sent the project: the dialog closes.
pub(crate) fn resent(app: &mut App, _: &Value) {
    let Some(step) = app.gallery.place_step.as_ref() else { return };
    let Some(unsent) = &step.unsent else { return };
    let said = say(unsent.service, tl!("Created {project} on GitLab."), tl!("Created {project} on GitHub.")).replace("{project}", &unsent.project);
    app.view.status = Some((said, false));
    app.gallery.place_step = None;
    app.gallery.open = false;
}

fn push_failed(app: &mut App, e: &str) {
    if let Some(step) = app.gallery.place_step.as_mut() {
        step.running = false;
        if let Some(unsent) = step.unsent.as_mut() {
            unsent.why = crate::i18n::msg(e);
        }
    }
}

/// An operation of the step failed: said in it, which stays open. Whether it was one.
pub(crate) fn failed(app: &mut App, was: Option<&str>, e: &str) -> bool {
    let Some(step) = app.gallery.place_step.as_mut() else { return false };
    match was {
        Some("new_project") => {
            step.running = false;
            step.error = Some(e.to_owned());
            true
        }
        Some("git_push") if step.unsent.is_some() => {
            push_failed(app, e);
            true
        }
        Some("owners") if !matches!(app.view.dialog, Some(crate::dialogs::Dialog::ForgeSave(_))) => {
            step.owners_error = Some(e.to_owned());
            true
        }
        Some("name_available") => {
            name_checked(app, Err(e));
            true
        }
        _ => false,
    }
}

/// Add a Connection… ended (connected, or closed): the step comes back, the new connection's card
/// chosen. Whether the step was waiting for it.
pub(crate) fn resume(app: &mut App) -> bool {
    let waiting = app.gallery.place_step.as_ref().and_then(|s| s.adding.clone());
    let Some(before) = waiting else { return false };
    let conns = connections(app);
    app.view.dialog = None;
    if let Some(step) = app.gallery.place_step.as_mut() {
        step.adding = None;
        step.focus_card = true;
        if let Some(new) = conns.iter().find(|c| !before.contains(&c.instance)) {
            step.place = Place::Forge(new.instance.clone());
            step.check = Check::None;
            step.due = Some(f64::NEG_INFINITY);
        }
    }
    app.gallery.open = true;
    true
}

/// Whether Add a Connection… was opened from the step (its server's dialog comes back here too).
pub(crate) fn adding(app: &App) -> bool {
    app.gallery.place_step.as_ref().is_some_and(|s| s.adding.is_some())
}

/// Change… chose a new parent folder: the folder follows it.
pub(crate) fn parent_chosen(app: &mut App) {
    if let Some(step) = app.gallery.place_step.as_mut() {
        step.folder_edited = false;
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing.

fn draw(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, conns: &[Connection], info: &Info<'_>, acts: &mut Vec<Act>) {
    header(ui, t, step, info, acts);
    if step.unsent.is_some() {
        unsent(ui, t, step, info, acts);
        return;
    }
    let chosen = cards(ui, t, step, conns, info);
    if let Some(choice) = chosen {
        match choice {
            Some(place) => acts.push(Act::Choose(place)),
            None => acts.push(Act::Add),
        }
    }
    ui.add_space(t.unit);
    ui.label(RichText::new(tl!("To start on Overleaf, create the project there, then Import it.")).font(t.small()).color(t.text_muted));
    ui.add_space(t.unit * 4.0);
    let mut ready = false;
    let mut enter = false;
    ui.add_enabled_ui(!info.running, |ui| {
        // As tall as a forge's fields, so that the dialog does not jump between the cards.
        if !info.narrow {
            ui.set_min_height(FIELDS_HEIGHT);
        }
        (ready, enter) = match step.place.clone() {
            Place::Local => local_fields(ui, t, step, info, acts),
            Place::Forge(instance) => match conns.iter().find(|c| c.instance == instance) {
                Some(c) => forge_fields(ui, t, step, c, info, acts),
                None => (false, false),
            },
        };
    });
    if let Some(e) = &step.error {
        ui.add_space(t.unit * 2.0);
        hint(ui, t, &crate::i18n::msg(e), Tone::Bad);
    }
    footer(ui, t, step, conns, info, (ready, enter), acts);
}

/// The title row: Back, the title, Close; and the template under it.
fn header(ui: &mut egui::Ui, t: &Tokens, step: &WhereStep, info: &Info<'_>, acts: &mut Vec<Act>) {
    let asked = crate::dialogs::header(ui, t, tl!("Where to Keep It"), Some(tl!("Back to templates")), !info.running, |_| {});
    if asked.back {
        acts.push(Act::Back);
    }
    if asked.close {
        acts.push(Act::Close);
    }
    let what = tl!("New project from the template {template}").replace("{template}", &step.title);
    ui.label(RichText::new(what).color(t.text_muted));
    ui.add_space(t.unit * 3.0);
}

/// The cards: here, each connection, Add a Connection…. Returns the card chosen (`None` inside:
/// Add a Connection…).
fn cards(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, conns: &[Connection], info: &Info<'_>) -> Option<Option<Place>> {
    let local_look = Look { title: if info.browser { tl!("This Browser") } else { tl!("This Computer") }, icon: "folder-open", brand: false };
    let local_line = Line {
        text: if info.browser { tl!("Kept in this browser") } else { tl!("A folder of your choice") }.to_owned(),
        signed_in: false,
        unavailable: false,
    };
    let mut items: Vec<(Option<Place>, Look<'_>, Line)> = vec![(Some(Place::Local), local_look, local_line)];
    for c in conns {
        let public = c.instance == texcraft_engine::forge::oauth::instance(c.service);
        let (title, icon, brand) = match (c.service, public) {
            (Service::GitLab, true) => (Service::GitLab.name(), "brand-gitlab", true),
            (Service::GitHub, true) => (Service::GitHub.name(), "brand-github", true),
            _ => (c.host.as_str(), "server", false),
        };
        let _ = c.server;
        items.push((
            Some(Place::Forge(c.instance.clone())),
            Look { title, icon, brand },
            Line { text: c.line.clone(), signed_in: c.connected, unavailable: false },
        ));
    }
    let add_line = Line { text: tl!("GitLab, GitHub or your server").to_owned(), signed_in: false, unavailable: false };
    items.push((None, Look { title: tl!("Add a Connection…"), icon: "plus", brand: false }, add_line));

    let width = ui.available_width();
    let columns = crate::import::columns(width, 4);
    let list = info.narrow || columns < 2;
    let card_width = crate::import::card_width(width, columns);
    let mut cells: Vec<(usize, egui::Response, usize, usize)> = Vec::new();
    let builder = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::RadioGroup).accessibility_label(tl!("Where to keep it"));
    ui.scope_builder(builder, |ui| {
        if list {
            for (i, (_, look, line)) in items.iter().enumerate() {
                let r = crate::import::list_row(ui, t, *look, line);
                cells.push((i, r, i, 0));
            }
        } else {
            for (row, chunk) in items.chunks(columns).enumerate() {
                let looks: Vec<(Look<'_>, &Line)> = chunk.iter().map(|(_, l, line)| (*l, line)).collect();
                for (c, r) in crate::import::cards_row(ui, t, &looks, card_width).into_iter().enumerate() {
                    cells.push((row * columns + c, r, row, c));
                }
            }
        }
    });
    let mut chosen = None;
    let focus = std::mem::take(&mut step.focus_card);
    for (i, r, ..) in &cells {
        let (place, look, line) = &items[*i];
        let selected = place.as_ref() == Some(&step.place);
        if place.is_some() {
            // A radio button among the places, its name its title then its line.
            let name = format!("{}, {}", look.title, line.text);
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, &name));
        }
        if selected {
            let painter = ui.painter();
            // The gallery's chosen template's look: a soft accent tint and a 2 px accent outline,
            // and a mark, so that colour is not the only sign.
            painter.rect_filled(r.rect, egui::CornerRadius::same(8), t.accent_soft);
            painter.rect_stroke(r.rect, egui::CornerRadius::same(8), egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
            // On a phone's list the chevron is at the right: the mark goes before it.
            let at =
                if list { egui::pos2(r.rect.right() - 46.0, r.rect.center().y - 7.0) } else { egui::pos2(r.rect.right() - 22.0, r.rect.top() + 8.0) };
            let mark = egui::Rect::from_min_size(at, vec2(14.0, 14.0));
            icons::paint(ui, mark, "circle-check", 14.0, t.accent);
            if focus {
                r.request_focus();
            }
        }
        if r.clicked() {
            chosen = Some(place.clone());
        }
    }
    crate::import::arrows(ui, &cells);
    chosen
}

/// A form's row: the label beside the field, or above it on a phone.
fn row<R>(ui: &mut egui::Ui, t: &Tokens, narrow: bool, label: &str, add: impl FnOnce(&mut egui::Ui, egui::Id) -> R) -> R {
    crate::import::row(ui, t, narrow, label, add)
}

/// This Computer's folder, or This Browser's name. Whether Create can run, and Enter.
fn local_fields(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, info: &Info<'_>, acts: &mut Vec<Act>) -> (bool, bool) {
    if info.browser {
        let r = row(ui, t, info.narrow, tl!("Name"), |ui, label| {
            let r = ui.add(egui::TextEdit::singleline(&mut step.name).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0))).labelled_by(label);
            hint(ui, t, tl!("The project is kept in this browser, under this name."), Tone::Plain);
            r
        });
        if r.changed() {
            step.name_edited = true;
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        return (!step.name.trim().is_empty(), enter);
    }
    folder_row(ui, t, step, info, acts)
}

/// The folder's row (the desktop). Whether it can be used, and Enter.
fn folder_row(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, info: &Info<'_>, acts: &mut Vec<Act>) -> (bool, bool) {
    let mut change = false;
    let shown = crate::import::FolderRow { label: tl!("Folder"), narrow: info.narrow, has_parent: info.parent.is_some() };
    let (ok, field) = crate::import::folder_field(ui, t, &shown, &mut step.folder, &mut step.folder_edited, &mut change);
    if change {
        acts.push(Act::ChangeFolder);
    }
    let enter = field.is_some_and(|f| f.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
    (ok, enter)
}

/// A forge's fields. Whether Create can run, and Enter.
fn forge_fields(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, c: &Connection, info: &Info<'_>, acts: &mut Vec<Act>) -> (bool, bool) {
    if !c.connected {
        row(ui, t, info.narrow, tl!("Account"), |ui, _| {
            hint(ui, t, &tl!("Not signed in to {host}.").replace("{host}", &c.host), Tone::Plain);
            if ui.button(RichText::new(tl!("Sign In")).font(t.strong())).clicked() {
                acts.push(Act::Sign(SignInAction::SignIn { service: c.service, instance: c.instance.clone(), write: true }));
            }
        });
        return (false, false);
    }
    if c.read_only {
        row(ui, t, info.narrow, tl!("Account"), |ui, _| {
            hint(
                ui,
                t,
                tl!("You signed in to GitLab to read only. To make a project, GitLab asks you to let TeXCraft write too (the api scope)."),
                Tone::Plain,
            );
            if ui.button(RichText::new(tl!("Allow Commits on GitLab")).font(t.strong())).clicked() {
                acts.push(Act::Sign(SignInAction::SignIn { service: c.service, instance: c.instance.clone(), write: true }));
            }
        });
        return (false, false);
    }
    let mut responses = Vec::new();
    let mut name_ok = false;
    let name = row(ui, t, info.narrow, tl!("Repository"), |ui, label| {
        let r = ui.add(egui::TextEdit::singleline(&mut step.name).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0))).labelled_by(label);
        let typed = step.name.trim();
        match texcraft_engine::forge::create::check_name(typed) {
            Err(e) => hint(ui, t, &crate::i18n::msg(&e.to_string()), if typed.is_empty() { Tone::Plain } else { Tone::Bad }),
            Ok(()) => match &step.check {
                Check::Running(_) => hint(ui, t, tl!("Checking…"), Tone::Busy),
                _ if step.due.is_some() => hint(ui, t, tl!("Checking…"), Tone::Busy),
                Check::Free(p) => {
                    name_ok = true;
                    hint(ui, t, &tl!("{project} is free.").replace("{project}", p), Tone::Good);
                }
                Check::Taken(p) => hint(ui, t, &tl!("{project} exists already: choose another name.").replace("{project}", p), Tone::Bad),
                Check::Failed(e) => {
                    // Not known: the forge's answer when it is made counts.
                    name_ok = true;
                    hint(ui, t, &crate::i18n::msg(e), Tone::Plain);
                }
                Check::None => name_ok = true,
            },
        }
        r
    });
    if name.changed() {
        step.name_edited = true;
        step.due = Some(info.now);
    }
    responses.push(name);
    owner_row(ui, t, step, c, info);
    visibility_row(ui, t, step, c, info);
    let mut ready = name_ok && !matches!(step.check, Check::Taken(_));
    if !info.browser {
        let (ok, enter) = folder_row(ui, t, step, info, acts);
        ready &= ok;
        if enter {
            return (ready, true);
        }
    }
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
    (ready, enter)
}

fn owner_row(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, c: &Connection, info: &Info<'_>) {
    let label = tl!("Owner");
    row(ui, t, info.narrow, label, |ui, label| {
        let Some(account) = step.owners.first().cloned() else {
            match &step.owners_error {
                Some(e) => hint(ui, t, &crate::i18n::msg(e), Tone::Bad),
                None => hint(ui, t, tl!("Asking where you can make repositories…"), Tone::Busy),
            }
            return;
        };
        let shown = step.owner.clone().unwrap_or_else(|| account.clone());
        let mut chosen = None;
        let combo = craft_ui::ComboBox::from_id_salt("where-owner").width(ui.available_width().min(360.0)).selected_text(&shown).show_ui(ui, |ui| {
            for (i, o) in step.owners.iter().enumerate() {
                if ui.selectable_label(*o == shown, o).clicked() {
                    chosen = Some(if i == 0 { None } else { Some(o.clone()) });
                }
            }
        });
        let _ = combo.response.labelled_by(label);
        hint(
            ui,
            t,
            say(
                c.service,
                tl!("You, or a group in which you can make projects."),
                tl!("You, or an organisation in which you can make repositories."),
            ),
            Tone::Plain,
        );
        if let Some(choice) = chosen
            && choice != step.owner
        {
            step.owner = choice;
            step.due = Some(f64::NEG_INFINITY);
        }
    });
}

fn visibility_row(ui: &mut egui::Ui, t: &Tokens, step: &mut WhereStep, c: &Connection, info: &Info<'_>) {
    let group = tl!("Who can see it");
    let mut choices = vec![("private", tl!("Private"))];
    if c.service == Service::GitLab {
        choices.push(("internal", tl!("Internal")));
    }
    choices.push(("public", tl!("Public")));
    row(ui, t, info.narrow, group, |ui, _| {
        ui.horizontal_wrapped(|ui| {
            for (id, text) in &choices {
                let on = step.visibility == *id;
                let r = ui.radio(on, *text);
                let name = format!("{text}: {group}");
                r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, &name));
                if r.clicked() {
                    step.visibility = id;
                }
            }
        });
        let note = match step.visibility {
            "public" => tl!("Anyone can see it, and copy it."),
            "internal" => tl!("Everyone signed in to this server can see it."),
            _ => tl!("Only you, and those you give access to, see it."),
        };
        hint(ui, t, note, Tone::Plain);
    });
}

/// Cancel and Create Project, or while it runs its phase.
fn footer(ui: &mut egui::Ui, t: &Tokens, step: &WhereStep, conns: &[Connection], info: &Info<'_>, (ready, enter): (bool, bool), acts: &mut Vec<Act>) {
    ui.add_space(t.unit * 3.0);
    if !info.narrow {
        widgets::hairline(ui, t);
        ui.add_space(t.unit * 2.0);
    }
    crate::dialogs::actions(ui, |ui| {
        if crate::dialogs::primary(ui, t, tl!("Create Project"), ready && !info.running, false).clicked() || (enter && ready && !info.running) {
            acts.push(Act::Create);
        }
        if crate::dialogs::secondary(ui, tl!("Cancel"), !info.running).clicked() {
            acts.push(Act::Close);
        }
        if info.running {
            let service = match &step.place {
                Place::Forge(i) => conns.iter().find(|c| c.instance == *i).map(|c| c.service),
                Place::Local => None,
            }
            .unwrap_or_default();
            let text = match (info.downloading, info.phase) {
                (Some(d), _) => d.to_owned(),
                (None, Some("push")) => say(service, tl!("Sending the project to GitLab…"), tl!("Sending the project to GitHub…")).to_owned(),
                (None, _) => say(service, tl!("Making {name} on GitLab…"), tl!("Making {name} on GitHub…")).replace("{name}", step.name.trim()),
            };
            crate::dialogs::progress(ui, t, &text);
        }
    });
}

/// The repository was made and the project is open, but sending it did not work.
fn unsent(ui: &mut egui::Ui, t: &Tokens, step: &WhereStep, info: &Info<'_>, acts: &mut Vec<Act>) {
    let Some(u) = &step.unsent else { return };
    let said = say(
        u.service,
        tl!("Created {project} on GitLab, and the project is in {folder}, but sending it did not work: {why}"),
        tl!("Created {project} on GitHub, and the project is in {folder}, but sending it did not work: {why}"),
    )
    .replace("{project}", &u.project)
    .replace("{folder}", &u.folder)
    .replace("{why}", &u.why);
    hint(ui, t, &said, Tone::Bad);
    ui.add_space(t.unit * 2.0);
    ui.label(RichText::new(tl!("Nothing is lost: Try Again sends it now, and Sync sends it later.")).font(t.small()).color(t.text_muted));
    ui.add_space(t.unit * 3.0);
    if !info.narrow {
        widgets::hairline(ui, t);
        ui.add_space(t.unit * 2.0);
    }
    crate::dialogs::actions(ui, |ui| {
        if crate::dialogs::primary(ui, t, tl!("Try Again"), !info.running, false).clicked() {
            acts.push(Act::TryAgain);
        }
        if crate::dialogs::secondary(ui, tl!("Send Later"), !info.running).clicked() {
            acts.push(Act::Close);
        }
        if info.running {
            crate::dialogs::progress(ui, t, say(u.service, tl!("Sending the project to GitLab…"), tl!("Sending the project to GitHub…")));
        }
    });
}
