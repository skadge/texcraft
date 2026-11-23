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

/// A running build, as the PDF pane shows it.
pub(crate) struct Building {
    /// What it does now ("Compiling page 2 of 26").
    pub now: String,
    /// The pass and why, and the time so far, dimmed under it.
    pub detail: String,
    /// How far through its pass, when the last pass's page count says.
    pub ratio: Option<f32>,
    /// The session's first build of the project: it says why it takes longer.
    pub first: bool,
    /// The pass whose draft PDF is shown, while the later passes run.
    pub draft: Option<u32>,
}

/// How the zoom is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fit {
    Width,
    Page,
    /// A fixed zoom (1.0 is a page at its printed size).
    Zoom(f32),
}

/// A page to draw: the document's generation, the page, the scale, the document.
struct Request {
    generation: u64,
    index: usize,
    scale: f32,
    doc: Arc<Document>,
}

/// A page drawn, or why not.
struct Drawn {
    generation: u64,
    index: usize,
    scale: f32,
    image: Result<texcraft_preview::Image, String>,
}

/// Draws pages on a thread of its own, so a slow page (a full-page photograph) never holds the
/// window up. The newest request for a page wins over older ones still queued. The browser build
/// has no threads: there the queue is drawn on the window's thread, when the view takes the pages.
struct Renderer {
    tx: std::sync::mpsc::Sender<Request>,
    rx: std::sync::mpsc::Receiver<Drawn>,
    #[cfg(target_arch = "wasm32")]
    queue: (std::sync::mpsc::Receiver<Request>, std::sync::mpsc::Sender<Drawn>),
}

/// Draws the newest request of each page of `queue`, in the order asked; stops when `done` refuses.
fn draw_newest(queue: Vec<Request>, mut done: impl FnMut(Drawn) -> bool) {
    let mut seen = std::collections::HashSet::new();
    let newest: Vec<Request> = queue.into_iter().rev().filter(|r| seen.insert(r.index)).collect();
    for r in newest.into_iter().rev() {
        let image = r.doc.render(r.index, r.scale).map_err(|e| e.to_string());
        if !done(Drawn { generation: r.generation, index: r.index, scale: r.scale, image }) {
            return;
        }
    }
}

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    fn start(ctx: &egui::Context) -> Option<Self> {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        let (done, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("texcraft-pdf-pages".to_owned())
            .spawn(move || {
                while let Ok(first) = requests.recv() {
                    let mut queue = vec![first];
                    queue.extend(requests.try_iter());
                    let mut open = true;
                    draw_newest(queue, |d| {
                        open = done.send(d).is_ok();
                        ctx.request_repaint();
                        open
                    });
                    if !open {
                        return;
                    }
                }
            })
            .ok()?;
        Some(Self { tx, rx })
    }

    #[cfg(target_arch = "wasm32")]
    fn start(_: &egui::Context) -> Option<Self> {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        let (done, rx) = std::sync::mpsc::channel();
        Some(Self { tx, rx, queue: (requests, done) })
    }

    /// The pages drawn since last asked.
    fn drawn(&self) -> Vec<Drawn> {
        #[cfg(target_arch = "wasm32")]
        draw_newest(self.queue.0.try_iter().collect(), |d| self.queue.1.send(d).is_ok());
        self.rx.try_iter().collect()
    }
}

pub(crate) struct PdfView {
    source: Option<Arc<Vec<u8>>>,
    doc: Option<Arc<Document>>,
    /// Goes up with each new PDF: a drawing of an older one is shown until its page is drawn again.
    generation: u64,
    renderer: Option<Renderer>,
    /// Pages asked of the renderer, at their scale, for this generation.
    pending: HashMap<usize, f32>,
    sizes: Vec<[f32; 2]>,
    error: Option<String>,
    fit: Fit,
    /// The zoom used for the last frame.
    zoom: f32,
    /// Drawn pages: the pixel scale and generation each was drawn at, and its texture.
    textures: HashMap<usize, (f32, u64, egui::TextureHandle)>,
    /// The first page in view, from 0.
    pub page: usize,
    /// Scroll so that this page is at the top, next frame.
    scroll_to: Option<usize>,
    /// A page double-clicked or Ctrl+clicked: its index and the point, in PDF points from its
    /// top left corner. The preview pane takes it to the source.
    pub clicked: Option<(usize, f32, f32)>,
    /// Places marked on the pages (the caret's line, shown in the PDF) and when, to fade them.
    mark: Option<(Vec<texcraft_engine::PageRect>, f64)>,
    /// Scroll to this page and height (PDF points) next frame.
    reveal: Option<(usize, f32)>,
}

