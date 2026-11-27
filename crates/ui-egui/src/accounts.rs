//! Settings › Accounts: the connections (docs/design/connections.md). Every forge account and
//! git server TeXCraft reaches for the user, as one list in two groups (GitLab and GitHub, then
//! other git servers), each row a card with the service's mark, its name and address, who is
//! signed in, its state in words, where the sign-in is kept, and its actions; Add a Connection…
//! opens a dialog of cards like Import a Project's, then the existing sign-in, server or token
//! step. Disconnect and Remove Server ask first.
//!
//! Everything goes through commands: `accounts.list` (what is drawn), `accounts.add`,
//! `accounts.disconnect`, `accounts.set_default`, `forge.sign_in`, `forge.remove_server`,
//! `git.keep_credential`.

use egui::{RichText, Sense, vec2};
use serde_json::{Value, json};
use texcraft_engine::forge::Service;

use crate::dialogs::Dialog;
use crate::forge::{SignInAction, say};
use crate::import::{Line, Look, Tone};
use crate::theme::Tokens;
use crate::{App, icons, widgets};

const SHEET: &str = "texcraft-connection";
const WIDTH: f32 = 640.0;

/// A row of `accounts.list`, as drawn.
struct Row {
    instance: String,
    host: String,
    /// `gitlab`, `github`, `overleaf` or `git`.
    service: String,
    /// A forge (gitlab.com, github.com, a server), not another git server.
    forge: bool,
    /// A self-managed server.
    server: bool,
    state: String,
    /// `sign_in` or `token`.
    how: Option<String>,
    user: Option<(String, String)>,
    can_commit: Option<bool>,
    kept: Option<String>,
    default: bool,
    can_default: bool,
    provided: bool,
    removable: bool,
    /// Why signing in is not offered, when it is not.
    no_sign_in: Option<String>,
    available: bool,
}

impl Row {
    fn of(v: &Value) -> Option<Self> {
        let text = |k: &str| v[k].as_str().map(str::to_owned);
        Some(Row {
            instance: text("instance")?,
            host: text("host")?,
            service: text("service")?,
            forge: v["kind"] == "forge",
            server: v["server"] == true,
            state: text("state").unwrap_or_default(),
            how: text("how"),
            user: v["user"]["username"].as_str().map(|u| (v["user"]["name"].as_str().unwrap_or(u).to_owned(), u.to_owned())),
            can_commit: v["can_commit"].as_bool(),
            kept: text("kept"),
            default: v["default"] == true,
            can_default: v["can_default"] == true,
            provided: v["provided"] == true,
            removable: v["removable"] == true,
            no_sign_in: v["sign_in"].as_str().map(str::to_owned),
            available: v["available"] != false,
        })
    }

    fn forge_service(&self) -> Option<Service> {
        if self.forge { Service::parse(&self.service) } else { None }
    }

    /// The row's title: the service's name, or the server's host.
    fn title(&self) -> String {
        match (self.forge, self.server, self.service.as_str()) {
            (true, false, "gitlab") => Service::GitLab.name().to_owned(),
            (true, false, _) => Service::GitHub.name().to_owned(),
            (false, _, "overleaf") => texcraft_engine::GitHosting::Overleaf.name().to_owned(),
            _ => self.host.clone(),
        }
    }

    /// The line under the title: the address, or what kind of server it is.
    fn address(&self) -> String {
        match (self.forge, self.server, self.service.as_str()) {
            (true, true, "gitlab") => tl!("Self-managed GitLab").to_owned(),
            (true, true, _) => tl!("GitHub Enterprise Server").to_owned(),
            (false, _, "git") => tl!("Git server over HTTPS").to_owned(),
            _ => self.host.clone(),
        }
    }

    /// The service's mark, in the text colour (true), or ours, in the accent.
    fn mark(&self) -> (&'static str, bool) {
        match self.service.as_str() {
            "gitlab" => ("brand-gitlab", true),
            "github" => ("brand-github", true),
            "overleaf" => ("brand-overleaf", true),
            _ => ("server", false),
        }
    }

    fn connected(&self) -> bool {
        matches!(self.state.as_str(), "signed_in" | "token" | "expired" | "signing_in")
    }

    /// The state, as its badge says it.
    fn state_words(&self) -> &'static str {
        match self.state.as_str() {
            "signed_in" => tl!("Signed in"),
            "token" => tl!("Token"),
            "expired" => tl!("Expired"),
            "signing_in" => tl!("Signing in…"),
            _ => tl!("Signed out"),
        }
    }

    /// Who is connected, in words: "Ada Lovelace (@ada)", a token, or no one.
    fn who(&self) -> String {
        match (&self.user, self.how.as_deref()) {
            (Some((name, user)), _) if name != user => format!("{name} (@{user})"),
            (Some((_, user)), _) => format!("@{user}"),
            (None, Some("token")) if self.forge => tl!("Personal access token").to_owned(),
            (None, Some("token")) => tl!("Git token").to_owned(),
            _ if self.state == "signing_in" => tl!("Signing in…").to_owned(),
            _ => tl!("Not signed in").to_owned(),
        }
    }

    /// What the connection can do, when it says: commit, or read only.
    fn can(&self) -> Option<&'static str> {
        match (self.state.as_str(), self.can_commit) {
            ("expired", _) => Some(tl!("The sign-in expired: sign in again")),
            ("signed_in", Some(true)) => Some(tl!("Opens and commits")),
            ("signed_in", Some(false)) => Some(tl!("Read only until the first commit")),
            _ => None,
        }
    }

    /// Where its sign-in or token is kept.
    fn kept_words(&self) -> Option<&'static str> {
        Some(match self.kept.as_deref()? {
            "keychain" => tl!("In the keychain"),
            "tab" => tl!("In this tab"),
            _ => tl!("This session only"),
        })
    }
}

