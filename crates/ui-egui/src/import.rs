//! Import a Project (docs/design/import-dialog.md; docs/ROADMAP.md, C11): one dialog that brings a
//! project in from GitLab, GitHub, Overleaf, a GitLab server, any git address over HTTPS, a folder
//! or a zip file. File › Import a Project… (`project.import_dialog`), the start page's Import…,
//! and `forge.open` (at the GitLab or GitHub card) open it.
//!
//! The first page is a grid of cards in two groups, flowing in up to four columns (a list on a
//! phone); a card is a button and a Tab stop, the arrows move the keyboard between them in two
//! dimensions (Home and End to the first and last), and its name is its title then its line. A
//! source the browser cannot reach stays focusable and opens a page that says why, with the way
//! out. Choosing a card replaces the grid with the source's page in the same dialog: Back
//! (Alt+Left, Cmd+[ on a Mac) returns to the grid with the card focused.
//!
//! On the desktop a git source is a real clone into a folder (`project.clone`, its progress in the
//! footer, Cancel stopping it and removing the folder made); in the browser GitLab and GitHub come
//! in through their web APIs (`forge.open`). The GitLab and GitHub pages reuse the forge's sign-in
//! and the project suggestions and branch list (`forge_pick.rs`); the Overleaf and Git Address
//! pages check the address once typing pauses (`git.check_address`).

use std::path::{Path, PathBuf};

use egui::{Key, Modifiers, RichText, Sense, vec2};
use serde_json::{Value, json};
use texcraft_engine::forge::Service;
use texcraft_engine::{ClonePhase, ImportProblem};

use crate::dialogs::{Dialog, actions};
use crate::forge::{OpenForm, SignInAction, SignInView};
use crate::theme::{Tokens, blend};
use crate::{App, icons, widgets};

pub(crate) mod link;

/// The dialog's sheet.
const SHEET: &str = "texcraft-import";
/// How wide the dialog is at most (docs/design.md's dialogs).
const WIDTH: f32 = 720.0;
/// How long typing pauses before an address is checked, in seconds.
const PAUSE: f64 = 0.5;
/// A card is no narrower than this: the grid drops a column first.
const CARD_MIN: f32 = 140.0;
const GAP: f32 = 12.0;
const CARD_HEIGHT: f32 = 76.0;
/// Overleaf's git addresses start so.
const OVERLEAF: &str = "https://git.overleaf.com/";

/// A place a project can come from, in the grid's order (the reading and the keyboard order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    GitLab,
    GitHub,
    Overleaf,
    GitLabServer,
    Git,
    Folder,
    Zip,
}

impl Source {
    const ALL: [Source; 7] = [Source::GitLab, Source::GitHub, Source::Overleaf, Source::GitLabServer, Source::Git, Source::Folder, Source::Zip];

    /// Its id, as `import.sources` and `project.import_dialog` name it.
    pub(crate) fn parse(id: &str) -> Option<Self> {
        Some(match id {
            "gitlab" => Source::GitLab,
            "github" => Source::GitHub,
            "overleaf" => Source::Overleaf,
            "gitlab_server" => Source::GitLabServer,
            "git" => Source::Git,
            "folder" => Source::Folder,
            "zip" => Source::Zip,
            _ => return None,
        })
    }

    fn online(self) -> bool {
        !matches!(self, Source::Folder | Source::Zip)
    }

    /// The card's title: the service's name, or our own words.
    fn title(self) -> &'static str {
        match self {
            Source::GitLab => Service::GitLab.name(),
            Source::GitHub => Service::GitHub.name(),
            Source::Overleaf => texcraft_engine::GitHosting::Overleaf.name(),
            Source::GitLabServer => tl!("GitLab Server"),
            Source::Git => tl!("Git Address"),
            Source::Folder => tl!("Folder"),
            Source::Zip => tl!("Zip File"),
        }
    }

    /// The page's title, which names the dialog while it is shown.
    fn page_title(self) -> &'static str {
        match self {
            Source::GitLab => tl!("Import from GitLab"),
            Source::GitHub => tl!("Import from GitHub"),
            Source::Overleaf => tl!("Import from Overleaf"),
            Source::GitLabServer => tl!("Import from a GitLab Server"),
            Source::Git => tl!("Import from a Git Address"),
            Source::Folder | Source::Zip => tl!("Import a Project"),
        }
    }

    /// Its icon: the service's mark (drawn in the text colour), or ours (in the accent).
    fn icon(self) -> (&'static str, bool) {
        match self {
            Source::GitLab => ("brand-gitlab", true),
            Source::GitHub => ("brand-github", true),
            Source::Overleaf => ("brand-overleaf", true),
            Source::GitLabServer => ("server", false),
            Source::Git => ("link", false),
            Source::Folder => ("folder-open", false),
            Source::Zip => ("file-archive", false),
        }
    }

    fn service(self) -> Option<Service> {
        match self {
            Source::GitLab | Source::GitLabServer => Some(Service::GitLab),
            Source::GitHub => Some(Service::GitHub),
            _ => None,
        }
    }
}

/// Where an address check stands.
#[derive(Debug, Clone, Default, PartialEq)]
enum Checked {
    #[default]
    Idle,
    /// Asked, for this address and key.
    Running(String),
    /// The server answered with its branches.
    Found {
        branches: Vec<String>,
        default: Option<String>,
        host: String,
        signed_in: bool,
    },
    /// The server wants signing in.
    NeedsSignIn,
    /// An SSH address: its HTTPS form.
    Ssh(String),
    Failed(String),
}

/// An address check: when typing paused, what was last asked, and the answer.
#[derive(Debug, Default)]
struct Check {
    due: Option<f64>,
    state: Checked,
}

impl Check {
    fn typed(&mut self, now: f64) {
        self.due = Some(now);
        self.state = Checked::Idle;
    }
}

/// The Overleaf page.
#[derive(Default)]
struct OverleafPage {
    address: String,
    token: String,
    show: bool,
    check: Check,
}

/// The Git Address page.
#[derive(Default)]
struct GitPage {
    address: String,
    username: String,
    token: String,
    show: bool,
    branch: String,
    check: Check,
}

enum Page {
    Grid,
    /// GitLab, GitHub or a GitLab server: the forge's sign-in, suggestions and branches.
    Forge(Box<OpenForm>, Source),
    Overleaf(OverleafPage),
    Git(GitPage),
    /// A source this place cannot reach: why, and the way out.
    Unavailable(Source),
}

impl Page {
    fn source(&self) -> Option<Source> {
        match self {
            Page::Grid => None,
            Page::Forge(_, s) | Page::Unavailable(s) => Some(*s),
            Page::Overleaf(_) => Some(Source::Overleaf),
            Page::Git(_) => Some(Source::Git),
        }
    }

    /// The address typed, for the folder's name.
    fn address(&self) -> &str {
        match self {
            Page::Forge(f, _) => &f.address,
            Page::Overleaf(p) => &p.address,
            Page::Git(p) => &p.address,
            _ => "",
        }
    }
}

/// Where the project lands: the folder's full path, which follows the source's name until edited.
#[derive(Default)]
struct Into {
    path: String,
    edited: bool,
}

/// Import a Project, while it is open.
pub(crate) struct ImportForm {
    page: Page,
    /// The card that gets the keyboard next frame (the dialog opened, or Back).
    focus_card: Option<Source>,
    /// The page's first field gets the keyboard next frame.
    focus_field: bool,
    into: Into,
    /// Remember on this computer (the keychain).
    remember: bool,
    /// Why the last import did not work, and its kind.
    error: Option<(ImportProblem, String)>,
    /// Link to a Repository (`link.rs`): the dialog links the open project instead of importing.
    link: Option<Box<link::LinkState>>,
}

