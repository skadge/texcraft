//! The New Project gallery (docs/plan.md, phase 1, item 2b): a blank project, or one of the
//! publishers' templates, by category and search, with each template's source and licence shown
//! before it is used. A template that is not kept yet is downloaded on a background thread (the
//! window never waits), then copied into the chosen folder by `project.from_template`. In the browser
//! the shell fetches it from the web build's own site ([`App::take_template_requests`]), and the
//! project is made in the browser's library under a name the user gives.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::{Align, Layout, RichText, Sense, vec2};
use serde_json::json;
use texcraft_templates::{Catalog, Template};

use crate::dialogs::Next;
use crate::picks::Pick;
use crate::theme::{Tokens, blend};
use crate::{App, icons, widgets};

const CATEGORIES: &[(&str, &str, &str)] = &[
    ("", "All", "list"),
    ("journal", "Journals", "book-open"),
    ("conference", "Conferences", "file-text"),
    ("cv", "CVs", "file-code"),
    ("poster", "Posters", "image"),
    ("report", "Reports", "file-text"),
];

/// A download running on its own thread: one template, or every one.
pub(crate) struct Job {
    rx: Receiver<Result<String, String>>,
    /// What to do when it has finished: create a project in this folder, or nothing.
    then: Option<(String, String)>,
    pub label: String,
}

/// The gallery's state while it is open.
#[derive(Default)]
pub(crate) struct Gallery {
    pub open: bool,
    category: usize,
    search: String,
    selected: Option<String>,
    pub job: Option<Job>,
    pub error: Option<String>,
    /// The template a folder is being picked for.
    pub pending: Option<String>,
    /// Focus goes to the search box on the next frame (when the gallery opens).
    focus_search: bool,
    /// In the browser: the templates the shell is asked to fetch, and where to say it did.
    requests: Vec<String>,
    fetched: Option<Sender<Result<String, String>>>,
}

fn icon_for(category: &str) -> &'static str {
    CATEGORIES.iter().find(|(c, _, _)| *c == category).map_or("file-text", |(_, _, icon)| icon)
}

impl App {
    /// Opens the gallery (File › New Project from Template, the start page's New Project).
    pub(crate) fn open_gallery(&mut self) {
        self.gallery.open = true;
        self.gallery.error = None;
        self.gallery.focus_search = true;
    }

    /// Whether template `t` can be used without downloading it.
    pub(crate) fn template_kept(&self, t: &Template) -> bool {
        self.session.has_template(&t.id) || self.session.template_store().is_some_and(|s| s.kept(t).is_some())
    }

