//! Link to a Repository (docs/design/add-remote.md; docs/ROADMAP.md, C10): Import a Project's
//! dialog, its Online cards and pages, for a repository the open project is linked to instead of
//! one it is made from. No Into: the project stays where it is. Under the page's fields, once the
//! repository and its branch are known, What happens: checked on the server (`git.check_remote`)
//! as typing pauses, before anything changes, with a choice for each file both sides have with
//! other contents and, when the project has a remote named `origin` already, this one's name.
//! Link runs `git.add_remote`; Unlink… in the History panel, `git.remove_remote`.

use egui::RichText;
use serde_json::{Value, json};
use texcraft_engine::ForgeProblem;
use texcraft_engine::forge::Choice;

use super::{Act, Checked, ImportForm, Info, Into, Page, Source, Tone, hint, row};
use crate::App;
use crate::dialogs::{Confirm, Dialog, actions};
use crate::theme::Tokens;

/// Where What happens stands.
#[derive(Default)]
enum Preview {
    #[default]
    Idle,
    Running,
    Seen(Value),
    Failed(String),
}

/// A file both sides have with other contents, and the side chosen.
struct Clash {
    path: String,
    choice: Option<Choice>,
}

/// Link to a Repository's own part of the dialog.
#[derive(Default)]
pub(crate) struct LinkState {
    preview: Preview,
    /// The check's parameters last asked (as text), and when typing paused on new ones.
    asked: Option<String>,
    due: Option<(f64, Value)>,
    /// The remote's name, when the project has a remote named `origin` already.
    name: String,
    clashes: Vec<Clash>,
    /// A link runs.
    running: bool,
    error: Option<String>,
}

impl LinkState {
    pub(super) fn running(&self) -> bool {
        self.running
    }
}

/// `git.add_remote` from the window (the File menu, the History panel, All Commands): the dialog,
/// at a card if `{source}` names one, `{url}` filled in on the Git Address page.
pub(crate) fn run(app: &mut App, params: &Value) {
    if let Err(why) = texcraft_engine::cmd::find("git.add_remote").map_or(Ok(()), |c| (c.enabled)(&app.session)) {
        app.view.status = Some((crate::i18n::msg(&why), true));
        return;
    }
    let address = params.get("url").and_then(Value::as_str).unwrap_or_default().to_owned();
    let source = params.get("source").and_then(Value::as_str).and_then(Source::parse).filter(|s| s.online());
    let source = source.or_else(|| (!address.is_empty()).then_some(Source::Git));
    let first = order(app).first().copied().unwrap_or(Source::GitLab);
    let form = ImportForm {
        page: Page::Grid,
        focus_card: Some(first),
        focus_field: false,
        into: Into::default(),
        remember: true,
        error: None,
        link: Some(Box::default()),
    };
    app.view.dialog = None;
    match source {
        Some(source) => super::choose(app, form, source, &address),
        None => app.view.dialog = Some(Dialog::Import(Box::new(form))),
    }
}

/// The cards, the default connection's first, then Import's order.
pub(super) fn order(app: &App) -> Vec<Source> {
    let mut sources: Vec<Source> = Source::ALL.iter().copied().filter(|s| s.online()).collect();
    if let Some(default) = super::default_source(app)
        && let Some(at) = sources.iter().position(|s| *s == default)
    {
        let first = sources.remove(at);
        sources.insert(0, first);
    }
    sources
}

/// The page's title, which names the dialog while it is shown.
pub(super) fn title(source: Option<Source>) -> &'static str {
    match source {
        None | Some(Source::Folder | Source::Zip) => tl!("Link to a Repository"),
        Some(Source::GitLab) => tl!("Link to GitLab"),
        Some(Source::GitHub) => tl!("Link to GitHub"),
        Some(Source::Overleaf) => tl!("Link to Overleaf"),
        Some(Source::GitLabServer) => tl!("Link to a GitLab Server"),
        Some(Source::Git) => tl!("Link to a Git Address"),
    }
}