/// What a row's button asked for.
enum Act {
    Add,
    SetDefault(String),
    SignIn(Service, String, bool),
    UseToken(Service),
    Replace(String),
    Disconnect(String, String),
    RemoveServer(String, String),
    Sign(SignInAction),
    Keep(String, String),
    CancelReplace,
}

/// The id under which the button to focus next (by its name) waits, once a dialog of this page
/// closes: the keyboard comes back where it was.
fn focus_key() -> egui::Id {
    egui::Id::unique("texcraft-accounts-focus")
}

/// Asks for the button named `name` to have the keyboard when Settings › Accounts is drawn next.
fn focus_next(app: &App, name: &str) {
    app.ctx.data_mut(|d| d.insert_temp(focus_key(), name.to_owned()));
}

/// A row's button: `label`, named "`label`: `host`" so that each row's is told apart; focused
/// when it was asked to be.
fn button(ui: &mut egui::Ui, t: &Tokens, icon: Option<&str>, label: &str, host: &str, accent: bool) -> egui::Response {
    let name = format!("{label}: {host}");
    let colour = if accent { t.on_accent } else { t.text };
    let text = RichText::new(label).color(colour);
    let mut b = match icon {
        Some(i) => egui::Button::image_and_text(icons::image(i, 14.0, colour), text),
        None => egui::Button::new(text),
    };
    if accent {
        b = b.fill(t.accent).stroke(egui::Stroke::NONE);
    }
    let r = ui.add(b.min_size(vec2(0.0, 28.0)));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    focus_if_asked(ui, &r, &name);
    r
}

fn focus_if_asked(ui: &egui::Ui, r: &egui::Response, name: &str) {
    if ui.data(|d| d.get_temp::<String>(focus_key())).as_deref() == Some(name) {
        r.request_focus();
        ui.data_mut(|d| d.remove::<String>(focus_key()));
    }
}

/// A small pill: an icon and words, on a tint of `tone`; the words are in the
/// text colour, so they read whatever the tint.
fn badge(ui: &mut egui::Ui, t: &Tokens, icon: Option<&str>, text: &str, tone: egui::Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), t.small(), t.text);
    let icon_room = if icon.is_some() { 16.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_room + 14.0, 20.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let fill = crate::theme::blend(t.surface, egui::Color32::from_rgba_unmultiplied(tone.r(), tone.g(), tone.b(), 56));
    ui.painter().rect_filled(rect, egui::CornerRadius::same(10), fill);
    let mut x = rect.left() + 7.0;
    if let Some(i) = icon {
        icons::paint(ui, egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 6.0), vec2(12.0, 12.0)), i, 12.0, tone);
        x += icon_room;
    }
    ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, t.text);
}

/// The account's initial in a circle, standing for its picture.
fn initial(ui: &mut egui::Ui, t: &Tokens, name: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let letter: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
    ui.painter().circle_filled(rect.center(), 11.0, t.accent_soft);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, letter, t.small(), t.text);
}

/// The page: the header with Add a Connection…, the empty state or the two groups, the problems.
pub(crate) fn settings(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let listed = app.session.execute("accounts.list", &json!({})).unwrap_or_default();
    let rows: Vec<Row> = listed["connections"].as_array().into_iter().flatten().filter_map(Row::of).collect();
    let mut acts = Vec::new();
    let narrow = crate::sheet::narrow(ui.ctx(), t);
    let empty = rows.is_empty();
    ui.spacing_mut().item_spacing.y = t.unit * 2.0;
    if empty {
        empty_state(ui, t, &mut acts);
    } else {
        let intro = RichText::new(tl!("Where your projects live: Import a Project, Sync and Save Project to… use these."))
            .font(t.small())
            .color(t.text_muted);
        if narrow {
            ui.add(egui::Label::new(intro).wrap());
            if add_button(ui, t, false).clicked() {
                acts.push(Act::Add);
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                let room = (ui.available_width() - 200.0).max(160.0);
                ui.vertical(|ui| {
                    ui.set_max_width(room);
                    ui.add(egui::Label::new(intro).wrap());
                });
                widgets::right_aligned(ui, "accounts-add", |ui| {
                    if add_button(ui, t, false).clicked() {
                        acts.push(Act::Add);
                    }
                });
            });
        }
        ui.add_space(t.unit * 2.0);
        let groups = [(tl!("GitLab and GitHub"), true), (tl!("Other git servers"), false)];
        for (name, forge) in groups {
            let group: Vec<&Row> = rows.iter().filter(|r| r.forge == forge).collect();
            if group.is_empty() {
                continue;
            }
            let builder = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Group).accessibility_label(name);
            ui.scope_builder(builder, |ui| {
                ui.label(RichText::new(name).font(t.small()).color(t.text_muted));
                for row in group {
                    row_card(app, ui, t, row, narrow, &mut acts);
                }
            });
            ui.add_space(t.unit * 2.0);
        }
    }
    if let Some(e) = listed["store_error"].as_str() {
        let text = tl!("The keychain cannot be read, so sign-ins are kept for this session only: {why}").replace("{why}", &crate::i18n::msg(e));
        crate::import::hint(ui, t, &text, Tone::Bad);
    }
    for problem in listed["server_problems"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let text =
            tl!("Your organisation's list of servers has an entry TeXCraft left out: {problem}").replace("{problem}", &crate::i18n::msg(problem));
        ui.label(RichText::new(text).font(t.small()).color(t.error));
    }
    let kept = match listed["kept"].as_str() {
        Some("keychain") => tl!("Sign-ins and tokens are kept in the system's keychain, never in the preferences."),
        Some("tab") => tl!("Sign-ins and tokens are kept in this tab's session storage: a reload keeps them, closing the tab forgets them."),
        _ => tl!("Sign-ins and tokens are kept while TeXCraft runs only."),
    };
    ui.label(RichText::new(kept).font(t.small()).color(t.text_muted));
    for act in acts {
        perform(app, act);
    }
}

