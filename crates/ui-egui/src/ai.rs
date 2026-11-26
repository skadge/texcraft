//! AI assistance in the window (docs/ROADMAP.md, AI1 to AI5): Settings › AI (the connection, each
//! kind of request with what it sends and its switch, agreed to by connecting, and each kind's
//! prompt), the explanations on build messages,
//! Rewrite with AI on a selection (Edit menu and the text's context menu), and the next words
//! suggested after the caret as one types. Making LaTeX and drafting references are in
//! [`crate::ai_make`], asking and reviewing in [`crate::ai_ask`]. Everything goes through the
//! engine's `ai.*` commands.

use egui::RichText;
use serde_json::{Value, json};
use texcraft_engine::ai::{Explanation, UNAVAILABLE, available};

use crate::dialogs::Dialog;

/// The icon of every AI entry point (decoration: names stay text first).
pub(crate) const ICON: &str = "sparkles";
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
    /// The Ask panel's question and scope.
    pub(crate) ask: crate::ai_ask::AskForm,
    /// The request whose text the request log shows, and that text (or why it cannot).
    shown: Option<(String, String)>,
    /// The token box hides what is typed.
    hide_token: bool,
    /// The prompts being edited, by kind, as typed (the engine keeps what is used).
    prompts: std::collections::BTreeMap<String, String>,
}

/// A kind of request, as Settings › AI lists it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Explain,
    Rewrite,
    Instruct,
    Complete,
    Make,
    Reference,
    Ask,
    Review,
    Grammar,
}

impl Kind {
    /// Every kind, in the order Settings › AI lists them.
    const ALL: [Kind; 9] =
        [Kind::Explain, Kind::Rewrite, Kind::Instruct, Kind::Complete, Kind::Make, Kind::Reference, Kind::Ask, Kind::Review, Kind::Grammar];

    /// Its name, in Settings › AI's list and in the reasons it is refused.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Explain => tl!("Explain"),
            Kind::Rewrite => tl!("Rewrite"),
            Kind::Instruct => tl!("Change as asked"),
            Kind::Complete => tl!("Next words"),
            Kind::Make => tl!("Make LaTeX"),
            Kind::Reference => tl!("Draft a reference"),
            Kind::Ask => tl!("Ask"),
            Kind::Review => tl!("Review"),
            Kind::Grammar => tl!("Check grammar"),
        }
    }

    /// What it sends, in a sentence.
    fn sends(self) -> &'static str {
        match self {
            Kind::Explain => tl!("The build message, its help and the source lines around it (and the preamble when the message is in it)."),
            Kind::Rewrite => tl!("The selected text and a few lines around it."),
            Kind::Instruct => tl!("Your instruction, the selected text and a few lines around it."),
            Kind::Complete => tl!("The text before the caret in the current paragraph, after each pause in typing."),
            Kind::Make => tl!("Your description, the main document's preamble and, for a figure, the names of the project's images."),
            Kind::Reference => tl!("The title or DOI you type, and nothing of the project."),
            Kind::Ask => tl!("Your question and the open file, or every text file of the project when you choose it."),
            Kind::Review => tl!("The section at the caret."),
            Kind::Grammar => tl!("The selected text, or the paragraph at the caret, and the document's language."),
        }
    }

    fn request(self) -> &'static str {
        match self {
            Kind::Explain => "explain",
            Kind::Rewrite => "rewrite",
            Kind::Instruct => "instruct",
            Kind::Complete => "complete",
            Kind::Make => "make",
            Kind::Reference => "reference",
            Kind::Ask => "ask",
            Kind::Review => "review",
            Kind::Grammar => "grammar",
        }
    }

    /// The engine command that makes the request.
    fn command(self) -> &'static str {
        match self {
            Kind::Explain => "ai.explain",
            Kind::Rewrite => "ai.rewrite",
            Kind::Instruct => "ai.instruct",
            Kind::Complete => "ai.complete",
            Kind::Make => "ai.make",
            Kind::Reference => "ai.reference",
            Kind::Ask => "ai.ask",
            Kind::Review => "ai.review",
            Kind::Grammar => "ai.grammar",
        }
    }
}

