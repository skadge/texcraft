//! A project's image shown in the editor pane: the file's name, its size in pixels (or, for a PDF
//! figure, in points) and in bytes, and the image fitted to the pane on a checkerboard, so
//! transparent parts show. Raster and PDF figures are decoded by `texcraft_preview::figure`; SVG
//! is drawn by egui's SVG loader.

use egui::{Rect, RichText, Sense, vec2};
use texcraft_engine::project::{Contents, ProjectPath};
use texcraft_preview::figure::{self, Kind};

use crate::App;
use crate::theme::Tokens;

/// A decoded figure: its texture and its size (pixels, or points for a PDF), or why it failed.
type Decoded = Result<(egui::TextureHandle, [f32; 2]), String>;

/// The image shown, kept while the file is unchanged (its path and length).
#[derive(Default)]
pub(crate) struct FigureView {
    shown: Option<(ProjectPath, usize, Decoded)>,
    /// SVG files shown as their text rather than as images.
    pub as_text: std::collections::HashSet<ProjectPath>,
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

/// Whether the editor pane shows `path` as an image.
pub(crate) fn is_figure(path: &ProjectPath) -> bool {
    figure::kind(path.as_str()).is_some()
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    let t = app.tokens;
    let bytes = match app.session.project().get(path) {
        Some(Contents::Binary(b)) => b.clone(),
        Some(Contents::Text(text)) => text.clone().into_bytes(),
        None => return,
    };
    let kind = figure::kind(path.as_str());
    let svg = kind == Some(Kind::Svg);
    let stale = !app.figure.shown.as_ref().is_some_and(|(p, len, _)| p == path && *len == bytes.len());
    if stale && !svg {
        let decoded = figure::decode(path.as_str(), &bytes).map(|(image, size)| {
            let pixels = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.rgba);
            (ui.ctx().load_texture(format!("figure:{}", path.as_str()), pixels, egui::TextureOptions::LINEAR), size)
        });
        app.figure.shown = Some((path.clone(), bytes.len(), decoded.map_err(|e| e.to_string())));
    }
    let size_text = |[w, h]: [f32; 2], pdf: bool| {
        if pdf {
            tl!("{w} × {h} pt").replace("{w}", &format!("{w:.0}")).replace("{h}", &format!("{h:.0}"))
        } else {
            tl!("{w} × {h} pixels").replace("{w}", &format!("{w:.0}")).replace("{h}", &format!("{h:.0}"))
        }
    };
    let kb = format!("{:.0} kB", bytes.len() as f64 / 1024.0);
    // The SVG is laid out by egui; its size is known once loaded.
    let uri = format!("bytes://project/{}", path.as_str());
    let (texture, size, error) = if svg {
        (None, None, None)
    } else {
        match app.figure.shown.as_ref().map(|(_, _, r)| r) {
            Some(Ok((tex, size))) => (Some(tex.clone()), Some(*size), None),
            Some(Err(e)) => (None, None, Some(e.clone())),
            None => (None, None, None),
        }
    };
    ui.horizontal(|ui| {
        ui.set_min_height(t.toolbar_height);
        ui.spacing_mut().item_spacing.x = t.unit * 2.0;
        ui.add_space(t.unit * 3.0);
        ui.label(RichText::new(path.file_name()).font(t.strong()));
        let mut info = Vec::new();
        if let Some(s) = size {
            info.push(size_text(s, kind == Some(Kind::Pdf)));
        }
        info.push(kb);
        ui.label(RichText::new(info.join(" · ")).font(t.small()).color(t.text_muted));
        if svg {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(t.unit * 3.0);
                if ui.button(tl!("Edit as Text")).clicked() {
                    app.figure.as_text.insert(path.clone());
                }
            });
        }
    });
    crate::widgets::hairline(ui, &t);
    let area = ui.available_rect_before_wrap().shrink(t.unit * 4.0);
    if let Some(e) = error {
        ui.colored_label(t.error, tl!("This image cannot be shown: {e}").replace("{e}", &e));
        return;
    }
    let name = tl!("Image {name}").replace("{name}", path.file_name());
    let place = |natural: egui::Vec2| {
        let scale = (area.width() / natural.x).min(area.height() / natural.y).min(2.0);
        Rect::from_center_size(area.center(), natural * scale)
    };
    if let (Some(tex), Some(_)) = (texture, size) {
        let rect = place(tex.size_vec2());
        checkerboard(ui, &t, rect);
        let r = ui.allocate_rect(rect, Sense::hover());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &name));
        ui.painter().image(tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    } else if svg {
        let image = egui::Image::from_bytes(uri, bytes).max_size(area.size()).fit_to_exact_size(area.size()).maintain_aspect_ratio(true);
        let r = ui.put(area, image);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &name));
    }
}

/// A light checkerboard behind an image, so its transparent parts show.
fn checkerboard(ui: &egui::Ui, t: &Tokens, rect: Rect) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.surface);
    let cell = 10.0;
    let (cols, rows) = ((rect.width() / cell).ceil() as usize, (rect.height() / cell).ceil() as usize);
    for y in 0..rows {
        for x in (y % 2..cols).step_by(2) {
            let r = Rect::from_min_size(rect.min + vec2(x as f32 * cell, y as f32 * cell), vec2(cell, cell));
            painter.rect_filled(r, 0.0, t.hover);
        }
    }
}
