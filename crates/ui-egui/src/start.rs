//! The start page, shown when TeXCraft starts without a project and from View › Start Page:
//! creating or opening a project, and the recent projects as cards with the first page of their
//! last PDF. Thumbnails are made here after a successful build and stored by the shell (the
//! desktop app keeps them in its data folder), which hands them back at the next start.

use std::collections::HashMap;

use egui::{Align, Layout, RichText, Sense, vec2};

use crate::dialogs::Next;
use crate::theme::{Tokens, blend};
use crate::{App, icons, widgets};

/// Width of a thumbnail, in pixels.
pub const THUMBNAIL_WIDTH: u32 = 240;
const CARD: egui::Vec2 = egui::vec2(184.0, 290.0);

/// The first page of a project's last PDF, small.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    /// The project's folder, as kept in the recent projects.
    pub project: String,
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8.
    pub rgba: Vec<u8>,
}

/// Thumbnails known to the interface: drawn ones, and ones to hand to the shell.
#[derive(Default)]
pub(crate) struct Thumbnails {
    images: HashMap<String, egui::ColorImage>,
    textures: HashMap<String, egui::TextureHandle>,
    new: Vec<Thumbnail>,
}

impl App {
    /// A stored thumbnail for `project`, from the shell at start.
    pub fn set_thumbnail(&mut self, thumbnail: Thumbnail) {
        let size = [thumbnail.width as usize, thumbnail.height as usize];
        if size[0] * size[1] * 4 != thumbnail.rgba.len() {
            return;
        }
        self.thumbs.textures.remove(&thumbnail.project);
        self.thumbs.images.insert(thumbnail.project, egui::ColorImage::from_rgba_premultiplied(size, &thumbnail.rgba));
    }

    /// Thumbnails made since the last call, for the shell to store.
    pub fn take_thumbnails(&mut self) -> Vec<Thumbnail> {
        std::mem::take(&mut self.thumbs.new)
    }

    /// Makes the open project's thumbnail from the PDF a build just wrote.
    pub(crate) fn update_thumbnail(&mut self) {
        let (Some(dir), Some(pdf)) = (self.session.dir(), self.session.pdf()) else { return };
        let project = dir.display().to_string();
        let Ok(doc) = texcraft_preview::Document::open(pdf.clone()) else { return };
        let Some([w, _]) = doc.page_sizes().first().copied() else { return };
        let Ok(image) = doc.render(0, THUMBNAIL_WIDTH as f32 / w.max(1.0)) else { return };
        let thumbnail = Thumbnail { project, width: image.width, height: image.height, rgba: image.rgba };
        self.set_thumbnail(thumbnail.clone());
        self.thumbs.new.push(thumbnail);
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let width = (ui.available_width() - 96.0).clamp(320.0, 1040.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(width);
                ui.add_space(48.0);
                header(app, ui, &t);
                ui.add_space(36.0);
                recent(app, ui, &t, width);
                ui.add_space(48.0);
            });
        });
    });
}

fn header(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 64.0), Layout::left_to_right(Align::Center), |ui| {
        let (icon, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        crate::app_icon::paint(ui, icon);
        ui.add_space(t.unit * 3.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("TeXCraft").font(egui::FontId::new(28.0, crate::fonts::semibold())));
            ui.label(RichText::new(tl!("Write LaTeX, see the PDF as you type.")).color(t.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.unit * 2.0;
            let open = egui::Button::image_and_text(icons::image("folder-open", t.icon_size, t.text), tl!("Open Folder…")).min_size(vec2(0.0, 34.0));
            if ui.add(open).clicked() {
                crate::ui_cmd::run(app, "project.open");
            }
            let new = egui::Button::image_and_text(
                icons::image("file-plus", t.icon_size, t.on_accent),
                RichText::new(tl!("New Project")).color(t.on_accent).font(t.strong()),
            )
            .fill(t.accent)
            .stroke(egui::Stroke::NONE)
            .min_size(vec2(0.0, 34.0));
            if ui.add(new).clicked() {
                // A blank project or a template, from the gallery.
                app.open_gallery();
            }
        });
    });
}