/// How a selection can be rewritten: `ai.rewrite`'s `how`, and the menu's label.
pub(crate) const REWRITES: [(&str, &str); 4] =
    [("clearer", tn!("Clearer")), ("shorter", tn!("Shorter")), ("formal", tn!("More Formal")), ("grammar", tn!("Fix Grammar and Spelling"))];

/// The languages Translate to… offers, in English (the name the model is given).
pub(crate) const LANGUAGES: [&str; 14] = [
    tn!("English"),
    tn!("French"),
    tn!("German"),
    tn!("Spanish"),
    tn!("Italian"),
    tn!("Portuguese"),
    tn!("Dutch"),
    tn!("Polish"),
    tn!("Swedish"),
    tn!("Russian"),
    tn!("Arabic"),
    tn!("Chinese"),
    tn!("Japanese"),
    tn!("Korean"),
];

/// The pause in typing after which the next words are asked for, in seconds.
const PAUSE: f64 = 1.0;

/// Whether the window offers AI actions: the build can, a model is connected.
pub(crate) fn offered(app: &App) -> bool {
    available() && app.session.ai_saved().0.connection.is_some() && app.session.ai_saved().0.model.is_some()
}

/// Why requests of `kind` cannot be sent now, if they cannot: no model, switched off, not agreed.
pub(crate) fn allowed(app: &App, kind: Kind) -> Result<(), String> {
    if !offered(app) {
        return Err(tl!("Connect a model first (Settings › AI).").to_owned());
    }
    match app.session.ai_permit(kind.request()) {
        texcraft_engine::ai::Permit::On => Ok(()),
        texcraft_engine::ai::Permit::Off => Err(tl!("{kind} is switched off: switch it on in Settings › AI.").replace("{kind}", kind.label())),
        texcraft_engine::ai::Permit::NotAgreed => {
            Err(tl!("{kind} is not agreed to for this connection and model: agree in Settings › AI.").replace("{kind}", kind.label()))
        }
    }
}

