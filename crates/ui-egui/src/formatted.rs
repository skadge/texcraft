//! The formatted view of a `.tex` file (docs/plan.md, phase 3): the file's view model
//! ([`texcraft_latex::view`]) drawn as a document, at a reading measure in the reading face.
//!
//! It is read-only for now. Double-clicking a part (or pressing Enter on it) shows the source with
//! the caret where that part starts. Formulas are shown as their source until retex typesets them
//! (phase 3, item 3); what the view cannot show faithfully stays as its source.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, FontFamily, FontId, Galley, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use texcraft_engine::project::{Contents, Project, ProjectPath};
use texcraft_latex::view::{Block, BlockKind, ChipKind, Inline, InlineKind, Item, ListKind, Style};
use texcraft_preview::figure;

use crate::App;
use crate::theme::{Tokens, blend};

/// The view model of the shown file, for the session revision it was made from.
struct Cache {
    path: ProjectPath,
    revision: u64,
    text: Arc<str>,
    blocks: Arc<Vec<Block>>,
    /// The preamble's summary: the class and how many packages.
    summary: String,
}

/// An image a figure shows: its texture and natural size, or why it cannot be shown.
type Picture = Result<(egui::TextureHandle, Vec2), String>;

#[derive(Default)]
pub(crate) struct Formatted {
    /// Files shown formatted.
    files: HashSet<ProjectPath>,
    /// Files whose preamble is shown.
    preamble_open: HashSet<ProjectPath>,
    cache: Option<Cache>,
    /// By the image's project path and length.
    images: HashMap<(String, usize), Picture>,
}

impl Formatted {
    /// Whether `path` is shown formatted.
    pub fn shows(&self, path: &ProjectPath) -> bool {
        self.files.contains(path)
    }

    pub fn set(&mut self, path: &ProjectPath, on: bool) {
        if on {
            self.files.insert(path.clone());
        } else {
            self.files.remove(path);
        }
    }
}

/// Whether `path` has a formatted view.
pub(crate) fn available(path: &ProjectPath) -> bool {
    path.as_str().ends_with(".tex")
}

/// The class and the number of packages of a preamble, for its folded bar.
fn summary(text: &str) -> String {
    let tree = texcraft_latex::tree::parse(text);
    let mut class = None;
    let mut packages = 0;
    for n in &tree {
        if let texcraft_latex::tree::Kind::Command { name, args, .. } = &n.kind
            && let Some(arg) = args.iter().rev().find(|a| !a.optional)
        {
            match name.as_str() {
                "documentclass" => class = text.get(arg.inner.clone()).map(str::trim),
                "usepackage" => packages += text.get(arg.inner.clone()).map_or(1, |p| p.split(',').filter(|p| !p.trim().is_empty()).count()),
                _ => {}
            }
        }
    }
    let packages = match packages {
        0 => String::new(),
        1 => tl!("1 package").to_owned(),
        n => tl!("{n} packages").replace("{n}", &n.to_string()),
    };
    match class {
        Some(c) if !packages.is_empty() => format!("{c} · {packages}"),
        Some(c) => c.to_owned(),
        None => packages,
    }
}

/// Where a part of the view sends the caret: its source's first byte.
struct Jump(usize);

/// What drawing the view needs and what it reports.
struct Cx<'a> {
    t: Tokens,
    text: &'a str,
    project: &'a Project,
    path: &'a ProjectPath,
    images: &'a mut HashMap<(String, usize), Picture>,
    preamble_open: bool,
    summary: &'a str,
    /// The byte to scroll to (a jump from the outline or the PDF), until a block at it is drawn.
    scroll_to: Option<usize>,
    /// Out: a part was double-clicked or activated.
    jump: Option<Jump>,
    /// Out: the preamble bar was toggled.
    toggle_preamble: bool,
    /// Out: the first focusable part, for F6.
    first: Option<egui::Id>,
    /// Out: a part has the keyboard focus.
    focused: bool,
    /// How paragraphs are aligned inside `center` and `flushright`.
    align: Align,
    /// Footnotes numbered so far.
    footnotes: usize,
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    let revision = app.session.revision();
    let fresh = app.editor.formatted.cache.as_ref().is_some_and(|c| &c.path == path && c.revision == revision);
    if !fresh {
        let text: Arc<str> = match app.session.project().get(path) {
            Some(Contents::Text(t)) => Arc::from(t.as_str()),
            _ => Arc::from(""),
        };
        // The whole file again: files are small, incremental parsing is still to come.
        let blocks = Arc::new(texcraft_latex::view::view(&text));
        let summary = match blocks.first() {
            Some(Block { kind: BlockKind::Preamble, range }) => summary(text.get(range.clone()).unwrap_or_default()),
            _ => String::new(),
        };
        app.editor.formatted.cache = Some(Cache { path: path.clone(), revision, text, blocks, summary });
    }
    let Some(cache) = app.editor.formatted.cache.as_ref() else { return };
    let (text, blocks, summary) = (Arc::clone(&cache.text), Arc::clone(&cache.blocks), cache.summary.clone());
    // A jump from the outline, the PDF or the build log scrolls to its line.
    let scroll_to = match app.view.goto.take() {
        Some((p, line)) if &p == path => Some(text.split_inclusive('\n').take(line.saturating_sub(1) as usize).map(str::len).sum()),
        other => {
            app.view.goto = other;
            None
        }
    };
    let t = app.tokens;
    let mut cx = Cx {
        t,
        text: &text,
        project: app.session.project(),
        path,
        images: &mut app.editor.formatted.images,
        preamble_open: app.editor.formatted.preamble_open.contains(path),
        summary: &summary,
        scroll_to,
        jump: None,
        toggle_preamble: false,
        first: None,
        focused: false,
        align: Align::Min,
        footnotes: 0,
    };
    let page = 6.0 * t.unit;
    app.view.cursor = None;
    egui::ScrollArea::vertical().id_salt(("formatted", path.as_str())).auto_shrink([false, false]).show(ui, |ui| {
        let avail = ui.available_width();
        let measure = measure(ui, &t).min(avail - 2.0 * page).max(120.0);
        ui.add_space(page * 1.5);
        let left = ((avail - measure) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(left);
            ui.vertical(|ui| {
                ui.set_width(measure);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                draw_blocks(ui, &mut cx, &blocks, 0);
            });
        });
        ui.add_space(page * 4.0);
        keyboard_scroll(ui, cx.focused);
    });
    let (jump, toggle, first) = (cx.jump.take(), cx.toggle_preamble, cx.first);
    if let Some(id) = first {
        app.region(crate::Region::Editor, id);
    }
    if toggle && !app.editor.formatted.preamble_open.remove(path) {
        app.editor.formatted.preamble_open.insert(path.clone());
    }
    if let Some(Jump(byte)) = jump {
        to_source(app, ui, path, &text, byte);
    }
}