    /// The templates to fetch for the browser's page (the shell's work), oldest first; each is
    /// answered with [`App::template_fetched`].
    pub fn take_template_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.gallery.requests)
    }

    /// The files of template `id` (project paths and bytes), or why they could not be fetched.
    pub fn template_fetched(&mut self, id: String, files: Result<Vec<(String, Vec<u8>)>, String>) {
        let result = files.map(|files| {
            self.session.keep_template(id.clone(), files);
            id
        });
        if let Some(tx) = &self.gallery.fetched {
            let _ = tx.send(result);
        }
    }

    /// Starts downloading `ids` on a thread; `then` creates a project from the first when done.
    pub(crate) fn start_download(&mut self, ids: Vec<String>, then: Option<(String, String)>, label: String) {
        if self.session.library().is_some() {
            // The browser: the shell fetches them.
            let (tx, rx) = channel();
            self.gallery.requests.extend(ids);
            self.gallery.fetched = Some(tx);
            self.gallery.job = Some(Job { rx, then, label });
            return;
        }
        let Some(store) = self.session.template_store().cloned() else {
            self.gallery.error = Some(tl!("This build of TeXCraft has no folder to keep templates in.").to_owned());
            return;
        };
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new().name("texcraft-templates".to_owned()).spawn(move || {
            let result = (|| -> Result<String, String> {
                let catalog = Catalog::builtin().map_err(|e| e.to_string())?;
                for id in &ids {
                    let template = catalog.find(id).map_err(|e| e.to_string())?;
                    store.fetch(template).map_err(|e| e.to_string())?;
                }
                Ok(ids.join(", "))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.gallery.job = Some(Job { rx, then, label }),
            Err(e) => self.gallery.error = Some(e.to_string()),
        }
    }

    /// Starts a project from template `id`: asks for a new or empty folder, then downloads the
    /// template if it is not kept yet (that one only) and opens the project made from it.
    pub(crate) fn create_from_template(&mut self, id: String) {
        if self.gallery.job.is_some() {
            return;
        }
        self.gallery.error = None;
        if self.session.library().is_some() {
            // The browser: fetched first if it is not yet, then named.
            let kept = Catalog::builtin().ok().and_then(|c| c.find(&id).ok().map(|t| self.template_kept(t))).unwrap_or(false);
            if kept {
                self.ask_template_project_name(id);
            } else {
                self.gallery.pending = Some(id.clone());
                self.start_download(vec![id], None, tl!("Fetching the template…").to_owned());
            }
        } else if self.native_pickers {
            self.gallery.pending = Some(id);
            self.request_pick(Pick::TemplateFolder);
        } else {
            // Without pickers the folder is typed, and the template is copied (and downloaded
            // where downloads exist) by the command itself.
            let prompt = crate::dialogs::Prompt {
                title: tl!("New Project from Template"),
                label: tl!("A new or empty folder for the project"),
                value: String::new(),
                action: tl!("Create"),
                command: "project.from_template",
                params: Box::new(move |v| json!({"template": id, "folder": v})),
                focus: true,
            };
            self.gallery.open = false;
            self.view.dialog = Some(crate::dialogs::Dialog::Prompt(prompt));
        }
    }

    /// Asks for the name of the project of the browser's library made from template `id`.
    fn ask_template_project_name(&mut self, id: String) {
        let prompt = crate::dialogs::Prompt {
            title: tl!("New Project from Template"),
            label: tl!("A name for the project, kept in this browser"),
            value: String::new(),
            action: tl!("Create"),
            command: "project.from_template",
            params: Box::new(move |v| json!({"template": id, "folder": v})),
            focus: true,
        };
        self.gallery.open = false;
        self.view.dialog = Some(crate::dialogs::Dialog::Prompt(prompt));
    }

    /// A project made from a template was opened: the editor shows it.
    pub(crate) fn template_project_opened(&mut self) {
        self.gallery.open = false;
        self.start = false;
        self.remember_project();
        self.pdf = crate::pdf_view::PdfView::default();
        self.view.last_edit = Some(0.0);
    }

    /// The folder for `template`'s project was chosen.
    pub(crate) fn template_folder_chosen(&mut self, folder: String) {
        let Some(id) = self.gallery.pending.take() else { return };
        let label = tl!("Downloading the template from TeX Live…").to_owned();
        self.start_download(vec![id.clone()], Some((id, folder)), label);
    }

    /// Checks the running download, each frame.
    pub(crate) fn poll_gallery(&mut self) {
        let Some(job) = &self.gallery.job else { return };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(tl!("The download stopped unexpectedly.").to_owned()),
        };
        let then = self.gallery.job.take().and_then(|j| j.then);
        match (result, then) {
            (Err(e), _) => {
                // Shown in the gallery, and in the status bar when the gallery is closed.
                self.view.status = Some((e.clone(), true));
                self.gallery.error = Some(e);
            }
            (Ok(_), Some((template, folder))) => {
                if self.run("project.from_template", json!({"template": template, "folder": folder})).is_some() {
                    self.template_project_opened();
                } else {
                    self.gallery.error = self.view.status.take().map(|(m, _)| m);
                }
            }
            (Ok(_), None) => {
                // The browser: the fetched template is named next.
                if self.session.library().is_some()
                    && let Some(id) = self.gallery.pending.take()
                {
                    self.ask_template_project_name(id);
                }
            }
        }
    }
}

pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    if !app.gallery.open {
        return;
    }
    let t = app.tokens;
    let catalog = Catalog::builtin().ok();
    let templates: Vec<Template> = catalog.map(|c| c.templates().to_vec()).unwrap_or_default();
    let kept_ids: Vec<String> =
        Catalog::builtin().map(|c| c.templates().iter().filter(|t| app.template_kept(t)).map(|t| t.id.clone()).collect()).unwrap_or_default();
    let kept = |t: &Template| kept_ids.contains(&t.id);
    let in_browser = app.session.library().is_some();
    let mut close = false;
    let mut create: Option<String> = None;
    let mut blank = false;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-gallery")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(866.0); // four cards and their gaps beside the categories, with no leftover strip
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("New Project")).font(t.title()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, &t, "x", tl!("Close"), true).clicked() {
                    close = true;
                }
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut app.gallery.search).hint_text(tl!("Search by title, publisher or class")).desired_width(280.0),
                );
                if std::mem::take(&mut app.gallery.focus_search) {
                    edit.request_focus();
                }
                let _ = edit.accessible_name(tl!("Search templates"));
            });
        });
        ui.add_space(t.unit * 3.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                for (i, (key, label, icon)) in CATEGORIES.iter().enumerate() {
                    let count = templates.iter().filter(|t| key.is_empty() || t.category == *key).count();
                    if count == 0 && !key.is_empty() {
                        continue;
                    }
                    let tag = count.to_string();
                    let row = widgets::Row {
                        depth: 0,
                        icon,
                        label: crate::i18n::t(label),
                        selected: app.gallery.category == i,
                        disclosure: None,
                        tag: Some(&tag),
                        icon_color: None,
                    };
                    if widgets::row(ui, &t, &row).clicked() {
                        app.gallery.category = i;
                    }
                }
            });
            ui.add_space(t.unit * 3.0);
            ui.vertical(|ui| {
                let (category, _, _) = CATEGORIES.get(app.gallery.category).copied().unwrap_or(("", "", ""));
                let needle = app.gallery.search.to_lowercase();
                let shown: Vec<&Template> = templates
                    .iter()
                    .filter(|t| category.is_empty() || t.category == category)
                    .filter(|t| {
                        needle.is_empty() || [&t.title, &t.publisher, &t.source_text(), &t.id].iter().any(|s| s.to_lowercase().contains(&needle))
                    })
                    .collect();
                egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                        if category.is_empty() && needle.is_empty() && blank_card(ui, &t) {
                            blank = true;
                        }
                        for template in &shown {
                            let selected = app.gallery.selected.as_deref() == Some(template.id.as_str());
                            if card(ui, &t, template, selected, kept(template), in_browser) {
                                app.gallery.selected = Some(template.id.clone());
                            }
                        }
                    });
                    if shown.is_empty() {
                        ui.label(RichText::new(tl!("No template matches.")).color(t.text_muted));
                    }
                });
                ui.add_space(t.unit * 3.0);
                widgets::hairline(ui, &t);
                ui.add_space(t.unit * 2.0);
                details(app, ui, &t, &templates, &kept, &mut create);
            });
        });
    });
    // While a download runs the gallery stays open, showing its progress.
    if (modal.should_close() || close) && app.gallery.job.is_none() {
        app.gallery.open = false;
    }
    if blank {
        app.gallery.open = false;
        app.guard(Next::NewProject);
    }
    if let Some(id) = create {
        app.create_from_template(id);
    }
}

