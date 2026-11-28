//! The question asked the first time a desktop build lacks a TeX Live package while Settings ›
//! Building › Missing packages is still Ask: download such packages automatically, install them
//! oneself, or download nothing. The answer is kept in the setting (`build.missing_packages`);
//! closing the question without one keeps Ask, and the next session asks again.

use egui::RichText;
use serde_json::json;
use texcraft_engine::missing::Lack;

use crate::App;
use crate::dialogs::{Dialog, actions, title};
use crate::theme::Tokens;

/// The open question: what the build lacked.
pub(crate) struct Form {
    pub lack: Lack,
    /// The first choice takes the keyboard when the question opens.
    pub focus: bool,
}

/// Opens the question about `lack`.
pub(crate) fn ask(app: &mut App, lack: Lack) {
    app.view.dialog = Some(Dialog::MissingPackages(Box::new(Form { lack, focus: true })));
}

#[derive(Clone, Copy)]
enum Choice {
    Value(&'static str),
    Settings,
    NotNow,
}

pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let mut choice = None;
    let modal = crate::sheet::show(ctx, &t, "texcraft-missing-packages", 480.0, crate::sheet::Leave::Close, |ui| {
        if let Some(Dialog::MissingPackages(form)) = app.view.dialog.as_mut() {
            choice = form_ui(ui, &t, form);
        }
    });
    if modal.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        choice = Some(Choice::NotNow);
    }
    let Some(choice) = choice else { return };
    let Some(Dialog::MissingPackages(form)) = app.view.dialog.take() else { return };
    let name = form.lack.name().unwrap_or("TeX Live").to_owned();
    let kept = tl!("You can change this at any time in Settings › Building › Missing packages.");
    match choice {
        Choice::Value(value) => {
            if app.run("build.missing_packages", json!({"value": value})).is_none() {
                return;
            }
            app.prefs_changed = true;
            let said = match value {
                "auto" => {
                    // At once: the build downloads what it lacks, and says so while it runs.
                    app.run("build.compile", json!({}));
                    tl!("Downloading {name} and compiling again. {kept}")
                }
                "manual" => tl!("TeXCraft will not download packages. Install {name} yourself: the build log says how. {kept}"),
                _ => tl!("TeXCraft will not download packages; a build that needs {name} stops at it. {kept}"),
            };
            app.view.status = Some((said.replace("{name}", &name).replace("{kept}", kept), false));
        }
        Choice::Settings => {
            crate::settings::open(app, &json!({"tab": "building", "search": tl!("Missing packages")}));
        }
        Choice::NotNow => {
            let said = tl!(
                "TeXCraft did not download {name}. It asks again the next time it starts; Settings › Building › Missing packages decides for good."
            );
            app.view.status = Some((said.replace("{name}", &name), false));
        }
    }
}

/// The question, its three answers each with what it does, and where to change the answer later.
fn form_ui(ui: &mut egui::Ui, t: &Tokens, form: &mut Form) -> Option<Choice> {
    let focus = std::mem::take(&mut form.focus);
    let heading = match form.lack.name() {
        Some(name) => tl!("{name} is not installed").replace("{name}", name),
        None => tl!("No TeX Live is installed").to_owned(),
    };
    title(ui, t, &heading);
    let question = tl!("What should TeXCraft do when a package is missing?");
    let asked = ui.label(question);
    // Said when the question opens, with what it is about.
    crate::status::announce(ui, &asked, &format!("{heading}. {question}"));
    ui.add_space(t.unit * 2.0);
    let answers = [
        (
            "auto",
            tl!("Download Automatically"),
            tl!("During the build, from TeX Live's own repository for the year of your TeX Live, so it matches the rest. Needs the network."),
        ),
        ("manual", tl!("Install Them Myself"), tl!("Nothing is downloaded; the build's message names the package and how to install it.")),
        ("off", tl!("Don't Download"), tl!("Nothing is downloaded; the build stops at the missing package.")),
    ];
    let mut choice = None;
    for (i, (value, label, what)) in answers.into_iter().enumerate() {
        let button = if i == 0 {
            egui::Button::new(RichText::new(label).color(t.on_accent).font(t.strong())).fill(t.accent).stroke(egui::Stroke::NONE)
        } else {
            egui::Button::new(RichText::new(label).font(t.strong()))
        };
        let r = ui.add(button.min_size(egui::vec2(200.0, 30.0)));
        if focus && i == 0 {
            r.request_focus();
        }
        if r.clicked() {
            choice = Some(Choice::Value(value));
        }
        ui.label(RichText::new(what).font(t.small()).color(t.text_muted));
        ui.add_space(t.unit);
    }
    ui.add_space(t.unit * 2.0);
    ui.label(
        RichText::new(tl!("TeXCraft keeps your choice. You can change it at any time in Settings › Building › Missing packages."))
            .color(t.text_muted),
    );
    ui.add_space(t.unit * 2.0);
    actions(ui, |ui| {
        if crate::dialogs::secondary(ui, tl!("Not Now"), true).clicked() {
            choice = Some(Choice::NotNow);
        }
        if crate::dialogs::secondary(ui, tl!("Open Settings"), true).clicked() {
            choice = Some(Choice::Settings);
        }
    });
    choice
}