/// Add a Connection…, in the accent; larger in the empty state.
fn add_button(ui: &mut egui::Ui, t: &Tokens, large: bool) -> egui::Response {
    let label = tl!("Add a Connection…");
    let text = RichText::new(label).color(t.on_accent).font(t.strong());
    let height = if large { 36.0 } else { 30.0 };
    let r = ui.add(
        egui::Button::image_and_text(icons::image("plus", 16.0, t.on_accent), text)
            .fill(t.accent)
            .stroke(egui::Stroke::NONE)
            .min_size(vec2(0.0, height)),
    );
    focus_if_asked(ui, &r, label);
    r
}

/// Nothing connected yet: what connections are for, and Add a Connection….
fn empty_state(ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
    egui::Frame::new()
        .fill(t.surface)
        .stroke(egui::Stroke::new(1.0, t.separator))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 20.0, t.accent_soft);
                icons::paint(ui, rect, "link", 20.0, t.accent);
                ui.add_space(t.unit * 2.0);
                ui.label(RichText::new(tl!("Connect TeXCraft to where your projects live")).font(t.strong()));
                ui.add(
                    egui::Label::new(
                        RichText::new(tl!(
                            "Sign in to GitLab or GitHub, add your organisation's server, or keep a token for Overleaf or another git server. Then Import a Project, Sync and Save Project to… reach them without asking again."
                        ))
                        .color(t.text_muted),
                    )
                    .wrap(),
                );
                ui.add_space(t.unit * 3.0);
                if add_button(ui, t, true).clicked() {
                    acts.push(Act::Add);
                }
            });
        });
}

/// One connection: mark, name and address, badges, who and how, where kept, and its actions.
fn row_card(app: &mut App, ui: &mut egui::Ui, t: &Tokens, row: &Row, narrow: bool, acts: &mut Vec<Act>) {
    let mut summary = vec![row.title(), row.address(), row.state_words().to_owned()];
    if row.default {
        summary.push(tl!("Default").to_owned());
    }
    if row.user.is_some() {
        summary.push(row.who());
    }
    let builder = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Group).accessibility_label(summary.join(", "));
    ui.scope_builder(builder, |ui| {
        egui::Frame::new()
            .fill(t.surface)
            .stroke(egui::Stroke::new(1.0, if row.default { t.accent } else { t.separator }))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = t.unit;
                ui.horizontal_top(|ui| {
                    let (mark, brand) = row.mark();
                    let (rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                    let colour = if !row.available {
                        t.text_muted
                    } else if brand {
                        t.text
                    } else {
                        t.accent
                    };
                    icons::paint(ui, rect, mark, 22.0, colour);
                    ui.add_space(t.unit * 2.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = t.unit;
                        let badges = |ui: &mut egui::Ui| {
                            if row.default {
                                badge(ui, t, Some("star"), tl!("Default"), t.accent);
                            }
                            let (icon, tone) = match row.state.as_str() {
                                "signed_in" | "token" => (Some("circle-check"), t.success),
                                "expired" => (Some("triangle-exclamation-point"), t.warning),
                                "signing_in" => (Some("loader-circle"), t.accent),
                                _ => (None, t.text_muted),
                            };
                            badge(ui, t, icon, row.state_words(), tone);
                            if !row.available {
                                badge(ui, t, Some("info"), tl!("Desktop app only"), t.text_muted);
                            }
                        };
                        let mut address = row.address();
                        if row.provided {
                            address = format!("{address} · {}", tl!("Provided by your organisation"));
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = t.unit * 2.0;
                            ui.label(RichText::new(row.title()).font(t.strong()));
                            ui.label(RichText::new(&address).font(t.small()).color(t.text_muted));
                            if narrow {
                                badges(ui);
                            } else {
                                widgets::right_aligned(ui, &format!("badges-{}", row.instance), badges);
                            }
                        });
                        // Who, and how; a row signed out says so in its badge alone.
                        if row.state != "signed_out" {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = t.unit * 1.5;
                                if let Some((name, _)) = &row.user {
                                    initial(ui, t, name);
                                }
                                ui.label(row.who());
                                let more: Vec<&str> = row.can().into_iter().chain(row.kept_words()).collect();
                                if !more.is_empty() {
                                    ui.label(RichText::new(format!("· {}", more.join(" · "))).font(t.small()).color(t.text_muted));
                                }
                            });
                        }
                        if row.state == "signing_in"
                            && let Some(service) = row.forge_service()
                        {
                            let view = crate::forge::sign_in_view(app, service, &row.instance);
                            if let Some(a) = crate::forge::sign_in_part(ui, t, &view, true) {
                                acts.push(Act::Sign(a));
                            }
                        }
                        ui.add_space(t.unit);
                        actions(app, ui, t, row, acts);
                    });
                });
            });
    });
}

