//! AI assistance in the window (docs/ROADMAP.md, AI1 to AI3): Settings › AI, the dialog that shows
//! what a kind of request sends before the first one goes, the explanations on build messages,
//! Rewrite with AI on a selection (Edit menu and the text's context menu), and the next words
//! suggested after the caret as one types. Everything goes through the engine's `ai.*` commands.

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
    /// The last edit typed in the source editor: when, and where the caret was after it.
    typed: Option<(f64, usize)>,
    /// The edit (its time) after which the next words were last asked for.
    asked: Option<f64>,
    /// The suggested next words last announced.
    announced: Option<String>,
}

/// A kind of request, as the consent dialog asks about it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Explain,
    Rewrite,
    Complete,
}

impl Kind {
    fn request(self) -> &'static str {
        match self {
            Kind::Explain => "explain",
            Kind::Rewrite => "rewrite",
            Kind::Complete => "complete",
        }
    }
}

/// The dialog asking to send a kind of request's text, the first time.
pub(crate) struct Consent {
    kind: Kind,
    /// The request's parameters (the message, the selection); none for continuations, which the
    /// dialog turns on.
    target: Value,
    /// `ai.disclose`'s answer.
    disclosure: Value,
}

/// How a selection can be rewritten: `ai.rewrite`'s `how`, and the menu's label.
pub(crate) const REWRITES: [(&str, &str); 4] =
    [("clearer", "Clearer"), ("shorter", "Shorter"), ("formal", "More Formal"), ("grammar", "Fix Grammar and Spelling")];

/// The languages Translate to… offers, in English (the name the model is given).
pub(crate) const LANGUAGES: [&str; 14] = [
    "English",
    "French",
    "German",
    "Spanish",
    "Italian",
    "Portuguese",
    "Dutch",
    "Polish",
    "Swedish",
    "Russian",
    "Arabic",
    "Chinese",
    "Japanese",
    "Korean",
];

/// The pause in typing after which the next words are asked for, in seconds.
const PAUSE: f64 = 1.0;

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
        Some(texcraft_engine::ai::Event::Rewritten(Ok(_))) => {
            let said = tl!("The rewrite is a suggested change: accept or reject it beside the text or in the Review panel.");
            app.view.status = Some((said.to_owned(), false));
        }
        Some(texcraft_engine::ai::Event::Rewritten(Err(e))) => {
            app.view.status = Some((tl!("No rewrite: {error}").replace("{error}", &e), true));
        }
        Some(texcraft_engine::ai::Event::Completed(Err(e))) => {
            app.view.status = Some((tl!("No words were suggested: {error}").replace("{error}", &e), true));
        }
        Some(texcraft_engine::ai::Event::Made(Ok(typeset))) => {
            let said = if typeset {
                tl!("The LaTeX is made and typeset: its preview is in the dialog.")
            } else {
                tl!("The LaTeX is made but does not typeset: the dialog shows why.")
            };
            app.view.status = Some((said.to_owned(), !typeset));
        }
        Some(texcraft_engine::ai::Event::Made(Err(e))) => {
            app.view.status = Some((tl!("Nothing was made: {error}").replace("{error}", &e), true));
        }
        Some(texcraft_engine::ai::Event::Referenced(Ok(bib))) => {
            let said = tl!("The drafted entry is a suggested change in {bib}: check every field before accepting it.");
            app.view.status = Some((said.replace("{bib}", &bib), false));
        }
        Some(texcraft_engine::ai::Event::Referenced(Err(e))) => {
            app.view.status = Some((tl!("No reference was drafted: {error}").replace("{error}", &e), true));
        }
        Some(texcraft_engine::ai::Event::Answered(Ok(_))) => {
            app.view.status = Some((tl!("The answer is in the Ask panel.").to_owned(), false));
        }
        Some(texcraft_engine::ai::Event::Answered(Err(e))) => {
            app.view.status = Some((tl!("No answer: {error}").replace("{error}", &e), true));
        }
        Some(texcraft_engine::ai::Event::Reviewed(Ok((left, dropped)))) => {
            let said = crate::i18n::trn(left, "The review left {n} comment.", "The review left {n} comments.").replace("{n}", &left.to_string());
            let said = if dropped > 0 {
                let lost = crate::i18n::trn(
                    dropped,
                    "{n} remark was dropped: the words it quoted are not in the text.",
                    "{n} remarks were dropped: the words they quoted are not in the text.",
                );
                format!("{said} {}", lost.replace("{n}", &dropped.to_string()))
            } else {
                said
            };
            app.view.status = Some((said, false));
        }
        Some(texcraft_engine::ai::Event::Reviewed(Err(e))) => {
            app.view.status = Some((tl!("No review: {error}").replace("{error}", &e), true));
        }
        // The editor shows (and announces) the words when the caret is still where they go.
        Some(texcraft_engine::ai::Event::Completed(Ok(_))) | None => {}
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
        app.view.dialog = Some(Dialog::AiConsent(Box::new(Consent { kind: Kind::Explain, target, disclosure })));
        return;
    }
    start(app, Kind::Explain, target);
}