impl Default for PdfView {
    fn default() -> Self {
        Self {
            source: None,
            doc: None,
            generation: 0,
            renderer: None,
            pending: HashMap::new(),
            sizes: Vec::new(),
            error: None,
            fit: Fit::Width,
            zoom: 1.0,
            textures: HashMap::new(),
            page: 0,
            scroll_to: None,
            clicked: None,
            mark: None,
            reveal: None,
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
        // The pages drawn stay on screen until the new PDF's are drawn.
        self.generation += 1;
        self.pending.clear();
        self.error = None;
        self.doc = None;
        self.sizes.clear();
        if let Some(bytes) = pdf {
            match Document::open(bytes.clone()) {
                Ok(doc) => {
                    self.sizes = doc.page_sizes();
                    self.textures.retain(|i, _| *i < self.sizes.len());
                    self.doc = Some(Arc::new(doc));
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

    /// Marks `rects` (a source line's place) for a moment and scrolls to the first.
    pub fn show_place(&mut self, rects: Vec<texcraft_engine::PageRect>, now: f64) {
        if let Some(first) = rects.first() {
            self.reveal = Some((first.page as usize, first.y));
            self.mark = Some((rects, now));
        }
    }

    /// `building`: the running build's progress, when one runs.
    pub fn show(&mut self, ui: &mut egui::Ui, t: &Tokens, building: Option<&Building>) {
        self.receive(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, t.canvas);
        // The first build: no PDF to show yet, so its progress takes the place.
        if let (None, Some(b)) = (&self.doc, building) {
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.set_max_width((rect.width() - 48.0).clamp(200.0, 460.0));
                ui.add_space(rect.height() * 0.34);
                let (spin, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                crate::widgets::spinner(ui, spin.center(), 11.0, t.accent);
                ui.add_space(t.unit * 3.0);
                ui.label(egui::RichText::new(tl!("Building the PDF")).font(t.strong()));
                ui.add_space(t.unit);
                // What it does now; under it, dimmed, the pass and why, and the time.
                ui.add(egui::Label::new(egui::RichText::new(&b.now).color(t.text)).truncate());
                if !b.detail.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&b.detail).font(t.small()).color(t.text_muted)).truncate());
                }
                if let Some(r) = b.ratio {
                    ui.add_space(t.unit * 3.0);
                    let (bar, _) = ui.allocate_exact_size(egui::vec2(220.0, 4.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 2.0, t.separator);
                    ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * r, bar.height())), 2.0, t.accent);
                }
                if b.first {
                    ui.add_space(t.unit * 4.0);
                    let note = tl!(
                        "The first build reads the packages and fonts the document uses, so it takes longer. The next ones only redo what changed."
                    );
                    ui.add(egui::Label::new(egui::RichText::new(note).font(t.small()).color(t.text_muted)).wrap().halign(egui::Align::Center));
                }
            });
            return;
        }
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
        // A place shown from the source: a third of the way down the pane.
        if let Some((page, y)) = self.reveal.take()
            && let Some(top) = tops.get(page)
        {
            area = area.vertical_scroll_offset((top + y * zoom - rect.height() / 3.0).max(0.0));
        }
        let now = ui.input(|i| i.time);
        const MARK_SECONDS: f64 = 1.8;
        if self.mark.as_ref().is_some_and(|(_, since)| now - since > MARK_SECONDS) {
            self.mark = None;
        }
        let mark = self.mark.clone();
        // Ctrl held: the modifiers' state, or the click's own (as the click event carries them).
        let command = ui.input(|i| {
            i.modifiers.command || i.events.iter().any(|e| matches!(e, egui::Event::PointerButton { modifiers, .. } if modifiers.command))
        });
        let mut clicked = None;
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
                // Double-click or Ctrl+click: the source of that point.
                let r = ui.interact(page, ui.scope_id().with(("pdf-page", i)), egui::Sense::click());
                if (r.double_clicked() || (r.clicked() && command))
                    && let Some(p) = r.interact_pointer_pos()
                {
                    clicked = Some((i, (p.x - page.left()) / zoom, (p.y - page.top()) / zoom));
                }
                // The place shown from the source, fading.
                if let Some((rects, since)) = &mark {
                    let fade = (1.0 - ((now - since) / MARK_SECONDS) as f32).clamp(0.0, 1.0);
                    for m in rects.iter().filter(|m| m.page as usize == i) {
                        let r = egui::Rect::from_min_size(page.min + egui::vec2(m.x * zoom, m.y * zoom), egui::vec2(m.width * zoom, m.height * zoom))
                            .expand(2.0);
                        ui.painter().rect_filled(r, 2.0, t.search_match.gamma_multiply(fade));
                        ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.5, t.accent.gamma_multiply(fade)), egui::StrokeKind::Outside);
                    }
                    ui.ctx().request_repaint();
                }
            }
            if let Some(i) = first_visible {
                self.page = i;
            }
        });
        if clicked.is_some() {
            self.clicked = clicked;
        }
        if let Some(pass) = building.and_then(|b| b.draft) {
            draft_badge(ui, t, rect, pass);
        }
    }

    fn texture(&mut self, ctx: &egui::Context, index: usize, scale: f32) -> Option<egui::TextureHandle> {
        let shown = self.textures.get(&index).map(|(drawn, generation, handle)| (*drawn, *generation, handle.clone()));
        let fresh = shown.as_ref().is_some_and(|(drawn, generation, _)| *generation == self.generation && (drawn - scale).abs() < 0.01);
        if fresh {
            return shown.map(|(_, _, h)| h);
        }
        let doc = self.doc.clone()?;
        if self.renderer.is_none() {
            self.renderer = Renderer::start(ctx);
        }
        match &self.renderer {
            Some(renderer) => {
                if self.pending.get(&index).is_none_or(|s| (s - scale).abs() >= 0.01) {
                    let request = Request { generation: self.generation, index, scale, doc };
                    if renderer.tx.send(request).is_ok() {
                        self.pending.insert(index, scale);
                        // The browser draws the queue next frame.
                        #[cfg(target_arch = "wasm32")]
                        ctx.request_repaint();
                    }
                }
                // Until then, the page as it was drawn before (another scale, the last PDF).
                shown.map(|(_, _, h)| h)
            }
            None => {
                let image = doc.render(index, scale).map_err(|e| e.to_string());
                self.take(ctx, Drawn { generation: self.generation, index, scale, image });
                self.textures.get(&index).map(|(_, _, h)| h.clone())
            }
        }
    }

    /// Takes the pages the renderer drew.
    fn receive(&mut self, ctx: &egui::Context) {
        let drawn: Vec<Drawn> = self.renderer.as_ref().map(Renderer::drawn).unwrap_or_default();
        for d in drawn {
            self.take(ctx, d);
        }
    }

    fn take(&mut self, ctx: &egui::Context, d: Drawn) {
        if d.generation != self.generation {
            return;
        }
        if self.pending.get(&d.index).is_some_and(|s| (s - d.scale).abs() < 0.01) {
            self.pending.remove(&d.index);
        }
        match d.image {
            Ok(image) => {
                let color = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.rgba);
                let handle = ctx.load_texture(format!("pdf-page-{}", d.index), color, egui::TextureOptions::LINEAR);
                self.textures.insert(d.index, (d.scale, d.generation, handle));
            }
            Err(e) => log::warn!("page {}: {e}", d.index + 1),
        }
    }
}

/// Marks the PDF shown as the running build's draft, in the pane's top right corner: the PDF of an
/// earlier pass, whose references may not be resolved until the build ends.
fn draft_badge(ui: &mut egui::Ui, t: &Tokens, pane: egui::Rect, pass: u32) {
    let text = tl!("Draft · pass {n}").replace("{n}", &pass.to_string());
    let strip = egui::Rect::from_min_max(pane.min, egui::pos2(pane.right(), pane.top() + t.unit * 12.0)).shrink(t.unit * 3.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::right_to_left(egui::Align::Min)));
    egui::Frame::new()
        .fill(t.raised)
        .stroke(egui::Stroke::new(1.0, t.control_border))
        .corner_radius(t.corner())
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(&mut child, |ui| {
            let label = ui.add(egui::Label::new(egui::RichText::new(&text).font(t.small()).color(t.text)).sense(egui::Sense::hover()));
            crate::status::announce(ui, &label, &text);
            label.on_hover_text(tl!("The PDF of an earlier pass, shown while the build goes on: its references may not be resolved yet."));
        });
}