/// A row's actions.
fn actions(app: &mut App, ui: &mut egui::Ui, t: &Tokens, row: &Row, acts: &mut Vec<Act>) {
    let host = row.host.as_str();
    ui.horizontal_wrapped(|ui| {
        if let Some(service) = row.forge_service() {
            let sign_in = say(service, tl!("Sign In with GitLab"), tl!("Sign In with GitHub"));
            // The one way forward the row's state offers, if any: its label, icon, accent and act.
            let next = match row.state.as_str() {
                "signed_out" if row.no_sign_in.is_none() => Some((sign_in, None, true, Act::SignIn(service, row.instance.clone(), false))),
                "signed_out" => Some((tl!("Use a Token…"), None, true, Act::UseToken(service))),
                "expired" if row.no_sign_in.is_none() => {
                    Some((tl!("Sign In Again"), Some("refresh-cw"), true, Act::SignIn(service, row.instance.clone(), row.can_commit == Some(true))))
                }
                "signed_in" if row.can_commit == Some(false) && row.no_sign_in.is_none() => {
                    Some((tl!("Allow Commits"), None, false, Act::SignIn(service, row.instance.clone(), true)))
                }
                _ => None,
            };
            if let Some((label, icon, accent, act)) = next
                && button(ui, t, icon, label, host, accent).clicked()
            {
                acts.push(act);
            }
            if row.can_default && !row.default && button(ui, t, Some("star"), tl!("Set as Default"), host, false).clicked() {
                acts.push(Act::SetDefault(row.instance.clone()));
            }
        }
        if row.how.as_deref() == Some("token") && button(ui, t, None, tl!("Replace…"), host, false).clicked() {
            acts.push(Act::Replace(host.to_owned()));
        }
        if row.connected() && button(ui, t, None, tl!("Disconnect…"), host, false).clicked() {
            acts.push(Act::Disconnect(row.instance.clone(), host.to_owned()));
        }
        if row.removable && button(ui, t, Some("trash"), tl!("Remove Server…"), host, false).clicked() {
            acts.push(Act::RemoveServer(row.instance.clone(), host.to_owned()));
        }
    });
    // Replace…: the new token's field, in place.
    if let Some(form) = app.token_replace.as_mut().filter(|f| f.host == host) {
        let label = tl!("New token for {host}").replace("{host}", host);
        let field = crate::forge::field(ui, t, &label, &mut form.token, "", true);
        if std::mem::take(&mut form.focus) {
            field.request_focus();
        }
        let ready = !form.token.trim().is_empty();
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            if ui.add_enabled(ready, egui::Button::new(tl!("Keep"))).clicked() || (enter && ready) {
                acts.push(Act::Keep(host.to_owned(), form.token.clone()));
            }
            if ui.button(tl!("Cancel")).clicked() {
                acts.push(Act::CancelReplace);
            }
        });
    }
}

fn perform(app: &mut App, act: Act) {
    match act {
        Act::Add => open_add(app, None),
        Act::SetDefault(instance) => {
            if app.run("accounts.set_default", json!({"instance": instance})).is_some() {
                let host = texcraft_engine::forge::servers::host_of(&instance);
                app.view.status = Some((tl!("{host} is the default connection.").replace("{host}", host), false));
            }
        }
        Act::SignIn(service, instance, write) => crate::forge::sign_in(app, service, &instance, write),
        Act::UseToken(service) => open_add(app, Some(if service == Service::GitLab { Kind::GitLab } else { Kind::GitHub })),
        Act::Replace(host) => app.token_replace = Some(crate::TokenReplace { host, token: String::new(), focus: true }),
        Act::CancelReplace => app.token_replace = None,
        Act::Keep(host, token) => {
            if app.run("git.keep_credential", json!({"host": host, "token": token.trim()})).is_some() {
                app.token_replace = None;
                app.view.status = Some((tl!("Replaced the token kept for {host}.").replace("{host}", &host), false));
            }
        }
        Act::Disconnect(instance, host) => confirm(app, Confirm::Disconnect, instance, host),
        Act::RemoveServer(instance, host) => confirm(app, Confirm::Remove, instance, host),
        Act::Sign(a) => crate::forge::act(app, a),
    }
}

// ---------------------------------------------------------------------------------------------
// Add a Connection…, and the questions before Disconnect and Remove Server.

/// What can be added, in the dialog's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    GitLab,
    GitHub,
    GitLabServer,
    GitHubServer,
    Overleaf,
    Git,
}

impl Kind {
    const ALL: [Kind; 6] = [Kind::GitLab, Kind::GitHub, Kind::GitLabServer, Kind::GitHubServer, Kind::Overleaf, Kind::Git];

    fn id(self) -> &'static str {
        match self {
            Kind::GitLab => "gitlab",
            Kind::GitHub => "github",
            Kind::GitLabServer => "gitlab_server",
            Kind::GitHubServer => "github_server",
            Kind::Overleaf => "overleaf",
            Kind::Git => "git",
        }
    }

    fn forge(self) -> bool {
        matches!(self, Kind::GitLab | Kind::GitHub | Kind::GitLabServer | Kind::GitHubServer)
    }

    fn look(self) -> Look<'static> {
        let (title, icon, brand) = match self {
            Kind::GitLab => (Service::GitLab.name(), "brand-gitlab", true),
            Kind::GitHub => (Service::GitHub.name(), "brand-github", true),
            Kind::GitLabServer => (tl!("GitLab Server"), "server", false),
            Kind::GitHubServer => (tl!("GitHub Enterprise Server"), "server", false),
            Kind::Overleaf => (texcraft_engine::GitHosting::Overleaf.name(), "brand-overleaf", true),
            Kind::Git => (tl!("Other Git Server"), "link", false),
        };
        Look { title, icon, brand }
    }

    fn page_title(self) -> &'static str {
        match self {
            Kind::GitLab => tl!("Connect to GitLab"),
            Kind::GitHub => tl!("Connect to GitHub"),
            Kind::GitLabServer => tl!("Connect to a GitLab Server"),
            Kind::GitHubServer => tl!("Connect to a GitHub Enterprise Server"),
            Kind::Overleaf => tl!("Connect to Overleaf"),
            Kind::Git => tl!("Connect to a Git Server"),
        }
    }
}

/// What the question before an action is about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Disconnect,
    Remove,
}

