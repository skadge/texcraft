//! AI assistance in the window (docs/ROADMAP.md, AI1, AI2): Settings › AI, the dialog that shows
//! what an explanation sends before the first one goes, and the explanations on build messages.
//! Everything goes through the engine's `ai.*` commands.

use egui::{Align, Layout, RichText};
use serde_json::{Value, json};
use texcraft_engine::ai::{Explanation, UNAVAILABLE, available};

use crate::dialogs::Dialog;
use crate::theme::Tokens;
use crate::{App, widgets};

/// The AI settings form and what the window last kept of the engine's AI settings.
#[derive(Default)]
pub(crate) struct AiView {
    endpoint: String,
    token: String,
    model: String,
    /// The form was filled from the session.
    filled: bool,
    /// The engine's AI settings revision last copied into the preferences.
    kept: u64,
}

/// The dialog asking to send an explanation's text, the first time.
pub(crate) struct Consent {
    /// Which message (`index`, or `path` and `line`).
    target: Value,
    /// `ai.disclose`'s answer.
    disclosure: Value,
}

/// Whether the window offers AI actions: the build can, a model is connected.
pub(crate) fn offered(app: &App) -> bool {
    available() && app.session.ai_saved().0.connection.is_some() && app.session.ai_saved().0.model.is_some()
}

/// Each frame: what finished in the background, and the settings to keep.
pub(crate) fn poll(app: &mut App, ctx: &egui::Context) {
    match app.session.poll_ai() {
        Some(texcraft_engine::ai::Event::Models(Ok(n))) => {
            let said = crate::i18n::trn(n, "Connected: the endpoint offers {n} model.", "Connected: the endpoint offers {n} models.");
            app.view.status = Some((said.replace("{n}", &n.to_string()), false));
        }
        Some(texcraft_engine::ai::Event::Models(Err(e))) => {
            app.view.status = Some((tl!("The endpoint did not answer: {error}").replace("{error}", &e), true));
        }
        Some(texcraft_engine::ai::Event::Explained(Ok(made))) => {
            let said = if made {
                tl!("The explanation is in the build log; its fix is a suggested change to review.")
            } else {
                tl!("The explanation is in the build log.")
            };
            app.view.status = Some((said.to_owned(), false));
        }
        Some(texcraft_engine::ai::Event::Explained(Err(e))) => {
            app.view.status = Some((tl!("No explanation: {error}").replace("{error}", &e), true));
        }
        None => {}
    }
    if app.session.ai_busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
    let (saved, revision) = app.session.ai_saved();
    if revision != app.ai.kept {
        app.prefs.ai = saved.clone();
        app.ai.kept = revision;
        app.prefs_changed = true;
    }
}

/// Explains message `target`: at once when the user agreed before, else after the dialog.
pub(crate) fn explain(app: &mut App, target: Value) {
    let Some(disclosure) = app.run("ai.disclose", target.clone()) else { return };
    if disclosure["needs_consent"].as_bool().unwrap_or(true) {
        app.view.dialog = Some(Dialog::AiConsent(Box::new(Consent { target, disclosure })));
        return;
    }
    start(app, target);
}

fn start(app: &mut App, mut target: Value) {
    if let Some(map) = target.as_object_mut() {
        map.insert("background".to_owned(), json!(true));
    }
    if app.run("ai.explain", target).is_some() {
        app.view.logs = true;
    }
}

