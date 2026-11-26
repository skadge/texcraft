//! The PDF preview: the pages of the last PDF a build wrote, stacked on the canvas with a soft
//! shadow, drawn by texcraft-preview at the zoom and the screen's pixel density. Pages are drawn
//! off the window's thread (a page or so a frame in the browser), those in view first, then those
//! around them; until a page is drawn it shows as drawn before, or blank at its size. Each is drawn
//! again only when the zoom changes or a new PDF arrives. The scroll position is kept across builds.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use texcraft_preview::{Document, Glyph, Link, Target};

use crate::theme::Tokens;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const PAGE_GAP: f32 = 16.0;
/// Miniatures kept on either side of those in view; the others are let go.
const THUMBNAILS_KEPT: usize = 24;

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

/// A page drawn, or why not; `None` when it was no longer wanted (scrolled away, another zoom,
/// a newer PDF) and was not drawn.
struct Drawn {
    generation: u64,
    index: usize,
    scale: f32,
    image: Option<Result<texcraft_preview::Image, String>>,
}

/// What the view shows now. The renderer draws only these pages, at this scale, of this PDF, in
/// this order (the pages in view first, then those around them); other requests are dropped.
#[derive(Default)]
struct Wanted {
    generation: u64,
    scale: f32,
    pages: Vec<usize>,
}

/// What a request is for: the PDF's generation, the page, the scale.
trait Asked {
    fn asked(&self) -> (u64, usize, f32);
}

impl Asked for Request {
    fn asked(&self) -> (u64, usize, f32) {
        (self.generation, self.index, self.scale)
    }
}

impl Wanted {
    fn rank(&self, r: &impl Asked) -> Option<usize> {
        let (generation, index, scale) = r.asked();
        if generation != self.generation || (scale - self.scale).abs() >= 0.01 {
            return None;
        }
        self.pages.iter().position(|p| *p == index)
    }
}

/// Takes from `queue` the request to draw next: the newest of each page, the first of those
/// `wanted` lists. The requests no longer wanted go to `dropped`.
fn next_request<R: Asked>(queue: &mut Vec<R>, wanted: &Mutex<Wanted>, mut dropped: impl FnMut(R)) -> Option<R> {
    let Ok(wanted) = wanted.lock() else {
        return queue.pop();
    };
    let mut seen = std::collections::HashSet::new();
    let mut keep = Vec::with_capacity(queue.len());
    for r in std::mem::take(queue).into_iter().rev() {
        if seen.insert(r.asked().1) && wanted.rank(&r).is_some() {
            keep.push(r);
        } else {
            dropped(r);
        }
    }
    keep.sort_by_key(|r| std::cmp::Reverse(wanted.rank(r)));
    let next = keep.pop();
    *queue = keep;
    next
}

fn draw(r: &Request) -> Drawn {
    let image = r.doc.render(r.index, r.scale).map_err(|e| e.to_string());
    Drawn { generation: r.generation, index: r.index, scale: r.scale, image: Some(image) }
}

fn dropped(r: &Request) -> Drawn {
    Drawn { generation: r.generation, index: r.index, scale: r.scale, image: None }
}

/// How many pages the browser draws in a frame: it has one thread, so a long queue is drawn over
/// several frames and the window keeps answering.
#[cfg(target_arch = "wasm32")]
const PAGES_PER_FRAME: usize = 1;

/// Draws pages on a thread of its own, so a slow page (a full-page photograph) never holds the
/// window up. Before each page it takes the view's [`Wanted`]: the pages in view are drawn first,
/// and pages scrolled away, drawings at a zoom left behind and pages of an older PDF are dropped.
/// The browser build has no threads: there the queue is drawn on the window's thread, a page or
/// so a frame, when the view takes the pages.
struct Renderer {
    tx: std::sync::mpsc::Sender<Request>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: std::sync::mpsc::Receiver<Drawn>,
    wanted: Arc<Mutex<Wanted>>,
    #[cfg(target_arch = "wasm32")]
    queue: (std::sync::mpsc::Receiver<Request>, Vec<Request>),
}

impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    fn start(ctx: &egui::Context) -> Option<Self> {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        let (done, rx) = std::sync::mpsc::channel();
        let wanted = Arc::new(Mutex::new(Wanted::default()));
        let ctx = ctx.clone();
        let shared = wanted.clone();
        std::thread::Builder::new()
            .name("texcraft-pdf-pages".to_owned())
            .spawn(move || {
                let mut queue = Vec::new();
                while let Ok(first) = requests.recv() {
                    queue.push(first);
                    loop {
                        queue.extend(requests.try_iter());
                        let mut open = true;
                        let next = next_request(&mut queue, &shared, |r| open &= done.send(dropped(&r)).is_ok());
                        let Some(r) = next else { break };
                        open &= done.send(draw(&r)).is_ok();
                        ctx.request_repaint();
                        if !open {
                            return;
                        }
                    }
                }
            })
            .ok()?;
        Some(Self { tx, rx, wanted })
    }

    #[cfg(target_arch = "wasm32")]
    fn start(_: &egui::Context) -> Option<Self> {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        Some(Self { tx, wanted: Arc::new(Mutex::new(Wanted::default())), queue: (requests, Vec::new()) })
    }

    /// The pages drawn since last asked.
    #[cfg(not(target_arch = "wasm32"))]
    fn drawn(&mut self, _: &egui::Context) -> Vec<Drawn> {
        self.rx.try_iter().collect()
    }

    /// The pages drawn since last asked: in the browser, drawn now, a few at most.
    #[cfg(target_arch = "wasm32")]
    fn drawn(&mut self, ctx: &egui::Context) -> Vec<Drawn> {
        let (requests, queue) = &mut self.queue;
        queue.extend(requests.try_iter());
        let mut out = Vec::new();
        for _ in 0..PAGES_PER_FRAME {
            let Some(r) = next_request(queue, &self.wanted, |r| out.push(dropped(&r))) else { break };
            out.push(draw(&r));
        }
        if !queue.is_empty() {
            ctx.request_repaint();
        }
        out
    }

    /// Tells the renderer what the view shows now.
    fn want(&self, generation: u64, scale: f32, pages: Vec<usize>) {
        if let Ok(mut w) = self.wanted.lock() {
            *w = Wanted { generation, scale, pages };
        }
    }
}

pub(crate) struct PdfView {
    source: Option<Arc<Vec<u8>>>,
    doc: Option<Arc<Document>>,
    /// Goes up with each new PDF: a drawing of an older one is shown until its page is drawn again.
    generation: u64,
    /// The pages as the view shows them.
    pages: Drawings,
    /// The pages as miniatures, for the thumbnails beside them.
    thumbs: Drawings,
    sizes: Vec<[f32; 2]>,
    error: Option<String>,
    fit: Fit,
    /// The last fit chosen (width or page), which the zoom's Fit button goes back to.
    last_fit: Fit,
    /// Two pages across at least (`view.two_pages`); the fits make room for both.
    two_pages: bool,
    /// The pages' miniatures beside them (`view.thumbnails`).
    thumbnails: bool,
    /// The pages across, last frame.
    columns: usize,
    /// The page the thumbnails last showed as the one in view, to follow it.
    thumbnail_page: Option<usize>,
    /// The zoom used for the last frame.
    zoom: f32,
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
    /// A link followed: scroll so that this page and height (PDF points) is at the top.
    jump: Option<(usize, f32)>,
    /// The links of the pages shown, read once a PDF.
    links: HashMap<usize, Arc<Vec<Link>>>,
    /// A link to the web followed, or refused: the preview pane says it in the status bar.
    pub followed: Option<Followed>,
    /// The glyphs of the pages text was selected on, read once a PDF.
    glyphs: HashMap<usize, Arc<Vec<Glyph>>>,
    /// The text selected by dragging over a page: the page, and the glyphs where the drag started
    /// and where it is.
    selection: Option<(usize, usize, usize)>,
    /// Text copied from the PDF this frame: the preview pane says it in the status bar.
    pub copied: Option<String>,
    /// Presentation mode: the page shown, the whole window for it.
    presenting: Option<usize>,
    /// The pages' scroll offset in the last frame.
    offset: egui::Vec2,
    /// The middle button was pressed over the pages and is held: the pointer drags them.
    panning: bool,
    /// When the presentation started (egui time), once drawn.
    presenting_since: Option<f64>,
}

/// A link to outside the document, clicked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Followed {
    /// Opened in the browser.
    Opened(String),
    /// Not opened: not a web or mail address (a file, a program, a script).
    Refused(String),
}