/// The GitLab, GitHub or GitLab Server page's form in `dialog`, when Import a Project shows one.
pub(crate) fn forge_form(dialog: &Option<Dialog>) -> Option<&OpenForm> {
    match dialog {
        Some(Dialog::Import(form)) => match &form.page {
            Page::Forge(f, _) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn forge_form_mut(dialog: &mut Option<Dialog>) -> Option<&mut OpenForm> {
    match dialog {
        Some(Dialog::Import(form)) => match &mut form.page {
            Page::Forge(f, _) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

/// `project.import_dialog {source?, address?}`: Import a Project, unsaved changes asked about
/// first (importing replaces the project).
pub(crate) fn run(app: &mut App, params: &Value) {
    let source = params.get("source").and_then(Value::as_str).and_then(Source::parse);
    let address = params.get("address").and_then(Value::as_str).unwrap_or_default().to_owned();
    app.guard(crate::dialogs::Next::Import(source, address));
}

/// `forge.open {url?, service?}` from the window: Import a Project at the GitLab or GitHub card,
/// the address filled in (the service its address or `service` names, else the last one).
pub(crate) fn run_forge_open(app: &mut App, params: &Value) {
    let address = params.get("url").and_then(Value::as_str).unwrap_or_default().to_owned();
    let named = params.get("service").and_then(Value::as_str).and_then(Service::parse);
    let service = named
        .or_else(|| texcraft_engine::forge::Address::parse(&address).ok().map(|a| a.service).filter(|_| address.contains('.')))
        .unwrap_or_else(|| crate::forge::last_service(app));
    let source = match service {
        Service::GitLab => Source::GitLab,
        Service::GitHub => Source::GitHub,
    };
    app.guard(crate::dialogs::Next::Import(Some(source), address));
}

/// The card of the default connection (Settings › Accounts › Set as Default), if it has one:
/// gitlab.com's, github.com's, or the GitLab Server card for a GitLab server.
fn default_source(app: &App) -> Option<Source> {
    let (instance, service) = app.session.default_connection()?;
    match service {
        _ if instance == texcraft_engine::forge::oauth::instance(service) => {
            Some(if service == Service::GitLab { Source::GitLab } else { Source::GitHub })
        }
        Service::GitLab => Some(Source::GitLabServer),
        // GitHub Enterprise servers are not in this dialog (docs/design/import-dialog.md).
        Service::GitHub => None,
    }
}

/// Opens Import a Project, at `source` if given, `address` filled in; the grid opens with the
/// default connection's card focused.
pub(crate) fn open_at(app: &mut App, source: Option<Source>, address: &str) {
    let first = default_source(app).unwrap_or(Source::GitLab);
    let mut form =
        ImportForm { page: Page::Grid, focus_card: Some(first), focus_field: false, into: Into::default(), remember: true, error: None, link: None };
    app.view.dialog = None;
    match source {
        Some(source) => {
            form.focus_card = Some(source);
            choose(app, form, source, address);
        }
        None => app.view.dialog = Some(Dialog::Import(Box::new(form))),
    }
}

/// Comes back from adding a server (Settings' Add a Server, from the GitLab Server page).
pub(crate) fn back_from_server(app: &mut App, mut form: Box<ImportForm>) {
    let servers = gitlab_servers(app);
    if let Page::Forge(f, Source::GitLabServer) = &mut form.page {
        if !servers.iter().any(|(i, ..)| *i == f.instance)
            && let Some((first, ..)) = servers.first()
        {
            f.instance = first.clone();
        }
        crate::forge::refresh_use_token(app, f);
        f.picker = crate::forge_pick::Picker::new();
    }
    if servers.is_empty() && matches!(form.page, Page::Forge(_, Source::GitLabServer)) {
        form.page = Page::Grid;
        form.focus_card = Some(Source::GitLabServer);
    }
    app.view.dialog = Some(Dialog::Import(form));
}

/// The GitLab servers added: instance, host, signed in.
fn gitlab_servers(app: &App) -> Vec<(String, String, bool)> {
    app.session
        .forge_servers()
        .into_iter()
        .filter(|s| s.service == Service::GitLab)
        .map(|s| {
            let signed = app.session.forge_signed_in_at(&s.instance).is_some();
            (s.instance.clone(), s.host().to_owned(), signed)
        })
        .collect()
}

/// Whether this window is the browser's.
fn in_browser(app: &App) -> bool {
    app.session.library().is_some() || cfg!(target_arch = "wasm32")
}

/// Chooses `source`'s card: its page, or straight away its picker (Folder, Zip File).
fn choose(app: &mut App, mut form: ImportForm, source: Source, address: &str) {
    let browser = in_browser(app);
    form.error = None;
    form.focus_card = Some(source);
    form.focus_field = true;
    match source {
        Source::GitLab | Source::GitHub | Source::GitLabServer => {
            let service = source.service().unwrap_or_default();
            let public = texcraft_engine::forge::oauth::instance(service).to_owned();
            let instance = if source == Source::GitLabServer {
                let servers = gitlab_servers(app);
                let named = texcraft_engine::forge::Address::parse_as(address, Some(service)).ok().map(|a| a.instance);
                let default = app.session.default_connection().filter(|(i, s)| *s == service && servers.iter().any(|v| v.0 == *i)).map(|(i, _)| i);
                match named.filter(|i| *i != public).or(default).or_else(|| servers.iter().find(|s| s.2).or(servers.first()).map(|s| s.0.clone())) {
                    Some(i) => i,
                    None => {
                        // No server yet: the add step first, then back here.
                        let mut f = forge_page(app, service, public, address);
                        f.instance = String::new();
                        form.page = Page::Forge(Box::new(f), source);
                        crate::forge_server::ask_add(app, service, "", crate::forge_server::Back::Import(Box::new(form)));
                        return;
                    }
                }
            } else {
                public
            };
            crate::forge::set_opening(app, service);
            form.page = Page::Forge(Box::new(forge_page(app, service, instance, address)), source);
        }
        Source::Overleaf | Source::Git if browser => form.page = Page::Unavailable(source),
        Source::Overleaf => {
            let mut page = OverleafPage { address: address.to_owned(), ..OverleafPage::default() };
            if !address.is_empty() {
                page.check.due = Some(f64::NEG_INFINITY);
            }
            form.page = Page::Overleaf(page);
        }
        Source::Git => {
            let mut page = GitPage { address: address.to_owned(), ..GitPage::default() };
            if !address.is_empty() {
                page.check.due = Some(f64::NEG_INFINITY);
            }
            form.page = Page::Git(page);
        }
        Source::Folder => {
            form.focus_field = false;
            app.view.dialog = Some(Dialog::Import(Box::new(form)));
            if browser {
                app.request_pick(crate::picks::Pick::ImportProjectFolder);
            } else {
                crate::ui_cmd::pick_project(app);
            }
            return;
        }
        Source::Zip => {
            form.focus_field = false;
            app.view.dialog = Some(Dialog::Import(Box::new(form)));
            app.request_pick(crate::picks::Pick::ImportProjectZip);
            return;
        }
    }
    form.into.edited = false;
    app.view.dialog = Some(Dialog::Import(Box::new(form)));
}

fn forge_page(app: &App, service: Service, instance: String, address: &str) -> OpenForm {
    let mut f = OpenForm {
        service,
        instance,
        address: address.to_owned(),
        branch: String::new(),
        token: String::new(),
        use_token: false,
        picker: crate::forge_pick::Picker::new(),
    };
    crate::forge::refresh_use_token(app, &mut f);
    f
}

/// What a card's line says, and how.
pub(crate) struct Line {
    pub text: String,
    pub signed_in: bool,
    pub unavailable: bool,
}

/// What a card shows besides its line: its title and its icon (the service's mark, drawn in the
/// text colour, or ours, in the accent). Import's cards and Add a Connection's share it.
#[derive(Clone, Copy)]
pub(crate) struct Look<'a> {
    pub title: &'a str,
    pub icon: &'a str,
    pub brand: bool,
}

impl Source {
    fn look(self) -> Look<'static> {
        let (icon, brand) = self.icon();
        Look { title: self.title(), icon, brand }
    }
}

/// What the dialog shows that comes from the session, gathered before it is drawn.
struct Info {
    browser: bool,
    narrow: bool,
    /// A clone runs, and where it is.
    running: Option<ClonePhase>,
    lines: Vec<(Source, Line)>,
    /// The forge page's sign-in.
    view: Option<SignInView>,
    /// The forge page's server is one TeXCraft has no application on.
    unknown: bool,
    servers: Vec<(String, String, bool)>,
    /// A credential is known for Overleaf's git server, or for the Git page's.
    known: bool,
    /// Where new folders go by default.
    parent: Option<PathBuf>,
    /// Where a remembered credential is kept.
    kept: &'static str,
    /// A token is kept for the forge page's server (Remember on this computer, earlier).
    kept_token: bool,
    /// The largest history a clone takes, in words.
    limit: String,
    /// The window's clock.
    now: f64,
    /// Link to a Repository, and whether its link runs.
    link: bool,
    linking: bool,
}

fn info(app: &App, ctx: &egui::Context) -> Info {
    let browser = in_browser(app);
    let account = |service: Service| {
        app.session
            .forge_signed_in_at(texcraft_engine::forge::oauth::instance(service))
            .filter(|g| g.service == service)
            .and_then(|g| g.user.as_ref().map(|u| u.username.clone()))
    };
    let servers = gitlab_servers(app);
    let default = default_source(app);
    let line = |source: Source| -> Line {
        let plain = |text: &str| Line { text: text.to_owned(), signed_in: false, unavailable: false };
        let off = || Line { text: tl!("Desktop app only").to_owned(), signed_in: false, unavailable: true };
        match source {
            Source::GitLab | Source::GitHub => match account(source.service().unwrap_or_default()) {
                Some(who) => Line { text: tl!("Signed in as {username}").replace("{username}", &who), signed_in: true, unavailable: false },
                None => token_line(app, source.service().unwrap_or_default()).unwrap_or_else(|| {
                    plain(if browser && source == Source::GitHub { tl!("Personal access token") } else { tl!("Sign in or use a token") })
                }),
            },
            Source::Overleaf | Source::Git if browser => off(),
            Source::Overleaf => plain(tl!("Clone address and token")),
            Source::Git => plain(tl!("Any HTTPS address")),
            Source::GitLabServer => match servers.as_slice() {
                [] => plain(tl!("Add your server")),
                [(_, host, _)] => plain(host),
                list => plain(&crate::i18n::trn(list.len(), "{n} server", "{n} servers")),
            },
            Source::Folder if browser => plain(tl!("Copied into this browser")),
            Source::Folder => plain(tl!("Opens it where it is")),
            Source::Zip if browser => plain(tl!("Unzipped into this browser")),
            Source::Zip => plain(tl!("Unzipped into a new folder")),
        }
    };
    let (view, unknown, known) = match &app.view.dialog {
        Some(Dialog::Import(form)) => match &form.page {
            Page::Forge(f, _) => {
                let instance = crate::forge::instance_of_form(f);
                let unknown =
                    app.session.forge_sign_in_unavailable_at(f.service, &instance) == Some(texcraft_engine::SignInUnavailable::UnknownServer);
                (Some(crate::forge::sign_in_view(app, f.service, &instance)), unknown, false)
            }
            Page::Overleaf(_) => (None, false, app.session.has_git_credential("git.overleaf.com")),
            Page::Git(p) => {
                let typed = texcraft_engine::typed_address(&p.address);
                let host =
                    texcraft_engine::git_hosting(&typed).map(|_| "git.overleaf.com".to_owned()).or_else(|| host_of(&typed)).unwrap_or_default();
                (None, false, !host.is_empty() && app.session.has_git_credential(&host))
            }
            _ => (None, false, false),
        },
        _ => (None, false, false),
    };
    let parent = app.prefs.clone_folder.as_ref().map(PathBuf::from).or_else(|| app.documents.clone());
    let kept_token = crate::import::forge_form(&app.view.dialog).is_some_and(|f| app.session.has_kept_token(&crate::forge::instance_of_form(f)));
    // The default connection's card says so on its line (and in its name).
    let marked = |source: Source| {
        let mut l = line(source);
        if default == Some(source) {
            l.text = format!("{} · {}", l.text, tl!("Default"));
        }
        l
    };
    let (linked, linking) = match &app.view.dialog {
        Some(Dialog::Import(form)) => (form.link.is_some(), form.link.as_ref().is_some_and(|l| l.running())),
        _ => (false, false),
    };
    let order: Vec<Source> = if linked { link::order(app) } else { Source::ALL.to_vec() };
    Info {
        browser,
        narrow: crate::sheet::narrow(ctx, &app.tokens),
        running: app.session.clone_phase(),
        lines: order.iter().map(|s| (*s, marked(*s))).collect(),
        view,
        unknown,
        servers,
        known,
        parent,
        kept: app.session.git_token_kept(),
        kept_token,
        limit: texcraft_engine::project::size_words(texcraft_engine::forge::MAX_PROJECT),
        now: ctx.input(|i| i.time),
        link: linked,
        linking,
    }
}

/// A card's line for a service reached with a token (kept, or given in this session) rather than
/// a sign-in: who it is for, when it was checked.
pub(crate) fn token_line(app: &App, service: Service) -> Option<Line> {
    let instance = texcraft_engine::forge::oauth::instance(service);
    if !app.session.forge_reaches(instance, service) {
        return None;
    }
    let text = match app.session.token_user(instance) {
        Some(u) => tl!("Token for {username}").replace("{username}", &u.username),
        None => tl!("Token kept").to_owned(),
    };
    Some(Line { text, signed_in: true, unavailable: false })
}

/// The host an HTTPS address names.
fn host_of(address: &str) -> Option<String> {
    let rest = address.split_once("://")?.1;
    let authority = rest.split('/').next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// What the user did this frame.
enum Act {
    Choose(Source),
    Back,
    Close,
    Import,
    StopImport,
    ChangeFolder,
    SignIn(SignInAction),
    /// Another card, with the address typed (Switch to GitHub, Use the GitLab card).
    Switch(Source, String),
    DesktopApp,
    AddServer,
    TryAgain,
}

/// The open Import a Project.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    crate::forge_pick::tick(app, ctx);
    send_checks(app, ctx);
    link::send(app, ctx);
    let t = app.tokens;
    let info = info(app, ctx);
    let on_page = matches!(&app.view.dialog, Some(Dialog::Import(f)) if !matches!(f.page, Page::Grid));
    let leave = if on_page { crate::sheet::Leave::Back(tl!("Back to sources")) } else { crate::sheet::Leave::Close };
    let mut acts = Vec::new();
    let shown = crate::sheet::show(ctx, &t, SHEET, WIDTH, leave, |ui| {
        let Some(Dialog::Import(form)) = app.view.dialog.as_mut() else { return };
        follow_name(form, &info);
        acts = draw(ui, &t, form, &info);
    });
    let running = info.running.is_some() || info.linking;
    if shown.back {
        acts.push(Act::Back);
    }
    let popup = egui::Popup::is_any_open(ctx);
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    if !running && (shown.close || (escape && !popup)) {
        acts.push(Act::Close);
    }
    let back_keys = ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowLeft) || i.consume_key(Modifiers::COMMAND, Key::OpenBracket));
    if back_keys && on_page && !running {
        acts.push(Act::Back);
    }
    for act in acts {
        perform(app, act);
    }
}

/// The folder's name follows the source's until the path is edited.
fn follow_name(form: &mut ImportForm, info: &Info) {
    if form.into.edited {
        return;
    }
    let Some(parent) = &info.parent else {
        form.into.path.clear();
        return;
    };
    let address = form.page.address().trim();
    let name = if address.is_empty() { String::new() } else { texcraft_engine::repository_name(address) };
    let path = if name.is_empty() { format!("{}{}", parent.display(), std::path::MAIN_SEPARATOR) } else { parent.join(name).display().to_string() };
    form.into.path = path;
}

fn perform(app: &mut App, act: Act) {
    match act {
        Act::Choose(source) => {
            if let Some(Dialog::Import(form)) = app.view.dialog.take() {
                choose(app, *form, source, "");
            }
        }
        Act::Switch(source, address) => {
            if let Some(Dialog::Import(form)) = app.view.dialog.take() {
                choose(app, *form, source, &address);
            }
        }
        Act::Back => {
            if let Some(Dialog::Import(form)) = app.view.dialog.as_mut() {
                form.focus_card = form.page.source().or(form.focus_card);
                form.page = Page::Grid;
                form.error = None;
            }
        }
        Act::Close => app.view.dialog = None,
        Act::Import => start(app),
        Act::StopImport => {
            app.importing = None;
            if let Some(done) = app.run("project.cancel_clone", json!({}))
                && done["cancelled"] == true
            {
                let said = if done["removed"] == true { tl!("Import stopped: the folder it made is removed.") } else { tl!("Import stopped.") };
                app.view.status = Some((said.to_owned(), false));
            }
        }
        Act::ChangeFolder => app.request_pick(crate::picks::Pick::CloneFolder),
        Act::SignIn(action) => crate::forge::act(app, action),
        Act::DesktopApp => {
            crate::ui_cmd::run(app, "help.desktop_app");
        }
        Act::AddServer => {
            if let Some(Dialog::Import(form)) = app.view.dialog.take() {
                crate::forge_server::ask_add(app, Service::GitLab, "", crate::forge_server::Back::Import(form));
            }
        }
        Act::TryAgain => start(app),
    }
}

/// Sends the address checks whose typing has paused (the Overleaf and Git Address pages).
fn send_checks(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let busy = app.session.import_busy().is_some();
    let Some(Dialog::Import(form)) = app.view.dialog.as_mut() else { return };
    let (check, params) = match &mut form.page {
        Page::Overleaf(p) => {
            let typed = texcraft_engine::typed_address(&p.address);
            let mut params = json!({"url": typed, "background": true});
            if !p.token.trim().is_empty() {
                params["token"] = json!(p.token.trim());
            }
            // Overleaf always wants its token: without one, nothing to check yet.
            let ready = overleaf_address(&typed) && (!p.token.trim().is_empty() || app.session.has_git_credential("git.overleaf.com"));
            if !ready {
                p.check.due = None;
                return;
            }
            (&mut p.check, params)
        }
        Page::Git(p) => {
            let typed = texcraft_engine::typed_address(&p.address);
            if typed.is_empty() {
                p.check.due = None;
                return;
            }
            let mut params = json!({"url": typed, "background": true});
            if !p.username.trim().is_empty() {
                params["username"] = json!(p.username.trim());
            }
            if !p.token.trim().is_empty() {
                params["token"] = json!(p.token.trim());
            }
            (&mut p.check, params)
        }
        _ => return,
    };
    let Some(at) = check.due else { return };
    let wait = PAUSE - (now - at);
    if wait > 0.0 || busy {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.1)));
        return;
    }
    check.due = None;
    let asked = params.to_string();
    check.state = Checked::Running(asked);
    match app.session.execute("git.check_address", &params) {
        Ok(v) if v["started"] == true => {}
        Ok(v) => checked(app, Ok(v)),
        Err(e) => checked(app, Err(e)),
    }
}