/// The dialog: what is sent, to whom, asked once per connection.
pub(crate) fn consent_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(Dialog::AiConsent(consent)) = &app.view.dialog else { return };
    let t = app.tokens;
    let d = &consent.disclosure;
    let destination = d["destination"].as_str().unwrap_or_default().to_owned();
    let prompt = d["sends"]["prompt"].as_str().unwrap_or_default().to_owned();
    let preamble = d["sends"]["preamble"].is_string();
    let mut agree = false;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-ai-consent")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new(tl!("Send this to the model?")).font(t.title()));
        ui.add_space(t.unit * 2.0);
        let what = if preamble {
            tl!("Explaining a message sends it, its help, the source lines around it and the main document's preamble to {destination}.")
        } else {
            tl!("Explaining a message sends it, its help and the source lines around it to {destination}.")
        };
        ui.label(what.replace("{destination}", &destination));
        ui.label(
            RichText::new(tl!("This is asked once for this connection; each request is logged in Settings › AI. Below is exactly what is sent."))
                .font(t.small())
                .color(t.text_muted),
        );
        ui.add_space(t.unit * 2.0);
        egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let text = ui.add(egui::Label::new(RichText::new(&prompt).font(t.code_font()).color(t.text)).wrap().selectable(true));
                let _ = text.accessible_name(tl!("What is sent"));
            });
        });
        ui.add_space(t.unit * 3.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(egui::Button::new(tl!("Agree and Explain")).min_size(egui::vec2(120.0, 30.0))).clicked() {
                agree = true;
            }
            let decline = ui.add(egui::Button::new(tl!("Don't Send")).min_size(egui::vec2(100.0, 30.0)));
            if decline.clicked() {
                close = true;
            }
            // The keyboard starts on the choice that sends nothing.
            if ui.memory(|m| m.focused().is_none()) {
                decline.request_focus();
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if agree {
        let Some(Dialog::AiConsent(consent)) = app.view.dialog.take() else { return };
        if app.run("ai.consent", json!({"request": "explain"})).is_some() {
            start(app, consent.target);
        }
    } else if close {
        app.view.dialog = None;
        app.view.status = Some((tl!("Nothing was sent.").to_owned(), false));
    }
}

/// What a message's explanation offers to do.
pub(crate) enum Act {
    None,
    Explain,
    Cancel,
}

/// The explanation of a message, or the button that asks for it, inside its card or popup.
pub(crate) fn explanation(ui: &mut egui::Ui, t: &Tokens, e: Option<&Explanation>, busy: bool) -> Act {
    let mut act = Act::None;
    match e {
        Some(Explanation::Running) => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new(tl!("The model is explaining it…")).font(t.small()).color(t.text_muted));
                if widgets::text_button(ui, t, tl!("Cancel"), true).clicked() {
                    act = Act::Cancel;
                }
            });
        }
        Some(Explanation::Done { text, model, suggestion, note }) => {
            egui::Frame::new().fill(t.accent_soft).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let by = tl!("Explained by AI ({model})").replace("{model}", model);
                ui.label(RichText::new(by).font(t.small()).color(t.text_muted));
                let label = ui.add(egui::Label::new(RichText::new(text).color(t.text)).wrap().selectable(true));
                crate::status::announce(ui, &label, text);
                if suggestion.is_some() {
                    let said = tl!("Its fix is a suggested change: accept or reject it in the Review panel.");
                    ui.label(RichText::new(said).font(t.small()).color(t.text));
                }
                if let Some(note) = note {
                    ui.label(RichText::new(note).font(t.small()).color(t.text_muted));
                }
            });
        }
        Some(Explanation::Failed(why)) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tl!("No explanation: {error}").replace("{error}", why)).font(t.small()).color(t.error));
                if widgets::text_button(ui, t, tl!("Try Again"), !busy).clicked() {
                    act = Act::Explain;
                }
            });
        }
        Some(Explanation::Cancelled) | None => {
            let r = ui.add_enabled(!busy, egui::Button::image_and_text(crate::icons::image("info", 14.0, t.text_muted), tl!("Explain")));
            let r = r.on_hover_text(tl!("Ask the connected model what this message means and how to fix it"));
            if r.clicked() {
                act = Act::Explain;
            }
        }
    }
    act
}