impl Default for PdfView {
    fn default() -> Self {
        Self {
            source: None,
            doc: None,
            generation: 0,
            pages: Drawings::new("pdf-page"),
            thumbs: Drawings::new("pdf-thumbnail"),
            sizes: Vec::new(),
            error: None,
            fit: Fit::Width,
            last_fit: Fit::Width,
            two_pages: false,
            thumbnails: false,
            columns: 1,
            thumbnail_page: None,
            zoom: 1.0,
            page: 0,
            scroll_to: None,
            clicked: None,
            mark: None,
            reveal: None,
            jump: None,
            links: HashMap::new(),
            followed: None,
            glyphs: HashMap::new(),
            selection: None,
            copied: None,
            presenting: None,
            presenting_since: None,
            offset: egui::Vec2::ZERO,
            panning: false,
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
        self.pages.pending.clear();
        self.thumbs.pending.clear();
        self.links.clear();
        self.glyphs.clear();
        self.selection = None;
        self.error = None;
        self.doc = None;
        self.sizes.clear();
        if let Some(bytes) = pdf {
            match Document::open(bytes.clone()) {
                Ok(doc) => {
                    self.sizes = doc.page_sizes();
                    let count = self.sizes.len();
                    self.pages.textures.retain(|i, _| *i < count);
                    self.thumbs.textures.retain(|i, _| *i < count);
                    self.doc = Some(Arc::new(doc));
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        self.page = self.page.min(self.sizes.len().saturating_sub(1));
        if let Some(p) = &mut self.presenting {
            *p = (*p).min(self.sizes.len().saturating_sub(1));
        }
    }

    /// Starts presenting from the first page in view.
    pub fn present(&mut self) {
        if self.doc.is_some() {
            self.presenting = Some(self.page);
            self.presenting_since = None;
        }
    }

    /// The page presented, while in presentation mode.
    pub fn presenting(&self) -> Option<usize> {
        self.presenting
    }

    /// When the presentation started (egui time): `now` the first time it is asked.
    pub fn presented_since(&mut self, now: f64) -> f64 {
        *self.presenting_since.get_or_insert(now)
    }

    /// Presents the page `by` pages on (back when negative), within the document.
    pub fn present_step(&mut self, by: isize) {
        if let Some(p) = &mut self.presenting {
            *p = p.saturating_add_signed(by).min(self.sizes.len().saturating_sub(1));
        }
    }

    /// Leaves presentation mode, the page presented in view.
    pub fn present_end(&mut self) {
        if let Some(p) = self.presenting.take() {
            self.scroll_to = Some(p);
            self.page = p;
        }
    }

    /// Draws the page presented, as large as `ui` allows, on the presentation backdrop. Links
    /// work; a click elsewhere on the page goes to the next one.
    pub fn show_slide(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        self.receive(ui.ctx());
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, t.presentation);
        let Some(p) = self.presenting else { return };
        let Some(size) = self.sizes.get(p).copied() else { return };
        let zoom = (rect.width() / size[0]).min(rect.height() / size[1]).max(0.01);
        let page = egui::Rect::from_center_size(rect.center(), egui::vec2(size[0], size[1]) * zoom);
        let next: Vec<usize> = [p, p + 1].into_iter().filter(|i| *i < self.sizes.len()).collect();
        self.request(ui.ctx(), &next, ui.ctx().pixels_per_point() * zoom);
        ui.painter().rect_filled(page, 0.0, egui::Color32::WHITE);
        if let Some(texture) = self.pages.texture(p) {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture.id(), page, uv, egui::Color32::WHITE);
        }
        let name = tl!("Page {n} of {count}").replace("{n}", &(p + 1).to_string()).replace("{count}", &self.sizes.len().to_string());
        let r = ui.interact(page, ui.scope_id().with("pdf-slide"), egui::Sense::click());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Document, true, &name));
        match self.page_links(ui, t, p, page, zoom) {
            Some((_, target)) => self.follow(ui.ctx(), target),
            None if r.clicked() => self.present_step(1),
            None => {}
        }
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

    /// How far the pages are scrolled, right and down, in points.
    pub fn offset(&self) -> egui::Vec2 {
        self.offset
    }

    /// Whether two pages go across at least.
    pub fn two_pages(&self) -> bool {
        self.two_pages
    }

    pub fn set_two_pages(&mut self, on: bool) {
        self.two_pages = on;
    }

    /// Whether the pages' miniatures are shown beside them.
    pub fn thumbnails(&self) -> bool {
        self.thumbnails
    }

    /// Shows or hides the miniatures; hidden, they are let go.
    pub fn set_thumbnails(&mut self, on: bool) {
        self.thumbnails = on;
        if !on {
            self.thumbs.textures.clear();
            self.thumbnail_page = None;
        }
    }

    /// The pages across, last frame.
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// The fit the zoom's Fit button goes back to: the last of fit width and fit page chosen.
    pub fn last_fit(&self) -> Fit {
        self.last_fit
    }

    pub fn set_fit(&mut self, fit: Fit) {
        self.fit = match fit {
            Fit::Zoom(z) => Fit::Zoom(z.clamp(MIN_ZOOM, MAX_ZOOM)),
            other => {
                self.last_fit = other;
                other
            }
        };
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.set_fit(Fit::Zoom(self.zoom * factor));
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
        // Ctrl with the wheel (Cmd on a Mac), or a pinch, zooms around the pointer.
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p) && ui.ctx().layer_id_at(*p) == Some(ui.layer_id()));
        let pinch = if pointer.is_some() { ui.input(|i| i.zoom_delta()) } else { 1.0 };
        let before = (self.zoom, self.columns);
        let anchor = pointer.filter(|_| (pinch - 1.0).abs() > 1e-4 && before.0 > 0.0);
        if anchor.is_some() {
            self.set_fit(Fit::Zoom(before.0 * pinch));
        }
        // The fits make room for two pages across with Two Pages on.
        let across = if self.two_pages { 2.0 } else { 1.0 };
        let fit_width = (rect.width() - (across + 1.0) * PAGE_GAP) / (across * widest);
        self.zoom = match self.fit {
            Fit::Width => fit_width,
            Fit::Page => ((rect.height() - 2.0 * PAGE_GAP) / tallest).min(fit_width),
            Fit::Zoom(z) => z,
        }
        .clamp(MIN_ZOOM, MAX_ZOOM);
        let zoom = self.zoom;
        self.columns = columns(rect.width(), widest * zoom, self.fit == Fit::Width, self.two_pages, self.sizes.len());
        let pixels = ui.ctx().pixels_per_point() * zoom;
        let (places, total) = layout(&self.sizes, zoom, self.columns, rect.width());
        let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
        if let Some(p) = anchor
            && (zoom - before.0).abs() > f32::EPSILON
        {
            let at = p - rect.min;
            let (old, _) = layout(&self.sizes, before.0, before.1, rect.width());
            let content = (self.offset + at).to_pos2();
            // The point under the pointer, in the page's points, stays under it.
            let page = old.iter().position(|r| r.contains(content)).or_else(|| old.iter().rposition(|r| r.top() <= content.y)).unwrap_or(0);
            let moved = match (old.get(page), places.get(page)) {
                (Some(o), Some(n)) => n.min + (content - o.min) / before.0 * zoom,
                _ => content,
            };
            area = area.scroll_offset((moved - at).to_vec2().max(egui::Vec2::ZERO));
        }
        // The middle button held down drags the pages, when they are larger than the pane.
        let (middle, pressed, moved) =
            ui.input(|i| (i.pointer.middle_down(), i.pointer.button_pressed(egui::PointerButton::Middle), i.pointer.delta()));
        if pressed && pointer.is_some() {
            self.panning = true;
        }
        self.panning &= middle;
        if self.panning {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if moved != egui::Vec2::ZERO {
                area = area.scroll_offset((self.offset - moved).max(egui::Vec2::ZERO));
            }
        }
        // A page to show: its row at the top, and its column in view when the rows are wider
        // than the pane.
        let wide = total.x > rect.width() + 1.0;
        let go = |area: egui::ScrollArea, page: usize, y: f32| -> egui::ScrollArea {
            let Some(place) = places.get(page) else { return area };
            let area = area.vertical_scroll_offset(y.max(0.0));
            if wide { area.horizontal_scroll_offset((place.left() - PAGE_GAP).max(0.0)) } else { area }
        };
        if let Some(page) = self.scroll_to.take() {
            let top = places.get(page).map_or(0.0, |r| r.top());
            area = go(area, page, top - PAGE_GAP);
        }
        // A place shown from the source: a third of the way down the pane.
        if let Some((page, y)) = self.reveal.take() {
            let top = places.get(page).map_or(0.0, |r| r.top());
            area = go(area, page, top + y * zoom - rect.height() / 3.0);
        }
        if let Some((page, y)) = self.jump.take() {
            let top = places.get(page).map_or(0.0, |r| r.top());
            area = go(area, page, top + y * zoom - PAGE_GAP);
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
        let mut follow = None;
        let shadow = egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: t.shadow };
        let count = self.sizes.len();
        area.show_viewport(ui, |ui, viewport| {
            self.offset = viewport.min.to_vec2();
            let (outer, _) = ui.allocate_exact_size(total, egui::Sense::hover());
            // The pages to draw: those in view, then those half a pane above and below it.
            let mut wanted: Vec<usize> = (0..places.len()).filter(|i| viewport.intersects(places[*i])).collect();
            let near = viewport.expand2(egui::vec2(0.0, viewport.height() / 2.0));
            wanted.extend((0..places.len()).filter(|i| near.intersects(places[*i]) && !viewport.intersects(places[*i])));
            self.request(ui.ctx(), &wanted, pixels);
            let mut first_visible = None;
            for (i, place) in places.iter().enumerate() {
                if !viewport.intersects(*place) {
                    continue;
                }
                let page = place.translate(outer.min.to_vec2());
                if first_visible.is_none() && place.center().y > viewport.top() {
                    first_visible = Some(i);
                }
                ui.painter().add(shadow.as_shape(page, 2));
                ui.painter().rect_filled(page, 2, egui::Color32::WHITE);
                // Until the page is drawn: as drawn before (another zoom, the last PDF), or blank.
                if let Some(texture) = self.pages.texture(i) {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(texture.id(), page, uv, egui::Color32::WHITE);
                }
                // Double-click or Ctrl+click: the source of that point. A drag selects text.
                let r = ui.interact(page, ui.scope_id().with(("pdf-page", i)), egui::Sense::click_and_drag());
                let name = tl!("Page {n} of {count}").replace("{n}", &(i + 1).to_string()).replace("{count}", &count.to_string());
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Document, true, &name));
                if (r.double_clicked() || (r.clicked() && command))
                    && let Some(p) = r.interact_pointer_pos()
                {
                    clicked = Some((i, (p.x - page.left()) / zoom, (p.y - page.top()) / zoom));
                }
                self.select(ui, t, i, page, zoom, &r);
                match self.page_links(ui, t, i, page, zoom) {
                    Some((point, _)) if command => clicked = Some((i, point.x, point.y)),
                    Some((_, target)) => follow = Some(target),
                    None => {}
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
        if let Some(target) = follow {
            self.follow(ui.ctx(), target);
        }
        if let Some(pass) = building.and_then(|b| b.draft) {
            draft_badge(ui, t, rect, pass);
        }
    }

    /// The pages' miniatures, one under the other in `ui`: the page in view framed in the accent
    /// and its number in bold, a click (or Enter) going to a page. They are drawn small, those
    /// in view only, as they scroll into view; those far from it are let go, so a long document
    /// keeps few of them. The list follows the page in view.
    pub fn show_thumbnails(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        self.receive(ui.ctx());
        let Some(doc) = self.doc.clone() else { return };
        let count = self.sizes.len();
        let widest = self.sizes.iter().map(|s| s[0]).fold(1.0_f32, f32::max);
        let tallest = self.sizes.iter().map(|s| s[1]).fold(1.0_f32, f32::max);
        // Points on screen per point of the page.
        let small = t.thumbnail_width / widest;
        let label = t.small_size + 2.0 * t.unit;
        let row = tallest * small + label + 3.0 * t.unit;
        let current = self.page.min(count.saturating_sub(1));
        let mut area = egui::ScrollArea::vertical().id_salt("pdf-thumbnails").auto_shrink([false, false]);
        if self.thumbnail_page != Some(current) {
            self.thumbnail_page = Some(current);
            let view = ui.available_height();
            area = area.vertical_scroll_offset((current as f32 * row - (view - row) / 2.0).max(0.0));
        }
        let pixels = ui.ctx().pixels_per_point() * small;
        let generation = self.generation;
        let mut go = None;
        let region = egui::UiBuilder::new().accessibility_role(egui::accesskit::Role::List).accessibility_label(tl!("Page thumbnails"));
        ui.scope_builder(region, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            area.show_rows(ui, row, count, |ui, range| {
                let wanted: Vec<usize> = range.clone().collect();
                self.thumbs.request(ui.ctx(), &doc, generation, &wanted, pixels);
                for i in range.clone() {
                    let (cell, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row), egui::Sense::click());
                    let name = tl!("Page {n}").replace("{n}", &(i + 1).to_string());
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                    // The page in view is the list's selected one (not a pressed button).
                    ui.ctx().accesskit_node_builder(r.id, |node| node.set_selected(i == current));
                    let fill = crate::widgets::state_fill(t, &r, t.frame);
                    if fill != t.frame {
                        ui.painter().rect_filled(cell.shrink(t.unit), t.corner(), fill);
                    }
                    let size = egui::vec2(self.sizes[i][0], self.sizes[i][1]) * small;
                    let page = egui::Rect::from_min_size(egui::pos2(cell.center().x - size.x / 2.0, cell.top() + 2.0 * t.unit), size);
                    ui.painter().rect_filled(page, 1.0, egui::Color32::WHITE);
                    if let Some(texture) = self.thumbs.texture(i) {
                        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        ui.painter().image(texture.id(), page, uv, egui::Color32::WHITE);
                    }
                    // The page in view: framed, and its number in bold, so not by colour alone.
                    let (stroke, font, color) = if i == current {
                        (egui::Stroke::new(2.0, t.accent), egui::FontId::new(t.small_size, crate::fonts::semibold()), t.text)
                    } else {
                        (egui::Stroke::new(1.0, t.separator), t.small(), t.text_muted)
                    };
                    ui.painter().rect_stroke(page, 1.0, stroke, egui::StrokeKind::Outside);
                    let number = egui::pos2(cell.center().x, page.bottom() + t.unit + label / 2.0);
                    ui.painter().text(number, egui::Align2::CENTER_CENTER, (i + 1).to_string(), font, color);
                    crate::widgets::focus_ring(ui, t, &r, cell.shrink(t.unit), t.corner());
                    if r.gained_focus() {
                        r.scroll_to_me(None);
                    }
                    if r.on_hover_text(&name).clicked() {
                        go = Some(i);
                    }
                }
                // Let go of the miniatures far from view.
                let keep = range.start.saturating_sub(THUMBNAILS_KEPT)..range.end + THUMBNAILS_KEPT;
                self.thumbs.textures.retain(|i, _| keep.contains(i));
            });
        });
        if let Some(i) = go {
            self.go_to_page(i);
            // Followed already: the list stays where it was clicked.
            self.page = i;
            self.thumbnail_page = Some(i);
        }
    }

    /// Follows a link: to its page (presented, or scrolled to the top of the pane at the
    /// destination's height), or to the web after the safe-URL check.
    fn follow(&mut self, ctx: &egui::Context, target: Target) {
        match target {
            Target::Page { page, y } => {
                if self.presenting.is_some() {
                    self.presenting = Some(page.min(self.sizes.len().saturating_sub(1)));
                } else {
                    self.jump = Some((page, y.unwrap_or(0.0)));
                }
                ctx.request_repaint();
            }
            Target::Uri(uri) if texcraft_preview::safe_url(&uri) => {
                ctx.open_url(egui::OpenUrl::new_tab(uri.trim()));
                self.followed = Some(Followed::Opened(uri));
            }
            Target::Uri(uri) => self.followed = Some(Followed::Refused(uri)),
        }
    }

    /// The links of page `i`, drawn at `page`: each a link for the pointer (a hand, its target
    /// as a tooltip), the keyboard and screen readers. The link clicked, if one was: the point
    /// clicked (PDF points from the page's top left) and where it goes.
    fn page_links(&mut self, ui: &mut egui::Ui, t: &Tokens, i: usize, page: egui::Rect, zoom: f32) -> Option<(egui::Pos2, Target)> {
        let doc = self.doc.as_ref()?;
        let links = self.links.entry(i).or_insert_with(|| Arc::new(doc.links(i))).clone();
        let mut clicked = None;
        for (k, link) in links.iter().enumerate() {
            let [l, top, r, b] = link.rect;
            let area = egui::Rect::from_min_max(page.min + egui::vec2(l, top) * zoom, page.min + egui::vec2(r, b) * zoom);
            let label = link_label(&link.target);
            let response = ui.interact(area, ui.scope_id().with(("pdf-link", i, k)), egui::Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
            if response.has_focus() {
                ui.painter().rect_stroke(area.expand(2.0), 2.0, egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
            }
            if response.clicked() {
                let at = response.interact_pointer_pos().unwrap_or(area.center());
                clicked = Some((((at - page.min) / zoom).to_pos2(), link.target.clone()));
            }
            let _ = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(label);
        }
        clicked
    }

    /// Text selection on page `i`, drawn at `page`, through the page's response `r`: a drag
    /// selects from the glyph nearest where it started to the one nearest the pointer; a click or
    /// Escape clears it; Ctrl+C (Cmd+C) copies it while the page has the keyboard.
    fn select(&mut self, ui: &mut egui::Ui, t: &Tokens, i: usize, page: egui::Rect, zoom: f32, r: &egui::Response) {
        let at = |p: egui::Pos2| ((p - page.min) / zoom).to_pos2();
        // The middle button pans instead ([`PdfView::show`]).
        let primary = !ui.input(|i| i.pointer.middle_down());
        // A finger's drag scrolls the pages (#26); its long press selects from there, and moving
        // it, still down, extends the selection.
        let finger = crate::touch::scroll(ui, r);
        let pointer = ui.input(|input| input.pointer.latest_pos());
        if r.long_touched()
            && let Some(p) = pointer
        {
            let p = at(p);
            self.selection = self.page_glyphs(i).and_then(|g| texcraft_preview::glyph_at(&g, p.x, p.y)).map(|g| (i, g, g));
        } else if crate::touch::selecting(ui.ctx())
            && let Some((page_selected, _, head)) = &mut self.selection
            && *page_selected == i
            && let Some(p) = pointer
            && let Some(glyphs) = self.glyphs.get(&i)
        {
            let p = at(p);
            *head = texcraft_preview::glyph_at(glyphs, p.x, p.y).unwrap_or(*head);
        } else if finger {
            if r.clicked() {
                self.selection = None;
            }
        } else if r.drag_started()
            && primary
            && let Some(start) = ui.input(|input| input.pointer.press_origin())
        {
            let start = at(start);
            self.selection = self.page_glyphs(i).and_then(|g| texcraft_preview::glyph_at(&g, start.x, start.y)).map(|g| (i, g, g));
            r.request_focus();
        } else if r.dragged()
            && primary
            && let Some((page_selected, _, head)) = &mut self.selection
            && *page_selected == i
            && let Some(p) = r.interact_pointer_pos()
            && let Some(glyphs) = self.glyphs.get(&i)
        {
            let p = at(p);
            *head = texcraft_preview::glyph_at(glyphs, p.x, p.y).unwrap_or(*head);
        } else if r.clicked() && !r.double_clicked() {
            self.selection = None;
        }
        let Some((page_selected, anchor, head)) = self.selection.filter(|s| s.0 == i) else { return };
        let Some(glyphs) = self.glyphs.get(&page_selected).cloned() else { return };
        let (a, b) = (anchor.min(head), anchor.max(head));
        for g in glyphs.iter().take(b + 1).skip(a) {
            let [l, top, right, bottom] = g.rect;
            let area = egui::Rect::from_min_max(page.min + egui::vec2(l, top) * zoom, page.min + egui::vec2(right, bottom) * zoom);
            ui.painter().rect_filled(area, 0.0, t.selection);
        }
        if !r.has_focus() {
            return;
        }
        let (copy, escape) = ui.input(|input| {
            let copy = input.events.iter().any(|e| matches!(e, egui::Event::Copy)) || (input.modifiers.command && input.key_pressed(egui::Key::C));
            (copy, input.key_pressed(egui::Key::Escape))
        });
        if copy {
            let text = texcraft_preview::text_between(&glyphs, anchor, head);
            ui.ctx().copy_text(text.clone());
            self.copied = Some(text);
        }
        if escape {
            self.selection = None;
        }
    }

    /// The glyphs of page `i`, read the first time they are asked for.
    fn page_glyphs(&mut self, i: usize) -> Option<Arc<Vec<Glyph>>> {
        if let Some(g) = self.glyphs.get(&i) {
            return Some(g.clone());
        }
        let glyphs = match self.doc.as_ref()?.glyphs(i) {
            Ok(g) => Arc::new(g),
            Err(e) => {
                log::warn!("page {}: {e}", i + 1);
                Arc::new(Vec::new())
            }
        };
        self.glyphs.insert(i, glyphs.clone());
        Some(glyphs)
    }

    /// Asks for `pages` (in that order) at `scale`, those not drawn at it yet; the renderer drops
    /// what it was asked before and is no longer wanted.
    fn request(&mut self, ctx: &egui::Context, pages: &[usize], scale: f32) {
        // No side of a page's texture over what the GPU takes (2048 in some browsers and CI's
        // software renderer): past that zoom the page is drawn at the largest scale that fits.
        let max_side = ctx.input(|i| i.max_texture_side) as f32;
        let largest = self.sizes.iter().map(|s| s[0].max(s[1])).fold(0.0, f32::max);
        let scale = if largest > 0.0 { scale.min(max_side / largest) } else { scale };
        if let Some(doc) = self.doc.clone() {
            self.pages.request(ctx, &doc, self.generation, pages, scale);
        }
    }

    /// Takes the pages the renderers drew.
    fn receive(&mut self, ctx: &egui::Context) {
        self.pages.receive(ctx, self.generation);
        self.thumbs.receive(ctx, self.generation);
    }
}

/// Pages drawn by a renderer of their own, at one scale at a time: the view's pages, or their
/// miniatures beside them.
struct Drawings {
    /// What their textures are named after.
    name: &'static str,
    renderer: Option<Renderer>,
    /// Pages asked of the renderer, at their scale, for this generation.
    pending: HashMap<usize, f32>,
    /// Drawn pages: the pixel scale and generation each was drawn at, and its texture.
    textures: HashMap<usize, (f32, u64, egui::TextureHandle)>,
}

impl Drawings {
    fn new(name: &'static str) -> Self {
        Self { name, renderer: None, pending: HashMap::new(), textures: HashMap::new() }
    }

    /// Page `i` as last drawn, at whatever scale or generation.
    fn texture(&self, i: usize) -> Option<&egui::TextureHandle> {
        self.textures.get(&i).map(|(_, _, t)| t)
    }

    /// Asks for `pages` (in that order) of `doc` at `scale`, those not drawn at it yet; the
    /// renderer drops what it was asked before and is no longer wanted.
    fn request(&mut self, ctx: &egui::Context, doc: &Arc<Document>, generation: u64, pages: &[usize], scale: f32) {
        if self.renderer.is_none() {
            self.renderer = Renderer::start(ctx);
        }
        let fresh = |textures: &HashMap<usize, (f32, u64, egui::TextureHandle)>, i: &usize| {
            textures.get(i).is_some_and(|(drawn, g, _)| *g == generation && (drawn - scale).abs() < 0.01)
        };
        let Some(renderer) = &self.renderer else {
            // No thread to draw on: drawn here and now.
            for i in pages.iter().filter(|i| !fresh(&self.textures, i)).copied().collect::<Vec<_>>() {
                let image = doc.render(i, scale).map_err(|e| e.to_string());
                self.take(ctx, generation, Drawn { generation, index: i, scale, image: Some(image) });
            }
            return;
        };
        renderer.want(generation, scale, pages.to_vec());
        for &i in pages {
            if fresh(&self.textures, &i) || self.pending.get(&i).is_some_and(|s| (s - scale).abs() < 0.01) {
                continue;
            }
            let request = Request { generation, index: i, scale, doc: doc.clone() };
            if renderer.tx.send(request).is_ok() {
                self.pending.insert(i, scale);
                // The browser draws the queue next frame.
                #[cfg(target_arch = "wasm32")]
                ctx.request_repaint();
            }
        }
    }

    /// Takes the pages the renderer drew.
    fn receive(&mut self, ctx: &egui::Context, generation: u64) {
        let drawn: Vec<Drawn> = self.renderer.as_mut().map(|r| r.drawn(ctx)).unwrap_or_default();
        for d in drawn {
            self.take(ctx, generation, d);
        }
    }

    fn take(&mut self, ctx: &egui::Context, generation: u64, d: Drawn) {
        if d.generation != generation {
            return;
        }
        if self.pending.get(&d.index).is_some_and(|s| (s - d.scale).abs() < 0.01) {
            self.pending.remove(&d.index);
        }
        match d.image {
            // Dropped: asked again when it is in view.
            None => {}
            Some(Ok(image)) => {
                let color = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.rgba);
                let handle = ctx.load_texture(format!("{}-{}", self.name, d.index), color, egui::TextureOptions::LINEAR);
                self.textures.insert(d.index, (d.scale, d.generation, handle));
            }
            Some(Err(e)) => log::warn!("page {}: {e}", d.index + 1),
        }
    }
}