/// Each frame: what finished in the background, and the settings to keep.
pub(crate) fn poll(app: &mut App, ctx: &egui::Context) {
    match app.session.poll_ai() {
        Some(texcraft_engine::ai::Event::Models(Ok(n))) => {
            let said = crate::i18n::trn(n, "Connected: the endpoint offers {n} model.", "Connected: the endpoint offers {n} models.");
            app.view.status = Some((said.replace("{n}", &n.to_string()), false));
        }
        Some(texcraft_engine::ai::Event::Models(Err(e))) => {
            app.view.status = Some((tl!("The endpoint did not answer: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
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
            app.view.status = Some((tl!("No explanation: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
        }
        Some(texcraft_engine::ai::Event::Rewritten(Ok(_))) => {
            let said = tl!("The rewrite is a suggested change: accept or reject it beside the text or in the Review panel.");
            app.view.status = Some((said.to_owned(), false));
        }
        Some(texcraft_engine::ai::Event::Rewritten(Err(e))) => {
            app.view.status = Some((tl!("No rewrite: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
        }
        Some(texcraft_engine::ai::Event::Completed(Err(e))) => {
            app.view.status = Some((tl!("No words were suggested: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
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
            app.view.status = Some((tl!("Nothing was made: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
        }
        Some(texcraft_engine::ai::Event::Referenced(Ok(bib))) => {
            let said = tl!("The drafted entry is a suggested change in {bib}: check every field before accepting it.");
            app.view.status = Some((said.replace("{bib}", &bib), false));
        }
        Some(texcraft_engine::ai::Event::Referenced(Err(e))) => {
            app.view.status = Some((tl!("No reference was drafted: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
        }
        Some(texcraft_engine::ai::Event::Answered(Ok(_))) => {
            app.view.status = Some((tl!("The answer is in the Ask panel.").to_owned(), false));
        }
        Some(texcraft_engine::ai::Event::Answered(Err(e))) => {
            app.view.status = Some((tl!("No answer: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
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
            app.view.status = Some((tl!("No review: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
        }
        Some(texcraft_engine::ai::Event::Grammar(Ok((found, dropped)))) => {
            let said = if found == 0 {
                tl!("The model found no grammar problems.").to_owned()
            } else {
                crate::i18n::trn(
                    found,
                    "The model found {n} grammar problem: it is underlined.",
                    "The model found {n} grammar problems: they are underlined.",
                )
                .replace("{n}", &found.to_string())
            };
            let said = if dropped > 0 {
                let lost = crate::i18n::trn(
                    dropped,
                    "{n} was dropped: the words it quoted are not in the text.",
                    "{n} were dropped: the words they quoted are not in the text.",
                );
                format!("{said} {}", lost.replace("{n}", &dropped.to_string()))
            } else {
                said
            };
            app.view.status = Some((said, false));
        }
        Some(texcraft_engine::ai::Event::Grammar(Err(e))) => {
            app.view.status = Some((tl!("No grammar check: {error}").replace("{error}", &crate::i18n::msg(&e)), true));
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

/// Explains message `target`.
pub(crate) fn explain(app: &mut App, target: Value) {
    request(app, Kind::Explain, target, None);
}

/// Makes request `kind` with `target`'s parameters, at once: agreeing happened when the model was
/// connected. Refused, saying why, when its kind is switched off. `back`, the dialog it is asked
/// from, stays open.
pub(crate) fn request(app: &mut App, kind: Kind, target: Value, back: Option<Dialog>) {
    app.view.dialog = back;
    if let Err(why) = allowed(app, kind) {
        app.view.status = Some((why, true));
        return;
    }
    start(app, kind, target);
}

/// Why the selection cannot be rewritten (or changed as asked, `kind`) now, if it cannot.
pub(crate) fn can_rewrite(app: &App, kind: Kind) -> Result<(), String> {
    allowed(app, kind)?;
    if app.editor.source.selected().is_none() {
        return Err(tl!("Select the text to rewrite first.").to_owned());
    }
    Ok(())
}

/// `edit.rewrite {how, language?}`: rewrites the source's selection. The rewrite arrives as a
/// suggested change.
pub(crate) fn rewrite(app: &mut App, params: &Value) {
    if let Err(why) = can_rewrite(app, Kind::Rewrite) {
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
    request(app, Kind::Rewrite, target, None);
}

/// The instructions remembered for Ask AI to Change This.
const MAX_INSTRUCTIONS: usize = 8;

/// The Ask AI to Change This dialog: the selection it is for, and the instruction being typed.
pub(crate) struct InstructForm {
    path: String,
    start: usize,
    end: usize,
    text: String,
    focus: bool,
}

/// `edit.instruct`: asks how to change the selection.
pub(crate) fn open_instruct(app: &mut App) {
    if let Err(why) = can_rewrite(app, Kind::Instruct) {
        app.view.status = Some((why, true));
        return;
    }
    crate::editor::send_edits(app);
    let Some(path) = app.session.current().cloned() else { return };
    let r = app.editor.source.doc.selection.primary();
    let (start, end) = (crate::suggest::to_source(app, &path, r.start()), crate::suggest::to_source(app, &path, r.end()));
    app.editor.source.end_visual();
    let form = InstructForm { path: path.as_str().to_owned(), start, end, text: String::new(), focus: true };
    app.view.dialog = Some(Dialog::AiInstruct(Box::new(form)));
}

/// The dialog: one line for the instruction (Enter sends, Escape cancels), the recent ones below.
pub(crate) fn instruct_dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let recent = app.prefs.ai_instructions.clone();
    let Some(Dialog::AiInstruct(form)) = &mut app.view.dialog else { return };
    let mut send = false;
    let mut close = false;
    let modal = crate::sheet::show(ctx, &t, "texcraft-ai-instruct", 480.0, crate::sheet::Leave::Close, |ui| {
        crate::dialogs::title(ui, &t, tl!("Ask AI to Change This"));
        let label = ui.label(RichText::new(tl!("How to change the selection")).color(t.text_muted));
        let edit = egui::TextEdit::singleline(&mut form.text)
            .hint_text(tl!("Flesh out these bullet points"))
            .char_limit(texcraft_engine::ai::MAX_INSTRUCTION_CHARS)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(8.0, 6.0));
        let edit = ui.add(edit).labelled_by(label.id);
        if std::mem::take(&mut form.focus) {
            edit.request_focus();
        }
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !form.text.trim().is_empty() {
            send = true;
        }
        ui.label(
            RichText::new(tl!("The change comes back as a suggested change. It may add text, but it is not offered if it drops or alters a label, reference, citation or formula."))
                .font(t.small())
                .color(t.text_muted),
        );
        if !recent.is_empty() {
            ui.add_space(t.unit * 2.0);
            ui.label(RichText::new(tl!("Recent")).font(t.strong()));
            for r in &recent {
                let b = ui.add(egui::Button::new(RichText::new(r).color(t.text)).frame_when_inactive(false).wrap());
                if b.on_hover_text(tl!("Use this instruction again")).clicked() {
                    form.text.clone_from(r);
                    form.focus = true;
                }
            }
        }
        ui.add_space(t.unit * 3.0);
        crate::dialogs::actions(ui, |ui| {
            let primary = egui::Button::new(RichText::new(tl!("Send")).color(t.on_accent).font(t.strong())).fill(t.accent).stroke(egui::Stroke::NONE);
            if ui.add_enabled(!form.text.trim().is_empty(), primary.min_size(egui::vec2(88.0, 30.0))).clicked() {
                send = true;
            }
            if ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                close = true;
            }
        });
    });
    if modal.close {
        close = true;
    }
    if send {
        let Some(Dialog::AiInstruct(form)) = app.view.dialog.take() else { return };
        let instruction = form.text.trim().to_owned();
        app.prefs.ai_instructions.retain(|i| *i != instruction);
        app.prefs.ai_instructions.insert(0, instruction.clone());
        app.prefs.ai_instructions.truncate(MAX_INSTRUCTIONS);
        app.prefs_changed = true;
        let target = json!({"path": form.path, "start": form.start, "end": form.end, "instruction": instruction});
        request(app, Kind::Instruct, target, None);
    } else if close {
        app.view.dialog = None;
    }
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
    let Some(done) = app.run(kind.command(), target) else { return };
    match kind {
        Kind::Rewrite => app.view.status = Some((tl!("The model is rewriting the selection…").to_owned(), false)),
        Kind::Instruct => app.view.status = Some((tl!("The model is changing the selection…").to_owned(), false)),
        Kind::Explain => app.view.logs = true,
        Kind::Make | Kind::Reference => crate::ai_make::started(app),
        Kind::Ask => app.view.side = Some(crate::Side::Ask),
        Kind::Review => app.view.status = Some((tl!("The model is reviewing the section…").to_owned(), false)),
        Kind::Grammar => app.view.status = Some((tl!("The model is checking the grammar…").to_owned(), false)),
        Kind::Complete => {}
    }
    // The person's prompt was set aside for the app's own: said over the rest.
    if done["prompt_note"].is_string() {
        let said = tl!("Your prompt for {kind} lacks a placeholder it needs, so the app's own was sent: fix it in Settings › AI › Prompts.");
        app.view.status = Some((said.replace("{kind}", kind.label()), true));
    }
}

/// The entries of Rewrite with AI (in the Edit menu and the text's context menu): what was
/// chosen, as `edit.rewrite`'s parameters.
pub(crate) fn rewrite_menu(app: &App, ui: &mut egui::Ui) -> Option<Value> {
    let t = app.tokens;
    let enabled = can_rewrite(app, Kind::Rewrite);
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
    if let Err(why) = &enabled {
        let _ = translate.inner.response.on_disabled_hover_text(why);
    }
    ui.separator();
    if crate::top_bar::item(ui, &t, Some(ICON), tl!("Ask AI to Change This…"), None, can_rewrite(app, Kind::Instruct)) {
        chosen = Some(json!({"instruct": true}));
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
    } else if choice["instruct"] == true {
        crate::ui_cmd::run(app, "edit.instruct");
    } else {
        crate::ui_cmd::run_with(app, "edit.rewrite", choice);
    }
}

/// Settings › AI › Suggest the next words as I type.
fn set_typing(app: &mut App, on: bool) {
    if !on {
        app.prefs.ai_complete = false;
        app.prefs_changed = true;
        let _ = app.session.execute("ai.cancel", &json!({"only": "completion"}));
        return;
    }
    request(app, Kind::Complete, json!({}), Some(Dialog::Settings));
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
    // Refused (switched off, the rate limit, the caret in the preamble): nothing is suggested.
    if let Err(e) = app.session.execute("ai.complete", &json!({"path": path.as_str(), "at": caret, "background": true})) {
        log::debug!("no next words asked for: {e}");
    }
}

/// What a message's explanation offers to do.
pub(crate) enum Act {
    None,
    Explain,
    Cancel,
}

/// Why a message cannot be explained now (another request in flight, the kind switched off), if
/// it cannot: for [`explanation`].
pub(crate) fn explain_blocked(app: &App) -> Option<String> {
    if app.session.ai_busy() {
        return Some(tl!("The model is still answering the last request.").to_owned());
    }
    allowed(app, Kind::Explain).err()
}

/// The explanation of a message, or the button that asks for it, inside its card or popup;
/// `blocked` says why it cannot be asked for now.
pub(crate) fn explanation(ui: &mut egui::Ui, t: &Tokens, e: Option<&Explanation>, blocked: Option<&str>) -> Act {
    let busy = blocked.is_some();
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
                ui.label(RichText::new(tl!("No explanation: {error}").replace("{error}", &crate::i18n::msg(why))).font(t.small()).color(t.error));
                let again = widgets::text_button(ui, t, tl!("Try Again"), !busy);
                if let Some(why) = blocked {
                    let _ = again.clone().on_disabled_hover_text(why);
                }
                if again.clicked() {
                    act = Act::Explain;
                }
            });
        }
        Some(Explanation::Cancelled) | None => {
            let r = ui.add_enabled(!busy, egui::Button::image_and_text(crate::icons::image(ICON, 14.0, t.text_muted), tl!("Explain")));
            let r = r.on_hover_text(tl!("Ask the connected model what this message means and how to fix it"));
            let r = match blocked {
                Some(why) => r.on_disabled_hover_text(why),
                None => r,
            };
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
        ui.label(RichText::new(crate::i18n::msg(UNAVAILABLE)).color(t.text));
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
    if ui.link(tl!("What is sent, and when: the user guide's AI page")).clicked() {
        crate::ui_cmd::run_with(app, "help.guide", &json!({"page": "ai.html"}));
    }
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
    let hide = app.ai.hide_token;
    let token = field(ui, t, tl!("Token"), &mut app.ai.token, hint, hide);
    ui.horizontal_wrapped(|ui| {
        let kept = if status["token_kept"] == "the system's keychain" {
            tl!("The token is kept in the system's keychain, never in the preferences.")
        } else {
            tl!("The token is kept for this session only.")
        };
        ui.label(RichText::new(kept).font(t.small()).color(t.text_muted));
        ui.checkbox(&mut app.ai.hide_token, tl!("Hide the token"));
    });
    kinds(app, ui, t, &status);
    let enter = [endpoint, token].iter().any(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        let ready = !app.ai.endpoint.trim().is_empty();
        let connect_button = ui
            .add_enabled(ready, egui::Button::new(tl!("Connect")))
            .on_hover_text(tl!("Save the connection and agree to send the kinds of request switched on to it"));
        if (connect_button.clicked() || (enter && ready)) && connect(app) {
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
            app.ai = AiView { kept: app.ai.kept, filled: true, hide_token: app.ai.hide_token, ..AiView::default() };
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
        requests(app, ui, t, &status);
    }
    prompts(app, ui, t);
}

/// Each kind of request: what it sends, where, and its switch (on by default). Connecting agrees
/// to the ones switched on; switching one off stops it at once.
fn kinds(app: &mut App, ui: &mut egui::Ui, t: &Tokens, status: &Value) {
    ui.add_space(t.unit * 2.0);
    ui.horizontal(|ui| {
        ui.add(crate::icons::image(ICON, 14.0, t.text_muted));
        ui.label(RichText::new(tl!("What may be sent")).font(t.strong()));
    });
    let connected = status["connected"] == true;
    let to = status["destination"].as_str().unwrap_or_default();
    let address = app.ai.endpoint.trim();
    let said = if !connected {
        if address.is_empty() {
            tl!("Connecting agrees to send the kinds switched on below to the endpoint's address, without asking again.").to_owned()
        } else {
            tl!("Connecting agrees to send the kinds switched on below to {address}, without asking again.").replace("{address}", address)
        }
    } else if status["local"] == true {
        tl!("Nothing leaves this computer. A kind switched off is never sent.").to_owned()
    } else {
        let model = status["model"].as_str().unwrap_or_default();
        tl!("Sent to {destination} ({model}) without asking again. Switch a kind off to stop it.")
            .replace("{destination}", to)
            .replace("{model}", model)
    };
    ui.label(RichText::new(said).font(t.small()).color(t.text_muted));
    let states: Vec<(String, String)> = status["kinds"]
        .as_array()
        .map(|k| {
            k.iter().map(|k| (k["request"].as_str().unwrap_or_default().to_owned(), k["state"].as_str().unwrap_or_default().to_owned())).collect()
        })
        .unwrap_or_default();
    let state = |kind: Kind| states.iter().find(|(r, _)| r == kind.request()).map_or("on", |(_, s)| s.as_str());
    let pending = connected && Kind::ALL.iter().any(|k| state(*k) == "not agreed");
    let mut switched = None;
    for kind in Kind::ALL {
        let mut on = state(kind) != "off";
        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut on, kind.label()).changed() {
                switched = Some((kind, on));
            }
            ui.label(RichText::new(kind.sends()).font(t.small()).color(t.text_muted));
            if connected && state(kind) == "not agreed" {
                ui.label(RichText::new(tl!("Not agreed to yet")).font(t.small()).color(t.warning));
            }
        });
    }
    if let Some((kind, on)) = switched {
        app.run("ai.switch", json!({"request": kind.request(), "on": on}));
        if kind == Kind::Complete && !on {
            app.prefs.ai_complete = false;
            app.prefs_changed = true;
        }
    }
    if pending {
        ui.label(
            RichText::new(tl!(
                "The address, the model or a withdrawal changed what was agreed: the kinds marked are not sent until you agree again."
            ))
            .font(t.small())
            .color(t.warning),
        );
        if ui.button(tl!("Agree to Send")).clicked() && app.run("ai.consent", json!({})).is_some() {
            let said = tl!("Agreed: the kinds switched on are sent to {destination} without asking again.").replace("{destination}", to);
            app.view.status = Some((said, false));
        }
    }
}

/// Settings › AI › Prompts: each kind's instructions, its placeholders, Reset to Default, and
/// what is missing from a changed one (kept, but the app's own is sent until it is fixed).
fn prompts(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let Ok(list) = app.session.execute("ai.prompts", &json!({})) else { return };
    let list = list["prompts"].as_array().cloned().unwrap_or_default();
    egui::CollapsingHeader::new(RichText::new(tl!("Prompts")).font(t.strong())).id_salt("settings-ai-prompts").show(ui, |ui| {
        ui.label(
            RichText::new(tl!(
                "What each kind of request tells the model. Names in braces are replaced by what the request sends. Whatever a prompt says, what comes back is checked the same way: a rewrite keeps the selection's LaTeX, made LaTeX is typeset first, and every change is a suggestion."
            ))
            .font(t.small())
            .color(t.text_muted),
        );
        for kind in Kind::ALL {
            let Some(p) = list.iter().find(|p| p["request"] == kind.request()) else { continue };
            let changed = p["changed"] == true;
            let default = p["default"].as_str().unwrap_or_default().to_owned();
            let title = if changed { tl!("{kind} (changed)").replace("{kind}", kind.label()) } else { kind.label().to_owned() };
            egui::CollapsingHeader::new(title).id_salt(("settings-ai-prompt", kind.request())).show(ui, |ui| {
                let text = app.ai.prompts.entry(kind.request().to_owned()).or_insert_with(|| p["current"].as_str().unwrap_or_default().to_owned());
                let name = tl!("Prompt for {kind}").replace("{kind}", kind.label());
                let label = ui.label(RichText::new(&name).font(t.small()).color(t.text_muted));
                let edit = egui::TextEdit::multiline(text).font(t.code_font()).desired_rows(6).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0));
                let r = ui.add(edit).labelled_by(label.id);
                let typed = r.changed().then(|| text.clone());
                let placeholders = texcraft_engine::ai::prompts::spec(kind.request()).map(|s| s.placeholders).unwrap_or_default();
                for ph in placeholders {
                    let name = format!("{{{}}}", ph.name);
                    let means = crate::i18n::t(ph.means);
                    let line = if ph.required {
                        tl!("{name} (needed): {means}").replace("{name}", &name).replace("{means}", means)
                    } else {
                        tl!("{name}: {means}").replace("{name}", &name).replace("{means}", means)
                    };
                    ui.label(RichText::new(line).font(t.small()).color(t.text));
                }
                let missing: Vec<String> =
                    p["missing"].as_array().map(|m| m.iter().filter_map(|m| m.as_str().map(|m| format!("{{{m}}}"))).collect()).unwrap_or_default();
                if !missing.is_empty() {
                    let said = tl!("It lacks {missing}: the app's own prompt is sent until they are back.").replace("{missing}", &missing.join(", "));
                    let warn = ui.label(RichText::new(&said).font(t.small()).color(t.warning));
                    crate::status::announce(ui, &warn, &said);
                }
                let reset = ui.add_enabled(changed, egui::Button::new(tl!("Reset to Default")));
                let reset_name = tl!("Reset to Default: {kind}").replace("{kind}", kind.label());
                reset.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, changed, &reset_name));
                if reset.clicked() {
                    app.run("ai.set_prompt", json!({"request": kind.request()}));
                    app.ai.prompts.insert(kind.request().to_owned(), default.clone());
                    app.view.status = Some((tl!("The prompt for {kind} is the app's own again.").replace("{kind}", kind.label()), false));
                } else if let Some(text) = typed {
                    app.run("ai.set_prompt", json!({"request": kind.request(), "text": text}));
                }
            });
        }
    });
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

/// The request log: when, where, how much, how it ended; what was sent on demand, for the last
/// requests of this session (the log itself never holds it).
fn requests(app: &mut App, ui: &mut egui::Ui, t: &Tokens, status: &Value) {
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
                .map(|d| crate::i18n::time(d.with_timezone(&chrono::Local).time()));
            let line = tl!("{time} · {size} to {destination} · {model} · {outcome}")
                .replace("{time}", &at.unwrap_or_default())
                .replace("{size}", &crate::i18n::file_size(bytes))
                .replace("{destination}", s["destination"].as_str().unwrap_or_default())
                .replace("{model}", s["model"].as_str().unwrap_or_default())
                .replace("{outcome}", outcome);
            let run = s["run"].as_str().unwrap_or_default().to_owned();
            let open = app.ai.shown.as_ref().is_some_and(|(r, _)| *r == run);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(line).font(t.small()).color(t.text));
                let label = if open { tl!("Hide What Was Sent") } else { tl!("Show What Was Sent") };
                if widgets::text_button(ui, t, label, true).clicked() {
                    app.ai.shown = if open {
                        None
                    } else {
                        let text = match app.session.execute("ai.sent", &json!({"run": run})) {
                            Ok(v) => v["text"].as_str().unwrap_or_default().to_owned(),
                            Err(e) => e,
                        };
                        Some((run.clone(), text))
                    };
                }
            });
            if let Some((_, text)) = app.ai.shown.as_ref().filter(|(r, _)| *r == run) {
                egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner()).inner_margin(egui::Margin::same(8)).show(
                    ui,
                    |ui| {
                        egui::ScrollArea::vertical().id_salt(("ai-sent", &run)).max_height(220.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let label = ui.add(egui::Label::new(RichText::new(text).font(t.code_font()).color(t.text)).wrap().selectable(true));
                            let _ = label.accessible_name(tl!("What was sent"));
                        });
                    },
                );
            }
        }
    });
}
