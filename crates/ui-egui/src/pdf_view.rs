//! The PDF preview: the pages of the last PDF a build wrote, stacked vertically, drawn by
//! texcraft-preview at the zoom and the screen's pixel density. Only the pages in view are drawn,
//! and each is drawn again only when the zoom changes or a new PDF arrives. The scroll position is
//! kept across builds.

use std::collections::HashMap;
use std::sync::Arc;

use texcraft_preview::Document;

use crate::theme::Tokens;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const PAGE_GAP: f32 = 12.0;

pub(crate) struct PdfView {
    source: Option<Arc<Vec<u8>>>,
    doc: Option<Document>,
    sizes: Vec<[f32; 2]>,
    error: Option<String>,
    /// Points to screen points; 1.0 shows a page at its printed size on a 96 dpi screen.
    zoom: f32,
    fit_width: bool,
    /// Drawn pages: the pixel scale each was drawn at, and its texture.
    textures: HashMap<usize, (f32, egui::TextureHandle)>,
    pub page: usize,
}

impl Default for PdfView {
    fn default() -> Self {
        Self { source: None, doc: None, sizes: Vec::new(), error: None, zoom: 1.0, fit_width: true, textures: HashMap::new(), page: 0 }
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
    }

    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.fit_width = false;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    pub fn fit_width(&mut self) {
        self.fit_width = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, tokens.canvas);
        if let Some(e) = &self.error {
            ui.colored_label(tokens.error, e.as_str());
            return;
        }
        if self.doc.is_none() {
            ui.centered_and_justified(|ui| ui.label(tl!("No PDF yet. Compile the project to see it here.")));
            return;
        }
        let widest = self.sizes.iter().map(|s| s[0]).fold(1.0_f32, f32::max);
        if self.fit_width {
            self.zoom = ((rect.width() - 2.0 * PAGE_GAP) / widest).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        let zoom = self.zoom;
        let pixels = ui.ctx().pixels_per_point() * zoom;
        let mut tops = Vec::with_capacity(self.sizes.len());
        let mut y = PAGE_GAP;
        for s in &self.sizes {
            tops.push(y);
            y += s[1] * zoom + PAGE_GAP;
        }
        let total = egui::vec2((widest * zoom + 2.0 * PAGE_GAP).max(rect.width()), y);
        egui::ScrollArea::both().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            let (area, _) = ui.allocate_exact_size(total, egui::Sense::hover());
            let mut first_visible = None;
            let sizes = self.sizes.clone();
            for (i, (size, top)) in sizes.iter().zip(&tops).enumerate() {
                let page = egui::Rect::from_min_size(
                    egui::pos2(area.left() + (total.x - size[0] * zoom) / 2.0, area.top() + top),
                    egui::vec2(size[0] * zoom, size[1] * zoom),
                );
                let local = page.translate(-area.min.to_vec2());
                if !viewport.intersects(local) {
                    continue;
                }
                first_visible.get_or_insert(i);
                ui.painter().rect_filled(page, 0.0, egui::Color32::WHITE);
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