/// Whether `address` is an Overleaf project's git address.
fn overleaf_address(address: &str) -> bool {
    address.strip_prefix(OVERLEAF).is_some_and(|id| !id.trim_matches('/').is_empty() && !id.contains(char::is_whitespace))
}

/// An address check ended: its answer on the page that asked.
pub(crate) fn checked(app: &mut App, outcome: Result<Value, String>) {
    let Some(Dialog::Import(form)) = app.view.dialog.as_mut() else { return };
    let check = match &mut form.page {
        Page::Overleaf(p) => &mut p.check,
        Page::Git(p) => &mut p.check,
        _ => return,
    };
    // The page was typed into again since: its own check follows.
    if check.due.is_some() || !matches!(check.state, Checked::Running(_)) {
        return;
    }
    check.state = match outcome {
        Ok(v) if v["ssh"] == true => Checked::Ssh(v["https_url"].as_str().unwrap_or_default().to_owned()),
        Ok(v) if v["needs_sign_in"] == true => Checked::NeedsSignIn,
        Ok(v) => Checked::Found {
            branches: v["branches"].as_array().into_iter().flatten().filter_map(|b| b.as_str().map(str::to_owned)).collect(),
            default: v["default_branch"].as_str().map(str::to_owned),
            host: v["host"].as_str().unwrap_or("other").to_owned(),
            signed_in: v["signed_in"] == true,
        },
        Err(e) => Checked::Failed(e),
    };
    // The default branch chosen, unless one of the list was.
    if let Page::Git(p) = &mut form.page
        && let Checked::Found { branches, default, .. } = &p.check.state
        && !branches.contains(&p.branch)
    {
        p.branch = default.clone().or_else(|| branches.first().cloned()).unwrap_or_default();
    }
}

