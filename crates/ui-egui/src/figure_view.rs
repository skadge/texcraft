//! A project's image shown in the editor pane: the file's name and size, zoom controls, and for
//! a PDF figure of several pages, its pages; then the image on a checkerboard (so transparent
//! parts show), fitted to the pane or at the zoom chosen.
//!
//! - **Zoom:** the buttons, or the mouse wheel over the image (Ctrl+wheel and pinching too); a
//!   zoomed image is moved by dragging it or with the scroll bars. Fit shows it whole again.
//! - **Sharpness:** PDF and SVG figures are drawn again at the zoom, raster images shown as
//!   decoded.
//! - **SVG** is drawn with the images it places, looked up among the project's files relative to
//!   its folder, and the system's fonts.
//!
//! Decoding is `texcraft_preview::figure`'s.

use egui::{Rect, RichText, Sense, vec2};
use texcraft_engine::project::{Contents, Project, ProjectPath};
use texcraft_preview::figure::{self, Kind};

use crate::App;
use crate::theme::Tokens;

/// The zoom's bounds, in screen points per unit of the figure's natural size.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 16.0;

/// A drawn figure: its texture, natural size, page count and the scale it was drawn at.
struct Drawn {
    texture: egui::TextureHandle,
    size: [f32; 2],
    pages: usize,
    scale: f32,
}

/// What was drawn, for which file (path, length) and page.
struct Shown {
    path: ProjectPath,
    len: usize,
    page: usize,
    drawn: Result<Drawn, String>,
}

#[derive(Default)]
pub(crate) struct FigureView {
    shown: Option<Shown>,
    /// SVG files shown as their text rather than as images.
    pub as_text: std::collections::HashSet<ProjectPath>,
    /// The file the zoom and page are for.
    path: Option<ProjectPath>,
    /// Screen points per unit of the figure's natural size; none to fit the pane.
    zoom: Option<f32>,
    page: usize,
}

/// Whether the editor pane shows `path` as an image.
pub(crate) fn is_figure(path: &ProjectPath) -> bool {
    figure::kind(path.as_str()).is_some()
}

/// Whether `path` is an SVG file, which can be shown either way.
pub(crate) fn is_svg(path: &ProjectPath) -> bool {
    figure::kind(path.as_str()) == Some(Kind::Svg)
}

/// Above an SVG file's text: a bar to show it as an image again.
pub(crate) fn svg_bar(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    let t = app.tokens;
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 3.0);
        ui.label(RichText::new(tl!("The text of an SVG image.")).font(t.small()).color(t.text_muted));
        ui.add_space(t.unit * 2.0);
        if ui.button(tl!("Show Image")).clicked() {
            app.figure.as_text.remove(path);
        }
    });
    crate::widgets::hairline(ui, &t);
}

/// A file an SVG names (`photos/a.jpg`, `../logo.png`), relative to the SVG's folder.
fn resolve(project: &Project, svg: &ProjectPath, href: &str) -> Option<Vec<u8>> {
    let href = href.strip_prefix("file://").unwrap_or(href);
    let mut parts: Vec<&str> = svg.as_str().split('/').collect();
    parts.pop();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    let path = ProjectPath::new(&parts.join("/")).ok()?;
    match project.get(&path)? {
        Contents::Binary(b) => Some(b.clone()),
        Contents::Text(t) => Some(t.clone().into_bytes()),
    }
}

fn bytes_of(project: &Project, path: &ProjectPath) -> Option<Vec<u8>> {
    match project.get(path)? {
        Contents::Binary(b) => Some(b.clone()),
        Contents::Text(t) => Some(t.clone().into_bytes()),
    }
}