/// What a link says: where it goes.
/// How many pages go across: as many as fit in `width` at their width `page` (more as the zoom
/// goes down), two at least with Two Pages on, and only those the fit makes room for at fit
/// width; never more than there are.
fn columns(width: f32, page: f32, fit_width: bool, two_pages: bool, count: usize) -> usize {
    let least = if two_pages { 2 } else { 1 };
    let fit = ((width - PAGE_GAP) / (page + PAGE_GAP)).floor().max(0.0) as usize;
    let across = if fit_width { least } else { fit.max(least) };
    across.min(count.max(1))
}

/// Where each page is at `zoom`, `columns` to a row, from the top left of the pages, in a pane
/// `width` wide; and the size of them all. The rows are centred in the pane, each page in its
/// column, and a row is as tall as its tallest page.
fn layout(sizes: &[[f32; 2]], zoom: f32, columns: usize, width: f32) -> (Vec<egui::Rect>, egui::Vec2) {
    let columns = columns.max(1);
    let column = sizes.iter().map(|s| s[0]).fold(0.0_f32, f32::max) * zoom;
    let row_width = columns as f32 * column + (columns as f32 - 1.0) * PAGE_GAP;
    let total_width = (row_width + 2.0 * PAGE_GAP).max(width);
    let left = (total_width - row_width) / 2.0;
    let mut places = Vec::with_capacity(sizes.len());
    let mut y = PAGE_GAP;
    for row in sizes.chunks(columns) {
        let height = row.iter().map(|s| s[1]).fold(0.0_f32, f32::max) * zoom;
        for (c, s) in row.iter().enumerate() {
            let x = left + c as f32 * (column + PAGE_GAP) + (column - s[0] * zoom) / 2.0;
            places.push(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(s[0] * zoom, s[1] * zoom)));
        }
        y += height + PAGE_GAP;
    }
    (places, egui::vec2(total_width, y))
}