/// The page's Import: a clone on the desktop (a forge's project in the browser).
fn start(app: &mut App) {
    if matches!(&app.view.dialog, Some(Dialog::Import(f)) if f.link.is_some()) {
        return link::start(app);
    }
    let browser = in_browser(app);
    let Some(Dialog::Import(form)) = app.view.dialog.as_mut() else { return };
    form.error = None;
    let folder = form.into.path.trim().to_owned();
    let remember = form.remember;
    let params = match &form.page {
        Page::Forge(f, _) if browser => {
            let token = if f.use_token { f.token.trim() } else { "" };
            let mut params = json!({"url": qualified(f), "service": f.service.id(), "token": token, "ref": f.branch.trim(), "background": true});
            if !token.is_empty() {
                params["remember"] = json!(remember);
            }
            let (service, name) = (f.service, texcraft_engine::repository_name(&qualified(f)));
            crate::forge::set_opening(app, service);
            match app.session.execute("forge.open", &params) {
                Ok(v) if v["started"] == true => begin(app, name, true),
                Ok(v) => crate::forge::opened(app, &v),
                Err(e) => {
                    if let Some(Dialog::Import(form)) = app.view.dialog.as_mut() {
                        form.error = Some((ImportProblem::Other, e));
                    }
                }
            }
            return;
        }
        Page::Forge(f, _) => {
            let url = match texcraft_engine::forge::Address::parse_as(&qualified(f), Some(f.service)) {
                Ok(a) => a.clone_url(),
                Err(e) => {
                    form.error = Some((ImportProblem::NotFound, e.to_string()));
                    return;
                }
            };
            let mut params = json!({"url": url, "folder": folder, "service": f.service.id(), "background": true, "open": false});
            if !f.branch.trim().is_empty() {
                params["branch"] = json!(f.branch.trim());
            }
            if f.use_token && !f.token.trim().is_empty() {
                params["token"] = json!(f.token.trim());
                params["remember"] = json!(remember);
            }
            params
        }
        Page::Overleaf(p) => {
            let mut params = json!({"url": texcraft_engine::typed_address(&p.address), "folder": folder, "background": true, "open": false});
            if !p.token.trim().is_empty() {
                params["token"] = json!(p.token.trim());
                params["remember"] = json!(remember);
            }
            params
        }
        Page::Git(p) => {
            let mut params = json!({"url": texcraft_engine::typed_address(&p.address), "folder": folder, "background": true, "open": false});
            if !p.branch.trim().is_empty() {
                params["branch"] = json!(p.branch.trim());
            }
            if !p.username.trim().is_empty() {
                params["username"] = json!(p.username.trim());
            }
            if !p.token.trim().is_empty() {
                params["token"] = json!(p.token.trim());
                params["remember"] = json!(remember);
            }
            params
        }
        _ => return,
    };
    let name = PathBuf::from(&folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match app.session.execute("project.clone", &params) {
        Ok(v) if v["started"] == true => begin(app, name, false),
        Ok(v) => cloned(app, &v),
        Err(e) => {
            let problem = app.session.import_problem().unwrap_or(ImportProblem::Other);
            if let Some(Dialog::Import(form)) = app.view.dialog.as_mut() {
                form.error = Some((problem, e));
            }
        }
    }
}

/// An import under way, or one that failed, shown as a card on the start page until it ends.
pub(crate) struct Importing {
    /// The project's name: the new folder's, or the repository's in the browser.
    name: String,
    /// The dialog's page as it was, for Try Again.
    form: Option<Box<ImportForm>>,
    /// The project open when the import began: another one open since means the user went on to
    /// something else, and the import does not take its place.
    before: Option<PathBuf>,
    /// Through the forge's API (the browser), not a clone.
    browser: bool,
    /// Why it failed, in words.
    error: Option<String>,
}

/// The import started: the dialog closes, and its card appears at once on the start page.
fn begin(app: &mut App, name: String, browser: bool) {
    let form = match app.view.dialog.take() {
        Some(Dialog::Import(mut form)) => {
            form.error = None;
            Some(form)
        }
        other => {
            app.view.dialog = other;
            None
        }
    };
    app.importing = Some(Importing { name, form, before: app.session.dir().cloned(), browser, error: None });
    app.start = true;
}

/// The forge page's address with its server when it names none.
fn qualified(f: &OpenForm) -> String {
    let text = f.address.trim();
    let bare = !text.contains("://") && !text.starts_with("git@") && !text.split('/').next().is_some_and(|first| first.contains('.'));
    let public = texcraft_engine::forge::oauth::instance(f.service);
    if bare && f.instance != public && !f.instance.is_empty() { format!("{}/{text}", f.instance) } else { text.to_owned() }
}

/// A clone ended: the project opens, unless another was opened meanwhile (then it joins the recent
/// projects); its card leaves the start page, and the status line says where it went.
pub(crate) fn cloned(app: &mut App, done: &Value) {
    if matches!(app.view.dialog, Some(Dialog::Import(_))) {
        app.view.dialog = None;
    }
    let importing = app.importing.take();
    let folder = done["folder"].as_str().unwrap_or_default().to_owned();
    let mut done = done.clone();
    if done["opened"] == false {
        let elsewhere = importing.as_ref().is_some_and(|i| !app.start || app.session.dir() != i.before.as_ref());
        if elsewhere {
            app.prefs.recent.retain(|p| *p != folder);
            app.prefs.recent.insert(0, folder.clone());
            app.prefs.recent.truncate(crate::prefs::RECENT_MAX);
            app.prefs_changed = true;
            let said = tl!("Imported {name} ({branch}) into {folder}: it is in the recent projects.")
                .replace("{name}", done["name"].as_str().unwrap_or_default())
                .replace("{branch}", done["branch"].as_str().unwrap_or_default())
                .replace("{folder}", &folder);
            app.view.status = Some((said, false));
            return;
        }
        let Some(opened) = app.run("project.open", json!({"path": folder})) else { return };
        if let Some(note) = opened.get("note") {
            done["note"] = note.clone();
        }
    }
    app.start = false;
    app.pdf = crate::pdf_view::PdfView::default();
    app.view.last_edit = Some(0.0);
    app.remember_project();
    let mut said = tl!("Imported {name} ({branch}) into {folder}.")
        .replace("{name}", done["name"].as_str().unwrap_or_default())
        .replace("{branch}", done["branch"].as_str().unwrap_or_default())
        .replace("{folder}", done["folder"].as_str().unwrap_or_default());
    if let Some(note) = done["note"].as_str() {
        said = format!("{said} {}", crate::i18n::msg(note));
    }
    let warning = done["warning"].as_str();
    if let Some(w) = warning {
        said = format!("{said} {}", crate::i18n::msg(w));
    }
    app.view.status = Some((said, warning.is_some()));
}

/// A check or a clone failed: said on its page. Whether it was one.
pub(crate) fn failed(app: &mut App, was: Option<&str>, e: &str) -> bool {
    match was {
        Some("check_address") => {
            checked(app, Err(e.to_owned()));
            true
        }
        Some("clone") => {
            let problem = app.session.import_problem().unwrap_or(ImportProblem::Other);
            if let Some(importing) = app.importing.as_mut() {
                importing.error = Some(e.to_owned());
                if let Some(form) = importing.form.as_mut() {
                    form.error = Some((problem, e.to_owned()));
                }
                return true;
            }
            match app.view.dialog.as_mut() {
                Some(Dialog::Import(form)) => form.error = Some((problem, e.to_owned())),
                _ => app.view.status = Some((e.to_owned(), true)),
            }
            true
        }
        Some("open") if app.importing.as_ref().is_some_and(|i| i.browser) => {
            if let Some(importing) = app.importing.as_mut() {
                importing.error = Some(e.to_owned());
                if let Some(form) = importing.form.as_mut() {
                    form.error = Some((ImportProblem::Other, e.to_owned()));
                }
            }
            true
        }
        _ => false,
    }
}

/// Change… chose `parent`: it becomes the default for new folders.
pub(crate) fn parent_chosen(app: &mut App, parent: &Path) {
    app.prefs.clone_folder = Some(parent.display().to_string());
    app.prefs_changed = true;
    if let Some(Dialog::Import(form)) = app.view.dialog.as_mut() {
        form.into.edited = false;
    }
    crate::where_step::parent_chosen(app);
}

/// The Zip File card's file was chosen (the desktop): it is unzipped into a new folder in the
/// folder new projects go to, and opens there.
pub(crate) fn zip_chosen(app: &mut App, zip: &Path) {
    let parent = app.prefs.clone_folder.as_ref().map(PathBuf::from).or_else(|| app.documents.clone());
    if matches!(app.view.dialog, Some(Dialog::Import(_))) {
        app.view.dialog = None;
    }
    let name = zip.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".to_owned());
    app.zip_into = parent;
    app.import_project(json!({"source": zip.display().to_string()}), name);
}

/// A folder was opened (the Folder card's picker answered): the dialog's work is done.
pub(crate) fn folder_opened(app: &mut App) {
    if matches!(app.view.dialog, Some(Dialog::Import(_))) {
        app.view.dialog = None;
    }
}

fn draw(ui: &mut egui::Ui, t: &Tokens, form: &mut ImportForm, info: &Info) -> Vec<Act> {
    let mut acts = Vec::new();
    let title =
        if form.link.is_some() { link::title(form.page.source()) } else { form.page.source().map_or(tl!("Import a Project"), Source::page_title) };
    header(ui, t, title, !matches!(form.page, Page::Grid), info, &mut acts);
    match &mut form.page {
        Page::Grid => {
            if let Some(s) = grid(ui, t, info, form.focus_card.take()) {
                acts.push(Act::Choose(s));
            }
        }
        Page::Unavailable(source) if form.link.is_some() => link::unavailable(ui, t, *source, &mut acts),
        Page::Unavailable(source) => unavailable(ui, t, *source, &mut acts),
        _ => page(ui, t, form, info, &mut acts),
    }
    acts
}

/// The title row: Back on a page, the title, Close. On a phone, the sheet's bar holds them.
fn header(ui: &mut egui::Ui, t: &Tokens, title: &str, on_page: bool, info: &Info, acts: &mut Vec<Act>) {
    let back = on_page.then_some(tl!("Back to sources"));
    let asked = crate::dialogs::header(ui, t, title, back, info.running.is_none() && !info.linking, |_| {});
    if asked.back {
        acts.push(Act::Back);
    }
    if asked.close {
        acts.push(Act::Close);
    }
}

/// The two groups of cards. Returns the card chosen.
fn grid(ui: &mut egui::Ui, t: &Tokens, info: &Info, focus: Option<Source>) -> Option<Source> {
    let width = ui.available_width();
    let columns = columns(width, 4);
    let list = info.narrow || columns < 2;
    let card_width = card_width(width, columns);
    let mut chosen = None;
    // Every card, in order: its source, its response, its row and column.
    let mut cells: Vec<(Source, egui::Response, usize, usize)> = Vec::new();
    let mut row_base = 0;
    let groups: &[(&str, bool)] = if info.link {
        &[(tl!("Where the repository is"), true)]
    } else {
        &[(tl!("Online"), true), (if info.browser { tl!("On this device") } else { tl!("On this computer") }, false)]
    };
    for &(name, online) in groups {
        let sources: Vec<&(Source, Line)> = info.lines.iter().filter(|(s, _)| s.online() == online).collect();
        let builder = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Group).accessibility_label(name);
        ui.scope_builder(builder, |ui| {
            ui.label(RichText::new(name).font(t.small()).color(t.text_muted));
            ui.add_space(t.unit * 1.5);
            if list {
                for (i, (source, line)) in sources.iter().enumerate() {
                    let r = list_row(ui, t, source.look(), line);
                    cells.push((*source, r, row_base + i, 0));
                }
                row_base += sources.len();
            } else {
                for (r, chunk) in sources.chunks(columns).enumerate() {
                    let looks: Vec<(Look<'_>, &Line)> = chunk.iter().map(|(s, l)| (s.look(), l)).collect();
                    for (c, resp) in cards_row(ui, t, &looks, card_width).into_iter().enumerate() {
                        cells.push((chunk[c].0, resp, row_base + r, c));
                    }
                }
                row_base += sources.len().div_ceil(columns);
            }
            ui.add_space(t.unit * 2.0);
        });
    }
    for (source, r, ..) in &cells {
        if r.clicked() {
            chosen = Some(*source);
        }
        if focus == Some(*source) {
            r.request_focus();
        }
    }
    arrows(ui, &cells);
    chosen
}

/// The arrows move the keyboard between the cards in two dimensions; Home and End to the first
/// and last.
pub(crate) fn arrows<S>(ui: &egui::Ui, cells: &[(S, egui::Response, usize, usize)]) {
    let Some(at) = cells.iter().position(|(_, r, ..)| r.has_focus()) else { return };
    let keys = [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown, Key::Home, Key::End];
    let Some(key) = ui.input(|i| keys.into_iter().find(|k| i.modifiers.is_none() && i.key_pressed(*k))) else { return };
    // The grid's own order, across its two groups, in place of egui's move by geometry.
    ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    let (row, col) = (cells[at].2, cells[at].3);
    let nearest = |r: usize| cells.iter().enumerate().filter(|(_, c)| c.2 == r).min_by_key(|(_, c)| c.3.abs_diff(col)).map(|(i, _)| i);
    let target = match key {
        Key::ArrowLeft => at.checked_sub(1),
        Key::ArrowRight => (at + 1 < cells.len()).then_some(at + 1),
        Key::ArrowUp => row.checked_sub(1).and_then(nearest),
        Key::ArrowDown => nearest(row + 1),
        Key::Home => Some(0),
        _ => Some(cells.len() - 1),
    };
    if let Some(i) = target {
        cells[i].1.request_focus();
    }
}

/// How many columns of cards at least `CARD_MIN` wide fit in `width`, up to `most`.
pub(crate) fn columns(width: f32, most: usize) -> usize {
    (((width + GAP) / (CARD_MIN + GAP)).floor() as usize).clamp(1, most)
}

/// A row of cards `width` wide each, as tall as the tallest; their responses, in order.
pub(crate) fn cards_row(ui: &mut egui::Ui, t: &Tokens, cards: &[(Look<'_>, &Line)], width: f32) -> Vec<egui::Response> {
    let galleys: Vec<_> = cards.iter().map(|(look, l)| card_text(ui, t, look.title, l, width)).collect();
    let height = galleys.iter().map(|(a, b)| 10.0 + 18.0 + 6.0 + a.size().y + 2.0 + b.size().y + 10.0).fold(CARD_HEIGHT, f32::max);
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for ((look, line), (title, text)) in cards.iter().zip(galleys) {
            out.push(card(ui, t, *look, line, (title, text), vec2(width, height)));
        }
    });
    ui.add_space(GAP - ui.spacing().item_spacing.y);
    out
}

/// The width of a card when `columns` share `width`.
pub(crate) fn card_width(width: f32, columns: usize) -> f32 {
    (width - GAP * (columns as f32 - 1.0)) / columns as f32
}

/// A card's title and line, laid out for its width.
fn card_text(ui: &egui::Ui, t: &Tokens, title: &str, line: &Line, width: f32) -> (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>) {
    let room = width - 24.0;
    let colour = if line.unavailable { t.text_muted } else { t.text };
    let title = ui.painter().layout(title.to_owned(), t.strong(), colour, room);
    let text_room = room - if line.signed_in || line.unavailable { 16.0 } else { 0.0 };
    let text = ui.painter().layout(line.text.clone(), t.small(), t.text_muted, text_room);
    (title, text)
}

/// The accessible name of a card: its title, then its line.
fn card_name(title: &str, line: &Line) -> String {
    format!("{title}, {}", line.text)
}

/// Marks a card's node: a button, unavailable here when it is (still focusable: it explains).
fn describe(ui: &egui::Ui, r: &egui::Response, title: &str, line: &Line) {
    let name = card_name(title, line);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    if line.unavailable {
        ui.ctx().accesskit_node_builder(r.id, |node| node.set_description(tl!("Not available here: choose it to see why")));
    }
}

fn card(
    ui: &mut egui::Ui,
    t: &Tokens,
    look: Look<'_>,
    line: &Line,
    (title, text): (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>),
    size: egui::Vec2,
) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    describe(ui, &r, look.title, line);
    if !ui.is_rect_visible(rect) {
        return r;
    }
    let painter = ui.painter();
    let radius = egui::CornerRadius::same(8);
    painter.rect_filled(rect, radius, widgets::state_fill(t, &r, t.surface));
    let border = blend(t.surface, t.control_border);
    if line.unavailable {
        dashed(painter, rect.shrink(0.5), egui::Stroke::new(1.0, border));
    } else {
        painter.rect_stroke(rect, radius, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    }
    let (icon, brand) = (look.icon, look.brand);
    let icon_colour = if line.unavailable {
        t.text_muted
    } else if brand {
        t.text
    } else {
        t.accent
    };
    let at = rect.min + vec2(12.0, 10.0);
    icons::paint(ui, egui::Rect::from_min_size(at, vec2(18.0, 18.0)), icon, 18.0, icon_colour);
    let title_at = at + vec2(0.0, 18.0 + 6.0);
    painter.galley(title_at, title.clone(), t.text);
    let mut line_at = title_at + vec2(0.0, title.size().y + 2.0);
    let mark = if line.signed_in {
        Some(("circle-check", t.success))
    } else if line.unavailable {
        Some(("info", t.text_muted))
    } else {
        None
    };
    if let Some((name, colour)) = mark {
        icons::paint(ui, egui::Rect::from_min_size(line_at + vec2(0.0, 1.0), vec2(12.0, 12.0)), name, 12.0, colour);
        line_at += vec2(16.0, 0.0);
    }
    painter.galley(line_at, text, t.text_muted);
    widgets::focus_ring(ui, t, &r, rect, radius);
    r
}

/// A dashed outline round `rect`: an unavailable card's.
fn dashed(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    painter.extend(egui::Shape::dashed_line(&corners, stroke, 5.0, 3.0));
}

/// A phone's row for a source: icon, title and line, and a chevron.
pub(crate) fn list_row(ui: &mut egui::Ui, t: &Tokens, look: Look<'_>, line: &Line) -> egui::Response {
    let width = ui.available_width();
    let room = width - 18.0 - 12.0 - 24.0 - 8.0;
    let colour = if line.unavailable { t.text_muted } else { t.text };
    let title = ui.painter().layout(look.title.to_owned(), t.strong(), colour, room);
    let text = ui.painter().layout(line.text.clone(), t.small(), t.text_muted, room - 16.0);
    let height = (title.size().y + text.size().y + 18.0).max(56.0);
    let (rect, r) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    describe(ui, &r, look.title, line);
    if !ui.is_rect_visible(rect) {
        return r;
    }
    let painter = ui.painter();
    let fill = widgets::state_fill(t, &r, t.surface);
    if fill != t.surface {
        painter.rect_filled(rect, t.corner(), fill);
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, t.separator));
    let (icon, brand) = (look.icon, look.brand);
    let icon_colour = if line.unavailable {
        t.text_muted
    } else if brand {
        t.text
    } else {
        t.accent
    };
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 4.0 + 9.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(ui, icon_rect, icon, 18.0, icon_colour);
    let x = rect.left() + 4.0 + 18.0 + 12.0;
    let top = rect.center().y - (title.size().y + text.size().y + 2.0) / 2.0;
    painter.galley(egui::pos2(x, top), title.clone(), t.text);
    let mut line_at = egui::pos2(x, top + title.size().y + 2.0);
    if line.signed_in || line.unavailable {
        let (name, c) = if line.signed_in { ("circle-check", t.success) } else { ("info", t.text_muted) };
        icons::paint(ui, egui::Rect::from_min_size(line_at + vec2(0.0, 1.0), vec2(12.0, 12.0)), name, 12.0, c);
        line_at.x += 16.0;
    }
    painter.galley(line_at, text, t.text_muted);
    let chevron = egui::Rect::from_center_size(egui::pos2(rect.right() - 12.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui, chevron, "chevron-right", 16.0, t.text_muted);
    widgets::focus_ring(ui, t, &r, rect.shrink(2.0), t.corner());
    r
}

/// The page of a source this place cannot reach: why, the way out, and Back.
fn unavailable(ui: &mut egui::Ui, t: &Tokens, source: Source, acts: &mut Vec<Act>) {
    let why = match source {
        Source::Overleaf => tl!("Overleaf's git server does not answer web pages, so the browser cannot reach it."),
        _ => tl!("Most git servers do not answer web pages, so the browser cannot reach them."),
    };
    egui::Frame::new()
        .fill(t.frame)
        .stroke(egui::Stroke::new(1.0, t.separator))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui, rect, "info", 16.0, t.text_muted);
                ui.vertical(|ui| {
                    let said = ui.label(why);
                    crate::status::announce(ui, &said, why);
                    ui.add_space(t.unit * 2.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(egui::Button::image_and_text(icons::image("download", t.icon_size, t.text), tl!("Get the Desktop App"))).clicked() {
                            acts.push(Act::DesktopApp);
                        }
                        if ui
                            .add(egui::Button::image_and_text(icons::image("file-archive", t.icon_size, t.text), tl!("Import a Zip File…")))
                            .clicked()
                        {
                            acts.push(Act::Choose(Source::Zip));
                        }
                    });
                });
            });
        });
    ui.add_space(t.unit * 2.0);
    let hint = match source {
        Source::Overleaf => tl!("For a zip: download the project as a zip on Overleaf, then import it here."),
        _ => tl!("For a zip: download the repository as a zip on its server's page, then import it here."),
    };
    ui.label(RichText::new(hint).font(t.small()).color(t.text_muted));
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        if crate::dialogs::secondary(ui, tl!("Back"), true).clicked() {
            acts.push(Act::Back);
        }
    });
}