fn recent(app: &mut App, ui: &mut egui::Ui, t: &Tokens, width: f32) {
    ui.horizontal(|ui| widgets::section_title(ui, t, tl!("Recent projects")));
    ui.add_space(t.unit * 3.0);
    if app.prefs.recent.is_empty() {
        ui.horizontal(|ui| ui.label(RichText::new(tl!("Projects you open appear here.")).color(t.text_muted)));
        return;
    }
    let columns = ((width + 16.0) / (CARD.x + 16.0)).floor().max(1.0) as usize;
    let recent = app.prefs.recent.clone();
    let mut open = None;
    let mut forget = None;
    for row in recent.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for path in row {
                match card(app, ui, t, path) {
                    Some(CardAction::Open) => open = Some(path.clone()),
                    Some(CardAction::Forget) => forget = Some(path.clone()),
                    None => {}
                }
            }
        });
        ui.add_space(16.0);
    }
    if let Some(path) = forget {
        app.prefs.recent.retain(|p| *p != path);
        app.prefs_changed = true;
    }
    if let Some(path) = open {
        app.guard(Next::OpenProject(path));
    }
}

enum CardAction {
    Open,
    Forget,
}

fn card(app: &mut App, ui: &mut egui::Ui, t: &Tokens, path: &str) -> Option<CardAction> {
    let p = std::path::Path::new(path);
    let name = p.file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
    let exists = cfg!(target_arch = "wasm32") || p.is_dir();
    let (rect, r) = ui.allocate_exact_size(CARD, if exists { Sense::click() } else { Sense::hover() });
    let label = if exists { name.clone() } else { format!("{name} ({})", tl!("folder not found")) };
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, exists, &label));
    let hovered = r.hovered() || r.has_focus();
    let fill = if hovered && exists { blend(t.surface, t.hover) } else { t.surface };
    ui.painter().rect_filled(rect, t.corner_large(), fill);
    ui.painter().rect_stroke(rect, t.corner_large(), t.hairline(), egui::StrokeKind::Inside);
    // The page: the thumbnail, or a blank sheet with an icon.
    let page = egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(CARD.x - 24.0, (CARD.x - 24.0) * 1.294));
    let texture = app.thumbs.textures.get(path).cloned().or_else(|| {
        let image = app.thumbs.images.get(path)?.clone();
        let handle = ui.ctx().load_texture(format!("thumbnail-{path}"), image, egui::TextureOptions::LINEAR);
        app.thumbs.textures.insert(path.to_owned(), handle.clone());
        Some(handle)
    });
    ui.painter().add(egui::Shadow { offset: [0, 1], blur: 6, spread: 0, color: t.shadow }.as_shape(page, 2));
    ui.painter().rect_filled(page, 2, egui::Color32::WHITE);
    match texture {
        Some(tex) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(tex.id(), page, uv, egui::Color32::WHITE);
        }
        None => icons::paint(ui, page, "file-text", 28.0, egui::Color32::from_gray(190)),
    }
    let text_top = page.bottom() + 10.0;
    let color = if exists { t.text } else { t.text_muted };
    let name_galley = ui.painter().layout(name, t.strong(), color, CARD.x - 24.0);
    ui.painter().galley(egui::pos2(rect.left() + 12.0, text_top), name_galley, color);
    let below = if exists { home_parent(p) } else { tl!("Folder not found").to_owned() };
    let below_color = if exists { t.text_muted } else { t.error };
    let mut job = egui::text::LayoutJob::simple_singleline(below, t.small(), below_color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(CARD.x - 24.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(rect.left() + 12.0, text_top + 20.0), galley, below_color);
    widgets::focus_ring(ui, t, &r, rect, t.corner_large());
    // Remove from the list: a small button on the card's corner, shown on hover.
    let close = egui::Rect::from_center_size(rect.right_top() + vec2(-16.0, 16.0), vec2(22.0, 22.0));
    let c = ui.interact(close, ui.scope_id().with(("forget", path)), Sense::click());
    c.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} {label}", tl!("Remove from recent projects:"))));
    if hovered || c.hovered() || !exists {
        ui.painter().circle_filled(close.center(), 11.0, if c.hovered() { blend(t.raised, t.pressed) } else { t.raised });
        ui.painter().circle_stroke(close.center(), 11.0, t.hairline());
        icons::paint(ui, close, "x", 13.0, t.text_muted);
    }
    let c = c.on_hover_text(tl!("Remove from recent projects"));
    let r = r.on_hover_text(path);
    if c.clicked() {
        return Some(CardAction::Forget);
    }
    if r.clicked() { Some(CardAction::Open) } else { None }
}

/// The folder holding `path`, with the home folder written as `~`.
fn home_parent(path: &std::path::Path) -> String {
    let parent = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
    match std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        Some(home) if parent == home => "~".to_owned(),
        Some(home) if parent.starts_with(&format!("{home}/")) => format!("~{}", &parent[home.len()..]),
        _ => parent,
    }
}