/// The details of the selected template, and Create.
fn details(app: &mut App, ui: &mut egui::Ui, t: &Tokens, templates: &[Template], kept: &dyn Fn(&Template) -> bool, create: &mut Option<String>) {
    let selected = app.gallery.selected.as_ref().and_then(|id| templates.iter().find(|t| &t.id == id));
    ui.horizontal(|ui| {
        ui.set_min_height(56.0);
        match selected {
            None => {
                ui.label(RichText::new(tl!("Choose a template to see where it comes from and its licence.")).color(t.text_muted));
            }
            Some(template) => {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&template.title).font(t.strong()));
                    let where_ = tl!("{publisher} · from the {source} · licence {licence}")
                        .replace("{publisher}", &template.publisher)
                        .replace("{source}", &template.source_text())
                        .replace("{licence}", &template.licence);
                    ui.label(RichText::new(where_).font(t.small()).color(t.text_muted));
                    let state = state(kept(template), app.session.library().is_some());
                    ui.label(RichText::new(state).font(t.small()).color(t.text_muted));
                });
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(job) = &app.gallery.job {
                ui.label(RichText::new(&job.label).color(t.text_muted));
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                widgets::spinner(ui, rect.center(), 6.0, t.accent);
                return;
            }
            let label = RichText::new(tl!("Create Project…")).color(t.on_accent).font(t.strong());
            let button = egui::Button::new(label).fill(t.accent).stroke(egui::Stroke::NONE).min_size(vec2(140.0, 32.0));
            if ui.add_enabled(selected.is_some(), button).clicked() {
                *create = selected.map(|t| t.id.clone());
            }
        });
    });
    if let Some(error) = &app.gallery.error {
        ui.colored_label(t.error, error.as_str());
    }
}