/// Page Up, Page Down, Home and End scroll the view while one of its parts has the focus.
fn keyboard_scroll(ui: &mut egui::Ui, focused: bool) {
    if !focused {
        return;
    }
    let page = ui.clip_rect().height() * 0.85;
    let delta = ui.input(|i| {
        if i.key_pressed(egui::Key::PageDown) {
            -page
        } else if i.key_pressed(egui::Key::PageUp) {
            page
        } else if i.key_pressed(egui::Key::End) {
            -1.0e7
        } else if i.key_pressed(egui::Key::Home) {
            1.0e7
        } else {
            0.0
        }
    });
    if delta != 0.0 {
        ui.scroll_with_delta(vec2(0.0, delta));
    }
}

/// Shows the source of `path` with the caret at byte `byte`.
fn to_source(app: &mut App, ui: &egui::Ui, path: &ProjectPath, text: &str, byte: usize) {
    app.editor.formatted.set(path, false);
    let at = text.get(..byte).map_or(0, |s| s.chars().count());
    let source = &mut app.editor.source;
    let at = at.min(source.doc.buffer.len_chars());
    source.select(at, at);
    let line = source.doc.buffer.char_to_line(at);
    source.flash_line(line, ui.input(|i| i.time));
    source.reveal = true;
    app.editor.focus = true;
}

/// The reading measure: about `reading_measure` average characters of the reading face.
fn measure(ui: &egui::Ui, t: &Tokens) -> f32 {
    let sample = "abcdefghijklmnopqrstuvwxyz";
    let width = ui.painter().layout_no_wrap(sample.to_owned(), t.reading(t.reading_size, false, false), t.text).size().x;
    width / sample.len() as f32 * t.reading_measure
}

fn draw_blocks(ui: &mut egui::Ui, cx: &mut Cx<'_>, blocks: &[Block], depth: usize) {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            ui.add_space(gap_before(&cx.t, &block.kind));
        }
        let top = ui.cursor().top();
        draw_block(ui, cx, block, depth);
        if let Some(target) = cx.scroll_to
            && block.range.end > target
        {
            let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top().max(top + 1.0));
            ui.scroll_to_rect(rect, Some(Align::TOP));
            cx.scroll_to = None;
        }
    }
}

/// The space above a block, after another.
fn gap_before(t: &Tokens, kind: &BlockKind) -> f32 {
    let em = t.reading_size;
    match kind {
        BlockKind::Heading { level, .. } if *level <= 3 => em * 1.75,
        BlockKind::Heading { .. } => em * 1.25,
        BlockKind::Figure { .. } | BlockKind::Formula { .. } | BlockKind::Source | BlockKind::Container { .. } => em,
        _ => em * 0.75,
    }
}

fn draw_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, block: &Block, depth: usize) {
    let range = &block.range;
    match &block.kind {
        BlockKind::Preamble => preamble(ui, cx, range),
        BlockKind::Title { title, author, date } => title_block(ui, cx, range, [title, author, date]),
        BlockKind::Heading { level, content, label, .. } => heading(ui, cx, range, *level, content, label.as_ref()),
        BlockKind::Paragraph(inlines) => {
            let size = cx.t.reading_size;
            let _ = paragraph(ui, cx, inlines, range, &Para { size, bold: false, color: cx.t.text, role: egui::Role::Paragraph });
        }
        BlockKind::List { kind, items } => list(ui, cx, *kind, items, depth),
        BlockKind::Container { name, blocks } => container(ui, cx, name, blocks, depth),
        BlockKind::Formula { source, env, closed } => formula(ui, cx, range, source, env.as_deref(), *closed),
        BlockKind::Figure { image, caption, label } => figure_card(ui, cx, range, image.as_ref(), caption, label.as_ref()),
        BlockKind::Source => source_block(ui, cx, range),
    }
}

/// How a paragraph is set.
struct Para {
    size: f32,
    bold: bool,
    color: Color32,
    role: egui::Role,
}

/// A run of a laid-out paragraph: the characters it takes in the galley and what it stands for.
struct Piece {
    chars: Range<usize>,
    range: Range<usize>,
    kind: PieceKind,
}

enum PieceKind {
    Text,
    Chip(ChipKind, String),
    Formula,
    Source,
    Link(String),
    Footnote(String),
}