/// The check's parameters for the page as it is: none until the repository is known.
fn params(page: &Page, ready: bool) -> Option<Value> {
    if !ready {
        return None;
    }
    let mut p = match page {
        Page::Forge(f, _) => {
            let address = texcraft_engine::forge::Address::parse_as(&super::qualified(f), Some(f.service)).ok()?;
            let mut p = json!({"url": address.clone_url(), "service": f.service.id()});
            if !f.branch.trim().is_empty() {
                p["branch"] = json!(f.branch.trim());
            }
            if f.use_token && !f.token.trim().is_empty() {
                p["token"] = json!(f.token.trim());
            }
            p
        }
        Page::Git(g) => {
            let mut p = json!({"url": texcraft_engine::typed_address(&g.address)});
            if !g.branch.trim().is_empty() {
                p["branch"] = json!(g.branch.trim());
            }
            if !g.username.trim().is_empty() {
                p["username"] = json!(g.username.trim());
            }
            if !g.token.trim().is_empty() {
                p["token"] = json!(g.token.trim());
            }
            p
        }
        Page::Overleaf(o) => {
            let mut p = json!({"url": texcraft_engine::typed_address(&o.address)});
            if !o.token.trim().is_empty() {
                p["token"] = json!(o.token.trim());
            }
            p
        }
        _ => return None,
    };
    if p["url"].as_str().is_none_or(str::is_empty) {
        return None;
    }
    p["background"] = json!(true);
    Some(p)
}

/// Whether the page's own check found the repository (the Git Address and Overleaf pages check
/// the address first; a forge page's project is known once typed and signed in to).
fn found(page: &Page) -> bool {
    match page {
        Page::Git(g) => matches!(g.check.state, Checked::Found { .. }),
        Page::Overleaf(o) => matches!(o.check.state, Checked::Found { .. }),
        Page::Forge(..) => true,
        _ => false,
    }
}

/// Under the page's fields: What happens, the choices it needs, and the footer. Whether Link can
/// run.
pub(super) fn part(ui: &mut egui::Ui, t: &Tokens, form: &mut ImportForm, info: &Info, page_ready: bool, enter: bool, acts: &mut Vec<Act>) {
    let ready = page_ready && found(&form.page);
    let asked = params(&form.page, ready);
    let Some(link) = form.link.as_mut() else { return };
    // New parameters: checked once typing pauses.
    let text = asked.as_ref().map(Value::to_string);
    if text != link.asked && link.due.as_ref().map(|(_, p)| p.to_string()) != text {
        link.preview = Preview::Idle;
        link.clashes.clear();
        link.due = asked.map(|p| (info.now, p));
        if link.due.is_none() {
            link.asked = None;
        }
    }
    ui.add_space(t.unit * 3.0);
    ui.label(RichText::new(tl!("What happens")).font(t.strong()));
    ui.add_space(t.unit);
    let mut can = false;
    let mut name_enter = false;
    let seen_now = match &link.preview {
        Preview::Seen(v) => Some(v.clone()),
        _ => None,
    };
    match (&link.preview, seen_now) {
        (_, Some(v)) => {
            can = seen(ui, t, &v, info);
            if can && v["name_taken"] == true {
                let (enter, valid) = name_row(ui, t, link, &v, info);
                name_enter = enter;
                can &= valid;
            }
            if !link.clashes.is_empty() {
                clash_rows(ui, t, &mut link.clashes, !link.running);
                can &= link.clashes.iter().all(|c| c.choice.is_some());
            }
        }
        (Preview::Idle, _) if link.due.is_none() => {
            let line = tl!("Once the repository is chosen, TeXCraft checks what linking does, before anything changes.");
            ui.label(RichText::new(line).font(t.small()).color(t.text_muted));
        }
        (Preview::Failed(e), _) => hint(ui, t, &crate::i18n::msg(e), Tone::Bad),
        _ => hint(ui, t, tl!("Checking the repository…"), Tone::Busy),
    }
    if let Some(e) = &link.error {
        ui.add_space(t.unit * 2.0);
        hint(ui, t, &crate::i18n::msg(e), Tone::Bad);
    }
    let running = link.running;
    footer(ui, t, can && !running, running, info, (enter || name_enter) && can && !running, acts);
}