pub(crate) const CARD: egui::Vec2 = egui::vec2(160.0, 168.0);

/// A template's card; returns whether it was clicked.
/// Whether the template is ready, or where it comes from when the project is made.
fn state(kept: bool, in_browser: bool) -> &'static str {
    match (kept, in_browser) {
        (true, false) => tl!("Downloaded: available offline."),
        (true, true) => tl!("Fetched: ready to use."),
        (false, false) => tl!("Downloaded from TeX Live when you create the project."),
        (false, true) => tl!("Fetched from TeXCraft's site when you create the project."),
    }
}

pub(crate) fn card(ui: &mut egui::Ui, t: &Tokens, template: &Template, selected: bool, kept: bool, in_browser: bool) -> bool {
    let (rect, r) = ui.allocate_exact_size(CARD, Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &template.title));
    let base = if selected { blend(t.surface, t.accent_soft) } else { t.surface };
    let fill = if r.hovered() { blend(base, t.hover) } else { base };
    ui.painter().rect_filled(rect, t.corner_large(), fill);
    let stroke = if selected { egui::Stroke::new(2.0, t.accent) } else { t.hairline() };
    ui.painter().rect_stroke(rect, t.corner_large(), stroke, egui::StrokeKind::Inside);
    let top = egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(CARD.x - 24.0, 40.0));
    icons::paint(ui, egui::Rect::from_min_size(top.min, vec2(24.0, 24.0)), icon_for(&template.category), 20.0, t.accent);
    if kept {
        let badge = egui::Rect::from_min_size(egui::pos2(rect.right() - 30.0, rect.top() + 10.0), vec2(20.0, 20.0));
        icons::paint(ui, badge, "circle-check", 16.0, t.success);
    }
    let mut y = top.top() + 32.0;
    for (text, font, color, rows) in [(&template.publisher, t.strong(), t.text, 2), (&template.title, t.small(), t.text_muted, 3)] {
        let mut job = egui::text::LayoutJob::simple(text.clone(), font, color, CARD.x - 24.0);
        job.wrap.max_rows = rows;
        let galley = ui.painter().layout_job(job);
        let h = galley.size().y;
        ui.painter().galley(egui::pos2(rect.left() + 12.0, y), galley, color);
        y += h + 4.0;
    }
    ui.painter().text(egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0), egui::Align2::LEFT_BOTTOM, &template.licence, t.small(), t.text_muted);
    widgets::focus_ring(ui, t, &r, rect, t.corner_large());
    let state = state(kept, in_browser);
    let tip = tl!("{source} · licence {licence}").replace("{source}", &template.source_text()).replace("{licence}", &template.licence);
    let clicked = r.clicked();
    let _ = r.on_hover_text(format!("{}\n{tip}\n{state}", template.title));
    clicked
}

/// The blank project's card; returns whether it was clicked.
pub(crate) fn blank_card(ui: &mut egui::Ui, t: &Tokens) -> bool {
    let (rect, r) = ui.allocate_exact_size(CARD, Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Blank project")));
    let fill = if r.hovered() { blend(t.surface, t.hover) } else { t.surface };
    ui.painter().rect_filled(rect, t.corner_large(), fill);
    ui.painter().rect_stroke(rect, t.corner_large(), egui::Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
    icons::paint(ui, egui::Rect::from_center_size(rect.center() - vec2(0.0, 14.0), vec2(28.0, 28.0)), "file-plus", 26.0, t.text_muted);
    ui.painter().text(rect.center() + vec2(0.0, 20.0), egui::Align2::CENTER_CENTER, tl!("Blank project"), t.strong(), t.text);
    widgets::focus_ring(ui, t, &r, rect, t.corner_large());
    r.clicked()
}