/// Builds a paragraph's layout job.
struct Builder<'a> {
    t: &'a Tokens,
    text: &'a str,
    size: f32,
    bold: bool,
    color: Color32,
    job: LayoutJob,
    pieces: Vec<Piece>,
    chars: usize,
    /// The paragraph as words, for its accessible name.
    plain: String,
    footnotes: &'a mut usize,
}

/// White space as single spaces.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Builder<'_> {
    fn line_height(&self) -> f32 {
        (self.size * self.t.reading_leading).round()
    }

    fn format(&self, style: Style, color: Color32) -> TextFormat {
        let bold = style.bold || self.bold;
        let size = if style.small_caps { self.size * 0.82 } else { self.size };
        let font = if style.mono {
            FontId::new(size * 0.88, FontFamily::Monospace)
        } else if style.sans {
            FontId::new(size * 0.94, if bold { crate::fonts::semibold() } else { FontFamily::Proportional })
        } else {
            self.t.reading(size, bold, style.italic && !bold)
        };
        TextFormat {
            font_id: font,
            color,
            line_height: Some(self.line_height()),
            italics: style.italic && bold,
            underline: if style.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
            valign: Align::Center,
            ..TextFormat::default()
        }
    }

    fn append(&mut self, s: &str, format: TextFormat, range: Range<usize>, kind: PieceKind) {
        if s.is_empty() {
            return;
        }
        let start = self.chars;
        self.chars += s.chars().count();
        self.job.append(s, 0.0, format);
        self.pieces.push(Piece { chars: start..self.chars, range, kind });
    }

    fn atom_format(&self, font: FontId, color: Color32) -> TextFormat {
        TextFormat { font_id: font, color, line_height: Some(self.line_height()), valign: Align::Center, ..TextFormat::default() }
    }

    fn inline(&mut self, inline: &Inline, outer: Style) {
        let range = inline.range.clone();
        match &inline.kind {
            InlineKind::Text { text, style } => {
                let style = merge(outer, *style);
                let shown = if style.small_caps { text.to_uppercase() } else { text.clone() };
                self.plain.push_str(text);
                let format = self.format(style, self.color);
                self.append(&shown, format, range, PieceKind::Text);
            }
            InlineKind::LineBreak => {
                self.plain.push(' ');
                let format = self.format(outer, self.color);
                self.append("\n", format, range, PieceKind::Text);
            }
            InlineKind::Formula { source, .. } => {
                let math = collapse(self.text.get(source.clone()).unwrap_or_default());
                self.plain.push_str(&math);
                let font = FontId::new(self.t.code_size * self.size / self.t.reading_size * 0.95, FontFamily::Monospace);
                let format = self.atom_format(font, self.t.syntax_math);
                self.append(&format!("\u{a0}{math}\u{a0}"), format, range, PieceKind::Formula);
            }
            InlineKind::Chip { kind, command, key } => {
                self.plain.push_str(key);
                let font = FontId::new(self.size * 0.8, FontFamily::Proportional);
                let format = self.atom_format(font, self.t.text);
                // No-break spaces hold the chip's icon (after the first) and keep it on one line.
                let shown = format!("\u{a0}{}{}\u{a0}", "\u{a0}".repeat(CHIP_ICON_SLOT), key.replace(' ', "\u{a0}"));
                self.append(&shown, format, range, PieceKind::Chip(*kind, command.clone()));
            }
            InlineKind::Footnote(inner) => {
                *self.footnotes += 1;
                let font = FontId::new(self.size * 0.65, crate::fonts::semibold());
                let format = TextFormat { valign: Align::TOP, ..self.atom_format(font, self.t.accent) };
                let note = plain(self.text, inner);
                self.append(&format!("\u{200a}{}", self.footnotes), format, range, PieceKind::Footnote(note));
            }
            InlineKind::Link { url, text } => {
                let start = self.chars;
                let color = self.color;
                self.color = self.t.accent;
                let style = Style { underline: true, ..outer };
                for i in text {
                    self.inline(i, style);
                }
                self.color = color;
                self.pieces.retain(|p| p.chars.start < start);
                self.pieces.push(Piece { chars: start..self.chars, range, kind: PieceKind::Link(url.clone()) });
            }
            InlineKind::Source => {
                let src = collapse(self.text.get(range.clone()).unwrap_or_default());
                self.plain.push_str(&src);
                let font = FontId::new(self.t.code_size * self.size / self.t.reading_size * 0.95, FontFamily::Monospace);
                let format = self.atom_format(font, self.t.text_muted);
                self.append(&format!("\u{a0}{src}\u{a0}"), format, range, PieceKind::Source);
            }
        }
    }
}

/// `inner` applied inside `outer`.
fn merge(outer: Style, inner: Style) -> Style {
    Style {
        bold: outer.bold || inner.bold,
        italic: outer.italic != inner.italic && (outer.italic || inner.italic),
        underline: outer.underline || inner.underline,
        mono: outer.mono || inner.mono,
        sans: outer.sans || inner.sans,
        small_caps: outer.small_caps || inner.small_caps,
    }
}

/// The words of inline runs, for tooltips and accessible names.
fn plain(text: &str, inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text { text, .. } => out.push_str(text),
            InlineKind::LineBreak => out.push(' '),
            InlineKind::Formula { source, .. } => out.push_str(&collapse(text.get(source.clone()).unwrap_or_default())),
            InlineKind::Chip { key, .. } => out.push_str(key),
            InlineKind::Footnote(_) => {}
            InlineKind::Link { text: inner, .. } => out.push_str(&plain(text, inner)),
            InlineKind::Source => out.push_str(&collapse(text.get(i.range.clone()).unwrap_or_default())),
        }
    }
    out
}