enum Page {
    Grid,
    /// GitLab or GitHub: the sign-in, or a token.
    Forge(Service),
    /// Overleaf or another git server: the host and the token.
    Token(Kind),
    /// What the browser cannot reach, and why.
    Unavailable(Kind),
    Confirm {
        what: Confirm,
        instance: String,
        host: String,
    },
}

/// The Add a Connection dialog, or the question before Disconnect and Remove Server.
pub(crate) struct Form {
    page: Page,
    focus_card: Option<Kind>,
    focus_field: bool,
    host: String,
    token: String,
    use_token: bool,
    /// A token is being checked (`accounts.add` in the background).
    running: bool,
    error: Option<String>,
}

impl Form {
    fn new(page: Page) -> Self {
        Form { page, focus_card: None, focus_field: true, host: String::new(), token: String::new(), use_token: false, running: false, error: None }
    }
}

/// Opens Add a Connection, at `kind`'s page if given.
pub(crate) fn open_add(app: &mut App, kind: Option<Kind>) {
    let mut form = Form::new(Page::Grid);
    form.focus_card = Some(Kind::GitLab);
    match kind {
        Some(k) => choose(app, form, k),
        None => app.view.dialog = Some(Dialog::Connection(Box::new(form))),
    }
}

/// `app.add_connection {kind?}`: the dialog, from a command (All Commands, agents' UI control).
pub(crate) fn run(app: &mut App, params: &Value) {
    let kind = params.get("kind").and_then(Value::as_str).and_then(|id| Kind::ALL.into_iter().find(|k| k.id() == id));
    open_add(app, kind);
}

fn confirm(app: &mut App, what: Confirm, instance: String, host: String) {
    focus_next(app, &format!("{}: {host}", if what == Confirm::Disconnect { tl!("Disconnect…") } else { tl!("Remove Server…") }));
    app.view.dialog = Some(Dialog::Connection(Box::new(Form::new(Page::Confirm { what, instance, host }))));
}

/// Whether the dialog waits for a token's check.
pub(crate) fn waiting(app: &App) -> bool {
    matches!(&app.view.dialog, Some(Dialog::Connection(f)) if f.running)
}

/// A token's check failed: said on its page, which stays.
pub(crate) fn failed(app: &mut App, e: &str) {
    if let Some(Dialog::Connection(f)) = app.view.dialog.as_mut() {
        f.running = false;
        f.error = Some(e.to_owned());
    }
}

/// Back to Settings › Accounts, the keyboard on Add a Connection… unless another button was asked.
fn back_to_settings(app: &mut App) {
    // Opened from New Project's "Where to Keep It": back there.
    if crate::where_step::resume(app) {
        return;
    }
    if app.ctx.data(|d| d.get_temp::<String>(focus_key())).is_none() {
        focus_next(app, tl!("Add a Connection…"));
    }
    crate::settings::open(app, &json!({"tab": "accounts"}));
}

/// A token was checked and kept: the dialog closes on the new row.
pub(crate) fn added(app: &mut App, done: &Value) {
    let host = done["host"].as_str().unwrap_or_default();
    let said = match done["username"].as_str() {
        Some(user) => tl!("Connected to {host} as @{username}.").replace("{host}", host).replace("{username}", user),
        None => tl!("Connected to {host}.").replace("{host}", host),
    };
    app.view.status = Some((said, false));
    if matches!(app.view.dialog, Some(Dialog::Connection(_))) {
        focus_next(app, &format!("{}: {host}", tl!("Disconnect…")));
        back_to_settings(app);
    }
}

/// A sign-in ended: from this dialog's page, it closes on the new row.
pub(crate) fn signed_in(app: &mut App, done: &Value) {
    if let Some(Dialog::Connection(f)) = &app.view.dialog
        && matches!(f.page, Page::Forge(_))
    {
        let host = texcraft_engine::forge::servers::host_of(done["instance"].as_str().unwrap_or_default()).to_owned();
        focus_next(app, &format!("{}: {host}", tl!("Disconnect…")));
        back_to_settings(app);
    }
}

fn browser(app: &App) -> bool {
    app.session.library().is_some() || cfg!(target_arch = "wasm32")
}

fn choose(app: &mut App, mut form: Form, kind: Kind) {
    form.focus_card = Some(kind);
    form.focus_field = true;
    form.error = None;
    let browser = browser(app);
    match kind {
        Kind::GitHubServer | Kind::Overleaf | Kind::Git if browser => form.page = Page::Unavailable(kind),
        Kind::GitLab | Kind::GitHub => {
            let service = if kind == Kind::GitLab { Service::GitLab } else { Service::GitHub };
            form.use_token = app.session.forge_sign_in_unavailable(service).is_some();
            form.page = Page::Forge(service);
        }
        Kind::GitLabServer | Kind::GitHubServer => {
            let service = if kind == Kind::GitLabServer { Service::GitLab } else { Service::GitHub };
            let back = if crate::where_step::adding(app) {
                crate::forge_server::Back::NewProject
            } else {
                focus_next(app, tl!("Add a Connection…"));
                crate::forge_server::Back::Settings
            };
            crate::forge_server::ask_add(app, service, "", back);
            return;
        }
        Kind::Overleaf | Kind::Git => form.page = Page::Token(kind),
    }
    app.view.dialog = Some(Dialog::Connection(Box::new(form)));
}

/// What the dialog's page did this frame.
enum Did {
    Choose(Kind),
    Back,
    Close,
    Sign(SignInAction),
    Connect,
    Confirmed,
}