/// A row of the form: its label beside the field (under it on a phone); the label's id names the
/// field.
pub(crate) fn row<R>(ui: &mut egui::Ui, t: &Tokens, narrow: bool, label: &str, add: impl FnOnce(&mut egui::Ui, egui::Id) -> R) -> R {
    if narrow {
        let l = ui.label(RichText::new(label).color(t.text_muted));
        let out = add(ui, l.id);
        ui.add_space(t.unit * 2.0);
        return out;
    }
    let out = ui
        .horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 30.0), Sense::hover());
            let l = ui.put(rect, egui::Label::new(RichText::new(label).color(t.text_muted)).wrap());
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                add(ui, l.id)
            })
            .inner
        })
        .inner;
    ui.add_space(t.unit * 2.0);
    out
}

/// A line under a field: muted, or a success or an error with its icon; announced when it changes.
pub(crate) fn hint(ui: &mut egui::Ui, t: &Tokens, text: &str, tone: Tone) {
    if text.is_empty() {
        return;
    }
    let (colour, icon) = match tone {
        Tone::Plain => (t.text_muted, None),
        Tone::Good => (t.success, Some("circle-check")),
        Tone::Bad => (t.error, Some("circle-exclamation-point")),
        Tone::Busy => (t.text_muted, None),
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        if tone == Tone::Busy {
            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            widgets::spinner(ui, rect.center(), 5.0, t.text_muted);
            ui.ctx().request_repaint();
        }
        if let Some(icon) = icon {
            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            icons::paint(ui, rect, icon, 12.0, colour);
        }
        let label = ui.label(RichText::new(text).font(t.small()).color(colour));
        if tone != Tone::Plain {
            crate::status::announce(ui, &label, text);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Good,
    Bad,
    Busy,
}

/// A source's page: its fields, Into, the error and the footer.
fn page(ui: &mut egui::Ui, t: &Tokens, form: &mut ImportForm, info: &Info, acts: &mut Vec<Act>) {
    let running = info.running.is_some() || info.linking;
    let focus = std::mem::take(&mut form.focus_field);
    let mut ready = false;
    let mut needs_folder = !info.browser && !info.link;
    let mut token_typed = false;
    let mut enter = false;
    ui.add_enabled_ui(!running, |ui| match &mut form.page {
        Page::Forge(f, source) => {
            let (r, e) = forge_fields(ui, t, f, *source, info, focus, acts);
            ready = r;
            enter = e;
            token_typed = f.use_token && !f.token.trim().is_empty();
            needs_folder = !info.browser && !info.link;
        }
        Page::Overleaf(p) => {
            let (r, e) = overleaf_fields(ui, t, p, info, focus);
            ready = r;
            enter = e;
            token_typed = !p.token.trim().is_empty();
        }
        Page::Git(p) => {
            let (r, e) = git_fields(ui, t, p, info, focus, acts);
            ready = r;
            enter = e;
            token_typed = !p.token.trim().is_empty();
        }
        _ => {}
    });
    if token_typed {
        ui.add_enabled_ui(!running, |ui| remember_row(ui, t, &mut form.remember, info));
    }
    if needs_folder {
        ui.add_enabled_ui(!running, |ui| {
            ready &= into_row(ui, t, &mut form.into, info, acts);
        });
        let note = tl!("TeXCraft makes a git clone in this folder: the whole history, up to {limit}, so that Sync works at once.")
            .replace("{limit}", &info.limit);
        ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
    }
    if let Some((problem, e)) = &form.error {
        ui.add_space(t.unit * 2.0);
        hint(ui, t, &crate::i18n::msg(e), Tone::Bad);
        if *problem == ImportProblem::Network && ui.button(tl!("Try Again")).clicked() {
            acts.push(Act::TryAgain);
        }
    }
    if info.link {
        link::part(ui, t, form, info, ready, enter, acts);
        return;
    }
    footer(ui, t, ready, running, info, enter, acts);
}

/// Remember on this computer (ticked by default), and where the token goes and how it is removed.
fn remember_row(ui: &mut egui::Ui, t: &Tokens, remember: &mut bool, info: &Info) {
    row(ui, t, info.narrow, "", |ui, _| {
        let label = match (info.browser, info.kept) {
            (true, "browser") => tl!("Remember in this browser"),
            (true, _) => tl!("Remember in this tab"),
            _ => tl!("Remember on this computer"),
        };
        ui.checkbox(remember, label);
        let kept = match (*remember, info.kept) {
            (true, "keychain") => tl!(
                "Kept in the system's keychain, so that Sync, push, pull and later imports need no token. Settings › Accounts lists it, with Replace… and Disconnect…."
            ),
            (true, "browser") => {
                tl!("Kept in this browser's storage for this site, for every tab. Settings › Accounts lists it, with Replace… and Disconnect….")
            }
            (true, "tab") => {
                tl!("Kept in this tab's session storage until the tab is closed. Settings › Accounts lists it, with Replace… and Disconnect….")
            }
            (true, _) if info.browser => tl!("This page has no storage for it: the token is kept while the page is open only."),
            (true, _) => tl!("This computer has no keychain TeXCraft can use: the token is kept while TeXCraft runs only."),
            (false, _) if info.browser => tl!("Kept while this page is open, then forgotten."),
            (false, _) => tl!("Kept while TeXCraft runs, then forgotten."),
        };
        ui.label(RichText::new(kept).font(t.small()).color(t.text_muted));
    });
}

/// Cancel and Import, or while a clone runs its progress and Cancel.
fn footer(ui: &mut egui::Ui, t: &Tokens, ready: bool, running: bool, info: &Info, enter: bool, acts: &mut Vec<Act>) {
    ui.add_space(t.unit * 3.0);
    if !info.narrow {
        widgets::hairline(ui, t);
        ui.add_space(t.unit * 2.0);
    }
    actions(ui, |ui| {
        if crate::dialogs::primary(ui, t, tl!("Import"), ready && !running, false).clicked() || (enter && ready && !running) {
            acts.push(Act::Import);
        }
        if crate::dialogs::secondary(ui, tl!("Cancel"), true).clicked() {
            acts.push(if running { Act::StopImport } else { Act::Close });
        }
        // The progress takes the footer's place, so that nothing moves.
        if let Some(phase) = info.running {
            progress(ui, t, phase);
        }
    });
}

/// A clone's progress: a thin bar and its phase, a live region said at each phase.
fn progress(ui: &mut egui::Ui, t: &Tokens, phase: ClonePhase) {
    let text = match phase {
        ClonePhase::Connecting => tl!("Asking the server for the project…"),
        ClonePhase::Receiving => tl!("Receiving the history…"),
        ClonePhase::Writing => tl!("Writing the files…"),
    };
    crate::dialogs::progress(ui, t, text);
}

/// The Into row: the folder's path and Change…, checked in place. Whether it can be used.
fn into_row(ui: &mut egui::Ui, t: &Tokens, into: &mut Into, info: &Info, acts: &mut Vec<Act>) -> bool {
    let mut change = false;
    let shown = FolderRow { label: tl!("Into"), narrow: info.narrow, has_parent: info.parent.is_some() };
    let (ok, _) = folder_field(ui, t, &shown, &mut into.path, &mut into.edited, &mut change);
    if change {
        acts.push(Act::ChangeFolder);
    }
    ok
}

/// How a new folder's row is shown: its label, on a phone or not, and whether a parent folder is
/// known for the default.
pub(crate) struct FolderRow<'a> {
    pub label: &'a str,
    pub narrow: bool,
    pub has_parent: bool,
}

/// A new folder's row: its full path and Change…, checked in place (a folder that is there and
/// not empty is said, with the next free name). Whether it can be used, and the path's field.
pub(crate) fn folder_field(
    ui: &mut egui::Ui,
    t: &Tokens,
    shown: &FolderRow<'_>,
    path: &mut String,
    edited: &mut bool,
    change: &mut bool,
) -> (bool, Option<egui::Response>) {
    let (narrow, has_parent) = (shown.narrow, shown.has_parent);
    row(ui, t, narrow, shown.label, |ui, label| {
        let edit = egui::TextEdit::singleline(path).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0));
        let field = if narrow {
            let field = ui.add(edit).labelled_by(label);
            if ui.button(tl!("Change…")).clicked() {
                *change = true;
            }
            field
        } else {
            ui.horizontal(|ui| {
                let room = ui.available_width() - 96.0;
                let field = ui.add_sized(vec2(room.max(120.0), 30.0), edit).labelled_by(label);
                if crate::dialogs::secondary(ui, tl!("Change…"), true).clicked() {
                    *change = true;
                }
                field
            })
            .inner
        };
        if field.changed() {
            *edited = true;
        }
        let typed = PathBuf::from(path.trim());
        let name = typed.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if path.trim().is_empty() || !has_parent && !*edited {
            hint(ui, t, tl!("Choose where to put the project: Change…"), Tone::Plain);
            return (false, Some(field));
        }
        if path.trim_end().ends_with(['/', '\\']) {
            // No project chosen yet: its name comes with it.
            hint(ui, t, tl!("The new folder takes the project's name."), Tone::Plain);
            return (false, Some(field));
        }
        if name.is_empty() || !typed.is_absolute() {
            hint(ui, t, tl!("Give the whole path of the new folder."), Tone::Bad);
            return (false, Some(field));
        }
        if !texcraft_engine::folder_free(&typed) {
            let parent = typed.parent().unwrap_or(&typed);
            let free = texcraft_engine::free_folder(parent, &name);
            let free_name = free.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            hint(ui, t, &tl!("A folder named {name} is there already.").replace("{name}", &name), Tone::Bad);
            if ui.button(tl!("Use {name}").replace("{name}", &free_name)).clicked() {
                *path = free.display().to_string();
                *edited = true;
            }
            return (false, Some(field));
        }
        (true, Some(field))
    })
}