/// The rectangles chars `s..e` of `galley` take, one per row.
fn span_rects(galley: &Galley, (s, e): (usize, usize)) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (a, z) = (s.max(row_start), e.min(row_start + n));
        if a < z {
            let x0 = row.pos.x + row.x_offset(egui::text::CharIndex(a - row_start));
            let x1 = row.pos.x + row.x_offset(egui::text::CharIndex(z - row_start));
            out.push(Rect::from_min_max(pos2(x0.min(x1), row.min_y()), pos2(x0.max(x1), row.max_y())));
        }
        row_start += row.char_count_including_newline().0;
    }
    out
}

/// The piece under `pos` (relative to the galley).
fn piece_at<'p>(galley: &Galley, pieces: &'p [Piece], pos: Vec2) -> Option<&'p Piece> {
    let at = galley.cursor_from_pos(pos).index.0;
    // The cursor sits between characters: take the one it is closest to that is under the pointer.
    let inside = |p: &&Piece| span_rects(galley, (p.chars.start, p.chars.end)).iter().any(|r| r.expand2(vec2(1.0, 0.0)).contains(pos.to_pos2()));
    pieces.iter().filter(|p| p.chars.start <= at && at <= p.chars.end).find(inside)
}

fn chip_icon(kind: ChipKind) -> &'static str {
    match kind {
        ChipKind::Label => "tag",
        ChipKind::Reference => "link",
        ChipKind::Citation => "book-open",
        ChipKind::File => "file-text",
    }
}

/// What hovering a piece explains.
fn piece_tip(kind: &PieceKind, text: &str, range: &Range<usize>) -> Option<String> {
    let key = || {
        let src = text.get(range.clone()).unwrap_or_default();
        src.split_once('{').map_or(src, |(_, k)| k.trim_end_matches('}')).to_owned()
    };
    Some(match kind {
        PieceKind::Text => return None,
        PieceKind::Chip(ChipKind::Label, _) => tl!("Label {key}").replace("{key}", &key()),
        PieceKind::Chip(ChipKind::Reference, command) => {
            tl!("Reference (\\{command}) to {key}").replace("{command}", command).replace("{key}", &key())
        }
        PieceKind::Chip(ChipKind::Citation, command) => tl!("Citation (\\{command}) of {key}").replace("{command}", command).replace("{key}", &key()),
        PieceKind::Chip(ChipKind::File, command) => tl!("File read with \\{command}: {key}").replace("{command}", command).replace("{key}", &key()),
        PieceKind::Formula => tl!("The formula's source. Typeset formulas come in a later version; double-click to edit it.").to_owned(),
        PieceKind::Source => tl!("Shown as its source: the formatted view does not show this command yet.").to_owned(),
        PieceKind::Link(url) => url.clone(),
        PieceKind::Footnote(note) => tl!("Footnote: {text}").replace("{text}", note),
    })
}

/// A laid-out paragraph with its pills, icons, tooltips, focus and jumps. Returns its response.
fn paragraph(ui: &mut egui::Ui, cx: &mut Cx<'_>, inlines: &[Inline], range: &Range<usize>, para: &Para) -> egui::Response {
    let t = cx.t;
    let width = ui.available_width();
    let mut b = Builder {
        t: &t,
        text: cx.text,
        size: para.size,
        bold: para.bold,
        color: para.color,
        job: LayoutJob::default(),
        pieces: Vec::new(),
        chars: 0,
        plain: String::new(),
        footnotes: &mut cx.footnotes,
    };
    for i in inlines {
        b.inline(i, Style::default());
    }
    let (mut job, pieces, plain_text) = (b.job, b.pieces, b.plain);
    job.wrap.max_width = width;
    job.halign = cx.align;
    let galley = ui.painter().layout_job(job);
    let line = (para.size * t.reading_leading).round();
    let (rect, r) = ui.allocate_exact_size(vec2(width, galley.size().y.max(line)), Sense::click());
    let origin = match cx.align {
        Align::Min => rect.min - galley.rect.min.to_vec2(),
        Align::Center => pos2(rect.center().x - galley.rect.center().x, rect.top() - galley.rect.top()),
        Align::Max => pos2(rect.right() - galley.rect.right(), rect.top() - galley.rect.top()),
    };
    if ui.is_rect_visible(rect) {
        paint_pieces(ui, &t, &galley, &pieces, origin, para.size);
        ui.painter().galley(origin, Arc::clone(&galley), t.text);
        paint_chip_icons(ui, &t, &galley, &pieces, origin, para.size);
    }
    let hovered = r.hover_pos().and_then(|p| piece_at(&galley, &pieces, p - origin));
    let r = match hovered.and_then(|p| piece_tip(&p.kind, cx.text, &p.range)) {
        Some(tip) => r.on_hover_text_at_pointer(tip),
        None => r,
    };
    // A double-click on a run goes to where that run's source starts.
    let target = r.double_clicked().then(|| hovered.map_or(range.start, |p| p.range.start));
    let role = para.role;
    r.widget_info(|| egui::WidgetInfo::labeled(role, true, &plain_text));
    activate(ui, cx, &r, rect, range.start, target);
    r
}

