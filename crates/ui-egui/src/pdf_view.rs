//! The PDF preview: the pages of the last PDF a build wrote, stacked on the canvas with a soft
//! shadow, drawn by texcraft-preview at the zoom and the screen's pixel density. Only the pages in
//! view are drawn, and each is drawn again only when the zoom changes or a new PDF arrives. The
//! scroll position is kept across builds.

use std::collections::HashMap;
use std::sync::Arc;

use texcraft_preview::Document;

use crate::theme::Tokens;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const PAGE_GAP: f32 = 16.0;

/// How the zoom is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fit {
    Width,
    Page,
    /// A fixed zoom (1.0 is a page at its printed size).
    Zoom(f32),
}

pub(crate) struct PdfView {
    source: Option<Arc<Vec<u8>>>,
    doc: Option<Document>,
    sizes: Vec<[f32; 2]>,
    error: Option<String>,
    fit: Fit,
    /// The zoom used for the last frame.
    zoom: f32,
    /// Drawn pages: the pixel scale each was drawn at, and its texture.
    textures: HashMap<usize, (f32, egui::TextureHandle)>,
    /// The first page in view, from 0.
    pub page: usize,
    /// Scroll so that this page is at the top, next frame.
    scroll_to: Option<usize>,
}

impl Default for PdfView {
    fn default() -> Self {
        Self {
            source: None,
            doc: None,
            sizes: Vec::new(),
            error: None,
            fit: Fit::Width,
            zoom: 1.0,
            textures: HashMap::new(),
            page: 0,
            scroll_to: None,
        }
    }
}

impl PdfView {
    /// Opens `pdf` if it is not the one shown.
    pub fn sync(&mut self, pdf: Option<&Arc<Vec<u8>>>) {
        let same = match (pdf, &self.source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.source = pdf.cloned();
        self.textures.clear();
        self.error = None;
        self.doc = None;
        self.sizes.clear();
        if let Some(bytes) = pdf {
            match Document::open(bytes.clone()) {
                Ok(doc) => {
                    self.sizes = doc.page_sizes();
                    self.doc = Some(doc);
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        self.page = self.page.min(self.sizes.len().saturating_sub(1));
    }

    pub fn has_pdf(&self) -> bool {
        self.doc.is_some()
    }

    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn fit(&self) -> Fit {
        self.fit
    }

    pub fn set_fit(&mut self, fit: Fit) {
        self.fit = match fit {
            Fit::Zoom(z) => Fit::Zoom(z.clamp(MIN_ZOOM, MAX_ZOOM)),
            other => other,
        };
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.set_fit(Fit::Zoom(self.zoom * factor));
    }

    pub fn fit_width(&mut self) {
        self.fit = Fit::Width;
    }

    pub fn go_to_page(&mut self, page: usize) {
        if !self.sizes.is_empty() {
            self.scroll_to = Some(page.min(self.sizes.len() - 1));
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, t.canvas);
        if let Some(e) = &self.error {
            ui.colored_label(t.error, e.as_str());
            return;
        }
        if self.doc.is_none() {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(rect.height() * 0.38);
                ui.add(crate::icons::image("file-text", 32.0, t.text_muted.gamma_multiply(0.6)));
                ui.add_space(t.unit * 3.0);
                ui.label(egui::RichText::new(tl!("No PDF yet")).font(t.strong()));
                ui.add_space(t.unit);
                ui.label(egui::RichText::new(tl!("Compile the project to see it here.")).color(t.text_muted));
            });
            return;
        }
        let widest = self.sizes.iter().map(|s| s[0]).fold(1.0_f32, f32::max);
        let tallest = self.sizes.iter().map(|s| s[1]).fold(1.0_f32, f32::max);
        self.zoom = match self.fit {
            Fit::Width => (rect.width() - 2.0 * PAGE_GAP) / widest,
            Fit::Page => ((rect.height() - 2.0 * PAGE_GAP) / tallest).min((rect.width() - 2.0 * PAGE_GAP) / widest),
            Fit::Zoom(z) => z,
        }
        .clamp(MIN_ZOOM, MAX_ZOOM);
        let zoom = self.zoom;
        let pixels = ui.ctx().pixels_per_point() * zoom;
        let mut tops = Vec::with_capacity(self.sizes.len());
        let mut y = PAGE_GAP;
        for s in &self.sizes {
            tops.push(y);
            y += s[1] * zoom + PAGE_GAP;
        }
        let total = egui::vec2((widest * zoom + 2.0 * PAGE_GAP).max(rect.width()), y);
        let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
        if let Some(page) = self.scroll_to.take()
            && let Some(top) = tops.get(page)
        {
            area = area.vertical_scroll_offset(top - PAGE_GAP);
        }
        let shadow = egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: t.shadow };
        let sizes = self.sizes.clone();
        area.show_viewport(ui, |ui, viewport| {
            let (outer, _) = ui.allocate_exact_size(total, egui::Sense::hover());
            let mut first_visible = None;
            for (i, (size, top)) in sizes.iter().zip(&tops).enumerate() {
                let page = egui::Rect::from_min_size(
                    egui::pos2(outer.left() + (total.x - size[0] * zoom) / 2.0, outer.top() + top),
                    egui::vec2(size[0] * zoom, size[1] * zoom),
                );
                if !viewport.intersects(page.translate(-outer.min.to_vec2())) {
                    continue;
                }
                if first_visible.is_none() && page.center().y - outer.top() > viewport.top() {
                    first_visible = Some(i);
                }
                ui.painter().add(shadow.as_shape(page, 2));
                ui.painter().rect_filled(page, 2, egui::Color32::WHITE);
                if let Some(texture) = self.texture(ui.ctx(), i, pixels) {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(texture.id(), page, uv, egui::Color32::WHITE);
                }
            }
            if let Some(i) = first_visible {
                self.page = i;
            }
        });
    }

    fn texture(&mut self, ctx: &egui::Context, index: usize, scale: f32) -> Option<egui::TextureHandle> {
        if let Some((drawn, handle)) = self.textures.get(&index)
            && (drawn - scale).abs() < 0.01
        {
            return Some(handle.clone());
        }
        let doc = self.doc.as_ref()?;
        match doc.render(index, scale) {
            Ok(image) => {
                let color = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.rgba);
                let handle = ctx.load_texture(format!("pdf-page-{index}"), color, egui::TextureOptions::LINEAR);
                self.textures.insert(index, (scale, handle.clone()));
                Some(handle)
            }
            Err(e) => {
                log::warn!("page {}: {e}", index + 1);
                None
            }
        }
    }
}