/// Settings › AI.
pub(crate) fn settings(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    if !available() {
        ui.label(RichText::new(tl!(UNAVAILABLE)).color(t.text));
        return;
    }
    let status = app.session.execute("ai.status", &json!({})).unwrap_or_default();
    if !app.ai.filled {
        app.ai.endpoint = status["endpoint"].as_str().unwrap_or_default().to_owned();
        app.ai.model = status["model"].as_str().unwrap_or_default().to_owned();
        app.ai.filled = true;
    }
    let connected = status["connected"].as_bool().unwrap_or(false);
    let busy = !status["busy"].is_null();
    ui.label(
        RichText::new(tl!(
            "AI is off until you connect a model: any OpenAI-compatible endpoint, a server on this computer (llama.cpp, Ollama, vLLM) included. Every change it proposes arrives as a suggested change."
        ))
        .font(t.small())
        .color(t.text_muted),
    );
    let endpoint = field(ui, t, tl!("Endpoint"), &mut app.ai.endpoint, "https://api.example.com/v1", false);
    let local = texcraft_engine::ai::is_local(&app.ai.endpoint);
    let place = match local {
        Some(true) => tl!("On this computer: nothing leaves it, and no token is needed."),
        Some(false) => tl!("Another computer: what is sent leaves this one."),
        None => tl!("The API's base address: https://, or http:// only for this computer."),
    };
    ui.label(RichText::new(place).font(t.small()).color(t.text_muted));
    let has_token = status["token"].as_bool().unwrap_or(false);
    let hint = if has_token { tl!("A token is kept: type to replace it") } else { tl!("None") };
    let token = field(ui, t, tl!("Token"), &mut app.ai.token, hint, true);
    let kept = if status["token_kept"] == "the system's keychain" {
        tl!("The token is kept in the system's keychain, never in the preferences.")
    } else {
        tl!("The token is kept for this session only.")
    };
    ui.label(RichText::new(kept).font(t.small()).color(t.text_muted));
    let enter = [endpoint, token].iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        let ready = !app.ai.endpoint.trim().is_empty();
        if (ui.add_enabled(ready, egui::Button::new(tl!("Connect"))).clicked() || (enter && ready)) && connect(app) {
            app.run("ai.models", json!({"background": true}));
        }
        if ui.add_enabled(connected && !busy, egui::Button::new(tl!("Test Connection"))).clicked() {
            app.run("ai.models", json!({"background": true}));
        }
        if ui
            .add_enabled(connected, egui::Button::new(tl!("Forget")))
            .on_hover_text(tl!("Remove the connection, its token and the agreements given"))
            .clicked()
            && app.run("ai.forget", json!({})).is_some()
        {
            app.ai = AiView { kept: app.ai.kept, filled: true, ..AiView::default() };
            app.view.status = Some((tl!("The AI connection and its token are forgotten.").to_owned(), false));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
    });
    if connected {
        model(app, ui, t, &status, busy);
        project(app, ui, t, &status);
        requests(ui, t, &status);
    }
}

fn field(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, hint: &str, password: bool) -> egui::Response {
    let label = ui.label(RichText::new(label).font(t.strong()));
    let edit = egui::TextEdit::singleline(value).password(password).hint_text(hint).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
    ui.add(edit).labelled_by(label.id)
}

/// Saves the form's endpoint, token and model; the token box empties.
fn connect(app: &mut App) -> bool {
    let mut params = json!({"endpoint": app.ai.endpoint.trim()});
    if !app.ai.token.trim().is_empty() {
        params["token"] = json!(app.ai.token.trim());
    } else if texcraft_engine::ai::is_local(&app.ai.endpoint) == Some(true) {
        // A server on this computer: no token unless one is typed.
        params["token"] = json!("");
    }
    if !app.ai.model.trim().is_empty() {
        params["model"] = json!(app.ai.model.trim());
    }
    let done = app.run("ai.connect", params).is_some();
    if done {
        app.ai.token.clear();
    }
    done
}

