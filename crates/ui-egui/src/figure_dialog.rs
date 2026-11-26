//! The figure dialog (docs/ROADMAP.md, F4): a figure card's Edit button opens it on the figure's
//! image (from the project's images), the image's width as a share of the line, where LaTeX may
//! place it, its caption (as LaTeX source) and its label. Applying it runs `figure.edit` with the
//! parts that changed, so only their source changes.

use egui::RichText;
use serde_json::json;
use texcraft_engine::project::{Contents, ProjectPath};
use texcraft_latex::figure;

use crate::App;
use crate::dialogs::{Dialog, Outcome, buttons, title};
use crate::theme::Tokens;

/// The placements offered, with their letters.
const PLACES: [(char, &str); 4] =
    [('h', tn!("Here, if it fits")), ('t', tn!("At the top of a page")), ('b', tn!("At the bottom of a page")), ('p', tn!("On a page of floats"))];

pub(crate) struct FigureForm {
    path: ProjectPath,
    /// A character inside the figure.
    at: usize,
    image: String,
    /// The project's images, as `\includegraphics` takes them.
    images: Vec<String>,
    /// The width as a percentage of the line; `None` when it is not one (`5cm`) and unchanged.
    width: Option<f32>,
    /// The width as written, when it is not a share of the line.
    other_width: Option<String>,
    places: [bool; 4],
    /// Other placement letters (`!`, `H`), kept.
    place_extra: String,
    caption: String,
    label: String,
    /// The values the figure had, to send only what changed.
    before: (String, Option<f32>, String, String, String),
    focus: bool,
}

/// Opens the dialog on the figure of `path` around byte `at` of `text`.
pub(crate) fn open(app: &mut App, path: &ProjectPath, text: &str, at: usize) {
    let Some(fig) = figure::find(text, at) else {
        app.view.status = Some((tl!("This figure cannot be edited in the dialog: it is not closed by \\end{figure}.").to_owned(), true));
        return;
    };
    let src = |r: &std::ops::Range<usize>| text.get(r.clone()).unwrap_or_default().to_owned();
    let image = fig.image.as_ref().map(|g| src(&g.path).trim().to_owned()).unwrap_or_default();
    let options = fig.image.as_ref().and_then(|g| g.options.as_ref()).map(&src).unwrap_or_default();
    let width = figure::width_fraction(&options).map(|f| (f * 100.0).round());
    let other_width = options.split(',').find_map(|o| o.split_once('=').filter(|(k, _)| k.trim() == "width").map(|(_, v)| v.trim().to_owned()));
    let placement = fig.placement.as_ref().map(&src).unwrap_or_default();
    let places = PLACES.map(|(c, _)| placement.contains(c));
    let place_extra: String = placement.chars().filter(|c| !"htbp".contains(*c)).collect();
    let caption = fig.caption.as_ref().map(|c| src(&c.0)).unwrap_or_default();
    let label = fig.label.as_ref().map(&src).unwrap_or_default();
    let main = app.session.project().main().cloned().unwrap_or_else(|| path.clone());
    let mut images: Vec<String> = app
        .session
        .project()
        .files()
        .filter(|(p, c)| texcraft_engine::cmd::is_image(p) && matches!(c, Contents::Binary(_) | Contents::Text(_)))
        .map(|(p, _)| texcraft_engine::cmd::image_path(&main, p))
        .collect();
    images.sort();
    let at = text.get(..fig.range.start + 1).map_or(0, |s| s.chars().count());
    let before = (image.clone(), width, placement.clone(), caption.clone(), label.clone());
    let other_width = other_width.filter(|_| width.is_none());
    let form = FigureForm { path: path.clone(), at, image, images, width, other_width, places, place_extra, caption, label, before, focus: true };
    app.view.dialog = Some(Dialog::Figure(Box::new(form)));
}

