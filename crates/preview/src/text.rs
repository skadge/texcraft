//! The text of a page, glyph by glyph, for selecting and copying it: each glyph's box on the page
//! and the characters it stands for (from the font's `ToUnicode` map or its glyph names), in the
//! order the page draws them. TeX draws a line's glyphs left to right and the lines top to
//! bottom, so that order is the reading order for the PDFs TeXCraft builds; the spaces between
//! words are not drawn, so they are put back from the gaps.

use craft_pdf::interpret::font::Glyph as PdfGlyph;
use craft_pdf::interpret::{BlendMode, ClipPath, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask};
use craft_pdf::syntax::Pdf;
use kurbo::{Affine, BezPath, Point, Rect};

/// At most this many glyphs are read from a page.
const MAX_GLYPHS: usize = 200_000;

/// A glyph drawn on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// Left, top, right, bottom, in points from the page's top left corner as displayed: the
    /// glyph's advance across, its em (from descender to ascender) up and down.
    pub rect: [f32; 4],
    /// What it reads as: a character, several for a ligature, empty when the font does not say.
    pub text: String,
}

impl Glyph {
    fn height(&self) -> f32 {
        self.rect[3] - self.rect[1]
    }

    /// How far `(x, y)` is from the glyph's box (0 inside); a line away counts more than a
    /// glyph away, so a point between lines takes the nearer line.
    fn distance(&self, x: f32, y: f32) -> f32 {
        let [l, t, r, b] = self.rect;
        let dx = (l - x).max(x - r).max(0.0);
        let dy = (t - y).max(y - b).max(0.0);
        dx + 4.0 * dy
    }
}

/// The glyphs of page `index` of `pdf`, in the order drawn.
pub(crate) fn glyphs(pdf: &Pdf, index: usize) -> Result<Vec<Glyph>, String> {
    let pages = pdf.pages();
    let Some(page) = pages.get(index) else { return Ok(Vec::new()) };
    let (width, height) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let initial = page.initial_transform(true).as_coeffs();
    let mut context = craft_pdf::interpret::Context::new(
        Affine::new(initial),
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        &cache,
        pdf.xref(),
        InterpreterSettings::default(),
    );
    let mut device = Collector::default();
    craft_pdf::interpret_page(page, &mut context, &mut device).map_err(|e| e.to_string())?;
    Ok(device.glyphs)
}

/// The glyph nearest to `(x, y)` (points from the page's top left), if the page has any.
pub fn glyph_at(glyphs: &[Glyph], x: f32, y: f32) -> Option<usize> {
    glyphs.iter().enumerate().min_by(|(_, a), (_, b)| a.distance(x, y).total_cmp(&b.distance(x, y))).map(|(i, _)| i)
}

/// The text of glyphs `from` to `to` (both included, in either order): a space where a gap
/// between two glyphs is wider than a sixth of an em, a line break where the next glyph is on
/// another line.
pub fn text_between(glyphs: &[Glyph], from: usize, to: usize) -> String {
    let (a, b) = (from.min(to), from.max(to));
    let Some(run) = glyphs.get(a..=b.min(glyphs.len().saturating_sub(1))) else { return String::new() };
    let mut out = String::new();
    let mut last: Option<&Glyph> = None;
    for g in run {
        if let Some(p) = last {
            let size = p.height().max(g.height()).max(1.0);
            let middle = (g.rect[1] + g.rect[3]) / 2.0;
            let new_line = middle < p.rect[1] || middle > p.rect[3];
            if new_line {
                out.push('\n');
            } else if g.rect[0] - p.rect[2] > size / 6.0 && !out.ends_with(char::is_whitespace) && !g.text.starts_with(char::is_whitespace) {
                out.push(' ');
            }
        }
        out.push_str(&g.text);
        last = Some(g);
    }
    out
}

#[derive(Default)]
struct Collector {
    glyphs: Vec<Glyph>,
    /// Inside the page's annotations, whose text is not the page's.
    annotations: bool,
}

impl<'a> Device<'a> for Collector {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, _: &BezPath, _: Affine, _: &Paint<'a>, _: &PathDrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph(&mut self, glyph: &PdfGlyph<'a>, transform: Affine, glyph_transform: Affine, _: &Paint<'a>, _: &GlyphDrawMode) {
        if self.annotations || self.glyphs.len() >= MAX_GLYPHS {
            return;
        }
        let advance = match glyph {
            PdfGlyph::Outline(g) => g.advance_width(),
            PdfGlyph::Type3(g) => g.advance_width(),
        }
        .filter(|a| a.is_finite() && *a > 0.0)
        .unwrap_or(500.0);
        // The em box in glyph space (1000 units an em), from the baseline's origin.
        let to_page = transform * glyph_transform;
        let corners =
            [(0.0, -200.0), (f64::from(advance), -200.0), (0.0, 800.0), (f64::from(advance), 800.0)].map(|(x, y)| to_page * Point::new(x, y));
        let (mut l, mut t, mut r, mut b) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in corners {
            l = l.min(p.x);
            t = t.min(p.y);
            r = r.max(p.x);
            b = b.max(p.y);
        }
        if ![l, t, r, b].iter().all(|v| v.is_finite()) {
            return;
        }
        let text = match glyph.as_unicode() {
            Some(craft_pdf::interpret::hayro_cmap::BfString::Char(c)) => c.to_string(),
            Some(craft_pdf::interpret::hayro_cmap::BfString::String(s)) => s,
            None => String::new(),
        };
        self.glyphs.push(Glyph { rect: [l as f32, t as f32, r as f32, b as f32], text });
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: Affine) {}
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
    fn begin_annotations(&mut self) {
        self.annotations = true;
    }
    fn end_annotations(&mut self) {
        self.annotations = false;
    }
}