/// The focus ring, Enter and double-click of a part of the view.
fn activate(ui: &egui::Ui, cx: &mut Cx<'_>, r: &egui::Response, rect: Rect, start: usize, target: Option<usize>) {
    if cx.first.is_none() {
        cx.first = Some(r.id);
    }
    if r.has_focus() {
        cx.focused = true;
        crate::widgets::focus_ring(ui, &cx.t, r, rect.expand(cx.t.unit), cx.t.corner());
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            cx.jump = Some(Jump(start));
        }
    }
    if let Some(at) = target {
        cx.jump = Some(Jump(at));
    } else if r.double_clicked() {
        cx.jump = Some(Jump(start));
    }
}

/// The soft pills behind formulas, chips and source runs.
fn paint_pieces(ui: &egui::Ui, t: &Tokens, galley: &Galley, pieces: &[Piece], origin: Pos2, size: f32) {
    for p in pieces {
        let fill = match p.kind {
            PieceKind::Chip(..) => blend(t.surface, t.accent_soft),
            PieceKind::Formula | PieceKind::Source => blend(t.surface, t.pressed),
            PieceKind::Text | PieceKind::Link(_) | PieceKind::Footnote(_) => continue,
        };
        // Chips and formulas are outlined too, so they read as one unit on dark surfaces.
        let stroke = match p.kind {
            PieceKind::Chip(..) => Stroke::new(1.0, t.accent.gamma_multiply(0.35)),
            PieceKind::Formula => formula_outline(t),
            _ => Stroke::NONE,
        };
        for r in span_rects(galley, (p.chars.start, p.chars.end)) {
            let r = r.translate(origin.to_vec2());
            let pill = Rect::from_center_size(r.center(), vec2(r.width(), (size * 1.3).min(r.height())));
            ui.painter().rect(pill, t.corner(), fill, stroke, StrokeKind::Inside);
        }
    }
}

/// The outline of a formula's pill.
fn formula_outline(t: &Tokens) -> Stroke {
    Stroke::new(1.0, t.syntax_math.gamma_multiply(0.3))
}

/// How many no-break spaces hold a chip's icon.
const CHIP_ICON_SLOT: usize = 5;

/// The icon in each chip's leading spaces.
fn paint_chip_icons(ui: &egui::Ui, t: &Tokens, galley: &Galley, pieces: &[Piece], origin: Pos2, size: f32) {
    for p in pieces {
        let PieceKind::Chip(kind, _) = p.kind else { continue };
        if let Some(r) = span_rects(galley, (p.chars.start + 1, p.chars.start + 1 + CHIP_ICON_SLOT)).first() {
            let r = r.translate(origin.to_vec2());
            crate::icons::paint(ui, r, chip_icon(kind), (size * 0.72).round(), t.accent);
        }
    }
}

fn preamble(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>) {
    let t = cx.t;
    let height = t.row_height + 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let open = cx.preamble_open;
    let label = tl!("Preamble");
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, label));
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_expanded(open));
    let fill = if r.hovered() { blend(t.frame, t.hover) } else { t.frame };
    let corner = if open { egui::CornerRadius { nw: t.radius_large, ne: t.radius_large, sw: 0, se: 0 } } else { t.corner_large() };
    ui.painter().rect_filled(rect, corner, fill);
    ui.painter().rect_stroke(rect, corner, t.hairline(), StrokeKind::Inside);
    let chevron = Rect::from_min_size(rect.min + vec2(2.0 * t.unit, 0.0), vec2(t.icon_size, height));
    crate::icons::paint(ui, chevron, if open { "chevron-down" } else { "chevron-right" }, t.icon_size - 2.0, t.text_muted);
    let x = chevron.right() + 2.0 * t.unit;
    let title = ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, t.strong(), t.text);
    if !cx.summary.is_empty() {
        let at = pos2(title.right() + 3.0 * t.unit, rect.center().y);
        ui.painter().text(at, Align2::LEFT_CENTER, cx.summary, t.small(), t.text_muted);
    }
    let hint = if open { tl!("Hide") } else { tl!("Show") };
    ui.painter().text(pos2(rect.right() - 3.0 * t.unit, rect.center().y), Align2::RIGHT_CENTER, hint, t.small(), t.text_muted);
    if r.clicked() {
        cx.toggle_preamble = true;
    }
    if cx.first.is_none() {
        cx.first = Some(r.id);
    }
    if r.has_focus() {
        cx.focused = true;
    }
    crate::widgets::focus_ring(ui, &t, &r, rect, t.corner_large());
    let _ = r.on_hover_text(tl!("The preamble: the document's class, packages and settings. Double-click its source to edit it."));
    if open {
        let src = cx.text.get(range.clone()).unwrap_or_default().trim_end();
        let _ = code(ui, cx, src, range.start, Some(egui::CornerRadius { nw: 0, ne: 0, sw: t.radius_large, se: t.radius_large }), None);
    }
}