/// The form; returns the outcome.
pub(crate) fn form(ui: &mut egui::Ui, t: &Tokens, f: &mut FigureForm) -> Outcome {
    title(ui, t, tl!("Edit Figure"));
    let label = ui.label(RichText::new(tl!("Image")).color(t.text_muted));
    let shown = if f.image.is_empty() { tl!("No image").to_owned() } else { f.image.clone() };
    let combo = craft_ui::ComboBox::from_id_salt("figure-image").width(ui.available_width()).selected_text(shown).show_ui(ui, |ui| {
        for image in &f.images {
            if ui.selectable_label(&f.image == image, image).clicked() {
                f.image = image.clone();
            }
        }
    });
    let combo = combo.response.labelled_by(label.id);
    if std::mem::take(&mut f.focus) {
        combo.request_focus();
    }
    if f.images.is_empty() {
        ui.label(
            RichText::new(tf!(
                "The project has no images: add one with {menu} › {command}.",
                menu = tl!("File"),
                command = crate::i18n::tid("file.import", "Add Files…").trim_end_matches('…')
            ))
            .font(t.small())
            .color(t.text_muted),
        );
    }
    ui.add_space(t.unit * 3.0);
    let label = ui.label(RichText::new(tl!("Width, as a share of the line")).color(t.text_muted));
    let mut percent = f.width.unwrap_or(100.0);
    let slider = ui.add(egui::Slider::new(&mut percent, 5.0..=100.0).step_by(5.0).suffix(" %")).labelled_by(label.id);
    if slider.changed() {
        f.width = Some(percent);
        f.other_width = None;
    }
    if let Some(other) = &f.other_width {
        ui.label(RichText::new(tl!("Now {width}: kept unless you move the slider.").replace("{width}", other)).font(t.small()).color(t.text_muted));
    }
    ui.add_space(t.unit * 3.0);
    ui.label(RichText::new(tl!("Where LaTeX may place it")).color(t.text_muted));
    for ((_, name), on) in PLACES.iter().zip(f.places.iter_mut()) {
        ui.checkbox(on, crate::i18n::t(name));
    }
    ui.add_space(t.unit * 3.0);
    let label = ui.label(RichText::new(tl!("Caption (LaTeX)")).color(t.text_muted));
    ui.add(egui::TextEdit::multiline(&mut f.caption).desired_rows(2).desired_width(f32::INFINITY)).labelled_by(label.id);
    ui.add_space(t.unit * 2.0);
    let label = ui.label(RichText::new(tl!("Label")).color(t.text_muted));
    ui.add(egui::TextEdit::singleline(&mut f.label).hint_text("fig:name").desired_width(f32::INFINITY)).labelled_by(label.id);
    let label_ok = !f.label.contains(['{', '}', '\\', '%', '#']);
    if !label_ok {
        ui.label(RichText::new(tl!("A label cannot hold braces, backslashes, % or #.")).font(t.small()).color(t.error));
    }
    buttons(ui, t, tl!("Apply"), false, label_ok)
}

/// Applies the form with `figure.edit`: only what changed is sent.
pub(crate) fn apply(app: &mut App, f: &FigureForm) {
    let mut params = json!({"path": f.path.as_str(), "at": f.at});
    let (image, width, placement, caption, label) = &f.before;
    if &f.image != image && !f.image.is_empty() {
        params["image"] = json!(f.image);
    }
    if let Some(w) = f.width.filter(|w| Some(*w) != *width) {
        params["width"] = json!(w / 100.0);
    }
    let letters: String = PLACES.iter().zip(f.places).filter(|(_, on)| *on).map(|((c, _), _)| *c).collect();
    let new_placement = if letters.is_empty() { String::new() } else { format!("{letters}{}", f.place_extra) };
    if &new_placement != placement {
        params["placement"] = json!(new_placement);
    }
    if &f.caption != caption {
        params["caption"] = json!(f.caption.trim());
    }
    if &f.label != label {
        params["label"] = json!(f.label.trim());
    }
    crate::editor::send_edits(app);
    if app.run("figure.edit", params).is_some() {
        app.view.status = Some((tl!("The figure is changed.").to_owned(), false));
    }
    app.editor.formatted.editing.focus = true;
}