/// The GitLab, GitHub and GitLab Server page's fields. Whether Import can run, and whether Enter
/// was pressed in a field.
fn forge_fields(ui: &mut egui::Ui, t: &Tokens, f: &mut OpenForm, source: Source, info: &Info, focus: bool, acts: &mut Vec<Act>) -> (bool, bool) {
    let (narrow, now) = (info.narrow, info.now);
    if source == Source::GitLabServer {
        row(ui, t, narrow, tl!("Server"), |ui, label| server_choice(ui, f, info, label, acts));
    }
    let mut action = None;
    let mut signed_in = false;
    if let Some(view) = &info.view {
        let was = f.use_token;
        signed_in =
            row(ui, t, narrow, tl!("Account"), |ui, _| crate::forge::sign_in_or_token(ui, t, view, &mut f.use_token, !info.unknown, &mut action));
        if was != f.use_token {
            crate::forge_pick::key_changed(f, now);
        }
    }
    if let Some(a) = action {
        acts.push(Act::SignIn(a));
    }
    let mut responses = Vec::new();
    if f.use_token {
        let token = row(ui, t, narrow, tl!("Personal access token"), |ui, label| {
            let edit = egui::TextEdit::singleline(&mut f.token)
                .password(true)
                .hint_text(crate::forge::token_hint(f.service))
                .desired_width(f32::INFINITY)
                .margin(vec2(8.0, 6.0));
            let r = ui.add(edit).labelled_by(label);
            let host = texcraft_engine::forge::servers::host_of(&crate::forge::instance_of_form(f)).to_owned();
            let kept = match (info.kept_token && f.token.trim().is_empty(), info.browser) {
                (true, true) => {
                    tl!("A token for {host} is kept in this tab: leave this empty to use it, or type another to replace it.").replace("{host}", &host)
                }
                (true, false) => tl!("A token for {host} is kept on this computer: leave this empty to use it, or type another to replace it.")
                    .replace("{host}", &host),
                (false, _) => String::new(),
            };
            crate::forge::token_note(ui, t, &kept, f.service);
            r
        });
        if token.changed() {
            crate::forge_pick::key_changed(f, now);
        }
        responses.push(token);
    }
    let address = row(ui, t, narrow, tl!("Project"), |ui, label| {
        let field = crate::forge_pick::project_field(ui, t, f, label);
        other_service(ui, t, f, acts);
        if info.unknown && !f.address.trim().is_empty() && !(f.service == Service::GitHub && info.browser) {
            let host = texcraft_engine::forge::servers::host_of(&crate::forge::instance_of_form(f)).to_owned();
            let text = tl!("TeXCraft cannot sign in to {host} yet: add the server once, and sign in to it as to gitlab.com. Until then, use a personal access token.").replace("{host}", &host);
            ui.label(RichText::new(text).font(t.small()).color(t.text_muted));
            if ui.button(tl!("Add {host} for Signing In…").replace("{host}", &host)).clicked() {
                acts.push(Act::SignIn(SignInAction::AddServer { service: f.service, address: crate::forge::instance_of_form(f) }));
            }
        }
        field
    });
    if focus && (signed_in || !f.use_token) {
        address.request_focus();
    } else if focus && let Some(first) = responses.first() {
        first.request_focus();
    }
    responses.push(address);
    if let Some(branch) = row(ui, t, narrow, tl!("Branch"), |ui, label| crate::forge_pick::branch_field(ui, t, f, label)) {
        responses.push(branch);
    }
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
    let key = signed_in || (f.use_token && !f.token.trim().is_empty()) || info.kept_token;
    let ready = !f.address.trim().is_empty() && key && !f.instance.is_empty();
    (ready, enter)
}