fn link_label(target: &Target) -> String {
    match target {
        Target::Page { page, .. } => tl!("Link to page {n}").replace("{n}", &(page + 1).to_string()),
        Target::Uri(uri) => tl!("Link to {address}").replace("{address}", uri.trim()),
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{Asked, Wanted, next_request};

    impl Asked for (u64, usize, f32) {
        fn asked(&self) -> (u64, usize, f32) {
            *self
        }
    }

    #[test]
    fn the_pages_in_view_are_drawn_first_and_the_others_dropped() {
        // In view: pages 7 then 8, at scale 2, of the PDF of generation 3.
        let wanted = Mutex::new(Wanted { generation: 3, scale: 2.0, pages: vec![7, 8] });
        let mut queue = vec![
            (3, 8, 2.0),
            (2, 7, 2.0), // an older PDF
            (3, 1, 2.0), // scrolled away
            (3, 7, 1.0), // another zoom
            (3, 7, 2.0),
        ];
        let mut dropped = Vec::new();
        let first = next_request(&mut queue, &wanted, |r| dropped.push(r));
        assert_eq!(first, Some((3, 7, 2.0)), "the first page in view is not drawn first");
        assert_eq!(next_request(&mut queue, &wanted, |r| dropped.push(r)), Some((3, 8, 2.0)));
        assert_eq!(next_request(&mut queue, &wanted, |r| dropped.push(r)), None);
        assert_eq!(dropped.len(), 3, "requests no longer wanted were kept: {dropped:?}");
    }
}