impl FigureView {
    /// Draws the figure when the file, its page, or (for a vector figure) the scale needed
    /// changed.
    fn draw(&mut self, ctx: &egui::Context, project: &Project, path: &ProjectPath, bytes: &[u8], scale: f32) {
        let vector = matches!(figure::kind(path.as_str()), Some(Kind::Pdf | Kind::Svg));
        let current = self.shown.as_ref().filter(|s| &s.path == path && s.len == bytes.len() && s.page == self.page);
        let fresh = match current.map(|s| &s.drawn) {
            None => false,
            Some(Err(_)) => true,
            // Drawn again when the zoom asks for much more (or much less) detail.
            Some(Ok(d)) => !vector || (d.scale >= scale * 0.9 && d.scale <= scale * 2.5),
        };
        if fresh {
            return;
        }
        // A little more than asked, so small zoom steps do not draw again.
        let scale = if vector { scale * 1.25 } else { 1.0 };
        let lookup = |href: &str| resolve(project, path, href);
        // No side longer than the graphics card takes.
        let max_side = ctx.input(|i| i.max_texture_side).min(u32::MAX as usize) as u32;
        let drawn = figure::decode(path.as_str(), bytes, self.page, scale, max_side, &lookup)
            .map(|f| {
                let pixels = egui::ColorImage::from_rgba_premultiplied([f.image.width as usize, f.image.height as usize], &f.image.rgba);
                let texture = ctx.load_texture(format!("figure:{}", path.as_str()), pixels, egui::TextureOptions::LINEAR);
                let drawn_scale = f.image.width as f32 / f.size[0].max(1.0);
                Drawn { texture, size: f.size, pages: f.pages, scale: drawn_scale }
            })
            .map_err(|e| e.to_string());
        self.shown = Some(Shown { path: path.clone(), len: bytes.len(), page: self.page, drawn });
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    let t = app.tokens;
    let Some(bytes) = bytes_of(app.session.project(), path) else { return };
    let kind = figure::kind(path.as_str());
    let view = &mut app.figure;
    if view.path.as_ref() != Some(path) {
        view.path = Some(path.clone());
        view.zoom = None;
        view.page = 0;
    }
    // A figure not drawn yet is drawn once to learn its natural size; the view's scale follows.
    let ppp = ui.ctx().pixels_per_point();
    let drawn_here = view.shown.as_ref().is_some_and(|sh| &sh.path == path && sh.len == bytes.len() && sh.page == view.page);
    if !drawn_here {
        view.draw(ui.ctx(), app.session.project(), path, &bytes, view.zoom.unwrap_or(1.0) * ppp);
    }
    let (size, pages) = match view.shown.as_ref().map(|s| &s.drawn) {
        Some(Ok(d)) => (Some(d.size), d.pages),
        _ => (None, 1),
    };
    let mut set_zoom = None;
    let mut fit = false;
    let mut page = view.page;
    // The header: on the right the controls (pages, zoom, Fit, for SVG its text); on the left, in
    // what they leave, the name and sizes.
    let mut info = Vec::new();
    if let Some([w, h]) = size {
        info.push(match kind {
            Some(Kind::Pdf) => tl!("{w} × {h} pt").replace("{w}", &format!("{w:.0}")).replace("{h}", &format!("{h:.0}")),
            _ => tl!("{w} × {h} pixels").replace("{w}", &format!("{w:.0}")).replace("{h}", &format!("{h:.0}")),
        });
    }
    info.push(crate::i18n::file_size(bytes.len() as u64));
    let zoom_shown = app.figure.zoom;
    ui.horizontal(|ui| {
        ui.set_min_height(t.toolbar_height);
        ui.spacing_mut().item_spacing.x = t.unit * 1.5;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(t.unit * 3.0);
            if kind == Some(Kind::Svg) && ui.button(tl!("Edit as Text")).clicked() {
                app.figure.as_text.insert(path.clone());
            }
            if ui.button(tl!("Fit")).on_hover_text(tl!("Show the whole image")).clicked() {
                fit = true;
            }
            if crate::widgets::icon_button(ui, &t, "zoom-in", tl!("Zoom In"), true).clicked() {
                set_zoom = Some(1.25);
            }
            let percent = zoom_shown.map_or_else(|| tl!("Fit").to_owned(), |z| format!("{:.0} %", z * 100.0));
            ui.label(RichText::new(percent).font(t.small()).color(t.text_muted));
            if crate::widgets::icon_button(ui, &t, "zoom-out", tl!("Zoom Out"), true).clicked() {
                set_zoom = Some(0.8);
            }
            if pages > 1 {
                ui.add_space(t.unit * 2.0);
                if crate::widgets::icon_button(ui, &t, "chevron-right", tl!("Next Page"), page + 1 < pages).clicked() {
                    page += 1;
                }
                let text = tl!("Page {n} of {total}").replace("{n}", &(page + 1).to_string()).replace("{total}", &pages.to_string());
                ui.label(RichText::new(text).font(t.small()).color(t.text_muted));
                if crate::widgets::icon_button(ui, &t, "chevron-left", tl!("Previous Page"), page > 0).clicked() {
                    page -= 1;
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(t.unit * 3.0);
                ui.add(egui::Label::new(RichText::new(path.file_name()).font(t.strong())).truncate());
                ui.add(egui::Label::new(RichText::new(info.join(" · ")).font(t.small()).color(t.text_muted)).truncate());
            });
        });
    });
    crate::widgets::hairline(ui, &t);
    let view = &mut app.figure;
    view.page = page.min(pages.saturating_sub(1));
    let area = ui.available_rect_before_wrap();
    let Some(shown) = view.shown.as_ref() else { return };
    let drawn = match &shown.drawn {
        Ok(d) => d,
        Err(e) => {
            ui.add_space(t.unit * 4.0);
            ui.horizontal(|ui| {
                ui.add_space(t.unit * 3.0);
                ui.colored_label(t.error, tl!("This image cannot be shown: {e}").replace("{e}", &crate::i18n::msg(e)));
            });
            return;
        }
    };
    let natural = vec2(drawn.size[0].max(1.0), drawn.size[1].max(1.0));
    let fitted = ((area.width() - 32.0) / natural.x).min((area.height() - 32.0) / natural.y).clamp(MIN_ZOOM, 2.0);
    let current = view.zoom.unwrap_or(fitted);
    if fit {
        view.zoom = None;
    } else if let Some(f) = set_zoom {
        view.zoom = Some((current * f).clamp(MIN_ZOOM, MAX_ZOOM));
    }
    // The mouse wheel (and Ctrl+wheel, pinching) over the image zooms.
    if ui.rect_contains_pointer(area) {
        let (wheel, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (wheel * 0.0015).exp() * pinch;
        if (factor - 1.0).abs() > 1e-4 {
            view.zoom = Some((view.zoom.unwrap_or(fitted) * factor).clamp(MIN_ZOOM, MAX_ZOOM));
        }
    }
    let zoom = view.zoom.unwrap_or(fitted);
    let shown_size = natural * zoom;
    let texture = drawn.texture.id();
    let name = tl!("Image {name}").replace("{name}", path.file_name());
    let source = egui::scroll_area::ScrollSource { scroll_bar: true, drag: egui::scroll_area::DragScroll::Always, mouse_wheel: false };
    egui::ScrollArea::both().id_salt(("figure", path.as_str())).scroll_source(source).auto_shrink([false, false]).show(ui, |ui| {
        let room = vec2(shown_size.x.max(area.width()), shown_size.y.max(area.height()));
        let (frame, _) = ui.allocate_exact_size(room, Sense::hover());
        let rect = Rect::from_center_size(frame.center(), shown_size);
        checkerboard(ui, &t, rect);
        let r = ui.allocate_rect(rect, Sense::hover());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &name));
        ui.painter().image(texture, rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    });
    // A vector figure is drawn again at the zoom (or the fitted size) when it needs more detail.
    view.draw(ui.ctx(), app.session.project(), path, &bytes, zoom * ppp);
}

/// A light checkerboard behind an image, so its transparent parts show.
fn checkerboard(ui: &egui::Ui, t: &Tokens, rect: Rect) {
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, t.surface);
    let cell = 10.0;
    let visible = rect.intersect(ui.clip_rect());
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return;
    }
    let (x0, y0) = (((visible.left() - rect.left()) / cell).floor() as usize, ((visible.top() - rect.top()) / cell).floor() as usize);
    let (x1, y1) = (((visible.right() - rect.left()) / cell).ceil() as usize, ((visible.bottom() - rect.top()) / cell).ceil() as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            if (x + y) % 2 == 0 {
                let r = Rect::from_min_size(rect.min + vec2(x as f32 * cell, y as f32 * cell), vec2(cell, cell));
                painter.rect_filled(r, 0.0, t.hover);
            }
        }
    }
}