/// The GitLab Server page's server: a drop-down of the servers added, each with its state, and
/// Add a Server… last.
fn server_choice(ui: &mut egui::Ui, f: &mut OpenForm, info: &Info, label: egui::Id, acts: &mut Vec<Act>) {
    let state = |signed: bool| if signed { tl!("signed in") } else { tl!("not signed in") };
    let text = |(_, host, signed): &(String, String, bool)| format!("{host} ({})", state(*signed));
    let shown = info.servers.iter().find(|s| s.0 == f.instance).map(text).unwrap_or_default();
    let mut chosen = None;
    let combo = craft_ui::ComboBox::from_id_salt("import-server").width(ui.available_width().min(360.0)).selected_text(shown).show_ui(ui, |ui| {
        for s in &info.servers {
            if ui.selectable_label(s.0 == f.instance, text(s)).clicked() {
                chosen = Some(s.0.clone());
            }
        }
        if ui.selectable_label(false, tl!("Add a Server…")).clicked() {
            acts.push(Act::AddServer);
        }
    });
    let _ = combo.response.labelled_by(label);
    if let Some(i) = chosen
        && i != f.instance
    {
        f.instance = i;
        f.picker = crate::forge_pick::Picker::new();
    }
}

/// An address on the other public forge than the page's: Switch to it, address kept.
fn other_service(ui: &mut egui::Ui, t: &Tokens, f: &mut OpenForm, acts: &mut Vec<Act>) {
    let text = f.address.trim();
    let names_host = text.contains("://") || text.starts_with("git@") || text.split('/').next().is_some_and(|first| first.contains('.'));
    let Ok(known) = texcraft_engine::forge::Address::parse(text) else { return };
    if !names_host
        || !matches!(known.instance.as_str(), texcraft_engine::forge::GITHUB_COM | texcraft_engine::forge::GITLAB_COM)
        || known.service == f.service
    {
        return;
    }
    let (said, button, source) = match known.service {
        Service::GitHub => (tl!("This is a GitHub address."), tl!("Switch to GitHub"), Source::GitHub),
        Service::GitLab => (tl!("This is a GitLab address."), tl!("Switch to GitLab"), Source::GitLab),
    };
    // No suggestions over the way to the other service's page.
    f.picker.open = false;
    ui.horizontal_wrapped(|ui| {
        let l = ui.label(RichText::new(said).font(t.small()).color(t.text_muted));
        crate::status::announce(ui, &l, said);
        if ui.small_button(button).clicked() {
            acts.push(Act::Switch(source, text.to_owned()));
        }
    });
}

/// A password field with Show, named by `label`.
fn secret(ui: &mut egui::Ui, value: &mut String, show: &mut bool, label: egui::Id) -> egui::Response {
    ui.horizontal(|ui| {
        let room = ui.available_width() - 72.0;
        let edit = egui::TextEdit::singleline(value).password(!*show).margin(vec2(8.0, 6.0));
        let r = ui.add_sized(vec2(room.max(120.0), 30.0), edit).labelled_by(label);
        let toggle = ui.add(egui::Button::selectable(*show, tl!("Show")).min_size(vec2(64.0, 30.0)));
        if toggle.clicked() {
            *show = !*show;
        }
        r
    })
    .inner
}

fn overleaf_fields(ui: &mut egui::Ui, t: &Tokens, p: &mut OverleafPage, info: &Info, focus: bool) -> (bool, bool) {
    let (narrow, now) = (info.narrow, info.now);
    let typed = texcraft_engine::typed_address(&p.address);
    let address = row(ui, t, narrow, tl!("Clone address"), |ui, label| {
        let edit =
            egui::TextEdit::singleline(&mut p.address).hint_text("https://git.overleaf.com/…").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0));
        let r = ui.add(edit).labelled_by(label);
        let typed = texcraft_engine::typed_address(&p.address);
        if !typed.is_empty() && !overleaf_address(&typed) {
            hint(ui, t, tl!("This is not an Overleaf git address. It starts with https://git.overleaf.com/."), Tone::Bad);
        } else {
            check_line(ui, t, &p.check.state, tl!("Project found"));
        }
        r
    });
    if address.changed() {
        p.check.typed(now);
    }
    if focus {
        address.request_focus();
    }
    let token = row(ui, t, narrow, tl!("Git token"), |ui, label| {
        let r = secret(ui, &mut p.token, &mut p.show, label);
        let line = if info.known && p.token.trim().is_empty() {
            tl!("A token is kept on this computer: leave this empty to use it.")
        } else {
            tl!("Made in your Overleaf account settings, under git. Your password does not work here.")
        };
        hint(ui, t, line, Tone::Plain);
        r
    });
    if token.changed() {
        p.check.typed(now);
    }
    let enter = (address.lost_focus() || token.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
    let failed = matches!(p.check.state, Checked::Failed(_));
    (overleaf_address(&typed) && (!p.token.trim().is_empty() || info.known) && !failed, enter)
}

