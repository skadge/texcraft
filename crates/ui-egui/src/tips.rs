//! The start page's tip of the day: one tip about a feature that is easy to miss, chosen by the
//! date (the same all day, another the next), with Previous and Next to look at others and, when
//! the feature has one, a button that does it. Only tips whose feature exists in this build are
//! shown (the AI and push and pull on the desktop, the zip download in the browser).

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::App;
use crate::theme::Tokens;

/// Where a tip's feature exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Needs {
    Anywhere,
    /// The desktop app (a project folder, a model connected from the computer).
    Desktop,
    /// The browser version (its projects live in the browser).
    Browser,
}

/// A tip: its text, where it applies, and what its button does.
pub(crate) struct Tip {
    pub text: &'static str,
    pub needs: Needs,
    pub action: Option<Action>,
}

/// A tip's button: its label, its command and the command's parameters.
pub(crate) struct Action {
    pub label: &'static str,
    pub command: &'static str,
    pub params: fn() -> Value,
}

fn none() -> Value {
    json!({})
}

const fn tip(text: &'static str, needs: Needs, action: Option<Action>) -> Tip {
    Tip { text, needs, action }
}

/// The guide's page `page`, as a tip's button opens it.
macro_rules! guide {
    ($page:literal) => {
        Some(Action { label: tn!("Read More in the Guide"), command: "help.guide", params: || json!({"page": $page}) })
    };
}

/// Every tip, in no particular order: the date picks one.
pub(crate) const TIPS: &[Tip] = &[
    tip(
        tn!("Did you know that you can drop a whole project as a zip file on the start page? It becomes a new project."),
        Needs::Anywhere,
        guide!("projects.html#import"),
    ),
    tip(
        tn!(
            "If you are a Vim user, you can turn on Vim mode in Settings › Editor: modes, motions, registers, and text objects for environments and formulas."
        ),
        Needs::Anywhere,
        Some(Action { label: tn!("Open Settings"), command: "app.settings", params: || json!({"tab": "editor"}) }),
    ),
    tip(
        tn!("The Formatted switch above the text shows your document as it reads, without the markup, and you can edit it there too."),
        Needs::Anywhere,
        guide!("writing.html#formatted"),
    ),
    tip(
        tn!("F6 moves the keyboard between the regions of the window: the menus, the rail, the side panel, the editor and the PDF."),
        Needs::Anywhere,
        guide!("keyboard.html#regions"),
    ),
    tip(tn!("F7 goes to the next spelling or grammar problem in the file."), Needs::Anywhere, guide!("writing.html#spelling")),
    tip(
        tn!("F12 on a reference or a command goes to where it is defined, even in another file."),
        Needs::Anywhere,
        guide!("writing.html#source-editor"),
    ),
    tip(
        tn!("Double-click a place in the PDF to go to the line that wrote it. Edit › Show in PDF goes the other way."),
        Needs::Anywhere,
        guide!("building.html#synctex"),
    ),
    tip(
        tn!(
            "Export, in the PDF's toolbar, saves your document as a DOCX or OpenDocument text, a web page or Markdown, and beamer slides as a PPTX presentation."
        ),
        Needs::Anywhere,
        guide!("building.html#export"),
    ),
    tip(
        tn!("Turn on Suggest in the editor's toolbar, and your edits become suggestions that can be accepted or rejected one by one."),
        Needs::Anywhere,
        guide!("review.html#suggest"),
    ),
    tip(
        tn!("Select some text and right-click it to add a comment beside it. The Review section lists every comment of the project."),
        Needs::Anywhere,
        guide!("review.html#comments"),
    ),
    tip(
        tn!("Every save that changes something is a version. The History section says what each one changed and can restore any of them."),
        Needs::Anywhere,
        guide!("projects.html#history"),
    ),
    tip(
        tn!("Save Project to GitLab or GitHub puts a project online in a new repository, ready to share."),
        Needs::Anywhere,
        guide!("forges.html#save-to"),
    ),
    tip(tn!("A project folder in a git repository can push and pull from its History section."), Needs::Desktop, guide!("forges.html#push-pull")),
    tip(
        tn!("Edit › Go to File… lists the project's files as you type, the ones you opened last first."),
        Needs::Anywhere,
        guide!("writing.html#find"),
    ),
    tip(tn!("Edit › Find in Project searches, and replaces, in every file of the project."), Needs::Anywhere, guide!("writing.html#find")),
    tip(
        tn!("Insert › Symbols… opens a palette of mathematical symbols you can search by name, and says which package a symbol needs."),
        Needs::Anywhere,
        guide!("writing.html#symbols"),
    ),
    tip(
        tn!(
            "Connect an AI model in Settings › AI to explain a build error, rewrite a selection or review a section. Nothing is sent before you connect one."
        ),
        Needs::Desktop,
        Some(Action { label: tn!("Open Settings"), command: "app.settings", params: || json!({"tab": "ai"}) }),
    ),
    tip(
        tn!("On a phone or in a narrow window, the editor and the PDF become two tabs, and the side panel a drawer."),
        Needs::Anywhere,
        guide!("getting-started.html#narrow"),
    ),
    tip(tn!("Drop an image file on the formatted view, and it becomes a figure with a caption."), Needs::Anywhere, guide!("writing.html#figures")),
    tip(
        tn!("Projects in the browser stay in this browser. File › Download Project as Zip keeps a copy elsewhere."),
        Needs::Browser,
        guide!("projects.html#library"),
    ),
    tip(
        tn!("Edit › Quick Fix lists the fixes for the error at the caret, such as adding the package a command needs."),
        Needs::Anywhere,
        guide!("writing.html#source-editor"),
    ),
    tip(
        tn!("Every shortcut can be changed: Keyboard Shortcuts, at the bottom of the rail, lists them all."),
        Needs::Anywhere,
        Some(Action { label: tn!("Keyboard Shortcuts"), command: "help.shortcuts", params: none }),
    ),
    tip(
        tn!("New Project has journal and conference templates from publishers such as Elsevier, IEEE and ACM, kept for offline use once downloaded."),
        Needs::Anywhere,
        Some(Action { label: tn!("New Project"), command: "project.from_template", params: none }),
    ),
];