/// What the check found, in words. Whether it can be linked.
fn seen(ui: &mut egui::Ui, t: &Tokens, v: &Value, info: &Info) -> bool {
    if v["needs_sign_in"] == true {
        hint(ui, t, tl!("This repository needs signing in."), Tone::Plain);
        return false;
    }
    if v["can_link"] == false {
        hint(ui, t, &crate::i18n::msg(v["why"].as_str().unwrap_or_default()), Tone::Bad);
        return false;
    }
    let versions = v["versions"].as_u64().unwrap_or(0) as usize;
    let said = if v["empty"] == true {
        if info.browser {
            tl!("The repository is empty: your project's files will be its first commit.").to_owned()
        } else {
            tl!("The repository is empty: your project's history will be sent to it.").to_owned()
        }
    } else if v["related"] == true {
        tl!("The repository has this project's history: Sync brings the two up to date.").to_owned()
    } else if info.browser {
        tl!("The repository has files: they will be brought together with your project's.").to_owned()
    } else {
        crate::i18n::trn(
            versions,
            "The repository has {n} version: it will be merged with your project's.",
            "The repository has {n} versions: they will be merged with your project's.",
        )
    };
    hint(ui, t, &said, Tone::Good);
    if v["empty"] != true && v["related"] != true {
        let there = v["only_there"].as_u64().unwrap_or(0) as usize;
        let mut parts = vec![crate::i18n::trn(there, "{n} file comes in", "{n} files come in")];
        parts.push(tl!("yours stay").to_owned());
        ui.label(RichText::new(parts.join(", ")).font(t.small()).color(t.text_muted));
    }
    true
}

/// The remote's name, when `origin` is taken. Whether Enter was pressed in it, and whether the
/// name can be used.
fn name_row(ui: &mut egui::Ui, t: &Tokens, link: &mut LinkState, v: &Value, info: &Info) -> (bool, bool) {
    if link.name.is_empty() {
        link.name = v["suggested_name"].as_str().unwrap_or("upstream").to_owned();
    }
    ui.add_space(t.unit * 2.0);
    let taken = v["name"].as_str().unwrap_or("origin").to_owned();
    let names: Vec<&str> = v["remotes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let valid = texcraft_engine::cmd::remote_name_ok(link.name.trim()) && !names.contains(&link.name.trim());
    let r = row(ui, t, info.narrow, tl!("Name"), |ui, label| {
        let edit = egui::TextEdit::singleline(&mut link.name).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
        let r = ui.add_enabled(!link.running, edit).labelled_by(label);
        let line = tl!("The project has a remote named {name} already. Sync will use this repository from now on; {name} stays as it is.")
            .replace("{name}", &taken);
        if valid {
            hint(ui, t, &line, Tone::Plain);
        } else {
            hint(ui, t, tl!("A remote's name has letters, digits, '-', '_' or '.', and is not taken."), Tone::Bad);
        }
        r
    });
    (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)), valid)
}

/// Each file both sides have with other contents: keep mine, or take the repository's.
fn clash_rows(ui: &mut egui::Ui, t: &Tokens, rows: &mut [Clash], enabled: bool) {
    ui.add_space(t.unit * 2.0);
    let said = tl!("Both have files with the same names: choose which to keep for each.");
    let l = ui.label(RichText::new(said).font(t.small()).color(t.text_muted));
    crate::status::announce(ui, &l, said);
    egui::ScrollArea::vertical().id_salt("link-clashes").max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
        for row in rows.iter_mut() {
            ui.add_space(t.unit);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&row.path).font(t.strong()));
                for (choice, label) in
                    [(Choice::Mine, crate::i18n::tc("file", "Keep mine")), (Choice::Theirs, crate::i18n::tc("file", "Take theirs"))]
                {
                    let on = row.choice == Some(choice);
                    let r = ui.add_enabled(enabled, egui::RadioButton::new(on, label));
                    let name = format!("{label}: {}", row.path);
                    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, on, &name));
                    if r.clicked() {
                        row.choice = Some(choice);
                    }
                }
            });
        }
    });
}

/// Cancel and Link, or while a link runs its progress.
fn footer(ui: &mut egui::Ui, t: &Tokens, ready: bool, running: bool, info: &Info, enter: bool, acts: &mut Vec<Act>) {
    ui.add_space(t.unit * 3.0);
    if !info.narrow {
        crate::widgets::hairline(ui, t);
        ui.add_space(t.unit * 2.0);
    }
    actions(ui, |ui| {
        if crate::dialogs::primary(ui, t, tl!("Link"), ready, false).clicked() || enter {
            acts.push(Act::Import);
        }
        if crate::dialogs::secondary(ui, tl!("Cancel"), !running).clicked() {
            acts.push(Act::Close);
        }
        if running {
            crate::dialogs::progress(ui, t, tl!("Linking and bringing the histories together…"));
        }
    });
}