/// The line under an address: the check's state.
fn check_line(ui: &mut egui::Ui, t: &Tokens, state: &Checked, found: &str) {
    match state {
        Checked::Idle => {}
        Checked::Running(_) => hint(ui, t, tl!("Checking…"), Tone::Busy),
        Checked::Found { .. } => hint(ui, t, found, Tone::Good),
        Checked::NeedsSignIn => hint(ui, t, tl!("This repository needs signing in."), Tone::Plain),
        Checked::Ssh(_) => hint(ui, t, tl!("TeXCraft uses git over HTTPS."), Tone::Bad),
        Checked::Failed(e) => hint(ui, t, &crate::i18n::msg(e), Tone::Bad),
    }
}

fn git_fields(ui: &mut egui::Ui, t: &Tokens, p: &mut GitPage, info: &Info, focus: bool, acts: &mut Vec<Act>) -> (bool, bool) {
    let (narrow, now) = (info.narrow, info.now);
    let state = p.check.state.clone();
    let address = row(ui, t, narrow, tl!("Address"), |ui, label| {
        let edit = egui::TextEdit::singleline(&mut p.address)
            .hint_text("https://git.example.org/lab/paper.git")
            .desired_width(f32::INFINITY)
            .margin(vec2(8.0, 6.0));
        let r = ui.add(edit).labelled_by(label);
        let found = match &state {
            Checked::Found { branches, signed_in, .. } if *signed_in => {
                crate::i18n::trn(branches.len(), "Signed in · {n} branch", "Signed in · {n} branches")
            }
            Checked::Found { branches, .. } => crate::i18n::trn(branches.len(), "Public repository · {n} branch", "Public repository · {n} branches"),
            _ => String::new(),
        };
        check_line(ui, t, &state, &found);
        if let Checked::Ssh(https) = &state
            && ui.button(tl!("Use {address}").replace("{address}", https)).clicked()
        {
            p.address = https.clone();
            p.check.typed(f64::NEG_INFINITY);
        }
        if let Checked::Found { host, .. } = &state {
            let (said, source) = match host.as_str() {
                "gitlab" => (tl!("This is a GitLab project: use the GitLab card for its suggestions."), Some(Source::GitLab)),
                "github" => (tl!("This is a GitHub repository: use the GitHub card for its suggestions."), Some(Source::GitHub)),
                _ => ("", None),
            };
            if let Some(source) = source {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(said).font(t.small()).color(t.text_muted));
                    if ui.small_button(tl!("Use the Card")).clicked() {
                        acts.push(Act::Switch(source, texcraft_engine::typed_address(&p.address)));
                    }
                });
            }
        }
        r
    });
    if address.changed() {
        p.check.typed(now);
    }
    if address.lost_focus() && p.check.due.is_some() {
        p.check.due = Some(f64::NEG_INFINITY);
    }
    if focus {
        address.request_focus();
    }
    let mut responses = vec![address];
    // The sign-in fields appear when the server wants them (and stay once something is typed).
    let asks = matches!(state, Checked::NeedsSignIn) || !p.username.is_empty() || !p.token.is_empty();
    if asks {
        let user = row(ui, t, narrow, tl!("User name"), |ui, label| {
            ui.add(egui::TextEdit::singleline(&mut p.username).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0))).labelled_by(label)
        });
        let token = row(ui, t, narrow, tl!("Password or token"), |ui, label| secret(ui, &mut p.token, &mut p.show, label));
        if user.changed() || token.changed() {
            p.check.typed(now);
        }
        responses.push(user);
        responses.push(token);
    }
    row(ui, t, narrow, tl!("Branch"), |ui, label| match &state {
        Checked::Found { branches, .. } if branches.len() > 1 => {
            let mut chosen = None;
            let combo = craft_ui::ComboBox::from_id_salt("import-git-branch")
                .width(ui.available_width().min(260.0))
                .selected_text(p.branch.clone())
                .show_ui(ui, |ui| {
                    for b in branches {
                        if ui.selectable_label(*b == p.branch, b).clicked() {
                            chosen = Some(b.clone());
                        }
                    }
                });
            let _ = combo.response.labelled_by(label);
            if let Some(b) = chosen {
                p.branch = b;
            }
        }
        Checked::Found { branches, .. } if branches.is_empty() && info.link => hint(ui, t, tl!("None yet: the repository is empty."), Tone::Plain),
        Checked::Found { branches, .. } if branches.is_empty() => {
            hint(ui, t, tl!("This repository is empty: there is nothing to import yet."), Tone::Bad)
        }
        Checked::Found { .. } => {
            let l = ui.label(p.branch.clone());
            let _ = l.labelled_by(label);
        }
        _ => {
            ui.add_enabled(false, egui::Label::new(RichText::new(tl!("After the address")).color(t.text_muted)));
        }
    });
    let enter = responses.iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
    let ready = matches!(&state, Checked::Found { branches, .. } if !branches.is_empty() || info.link);
    (ready, enter)
}

/// What was chosen on the import's card.
enum CardAct {
    Cancel,
    TryAgain,
    Dismiss,
}

/// The import's card, first among the recent projects (`size` as theirs): a spinner, the name,
/// "Importing…" and the phase (a live region, said at each phase), Cancel; or, when it failed, why
/// in words with Try Again (the dialog's page as it was) and Dismiss. Nothing when no import runs.
pub(crate) fn card_of_import(app: &mut App, ui: &mut egui::Ui, t: &Tokens, size: egui::Vec2) {
    let Some(importing) = &app.importing else { return };
    let phase = if importing.browser {
        app.session.forge_busy().map(|_| tl!("Receiving the files…"))
    } else {
        app.session.clone_phase().map(|phase| match phase {
            ClonePhase::Connecting => tl!("Asking the server for the project…"),
            ClonePhase::Receiving => tl!("Receiving the history…"),
            ClonePhase::Writing => tl!("Writing the files…"),
        })
    };
    let name = importing.name.clone();
    let error = importing.error.clone();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, t.corner_large(), t.surface);
    let border = if error.is_some() { t.error } else { t.accent };
    ui.painter().rect_stroke(rect, t.corner_large(), egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let state = if error.is_some() { tl!("Import failed") } else { tl!("Importing…") };
    let group = format!("{name}, {state}");
    let mut chosen = None;
    let builder = egui::UiBuilder::new()
        .max_rect(rect.shrink(12.0))
        .layout(egui::Layout::top_down(egui::Align::Min))
        .accessibility_role(egui::accesskit::Role::Group)
        .accessibility_label(&group);
    ui.scope_builder(builder, |ui| {
        ui.spacing_mut().item_spacing.y = t.unit * 2.0;
        let (page, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
        match &error {
            None => {
                widgets::spinner(ui, page.center(), 14.0, t.accent);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
            Some(_) => icons::paint(ui, page, "circle-exclamation-point", 28.0, t.error),
        }
        ui.add(egui::Label::new(RichText::new(&name).font(t.strong())).truncate());
        match &error {
            None => {
                let text = match phase {
                    Some(phase) => format!("{state} {phase}"),
                    None => state.to_owned(),
                };
                let said = ui.add(egui::Label::new(RichText::new(&text).font(t.small()).color(t.text_muted)).wrap());
                crate::status::announce(ui, &said, &text);
                let r = ui.button(tl!("Cancel"));
                let label = format!("{}: {name}", tl!("Cancel"));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                if r.clicked() {
                    chosen = Some(CardAct::Cancel);
                }
            }
            Some(e) => {
                let words = crate::i18n::msg(e);
                egui::ScrollArea::vertical().id_salt("import-card-error").max_height(64.0).show(ui, |ui| {
                    let said = ui.add(egui::Label::new(RichText::new(&words).font(t.small()).color(t.error)).wrap());
                    crate::status::announce(ui, &said, &words);
                });
                ui.horizontal_wrapped(|ui| {
                    let r = ui.button(tl!("Try Again"));
                    let label = format!("{}: {name}", tl!("Try Again"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if r.clicked() {
                        chosen = Some(CardAct::TryAgain);
                    }
                    let r = ui.button(tl!("Dismiss"));
                    let label = format!("{}: {name}", tl!("Dismiss"));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if r.clicked() {
                        chosen = Some(CardAct::Dismiss);
                    }
                });
            }
        }
    });
    match chosen {
        Some(CardAct::Cancel) => stop(app),
        Some(CardAct::TryAgain) => {
            if let Some(importing) = app.importing.take() {
                match importing.form {
                    Some(form) => app.view.dialog = Some(Dialog::Import(form)),
                    None => open_at(app, None, ""),
                }
            }
        }
        Some(CardAct::Dismiss) => app.importing = None,
        None => {}
    }
}

/// Cancel on the import's card: the clone (or the browser's opening) stops, the folder it made is
/// removed, and the card goes.
fn stop(app: &mut App) {
    let Some(importing) = app.importing.take() else { return };
    let command = if importing.browser { "forge.cancel_open" } else { "project.cancel_clone" };
    if let Some(done) = app.run(command, json!({})) {
        let said = if done["removed"] == true { tl!("Import stopped: the folder it made is removed.") } else { tl!("Import stopped.") };
        app.view.status = Some((said.to_owned(), false));
    }
}