/// The dialog, while it is open.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let browser = browser(app);
    let lines: Vec<(Kind, Line)> = Kind::ALL.iter().map(|k| (*k, line(app, *k, browser))).collect();
    let view = match &app.view.dialog {
        Some(Dialog::Connection(f)) => match f.page {
            Page::Forge(service) => Some(crate::forge::sign_in_view(app, service, texcraft_engine::forge::oauth::instance(service))),
            _ => None,
        },
        _ => return,
    };
    let kept = app.session.git_token_kept();
    let on_page = matches!(&app.view.dialog, Some(Dialog::Connection(f)) if !matches!(f.page, Page::Grid | Page::Confirm { .. }));
    let leave = if on_page { crate::sheet::Leave::Back(tl!("Back to connections")) } else { crate::sheet::Leave::Close };
    let narrow = crate::sheet::narrow(ctx, &t);
    let mut did = Vec::new();
    let width = if matches!(&app.view.dialog, Some(Dialog::Connection(f)) if matches!(f.page, Page::Confirm { .. })) { 440.0 } else { WIDTH };
    let shown = crate::sheet::show(ctx, &t, SHEET, width, leave, |ui| {
        let Some(Dialog::Connection(form)) = app.view.dialog.as_mut() else { return };
        did = draw(ui, &t, form, &lines, view.as_ref(), kept, narrow);
    });
    if shown.back {
        did.push(Did::Back);
    }
    let popup = egui::Popup::is_any_open(ctx);
    if shown.close || (ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !popup) {
        did.push(Did::Close);
    }
    if on_page
        && ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::OpenBracket)
        })
    {
        did.push(Did::Back);
    }
    for d in did {
        act(app, d);
    }
}

/// What a card's line says.
fn line(app: &App, kind: Kind, browser: bool) -> Line {
    let plain = |text: &str| Line { text: text.to_owned(), signed_in: false, unavailable: false };
    let off = || Line { text: tl!("Desktop app only").to_owned(), signed_in: false, unavailable: true };
    let account = |service: Service| app.session.forge_signed_in(service).and_then(|g| g.user.as_ref()).map(|u| u.username.clone());
    match kind {
        Kind::GitLab | Kind::GitHub => {
            let service = if kind == Kind::GitLab { Service::GitLab } else { Service::GitHub };
            match account(service) {
                Some(who) => Line { text: tl!("Signed in as {username}").replace("{username}", &who), signed_in: true, unavailable: false },
                None => crate::import::token_line(app, service).unwrap_or_else(|| {
                    plain(if browser && service == Service::GitHub { tl!("Personal access token") } else { tl!("Sign in or use a token") })
                }),
            }
        }
        Kind::GitHubServer | Kind::Overleaf | Kind::Git if browser => off(),
        Kind::GitLabServer => plain(tl!("Your organisation's GitLab")),
        Kind::GitHubServer => plain(tl!("Your organisation's GitHub")),
        Kind::Overleaf => plain(tl!("Git token")),
        Kind::Git => plain(tl!("Any HTTPS host and a token")),
    }
}

fn draw(
    ui: &mut egui::Ui,
    t: &Tokens,
    form: &mut Form,
    lines: &[(Kind, Line)],
    view: Option<&crate::forge::SignInView>,
    kept: &str,
    narrow: bool,
) -> Vec<Did> {
    let mut did = Vec::new();
    let title = match &form.page {
        Page::Grid => tl!("Add a Connection").to_owned(),
        Page::Forge(service) => (if *service == Service::GitLab { Kind::GitLab } else { Kind::GitHub }).page_title().to_owned(),
        Page::Token(k) | Page::Unavailable(k) => k.page_title().to_owned(),
        Page::Confirm { what: Confirm::Disconnect, host, .. } => tl!("Disconnect from {host}?").replace("{host}", host),
        Page::Confirm { what: Confirm::Remove, host, .. } => tl!("Remove {host}?").replace("{host}", host),
    };
    header(ui, t, &title, matches!(form.page, Page::Forge(_) | Page::Token(_) | Page::Unavailable(_)), narrow, form.running, &mut did);
    match &mut form.page {
        Page::Grid => {
            if let Some(k) = grid(ui, t, lines, form.focus_card.take(), narrow) {
                did.push(Did::Choose(k));
            }
        }
        Page::Forge(service) => forge_page(ui, t, *service, form, view, kept, &mut did),
        Page::Token(kind) => {
            let kind = *kind;
            token_page(ui, t, kind, form, kept, &mut did);
        }
        Page::Unavailable(kind) => unavailable(ui, t, *kind, &mut did),
        Page::Confirm { what, host, .. } => {
            let (what, host) = (*what, host.clone());
            confirm_page(ui, t, what, &host, &mut did);
        }
    }
    did
}

/// The title row: Back on a page, the title, Close. On a phone, the sheet's bar holds them.
fn header(ui: &mut egui::Ui, t: &Tokens, title: &str, on_page: bool, narrow: bool, running: bool, did: &mut Vec<Did>) {
    if narrow {
        crate::dialogs::title(ui, t, title);
        return;
    }
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        if on_page && widgets::icon_button(ui, t, "arrow-left", tl!("Back to connections"), !running).clicked() {
            did.push(Did::Back);
        }
        ui.add(egui::Label::new(RichText::new(title).font(t.title())).truncate());
        widgets::right_aligned(ui, "connection-close", |ui| {
            if widgets::icon_button(ui, t, "x", tl!("Close"), !running).clicked() {
                did.push(Did::Close);
            }
        });
    });
    ui.add_space(t.unit * 3.0);
}