/// Why the selection cannot be rewritten now, if it cannot.
pub(crate) fn can_rewrite(app: &App) -> Result<(), String> {
    if !offered(app) {
        return Err(tl!("Connect a model first (Settings › AI).").to_owned());
    }
    if app.editor.source.selected().is_none() {
        return Err(tl!("Select the text to rewrite first.").to_owned());
    }
    Ok(())
}

/// `edit.rewrite {how, language?}`: rewrites the source's selection, at once when the user
/// agreed before, else after the dialog. The rewrite arrives as a suggested change.
pub(crate) fn rewrite(app: &mut App, params: &Value) {
    if let Err(why) = can_rewrite(app) {
        app.view.status = Some((why, true));
        return;
    }
    let Some(path) = app.session.current().cloned() else { return };
    let r = app.editor.source.doc.selection.primary();
    let (from, to) = (crate::suggest::to_source(app, &path, r.start()), crate::suggest::to_source(app, &path, r.end()));
    let mut target = json!({"path": path.as_str(), "start": from, "end": to, "how": params["how"]});
    if let Some(l) = params.get("language").filter(|l| l.is_string()) {
        target["language"] = l.clone();
    }
    // As after a Vim operator: the selection was used.
    app.editor.source.end_visual();
    let mut ask = target.clone();
    ask["request"] = json!("rewrite");
    let Some(disclosure) = app.run("ai.disclose", ask) else { return };
    if disclosure["needs_consent"].as_bool().unwrap_or(true) {
        app.view.dialog = Some(Dialog::AiConsent(Box::new(Consent { kind: Kind::Rewrite, target, disclosure })));
        return;
    }
    start(app, Kind::Rewrite, target);
}

fn start(app: &mut App, kind: Kind, mut target: Value) {
    if kind == Kind::Complete {
        app.prefs.ai_complete = true;
        app.prefs_changed = true;
        app.view.status = Some((tl!("The next words are suggested as you type: Tab takes them.").to_owned(), false));
        return;
    }
    if let Some(map) = target.as_object_mut() {
        map.insert("background".to_owned(), json!(true));
    }
    let command = if kind == Kind::Rewrite { "ai.rewrite" } else { "ai.explain" };
    if app.run(command, target).is_some() {
        if kind == Kind::Rewrite {
            app.view.status = Some((tl!("The model is rewriting the selection…").to_owned(), false));
        } else {
            app.view.logs = true;
        }
    }
}

/// The entries of Rewrite with AI (in the Edit menu and the text's context menu): what was
/// chosen, as `edit.rewrite`'s parameters.
pub(crate) fn rewrite_menu(app: &App, ui: &mut egui::Ui) -> Option<Value> {
    let t = app.tokens;
    let enabled = can_rewrite(app);
    let mut chosen = None;
    for (how, label) in REWRITES {
        if crate::top_bar::item(ui, &t, None, crate::i18n::t(label), None, enabled.clone()) {
            chosen = Some(json!({"how": how}));
        }
    }
    let translate = ui.add_enabled_ui(enabled.is_ok(), |ui| {
        ui.menu_button(tl!("Translate to…"), |ui| {
            ui.set_min_width(180.0);
            for language in LANGUAGES {
                if ui.add(egui::Button::new(crate::i18n::t(language)).frame_when_inactive(false)).clicked() {
                    chosen = Some(json!({"how": "translate", "language": language}));
                }
            }
        })
    });
    if let Err(why) = enabled {
        let _ = translate.inner.response.on_disabled_hover_text(why);
    }
    if app.session.ai_busy() {
        ui.separator();
        if crate::top_bar::item(ui, &t, None, tl!("Cancel the AI Request"), None, Ok(())) {
            chosen = Some(json!({"cancel": true}));
        }
    }
    chosen
}

/// Runs what [`rewrite_menu`] chose.
pub(crate) fn run_rewrite_choice(app: &mut App, choice: &Value) {
    if choice["cancel"] == true {
        app.run("ai.cancel", json!({}));
    } else {
        crate::ui_cmd::run_with(app, "edit.rewrite", choice);
    }
}

/// Settings › AI › Suggest the next words as I type: turned on after the dialog, the first time.
fn set_typing(app: &mut App, on: bool) {
    if !on {
        app.prefs.ai_complete = false;
        app.prefs_changed = true;
        let _ = app.session.execute("ai.cancel", &json!({"only": "completion"}));
        return;
    }
    let Some(disclosure) = app.run("ai.disclose", json!({"request": "complete"})) else { return };
    if disclosure["needs_consent"].as_bool().unwrap_or(true) {
        app.view.dialog = Some(Dialog::AiConsent(Box::new(Consent { kind: Kind::Complete, target: json!({}), disclosure })));
        return;
    }
    start(app, Kind::Complete, json!({}));
}