/// Sends the check whose typing has paused.
pub(super) fn send(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let busy = app.session.git_busy().is_some() || app.session.forge_busy().is_some();
    let Some(Dialog::Import(form)) = app.view.dialog.as_mut() else { return };
    let Some(link) = form.link.as_mut() else { return };
    let Some((at, _)) = &link.due else { return };
    let wait = super::PAUSE - (now - at);
    if wait > 0.0 || busy || link.running {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.1)));
        return;
    }
    let Some((_, params)) = link.due.take() else { return };
    link.asked = Some(params.to_string());
    link.preview = Preview::Running;
    match app.session.execute("git.check_remote", &params) {
        Ok(v) if v["started"] == true => {}
        Ok(v) => checked(app, Ok(v)),
        Err(e) => checked(app, Err(e)),
    }
}

fn link_of(app: &mut App) -> Option<&mut LinkState> {
    match app.view.dialog.as_mut() {
        Some(Dialog::Import(form)) => form.link.as_deref_mut(),
        _ => None,
    }
}

/// The check ended: What happens says it.
pub(crate) fn checked(app: &mut App, outcome: Result<Value, String>) {
    let Some(link) = link_of(app) else { return };
    if link.due.is_some() {
        // Typed into again since: its own check follows.
        return;
    }
    link.preview = match outcome {
        Ok(v) => {
            link.clashes =
                v["clashes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|p| Clash { path: p.to_owned(), choice: None }).collect();
            Preview::Seen(v)
        }
        Err(e) => Preview::Failed(e),
    };
}

/// Link: `git.add_remote` with what the check found and the choices made.
pub(super) fn start(app: &mut App) {
    let Some(Dialog::Import(form)) = app.view.dialog.as_mut() else { return };
    let remember = form.remember;
    let Some(mut params) = params(&form.page, true) else { return };
    let Some(link) = form.link.as_mut() else { return };
    let Preview::Seen(seen) = &link.preview else { return };
    if let Some(branch) = seen["branch"].as_str() {
        params["branch"] = json!(branch);
    }
    if seen["name_taken"] == true {
        params["name"] = json!(link.name.trim());
    }
    if !link.clashes.is_empty() {
        let resolve: serde_json::Map<String, Value> = link
            .clashes
            .iter()
            .filter_map(|c| {
                let side = match c.choice? {
                    Choice::Mine => "mine",
                    Choice::Theirs => "theirs",
                };
                Some((c.path.clone(), json!(side)))
            })
            .collect();
        params["resolve"] = Value::Object(resolve);
    }
    if params.get("token").is_some() {
        params["remember"] = json!(remember);
    }
    link.error = None;
    match app.session.execute("git.add_remote", &params) {
        Ok(v) if v["started"] == true => {
            if let Some(link) = link_of(app) {
                link.running = true;
            }
            app.view.status = Some((tl!("Linking…").to_owned(), false));
        }
        Ok(v) => linked(app, &v),
        Err(e) => {
            if let Some(link) = link_of(app) {
                link.error = Some(e);
            }
        }
    }
}

/// Whether a link's outcome is this one's: Sync's outcome with `added` (the desktop), or
/// `add_remote` (the browser).
pub(crate) fn is_link(done: &Value) -> bool {
    done["operation"] == "add_remote" || done.get("added").is_some_and(|a| !a.is_null())
}

/// The link was made: the dialog closes and the status line says what happened; in the browser,
/// a Sync sends the project's files.
pub(crate) fn linked(app: &mut App, done: &Value) {
    if matches!(app.view.dialog, Some(Dialog::Import(ref f)) if f.link.is_some()) {
        app.view.dialog = None;
    }
    app.sync.running = false;
    app.sync.key = None;
    if done["operation"] == "add_remote" {
        let place = format!("{} ({})", done["project"].as_str().unwrap_or_default(), done["branch"].as_str().unwrap_or_default());
        if done["to_send"].as_u64().unwrap_or(0) > 0 {
            crate::sync::start(app, json!({}));
            if !app.view.status.as_ref().is_some_and(|(_, bad)| *bad) {
                app.view.status = Some((tl!("Linked to {place}: sending your files…").replace("{place}", &place), false));
            }
        } else {
            app.view.status = Some((tl!("Linked to {place}.").replace("{place}", &place), false));
        }
        return;
    }
    let added = &done["added"];
    let url = added["url"].as_str().unwrap_or_default();
    let shown = url.trim_start_matches("https://").trim_end_matches(".git");
    let place = format!("{shown} ({})", added["branch"].as_str().unwrap_or_default());
    let files: Vec<&str> = done["pulled"]["changed"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str()).collect();
    let mut said = if files.is_empty() {
        tl!("Linked to {place}: your history is sent.").replace("{place}", &place)
    } else {
        tl!("Linked to {place}: took in {files}.").replace("{place}", &place).replace("{files}", &files.join(", "))
    };
    if let Some(w) = added["warning"].as_str() {
        said = format!("{said} {}", crate::i18n::msg(w));
    }
    app.view.status = Some((said, added["warning"].is_string()));
}

/// A check or a link failed (`was`: what ran): said in the dialog; files in conflict found by the
/// link's Sync open Sync's dialog (the link stays, and choosing gets it through), a server that
/// wants a token asks for one. Whether it was one.
pub(crate) fn failed(app: &mut App, was: Option<&str>, e: &str) -> bool {
    let running = link_of(app).is_some_and(|l| l.running);
    match was {
        Some("check_remote") => {
            checked(app, Err(e.to_owned()));
            true
        }
        Some("sync" | "add_remote") if running => {
            let problem = app.session.forge_problem().cloned();
            let browser = was == Some("add_remote");
            match problem {
                Some(ForgeProblem::Conflicts(files)) if browser => {
                    if let Some(link) = link_of(app) {
                        link.running = false;
                        link.clashes = files.iter().map(|c| Clash { path: c.path.clone(), choice: None }).collect();
                        link.error = Some(e.to_owned());
                    }
                }
                Some(ForgeProblem::Conflicts(_) | ForgeProblem::NeedsToken { .. }) => {
                    app.view.dialog = None;
                    app.view.status = Some((crate::i18n::msg(e), true));
                    crate::sync::after_failure(app, "project.sync");
                }
                _ => {
                    let said = if browser {
                        crate::i18n::msg(e)
                    } else {
                        tl!("The project is linked, but bringing the two together did not work: {why} Link or Sync tries again.")
                            .replace("{why}", &crate::i18n::msg(e))
                    };
                    if let Some(link) = link_of(app) {
                        link.running = false;
                        link.error = Some(said);
                    }
                    app.sync.running = false;
                }
            }
            true
        }
        _ => false,
    }
}

/// The browser cannot reach this source: why, and the desktop app.
pub(super) fn unavailable(ui: &mut egui::Ui, t: &Tokens, source: Source, acts: &mut Vec<Act>) {
    let why = match source {
        Source::Overleaf => {
            tl!("Overleaf's git server does not answer web pages, so the browser cannot reach it: link the project from the desktop app.")
        }
        _ => tl!(
            "Most git servers do not answer web pages, so the browser cannot reach them: link the project from the desktop app, or to GitLab or GitHub here."
        ),
    };
    let said = ui.label(why);
    crate::status::announce(ui, &said, why);
    ui.add_space(t.unit * 2.0);
    if ui.add(egui::Button::image_and_text(crate::icons::image("download", t.icon_size, t.text), tl!("Get the Desktop App"))).clicked() {
        acts.push(Act::DesktopApp);
    }
    ui.add_space(t.unit * 4.0);
    actions(ui, |ui| {
        if crate::dialogs::secondary(ui, tl!("Back"), true).clicked() {
            acts.push(Act::Back);
        }
    });
}

/// Unlink…: asks first, then `git.remove_remote`.
pub(crate) fn ask_unlink(app: &mut App, params: &Value) {
    if let Err(why) = texcraft_engine::cmd::find("git.remove_remote").map_or(Ok(()), |c| (c.enabled)(&app.session)) {
        app.view.status = Some((crate::i18n::msg(&why), true));
        return;
    }
    let place = match app.session.forge() {
        Some(link) => format!("{} ({})", link.address.path, link.branch),
        None => {
            let remote = app.session.execute("git.remotes", &json!({})).unwrap_or_default();
            params.get("name").and_then(Value::as_str).or(remote["remote"].as_str()).unwrap_or("origin").to_owned()
        }
    };
    app.view.dialog = Some(Dialog::Confirm(Confirm {
        title: tl!("Unlink the project from {place}?").replace("{place}", &place),
        message: tl!("Neither the project nor the repository changes: the project just stops syncing with it. Link to a Repository… links it again.")
            .to_owned(),
        action: tl!("Unlink"),
        command: "git.remove_remote",
        params: if params.is_object() { params.clone() } else { json!({}) },
    }));
}

/// Unlinked: said on the status line.
pub(crate) fn unlinked(app: &mut App, done: &Value) {
    app.sync.key = None;
    let place = done["name"].as_str().or(done["project"].as_str()).unwrap_or_default();
    app.view.status = Some((tl!("Unlinked from {place}: the project and the repository are as they were.").replace("{place}", place), false));
}