/// Source text in the code font with its syntax colours, in a soft frame under an optional
/// `header`; double-clicking a character jumps to it.
fn code(ui: &mut egui::Ui, cx: &mut Cx<'_>, src: &str, offset: usize, corner: Option<egui::CornerRadius>, header: Option<&str>) -> egui::Response {
    let t = cx.t;
    let pad = 3.0 * t.unit;
    let head = header.map(|h| ui.painter().layout_no_wrap(h.to_owned(), t.small(), t.text_muted));
    let head_h = head.as_ref().map_or(0.0, |g| g.size().y + 2.0 * t.unit);
    let width = ui.available_width();
    let mut job = LayoutJob::default();
    let font = FontId::new(t.code_size * 0.95, FontFamily::Monospace);
    for token in texcraft_latex::lex(src) {
        let piece = src.get(token.range.clone()).unwrap_or_default();
        job.append(piece, 0.0, TextFormat::simple(font.clone(), t.syntax(token.kind)));
    }
    job.wrap.max_width = width - 2.0 * pad;
    let galley = ui.painter().layout_job(job);
    let (rect, r) = ui.allocate_exact_size(vec2(width, head_h + galley.size().y + 2.0 * pad), Sense::click());
    let corner = corner.unwrap_or_else(|| t.corner_large());
    ui.painter().rect_filled(rect, corner, blend(t.surface, t.current_line));
    ui.painter().rect_stroke(rect, corner, t.hairline(), StrokeKind::Inside);
    if let Some(head) = head {
        let strip = Rect::from_min_size(rect.min, vec2(width, head_h));
        ui.painter().galley(pos2(strip.left() + pad, strip.center().y - head.size().y / 2.0), head, t.text_muted);
        ui.painter().hline(strip.x_range().shrink(1.0), strip.bottom(), t.hairline());
    }
    let origin = rect.min + vec2(pad, pad + head_h);
    ui.painter().galley(origin, Arc::clone(&galley), t.text);
    let target = if r.double_clicked() {
        r.interact_pointer_pos().map(|p| {
            let chars = galley.cursor_from_pos(p - origin).index.0;
            offset + src.char_indices().nth(chars).map_or(src.len(), |(b, _)| b)
        })
    } else {
        None
    };
    let label = tl!("Source: {text}").replace("{text}", src);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Code, true, &label));
    activate(ui, cx, &r, rect, offset, target);
    r
}

fn title_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, [title, author, date]: [&Vec<Inline>; 3]) {
    let t = cx.t;
    let align = std::mem::replace(&mut cx.align, Align::Center);
    ui.add_space(t.reading_size);
    if title.is_empty() && author.is_empty() {
        let tip = [Inline { kind: InlineKind::Source, range: range.clone() }];
        let _ = paragraph(ui, cx, &tip, range, &Para { size: t.reading_size, bold: false, color: t.text_muted, role: egui::Role::Paragraph });
    }
    if !title.is_empty() {
        let r = paragraph(ui, cx, title, range, &Para { size: t.heading_sizes[0], bold: true, color: t.text, role: egui::Role::Heading });
        ui.ctx().accesskit_node_builder(r.id, |node| node.set_level(1));
    }
    if !author.is_empty() {
        ui.add_space(t.reading_size * 0.75);
        let _ = paragraph(ui, cx, author, range, &Para { size: t.reading_size * 1.1, bold: false, color: t.text_muted, role: egui::Role::Paragraph });
    }
    if !date.is_empty() {
        ui.add_space(t.reading_size * 0.25);
        let _ = paragraph(ui, cx, date, range, &Para { size: t.reading_size, bold: false, color: t.text_muted, role: egui::Role::Paragraph });
    }
    ui.add_space(t.reading_size * 0.75);
    cx.align = align;
}

fn heading(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, level: u8, content: &[Inline], label: Option<&Inline>) {
    let t = cx.t;
    let size = t.heading_sizes.get(usize::from(level)).copied().unwrap_or(t.reading_size);
    let mut inlines = content.to_vec();
    if let Some(label) = label {
        inlines.push(Inline {
            kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() },
            range: label.range.start..label.range.start,
        });
        inlines.push(label.clone());
    }
    let r = paragraph(ui, cx, &inlines, range, &Para { size, bold: true, color: t.text, role: egui::Role::Heading });
    // Part and chapter are the first level, section the second, and so on.
    let aria = usize::from(level.max(1));
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_level(aria));
    ui.add_space(t.reading_size * 0.25);
}

/// A list's marker at `depth` for item `n` (from 0).
fn marker(kind: ListKind, depth: usize, n: usize) -> String {
    match kind {
        ListKind::Bullets => ["•", "◦", "▪", "·"][depth % 4].to_owned(),
        ListKind::Numbers => match depth % 4 {
            0 => format!("{}.", n + 1),
            1 => format!("({})", letters(n)),
            2 => format!("{}.", roman(n + 1)),
            _ => format!("{}.", letters(n).to_uppercase()),
        },
        ListKind::Descriptions => String::new(),
    }
}

/// `a`, `b`, … `z`, `aa`, …
fn letters(n: usize) -> String {
    let mut n = n;
    let mut out = Vec::new();
    loop {
        out.push(char::from(b'a' + (n % 26) as u8));
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.iter().rev().collect()
}

fn roman(n: usize) -> String {
    let mut n = n;
    let mut out = String::new();
    for (value, digits) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

fn list(ui: &mut egui::Ui, cx: &mut Cx<'_>, kind: ListKind, items: &[Item], depth: usize) {
    let t = cx.t;
    let indent = if kind == ListKind::Descriptions { t.reading_size * 1.25 } else { t.reading_size * 1.75 };
    let line = (t.reading_size * t.reading_leading).round();
    for (n, item) in items.iter().enumerate() {
        if n > 0 {
            ui.add_space(t.reading_size * 0.35);
        }
        let top = ui.cursor().top();
        let left = ui.cursor().left();
        let mark = marker(kind, depth, n);
        if !mark.is_empty() {
            let at = pos2(left + indent - t.reading_size * 0.55, top + line / 2.0);
            let font =
                if kind == ListKind::Numbers { t.reading(t.reading_size, false, false) } else { t.reading(t.reading_size * 1.05, true, false) };
            ui.painter().text(at, Align2::RIGHT_CENTER, mark, font, t.text_muted);
        }
        let mut blocks = item.blocks.clone();
        if let Some(label) = &item.label {
            let bold: Vec<Inline> = label
                .iter()
                .cloned()
                .map(|mut i| {
                    if let InlineKind::Text { style, .. } = &mut i.kind {
                        style.bold = true;
                    }
                    i
                })
                .collect();
            let space =
                Inline { kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() }, range: item.range.start..item.range.start };
            match blocks.first_mut() {
                Some(Block { kind: BlockKind::Paragraph(p), .. }) => {
                    let rest = std::mem::take(p);
                    *p = bold.into_iter().chain([space]).chain(rest).collect();
                }
                _ => blocks.insert(0, Block { kind: BlockKind::Paragraph(bold), range: item.range.clone() }),
            }
        }
        if blocks.is_empty() {
            blocks.push(Block { kind: BlockKind::Paragraph(Vec::new()), range: item.range.clone() });
        }
        ui.horizontal_top(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                draw_blocks(ui, cx, &blocks, depth + 1);
            });
        });
    }
}