/// The cards, in two groups as on the Accounts page. Returns the card chosen.
fn grid(ui: &mut egui::Ui, t: &Tokens, lines: &[(Kind, Line)], focus: Option<Kind>, narrow: bool) -> Option<Kind> {
    let width = ui.available_width();
    let columns = crate::import::columns(width, 4);
    let list = narrow || columns < 2;
    let card_width = crate::import::card_width(width, columns);
    let mut cells: Vec<(Kind, egui::Response, usize, usize)> = Vec::new();
    let mut row_base = 0;
    for (name, forge) in [(tl!("GitLab and GitHub"), true), (tl!("Other git servers"), false)] {
        let kinds: Vec<&(Kind, Line)> = lines.iter().filter(|(k, _)| k.forge() == forge).collect();
        let builder = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Group).accessibility_label(name);
        ui.scope_builder(builder, |ui| {
            ui.label(RichText::new(name).font(t.small()).color(t.text_muted));
            ui.add_space(t.unit * 1.5);
            if list {
                for (i, (kind, line)) in kinds.iter().enumerate() {
                    let r = crate::import::list_row(ui, t, kind.look(), line);
                    cells.push((*kind, r, row_base + i, 0));
                }
                row_base += kinds.len();
            } else {
                for (r, chunk) in kinds.chunks(columns).enumerate() {
                    let looks: Vec<(Look<'_>, &Line)> = chunk.iter().map(|(k, l)| (k.look(), l)).collect();
                    for (c, resp) in crate::import::cards_row(ui, t, &looks, card_width).into_iter().enumerate() {
                        cells.push((chunk[c].0, resp, row_base + r, c));
                    }
                }
                row_base += kinds.len().div_ceil(columns);
            }
            ui.add_space(t.unit * 2.0);
        });
    }
    let mut chosen = None;
    for (kind, r, ..) in &cells {
        if r.clicked() {
            chosen = Some(*kind);
        }
        if focus == Some(*kind) {
            r.request_focus();
        }
    }
    crate::import::arrows(ui, &cells);
    chosen
}

/// GitLab or GitHub: the sign-in part, or a token checked then kept.
fn forge_page(
    ui: &mut egui::Ui,
    t: &Tokens,
    service: Service,
    form: &mut Form,
    view: Option<&crate::forge::SignInView>,
    kept: &str,
    did: &mut Vec<Did>,
) {
    ui.label(
        RichText::new(say(
            service,
            tl!("Signing in lets TeXCraft list your GitLab projects, import them, commit and make new ones. A personal access token works too."),
            tl!("Signing in lets TeXCraft list your GitHub repositories, import them, commit and make new ones. A personal access token works too."),
        ))
        .color(t.text_muted),
    );
    let mut action = None;
    if let Some(view) = view {
        let signed_in = crate::forge::sign_in_or_token(ui, t, view, &mut form.use_token, true, &mut action);
        if signed_in && !form.use_token {
            crate::import::hint(ui, t, tl!("Connected: it is in the list of connections."), Tone::Good);
        }
    }
    if let Some(a) = action {
        did.push(Did::Sign(a));
    }
    let mut ready = false;
    let mut enter = false;
    if form.use_token {
        ui.add_space(t.unit * 2.0);
        let field = crate::forge::field(ui, t, tl!("Personal access token"), &mut form.token, crate::forge::token_hint(service), true);
        if std::mem::take(&mut form.focus_field) {
            field.request_focus();
        }
        if field.changed() {
            form.error = None;
        }
        enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        crate::forge::token_note(ui, t, kept_words(kept), service);
        ready = !form.token.trim().is_empty() && !form.running;
    }
    status(ui, t, form);
    if form.use_token {
        footer(ui, t, tl!("Connect"), ready, enter, did);
    }
}

/// Where a token connected here is kept, in words.
fn kept_words(kept: &str) -> &'static str {
    match kept {
        "keychain" => tl!("Checked with the service, then kept in the system's keychain; never in the preferences."),
        "tab" => tl!("Checked with the service, then kept in this tab's session storage until the tab is closed."),
        _ => tl!("Checked with the service, then kept while TeXCraft runs."),
    }
}