fn model(app: &mut App, ui: &mut egui::Ui, t: &Tokens, status: &Value, busy: bool) {
    let models: Vec<String> =
        status["models"].as_array().map(|a| a.iter().filter_map(|m| m.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    let current = status["model"].as_str().unwrap_or_default().to_owned();
    let label = ui.label(RichText::new(tl!("Model")).font(t.strong()));
    ui.horizontal(|ui| {
        let shown = if current.is_empty() { tl!("Choose a model").to_owned() } else { current.clone() };
        let mut chosen = None;
        let combo = craft_ui::ComboBox::from_id_salt("settings-ai-model").width(260.0).selected_text(shown).show_ui(ui, |ui| {
            if models.is_empty() {
                ui.label(RichText::new(tl!("List the models first")).color(t.text_muted));
            }
            for m in &models {
                if ui.selectable_label(*m == current, m).clicked() {
                    chosen = Some(m.clone());
                }
            }
        });
        let _ = combo.response.labelled_by(label.id);
        if ui.add_enabled(!busy, egui::Button::new(tl!("List Models"))).clicked() {
            app.run("ai.models", json!({"background": true}));
        }
        if let Some(m) = chosen
            && app.run("ai.model", json!({"model": m})).is_some()
        {
            app.ai.model = m;
        }
    });
    if current.is_empty() {
        ui.label(RichText::new(tl!("Choose the model that answers: nothing is sent until one is chosen.")).font(t.small()).color(t.warning));
    } else if !models.is_empty() && !models.contains(&current) {
        ui.label(RichText::new(tl!("The endpoint does not list this model.")).font(t.small()).color(t.warning));
    }
}

fn project(app: &mut App, ui: &mut egui::Ui, t: &Tokens, status: &Value) {
    let mut on = !status["project_off"].as_bool().unwrap_or(false);
    ui.horizontal(|ui| {
        let r = ui.checkbox(&mut on, tl!("Use AI in this project"));
        if r.changed() {
            app.run("ai.project", json!({"enabled": on}));
        }
    });
    ui.label(
        RichText::new(tl!("Turned off, the project's sources are never sent; the choice is kept in its .texcraft folder."))
            .font(t.small())
            .color(t.text_muted),
    );
}

/// The request log: when, where, how much, how it ended; never what.
fn requests(ui: &mut egui::Ui, t: &Tokens, status: &Value) {
    let records = status["requests"].as_array().cloned().unwrap_or_default();
    let starts: Vec<&Value> = records.iter().filter(|r| r["phase"] == "start").collect();
    let title = crate::i18n::trn(starts.len(), "{n} request sent", "{n} requests sent").replace("{n}", &starts.len().to_string());
    egui::CollapsingHeader::new(RichText::new(title).font(t.strong())).id_salt("settings-ai-log").show(ui, |ui| {
        for s in starts.iter().rev() {
            let bytes: u64 = s["sent"].as_array().map(|a| a.iter().filter_map(|i| i["bytes"].as_u64()).sum()).unwrap_or_default();
            let end = records.iter().find(|r| r["phase"] == "end" && r["run"] == s["run"]);
            let outcome = match end.map(|e| e["outcome"]["status"].as_str().unwrap_or_default()) {
                Some("done") => tl!("answered"),
                Some("cancelled") => tl!("cancelled"),
                Some(_) => tl!("failed"),
                None => tl!("waiting"),
            };
            let at = s["at_ms"]
                .as_i64()
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string());
            let line = tl!("{time} · {bytes} bytes to {destination} · {model} · {outcome}")
                .replace("{time}", &at.unwrap_or_default())
                .replace("{bytes}", &bytes.to_string())
                .replace("{destination}", s["destination"].as_str().unwrap_or_default())
                .replace("{model}", s["model"].as_str().unwrap_or_default())
                .replace("{outcome}", outcome);
            ui.label(RichText::new(line).font(t.small()).color(t.text));
        }
    });
}