fn container(ui: &mut egui::Ui, cx: &mut Cx<'_>, name: &str, blocks: &[Block], depth: usize) {
    let t = cx.t;
    match name {
        "abstract" => {
            let title = egui::RichText::new(tl!("Abstract").to_uppercase())
                .font(FontId::new(t.small_size, crate::fonts::semibold()))
                .extra_letter_spacing(1.2)
                .color(t.text_muted);
            ui.vertical_centered(|ui| ui.label(title));
            ui.add_space(t.unit * 2.0);
            indented(ui, cx, blocks, depth, t.reading_size * 1.5, false);
        }
        "quote" | "quotation" | "verse" => indented(ui, cx, blocks, depth, t.reading_size * 1.25, true),
        _ => {
            let align = match name {
                "center" => Align::Center,
                "flushright" => Align::Max,
                _ => Align::Min,
            };
            let outer = std::mem::replace(&mut cx.align, align);
            draw_blocks(ui, cx, blocks, depth);
            cx.align = outer;
        }
    }
}

/// Blocks inset by `inset` on both sides, with a rule on the left when `rule`.
fn indented(ui: &mut egui::Ui, cx: &mut Cx<'_>, blocks: &[Block], depth: usize, inset: f32, rule: bool) {
    let t = cx.t;
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let width = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.add_space(inset);
        ui.vertical(|ui| {
            ui.set_width(width - if rule { inset } else { 2.0 * inset });
            draw_blocks(ui, cx, blocks, depth);
        });
    });
    if rule {
        let x = left + t.unit;
        ui.painter().line_segment([pos2(x, top), pos2(x, ui.cursor().top())], Stroke::new(3.0, t.separator));
    }
}

fn formula(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, source: &Range<usize>, env: Option<&str>, closed: bool) {
    let t = cx.t;
    let math = cx.text.get(source.clone()).unwrap_or_default();
    let lines: Vec<&str> = math.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let shown = lines.join("\n");
    let pad = vec2(4.0 * t.unit, 3.0 * t.unit);
    let width = ui.available_width();
    let tag = match (env, closed) {
        (_, false) => Some(tl!("not closed").to_owned()),
        (Some(env), true) => Some(env.to_owned()),
        (None, true) => None,
    };
    let color = if closed { t.text_muted } else { t.error };
    let tag = tag.map(|tag| ui.painter().layout_no_wrap(tag, t.small(), color));
    // The environment's name sits in the margin on the right, as an equation number would.
    let margin = tag.as_ref().map_or(0.0, |g| g.size().x + 2.0 * t.unit);
    let room = width - 2.0 * margin;
    let font = FontId::new(t.code_size, FontFamily::Monospace);
    let galley = ui.painter().layout_job(LayoutJob::simple(shown.clone(), font, t.syntax_math, room - 2.0 * pad.x));
    let (rect, r) = ui.allocate_exact_size(vec2(width, galley.size().y + 2.0 * pad.y), Sense::click());
    let pill = Rect::from_center_size(rect.center(), vec2((galley.rect.width() + 2.0 * pad.x).min(room), rect.height()));
    ui.painter().rect(pill, t.corner_large(), blend(t.surface, t.pressed), formula_outline(&t), StrokeKind::Inside);
    // Lines of a multi-line formula start together, as the source aligns them.
    let origin = pos2(pill.left() + pad.x - galley.rect.left(), rect.top() + pad.y - galley.rect.top());
    ui.painter().galley(origin, galley, t.syntax_math);
    if let Some(tag) = tag {
        let at = pos2(rect.right() - tag.size().x, rect.center().y - tag.size().y / 2.0);
        ui.painter().galley(at, tag, color);
    }
    let label = tl!("Formula: {text}").replace("{text}", &shown);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Math, true, &label));
    let r = r.on_hover_text(tl!("The formula's source. Typeset formulas come in a later version; double-click to edit it."));
    activate(ui, cx, &r, pill, range.start, None);
}

/// A project file `\includegraphics` names: as written or with a figure extension, from the
/// project's root or the file's folder.
fn find_image(project: &Project, file: &ProjectPath, name: &str) -> Option<(ProjectPath, Vec<u8>)> {
    let folder = file.as_str().rsplit_once('/').map(|(f, _)| f);
    let bases = [Some(name.to_owned()), folder.map(|f| format!("{f}/{name}"))];
    for base in bases.into_iter().flatten() {
        for ext in ["", ".pdf", ".png", ".jpg", ".jpeg", ".svg"] {
            let Ok(path) = ProjectPath::new(&format!("{base}{ext}")) else { continue };
            if figure::kind(path.as_str()).is_none() {
                continue;
            }
            match project.get(&path) {
                Some(Contents::Binary(b)) => return Some((path, b.clone())),
                Some(Contents::Text(s)) => return Some((path, s.clone().into_bytes())),
                None => {}
            }
        }
    }
    None
}