/// Each frame of the source editor, when the next words are suggested as one types: drops them
/// when the text `changed` or Escape `dismissed` them, shows them when the caret is where they
/// go, and asks for them after a pause in typing at the end of a line.
pub(crate) fn typing(
    app: &mut App,
    ctx: &egui::Context,
    path: &texcraft_engine::project::ProjectPath,
    changed: bool,
    dismissed: bool,
    focused: bool,
) {
    if !app.prefs.ai_complete || !offered(app) {
        app.editor.source.ghost = None;
        return;
    }
    if changed || dismissed {
        let _ = app.session.execute("ai.cancel", &json!({"only": "completion"}));
    }
    let src = &app.editor.source;
    let caret = src.caret();
    let now = ctx.input(|i| i.time);
    if changed && focused {
        app.ai.typed = Some((now, caret));
    }
    let plain = crate::suggest::view(app, path).is_none_or(|v| v.is_plain());
    let ready = focused && plain && src.inserting() && src.selected().is_none() && src.completion.is_none() && src.at_line_end();
    let ghost = if ready { app.session.ai_ghost(path).filter(|(at, _)| *at == caret).map(|(at, text)| (at, text.to_owned())) } else { None };
    let text = ghost.as_ref().map(|(_, text)| text.clone());
    if text != app.ai.announced {
        ctx.request_repaint();
    }
    if text.is_some() && text != app.ai.announced {
        let said = tl!("Suggested next words: {text}. Tab takes them; Escape dismisses them.").replace("{text}", text.as_deref().unwrap_or_default());
        app.view.status = Some((said, false));
    }
    app.ai.announced = text;
    app.editor.source.ghost = ghost;
    // Only after typing, with the caret still where the typing left it.
    let Some((edited, typed_at)) = app.ai.typed else { return };
    if !ready || typed_at != caret || app.ai.asked == Some(edited) || app.session.ai_busy() || app.editor.source.ghost.is_some() {
        return;
    }
    let wait = PAUSE - (now - edited);
    if wait > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return;
    }
    app.ai.asked = Some(edited);
    // Refused (no consent yet, the rate limit, the caret in the preamble): nothing is suggested.
    if let Err(e) = app.session.execute("ai.complete", &json!({"path": path.as_str(), "at": caret, "background": true})) {
        log::debug!("no next words asked for: {e}");
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
    let kind = consent.kind;
    let mut agree = false;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-ai-consent")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new(tl!("Send this to the model?")).font(t.title()));
        ui.add_space(t.unit * 2.0);
        let what = match kind {
            Kind::Explain if preamble => {
                tl!("Explaining a message sends it, its help, the source lines around it and the main document's preamble to {destination}.")
            }
            Kind::Explain => tl!("Explaining a message sends it, its help and the source lines around it to {destination}."),
            Kind::Rewrite => tl!("Rewriting sends the selected text and a few lines around it to {destination}. The rewrite comes back as a suggested change."),
            Kind::Complete => tl!(
                "Suggesting the next words sends the paragraph you are typing, up to the caret, to {destination} after each pause in typing, at most every two seconds."
            ),
        };
        ui.label(what.replace("{destination}", &destination));
        let below = if kind == Kind::Complete {
            tl!("This is asked once for this connection; each request is logged in Settings › AI. Below is an example of what is sent.")
        } else {
            tl!("This is asked once for this connection; each request is logged in Settings › AI. Below is exactly what is sent.")
        };
        ui.label(RichText::new(below).font(t.small()).color(t.text_muted));
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
            let label = match kind {
                Kind::Explain => tl!("Agree and Explain"),
                Kind::Rewrite => tl!("Agree and Rewrite"),
                Kind::Complete => tl!("Agree and Turn On"),
            };
            if ui.add(egui::Button::new(label).min_size(egui::vec2(120.0, 30.0))).clicked() {
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
        if kind == Kind::Complete {
            app.view.dialog = Some(Dialog::Settings);
        }
        if app.run("ai.consent", json!({"request": kind.request()})).is_some() {
            start(app, kind, consent.target);
        }
    } else if close {
        app.view.dialog = if kind == Kind::Complete { Some(Dialog::Settings) } else { None };
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
        typing_setting(app, ui, t);
        requests(ui, t, &status);
    }
}

fn typing_setting(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let mut on = app.prefs.ai_complete;
    if ui.checkbox(&mut on, tl!("Suggest the next words as I type")).changed() {
        set_typing(app, on);
    }
    ui.label(
        RichText::new(tl!(
            "After a pause in typing at the end of a line, the paragraph up to the caret is sent and the model's next words are shown in grey after it: Tab takes them, typing on or Escape dismisses them."
        ))
        .font(t.small())
        .color(t.text_muted),
    );
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