/// The tip of the day's state: how far Previous and Next moved from the day's tip, and the date
/// when one is given (tests).
#[derive(Debug, Default)]
pub(crate) struct TipState {
    pub offset: i64,
    pub today: Option<chrono::NaiveDate>,
}

/// The tips whose feature exists here.
pub(crate) fn available(app: &App) -> Vec<&'static Tip> {
    let in_browser = app.session.library().is_some();
    TIPS.iter()
        .filter(|t| match t.needs {
            Needs::Anywhere => true,
            Needs::Desktop => !in_browser,
            Needs::Browser => in_browser,
        })
        .collect()
}

/// The tip shown: the day's, moved by Previous and Next.
pub(crate) fn current(app: &App) -> Option<&'static Tip> {
    use chrono::Datelike as _;
    let tips = available(app);
    let today = app.tips.today.unwrap_or_else(|| chrono::Local::now().date_naive());
    let len = i64::try_from(tips.len()).ok().filter(|n| *n > 0)?;
    let i = (i64::from(today.num_days_from_ce()) + app.tips.offset).rem_euclid(len);
    tips.get(usize::try_from(i).ok()?).copied()
}

/// The card, when tips are on.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    if !app.prefs.show_tips {
        return;
    }
    let Some(tip) = current(app) else { return };
    let mut run: Option<(&'static str, Value)> = None;
    let frame = egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner_large()).inner_margin(egui::Margin::symmetric(16, 12));
    // A region of its own, named, so a screen reader can find it and skip it.
    let region = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::Region).accessibility_label(tl!("Tip of the day"));
    ui.scope_builder(region, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                crate::icons::paint(ui, icon, "info", 16.0, t.accent);
                ui.label(RichText::new(tl!("Tip of the day")).font(t.strong()));
            });
            ui.add_space(t.unit);
            ui.horizontal_wrapped(|ui| ui.label(RichText::new(crate::i18n::t(tip.text)).color(t.text)));
            ui.add_space(t.unit * 2.0);
            // In reading order: the tip's own action, then Previous and Next, then the switch.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = t.unit * 2.0;
                if let Some(action) = &tip.action
                    && ui.button(crate::i18n::t(action.label)).clicked()
                {
                    run = Some((action.command, (action.params)()));
                }
                if ui.button(tl!("Previous Tip")).clicked() {
                    app.tips.offset -= 1;
                }
                if ui.button(tl!("Next Tip")).clicked() {
                    app.tips.offset += 1;
                }
                let mut on = app.prefs.show_tips;
                if ui.checkbox(&mut on, tl!("Show tips at startup")).changed() {
                    set_shown(app, on);
                }
            });
        });
    });
    if let Some((id, params)) = run {
        crate::ui_cmd::run_with(app, id, &params);
    }
}

/// Turns the tip of the day on or off (the card's switch, Settings › Appearance), saying where to
/// turn it back on when the card goes.
pub(crate) fn set_shown(app: &mut App, on: bool) {
    app.prefs.show_tips = on;
    app.prefs_changed = true;
    if !on {
        app.view.status = Some((tl!("Tips are off. Settings › Appearance turns them on again.").to_owned(), false));
    }
}

#[cfg(test)]
mod tests {
    /// A tip's Read More opens a page of the guide: the page and its heading must exist.
    #[test]
    fn every_tip_s_guide_page_and_heading_exist() {
        let guide = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide");
        for tip in super::TIPS {
            let Some(action) = &tip.action else { continue };
            if action.command != "help.guide" {
                continue;
            }
            let params = (action.params)();
            let page = params["page"].as_str().unwrap_or_default();
            let (file, anchor) = page.split_once('#').unwrap_or((page, ""));
            let html = std::fs::read_to_string(guide.join(file)).unwrap_or_default();
            assert!(!html.is_empty(), "{page}: no such page");
            assert!(anchor.is_empty() || html.contains(&format!("id=\"{anchor}\"")), "{page}: no such heading");
        }
    }
}