/// The picture of an image, decoded once per file and length.
fn picture(ui: &egui::Ui, cx: &mut Cx<'_>, name: &str) -> Picture {
    let Some((path, bytes)) = find_image(cx.project, cx.path, name) else {
        return Err(tl!("{name} is not among the project's files.").replace("{name}", name));
    };
    let key = (path.as_str().to_owned(), bytes.len());
    if let Some(p) = cx.images.get(&key) {
        return p.clone();
    }
    let project = cx.project;
    let lookup = |href: &str| -> Option<Vec<u8>> {
        let p = ProjectPath::new(href).ok()?;
        match project.get(&p)? {
            Contents::Binary(b) => Some(b.clone()),
            Contents::Text(t) => Some(t.clone().into_bytes()),
        }
    };
    let max_side = ui.ctx().input(|i| i.max_texture_side).min(4096) as u32;
    let scale = 2.0 * ui.ctx().pixels_per_point();
    let decoded = figure::decode(path.as_str(), &bytes, 0, scale, max_side, &lookup)
        .map(|f| {
            let pixels = egui::ColorImage::from_rgba_premultiplied([f.image.width as usize, f.image.height as usize], &f.image.rgba);
            let texture = ui.ctx().load_texture(format!("formatted:{}", path.as_str()), pixels, egui::TextureOptions::LINEAR);
            (texture, vec2(f.size[0].max(1.0), f.size[1].max(1.0)))
        })
        .map_err(|e| tl!("{name} cannot be shown: {e}").replace("{name}", name).replace("{e}", &e.to_string()));
    cx.images.insert(key, decoded.clone());
    decoded
}

fn figure_card(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    range: &Range<usize>,
    image: Option<&texcraft_latex::view::Image>,
    caption: &[Inline],
    label: Option<&Inline>,
) {
    let t = cx.t;
    let width = ui.available_width();
    let pad = 4.0 * t.unit;
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    // The card's frame goes under what is drawn in it.
    let frame_shape = ui.painter().add(egui::Shape::Noop);
    ui.add_space(pad);
    let inner = width - 2.0 * pad;
    let max_h = t.reading_size * 20.0;
    let (shown, name) = match image {
        Some(img) => (picture(ui, cx, &img.path), img.path.clone()),
        None => (Err(tl!("This figure has no image.").to_owned()), String::new()),
    };
    let image_r = match shown {
        Ok((texture, natural)) => {
            let scale = (inner / natural.x).min(max_h / natural.y).min(1.5);
            let size = natural * scale;
            let (row, r) = ui.allocate_exact_size(vec2(width, size.y), Sense::click());
            let rect = Rect::from_center_size(row.center(), size);
            ui.painter().image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let label = tl!("Image {name}").replace("{name}", &name);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &label));
            r
        }
        Err(why) => {
            let (row, r) = ui.allocate_exact_size(vec2(width, t.reading_size * 7.0), Sense::click());
            let rect = row.shrink2(vec2(pad, 0.0));
            ui.painter().rect_filled(rect, t.corner(), blend(t.surface, t.pressed));
            crate::icons::paint(ui, Rect::from_center_size(rect.center() - vec2(0.0, 12.0), Vec2::splat(28.0)), "image", 24.0, t.text_muted);
            let galley = ui.painter().layout(why.clone(), t.small(), t.text_muted, rect.width() - 2.0 * pad);
            let at = pos2(rect.center().x - galley.size().x / 2.0, rect.center().y + 10.0);
            ui.painter().galley(at, galley, t.text_muted);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &why));
            r
        }
    };
    let image_start = image.map_or(range.start, |i| i.range.start);
    let image_rect = image_r.rect;
    activate(ui, cx, &image_r, image_rect, image_start, None);
    ui.add_space(3.0 * t.unit);
    let mut inlines = vec![Inline {
        kind: InlineKind::Text { text: format!("{} ", tl!("Figure.")), style: Style { bold: true, sans: true, ..Style::default() } },
        range: range.start..range.start,
    }];
    inlines.extend(caption.iter().cloned());
    if let Some(label) = label {
        inlines.push(Inline {
            kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() },
            range: label.range.start..label.range.start,
        });
        inlines.push(label.clone());
    }
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(inner);
            let _ =
                paragraph(ui, cx, &inlines, range, &Para { size: t.reading_size * 0.9, bold: false, color: t.text, role: egui::Role::FigureCaption });
        });
    });
    ui.add_space(pad);
    let card = Rect::from_min_max(pos2(left, top), pos2(left + width, ui.cursor().top()));
    ui.painter().set(
        frame_shape,
        egui::Shape::Rect(egui::epaint::RectShape::new(card, t.corner_large(), blend(t.surface, t.current_line), t.hairline(), StrokeKind::Inside)),
    );
}

fn source_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>) {
    let src = cx.text.get(range.clone()).unwrap_or_default().trim_end();
    let is_table = src.starts_with("\\begin{table") || src.starts_with("\\begin{tabular");
    let (tag, tip) = if is_table {
        (tl!("Table, shown as source"), tl!("Tables are shown as their source until the table editor comes."))
    } else {
        (tl!("Shown as source"), tl!("The formatted view cannot show this faithfully yet, so it shows its source."))
    };
    let r = code(ui, cx, src, range.start, None, Some(tag));
    let _ = r.on_hover_text(tip);
}