/// Overleaf or another git server: its host (Overleaf's is known) and its token.
fn token_page(ui: &mut egui::Ui, t: &Tokens, kind: Kind, form: &mut Form, kept: &str, did: &mut Vec<Did>) {
    let mut fields = Vec::new();
    if kind == Kind::Git {
        let host = crate::forge::field(ui, t, tl!("Server"), &mut form.host, "git.example.org", false);
        if std::mem::take(&mut form.focus_field) {
            host.request_focus();
        }
        fields.push(host);
    } else {
        ui.label(RichText::new(tl!("Made in your Overleaf account settings, under git. Your password does not work here.")).color(t.text_muted));
    }
    let token = crate::forge::field(ui, t, tl!("Git token"), &mut form.token, "", true);
    if std::mem::take(&mut form.focus_field) {
        token.request_focus();
    }
    fields.push(token);
    let place = match kept {
        "keychain" => tl!("Kept in the system's keychain; Sync, push, pull and imports from this server use it."),
        "tab" => tl!("Kept in this tab's session storage until the tab is closed."),
        _ => tl!("Kept while TeXCraft runs."),
    };
    ui.label(RichText::new(place).font(t.small()).color(t.text_muted));
    let enter = fields.iter().any(|f| f.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let ready = !form.token.trim().is_empty() && (kind != Kind::Git || !form.host.trim().is_empty()) && !form.running;
    status(ui, t, form);
    footer(ui, t, tl!("Connect"), ready, enter, did);
}

/// The check under way, or why the last did not work: a live region.
fn status(ui: &mut egui::Ui, t: &Tokens, form: &Form) {
    if form.running {
        ui.add_space(t.unit * 2.0);
        crate::import::hint(ui, t, tl!("Checking the token…"), Tone::Busy);
    } else if let Some(e) = &form.error {
        ui.add_space(t.unit * 2.0);
        crate::import::hint(ui, t, &crate::i18n::msg(e), Tone::Bad);
    }
}

fn footer(ui: &mut egui::Ui, t: &Tokens, action: &str, ready: bool, enter: bool, did: &mut Vec<Did>) {
    match crate::dialogs::buttons(ui, t, action, false, ready) {
        crate::dialogs::Outcome::Run => did.push(Did::Connect),
        crate::dialogs::Outcome::Cancel => did.push(Did::Back),
        _ if enter && ready => did.push(Did::Connect),
        _ => {}
    }
}

/// What the browser cannot reach: why, and the way out.
fn unavailable(ui: &mut egui::Ui, t: &Tokens, kind: Kind, did: &mut Vec<Did>) {
    let why = match kind {
        Kind::Overleaf => tl!("Overleaf's git server does not answer web pages, so the browser cannot reach it."),
        Kind::GitHubServer => tl!("In the browser, GitHub needs a personal access token: GitHub's sign-in does not answer web pages."),
        _ => tl!("Most git servers do not answer web pages, so the browser cannot reach them."),
    };
    crate::import::hint(ui, t, why, Tone::Bad);
    ui.label(RichText::new(tl!("The desktop app connects to it; projects from it can be imported here as zip files.")).color(t.text_muted));
    ui.add_space(t.unit * 4.0);
    crate::dialogs::actions(ui, |ui| {
        if ui.add(egui::Button::new(tl!("Back")).min_size(vec2(80.0, 30.0))).clicked() {
            did.push(Did::Back);
        }
    });
}

/// The question before Disconnect or Remove Server: what goes, and what stays.
fn confirm_page(ui: &mut egui::Ui, t: &Tokens, what: Confirm, host: &str, did: &mut Vec<Did>) {
    let goes = match what {
        Confirm::Disconnect => {
            tl!("TeXCraft signs out of {host}, revoking the sign-in where the service allows it, and removes its token from where it is kept.")
        }
        Confirm::Remove => tl!("TeXCraft signs out of {host}, removes its token, and forgets the server and its application."),
    }
    .replace("{host}", host);
    ui.add(egui::Label::new(goes).wrap());
    ui.add_space(t.unit * 2.0);
    ui.add(
        egui::Label::new(
            RichText::new(tl!("Projects linked to it stay as they are; their next Sync asks you to sign in or for a token.")).color(t.text_muted),
        )
        .wrap(),
    );
    let action = if what == Confirm::Disconnect { tl!("Disconnect") } else { tl!("Remove Server") };
    match crate::dialogs::buttons(ui, t, action, true, true) {
        crate::dialogs::Outcome::Run => did.push(Did::Confirmed),
        crate::dialogs::Outcome::Cancel => did.push(Did::Close),
        _ => {}
    }
}

fn act(app: &mut App, did: Did) {
    match did {
        Did::Choose(kind) => {
            if let Some(Dialog::Connection(form)) = app.view.dialog.take() {
                choose(app, *form, kind);
            }
        }
        Did::Back => {
            if let Some(Dialog::Connection(form)) = app.view.dialog.as_mut() {
                if form.running {
                    return;
                }
                form.focus_card = match form.page {
                    Page::Forge(Service::GitLab) => Some(Kind::GitLab),
                    Page::Forge(Service::GitHub) => Some(Kind::GitHub),
                    Page::Token(k) | Page::Unavailable(k) => Some(k),
                    _ => form.focus_card,
                };
                form.page = Page::Grid;
                form.error = None;
                form.token.clear();
            }
        }
        Did::Close => {
            if !waiting(app) {
                back_to_settings(app);
            }
        }
        Did::Sign(a) => crate::forge::act(app, a),
        Did::Connect => connect(app),
        Did::Confirmed => confirmed(app),
    }
}

/// Connect: the token, for the page's service or host.
fn connect(app: &mut App) {
    let Some(Dialog::Connection(form)) = app.view.dialog.as_ref() else { return };
    let params = match form.page {
        Page::Forge(service) => json!({"kind": service.id(), "token": form.token.trim(), "background": true}),
        Page::Token(Kind::Overleaf) => json!({"kind": "overleaf", "token": form.token.trim()}),
        Page::Token(_) => json!({"kind": "git", "host": form.host.trim(), "token": form.token.trim()}),
        _ => return,
    };
    let outcome = app.session.execute("accounts.add", &params);
    match outcome {
        Ok(done) if done["started"] == true => {
            if let Some(Dialog::Connection(f)) = app.view.dialog.as_mut() {
                f.running = true;
                f.error = None;
            }
        }
        Ok(done) => added(app, &done),
        Err(e) => failed(app, &e),
    }
}

/// Disconnect or Remove Server, confirmed.
fn confirmed(app: &mut App) {
    let Some(Dialog::Connection(form)) = app.view.dialog.as_ref() else { return };
    let Page::Confirm { what, instance, host } = &form.page else { return };
    let (what, instance, host) = (*what, instance.clone(), host.clone());
    let done = match what {
        Confirm::Disconnect => app.run("accounts.disconnect", json!({"instance": instance})),
        Confirm::Remove => app.run("forge.remove_server", json!({"address": instance})),
    };
    if let Some(done) = done {
        let said = match what {
            Confirm::Disconnect => tl!("Disconnected from {host}.").replace("{host}", &host),
            Confirm::Remove => tl!("Removed {host}.").replace("{host}", &host),
        };
        let mut said = (said, false);
        let out = &done["signed_out"];
        if out["revoked"] == false {
            said.0 = format!(
                "{} {}",
                said.0,
                tl!("To end TeXCraft's access, remove it on GitHub, under Settings › Applications › Authorized OAuth Apps.")
            );
        }
        if let Some(w) = done["warning"].as_str().or(out["warning"].as_str()) {
            said = (format!("{} {}", said.0, crate::i18n::msg(w)), true);
        }
        app.view.status = Some(said);
        // The row is gone, or has no such button any more: the keyboard goes to Add a Connection….
        app.ctx.data_mut(|d| d.remove::<String>(focus_key()));
    }
    back_to_settings(app);
}
