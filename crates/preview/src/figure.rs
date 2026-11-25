//! A project's graphics as pixels, for the editor pane: what `\includegraphics` takes with retex.
//!
//! - Raster images go through retex's own decoders (`retex_image::decode_rgba8`: PNG, JPEG, GIF,
//!   WebP, TIFF, JPEG XL).
//! - PDF figures through the PDF renderer, any page.
//! - SVG through resvg, with the images it places looked up among the project's files (the host
//!   says how, [`Lookup`]), and its text in the system's fonts and those the host adds
//!   ([`add_fonts`]: in the browser, which shows a page no installed font, the interface's own).
//!
//! PDF and SVG are drawn at the scale asked for, so a zoomed figure stays sharp.

use std::sync::{Arc, Mutex, PoisonError};

use resvg::usvg;
use retex_image::Kind as Format;

use crate::{Document, Error, Image};

/// The kind of graphic a file name says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Raster,
    Pdf,
    Svg,
}

pub fn kind(name: &str) -> Option<Kind> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "tif" | "tiff" | "jxl" => Some(Kind::Raster),
        "pdf" => Some(Kind::Pdf),
        "svg" => Some(Kind::Svg),
        _ => None,
    }
}

/// Another file's bytes, by the path an SVG names it with (relative to the SVG's folder).
pub type Lookup<'a> = &'a (dyn Fn(&str) -> Option<Vec<u8>> + Sync);

/// A figure drawn: its pixels (premultiplied RGBA8), its natural size (pixels for a raster image,
/// points for a PDF page, user units for an SVG) and how many pages it has.
#[derive(Debug, Clone)]
pub struct Figure {
    pub image: Image,
    pub size: [f32; 2],
    pub pages: usize,
}

/// Draws page `page` of a figure, at `scale` pixels per unit of its natural size (vector
/// figures; a raster image is decoded as it is), with no side longer than `max_side` pixels (a
/// larger raster image is scaled down).
pub fn decode(name: &str, bytes: &[u8], page: usize, scale: f32, max_side: u32, lookup: Lookup<'_>) -> Result<Figure, Error> {
    let max_side = max_side.max(16) as f32;
    match kind(name) {
        Some(Kind::Pdf) => pdf(bytes, page, scale, max_side),
        Some(Kind::Svg) => svg(bytes, scale, max_side, lookup),
        _ => {
            let image = raster(bytes)?;
            let size = [image.width as f32, image.height as f32];
            Ok(Figure { image: shrink(image, max_side as u32), size, pages: 1 })
        }
    }
}

/// A scale at most `scale` that keeps the larger side within `max_side`.
fn bounded(scale: f32, [w, h]: [f32; 2], max_side: f32) -> f32 {
    scale.min((max_side - 1.0) / w.max(h).max(1.0)).max(0.001)
}

/// `image` with no side longer than `max_side`: blocks of pixels averaged.
fn shrink(image: Image, max_side: u32) -> Image {
    let factor = image.width.max(image.height).div_ceil(max_side.max(1)) as usize;
    if factor <= 1 {
        return image;
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let (nw, nh) = (w.div_ceil(factor), h.div_ceil(factor));
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for yy in y * factor..((y + 1) * factor).min(h) {
                for xx in x * factor..((x + 1) * factor).min(w) {
                    let i = (yy * w + xx) * 4;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(image.rgba[i + c]);
                    }
                    n += 1;
                }
            }
            out.extend(sum.iter().map(|s| (s / n.max(1)) as u8));
        }
    }
    Image { width: nw as u32, height: nh as u32, rgba: out }
}

fn pdf(bytes: &[u8], page: usize, scale: f32, max_side: f32) -> Result<Figure, Error> {
    let doc = Document::open(Arc::new(bytes.to_vec()))?;
    let sizes = doc.page_sizes();
    let size = *sizes.get(page).ok_or(Error::NoSuchPage(page))?;
    let image = doc.render(page, bounded(scale, size, max_side))?;
    Ok(Figure { image, size, pages: sizes.len() })
}

/// A font file's bytes, shared (as egui's and fontdb's are).
pub type FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// The fonts of SVG text: the system's, loaded when first needed, and the host's.
#[derive(Default)]
struct Fonts {
    db: Option<Arc<usvg::fontdb::Database>>,
    added: Vec<FontBytes>,
}

static FONTS: Mutex<Fonts> = Mutex::new(Fonts { db: None, added: Vec::new() });

/// The fonts SVG text is drawn with.
fn fonts() -> Arc<usvg::fontdb::Database> {
    let mut fonts = FONTS.lock().unwrap_or_else(PoisonError::into_inner);
    let added = fonts.added.clone();
    fonts.db.get_or_insert_with(|| Arc::new(database(true, added))).clone()
}

/// Adds font files to those SVG text is drawn with: in the browser, the only ones there are.
/// The generic families (`sans-serif`, `serif`, `monospace`) go to faces among them when the
/// system has none of the usual ones.
pub fn add_fonts(files: impl IntoIterator<Item = FontBytes>) {
    let mut fonts = FONTS.lock().unwrap_or_else(PoisonError::into_inner);
    let files: Vec<FontBytes> = files.into_iter().collect();
    fonts.added.extend(files.iter().cloned());
    // Once loaded, the database takes them too; otherwise it is made with them when first needed.
    if let Some(db) = fonts.db.as_mut() {
        let db = Arc::make_mut(db);
        for file in files {
            db.load_font_source(usvg::fontdb::Source::Binary(file));
        }
        generic_families(db);
    }
}

/// A font database of the system's fonts (when `system`) and `files`.
fn database(system: bool, files: Vec<FontBytes>) -> usvg::fontdb::Database {
    let mut db = usvg::fontdb::Database::new();
    if system {
        db.load_system_fonts();
    }
    for file in files {
        db.load_font_source(usvg::fontdb::Source::Binary(file));
    }
    generic_families(&mut db);
    db
}

/// Faces for the generic families, first found: the usual ones of each system, then the
/// interface's and the TeX trees'.
const SANS: &[&str] = &["Arial", "Helvetica", "Liberation Sans", "DejaVu Sans", "Noto Sans", "Inter", "Roboto", "Ubuntu", "TeX Gyre Heros"];
const SERIF: &[&str] = &["Times New Roman", "Times", "Liberation Serif", "DejaVu Serif", "Noto Serif", "Source Serif 4", "TeX Gyre Termes"];
const MONO: &[&str] = &["Courier New", "Liberation Mono", "DejaVu Sans Mono", "Noto Sans Mono", "Hack", "TeX Gyre Cursor"];

fn has_family(db: &usvg::fontdb::Database, name: &str) -> bool {
    db.faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
}

/// Points each generic family at a face the database has, when the one it names is missing (the
/// browser has none of them). Text in a family with no face at all goes to the sans-serif one.
fn generic_families(db: &mut usvg::fontdb::Database) {
    use usvg::fontdb::Family;
    let any = db.faces().find_map(|f| f.families.first().map(|(n, _)| n.clone()));
    let pick = |db: &usvg::fontdb::Database, generic: Family<'_>, list: &[&str]| -> Option<String> {
        let current = db.family_name(&generic).to_owned();
        if has_family(db, &current) {
            return None;
        }
        list.iter().find(|n| has_family(db, n)).map(|n| (*n).to_owned())
    };
    if let Some(name) = pick(db, Family::SansSerif, SANS) {
        db.set_sans_serif_family(name);
    }
    let sans = Some(db.family_name(&Family::SansSerif).to_owned()).filter(|n| has_family(db, n)).or(any);
    let Some(sans) = sans else { return };
    for (generic, list) in [(Family::Serif, SERIF), (Family::Monospace, MONO), (Family::Cursive, &[][..]), (Family::Fantasy, &[][..])] {
        let name = pick(db, generic, list).or_else(|| (!has_family(db, db.family_name(&generic))).then(|| sans.clone()));
        match (generic, name) {
            (Family::Serif, Some(n)) => db.set_serif_family(n),
            (Family::Monospace, Some(n)) => db.set_monospace_family(n),
            (Family::Cursive, Some(n)) => db.set_cursive_family(n),
            (Family::Fantasy, Some(n)) => db.set_fantasy_family(n),
            _ => {}
        }
    }
    if !has_family(db, db.family_name(&Family::SansSerif)) {
        db.set_sans_serif_family(sans);
    }
}

/// The generic family a font name most likely belongs to, for a family the database lacks.
fn likely_generic(name: &str) -> usvg::fontdb::Family<'static> {
    use usvg::fontdb::Family;
    let n = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["mono", "courier", "code", "consol", "menlo", "typewriter", "cursor"]) {
        Family::Monospace
    } else if !n.contains("sans") && has(&["serif", "times", "roman", "georgia", "garamond", "palatino", "book", "termes", "pagella", "schola"]) {
        Family::Serif
    } else {
        Family::SansSerif
    }
}

/// usvg's font choice, except that a family the database lacks stands for the generic family its
/// name suggests (a missing Roboto is drawn sans-serif, not in the serif face usvg falls back to).
fn font_resolver() -> usvg::FontResolver<'static> {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    let select: usvg::FontSelectionFn<'static> = Box::new(|font, db| {
        let mut families = Vec::new();
        for family in font.families() {
            match family {
                usvg::FontFamily::Serif => families.push(Family::Serif),
                usvg::FontFamily::SansSerif => families.push(Family::SansSerif),
                usvg::FontFamily::Cursive => families.push(Family::Cursive),
                usvg::FontFamily::Fantasy => families.push(Family::Fantasy),
                usvg::FontFamily::Monospace => families.push(Family::Monospace),
                usvg::FontFamily::Named(name) if has_family(db, name) => families.push(Family::Name(name)),
                usvg::FontFamily::Named(name) => families.push(likely_generic(name)),
            }
        }
        families.push(Family::SansSerif);
        let stretch = match font.stretch() {
            usvg::FontStretch::UltraCondensed => Stretch::UltraCondensed,
            usvg::FontStretch::ExtraCondensed => Stretch::ExtraCondensed,
            usvg::FontStretch::Condensed => Stretch::Condensed,
            usvg::FontStretch::SemiCondensed => Stretch::SemiCondensed,
            usvg::FontStretch::Normal => Stretch::Normal,
            usvg::FontStretch::SemiExpanded => Stretch::SemiExpanded,
            usvg::FontStretch::Expanded => Stretch::Expanded,
            usvg::FontStretch::ExtraExpanded => Stretch::ExtraExpanded,
            usvg::FontStretch::UltraExpanded => Stretch::UltraExpanded,
        };
        let style = match font.style() {
            usvg::FontStyle::Normal => Style::Normal,
            usvg::FontStyle::Italic => Style::Italic,
            usvg::FontStyle::Oblique => Style::Oblique,
        };
        db.query(&Query { families: &families, weight: Weight(font.weight()), stretch, style })
    });
    usvg::FontResolver { select_font: select, select_fallback: usvg::FontResolver::default_fallback_selector() }
}

fn svg(bytes: &[u8], scale: f32, max_side: f32, lookup: Lookup<'_>) -> Result<Figure, Error> {
    svg_with(bytes, scale, max_side, lookup, fonts())
}

fn svg_with(bytes: &[u8], scale: f32, max_side: f32, lookup: Lookup<'_>, fontdb: Arc<usvg::fontdb::Database>) -> Result<Figure, Error> {
    let bad = |e: &dyn std::fmt::Display| Error::Image(e.to_string());
    let mut options = usvg::Options { fontdb, font_resolver: font_resolver(), ..usvg::Options::default() };
    options.image_href_resolver.resolve_string = Box::new(move |href: &str, _: &usvg::Options| {
        let data = Arc::new(lookup(href)?);
        match retex_image::kind_of(&data)? {
            Format::Png => Some(usvg::ImageKind::PNG(data)),
            Format::Jpeg => Some(usvg::ImageKind::JPEG(data)),
            Format::Gif => Some(usvg::ImageKind::GIF(data)),
            Format::Webp => Some(usvg::ImageKind::WEBP(data)),
            _ => None,
        }
    });
    let tree = retex_image::contained(|| usvg::Tree::from_data(bytes, &options))
        .ok_or_else(|| Error::Image("the file is damaged".to_owned()))?
        .map_err(|e| bad(&e))?;
    let size = [tree.size().width(), tree.size().height()];
    let scale = bounded(scale, size, max_side);
    let (w, h) = ((size[0] * scale).ceil().max(1.0) as u32, (size[1] * scale).ceil().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| Error::Image("the drawing has no size".to_owned()))?;
    retex_image::contained(|| resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut()))
        .ok_or_else(|| Error::Image("the drawing could not be rendered".to_owned()))?;
    // tiny-skia's pixels are premultiplied RGBA8, as the preview's are.
    Ok(Figure { image: Image { width: w, height: h, rgba: pixmap.take() }, size, pages: 1 })
}

fn raster(bytes: &[u8]) -> Result<Image, Error> {
    // The decoders may panic on a damaged file: that is an error in the file.
    let decoded = retex_image::contained(|| retex_image::decode_rgba8(bytes)).ok_or_else(|| Error::Image("the file is damaged".to_owned()))?;
    let mut r = decoded.map_err(|e| Error::Image(e.to_string()))?;
    if r.data.len() < (r.width as usize) * (r.height as usize) * 4 {
        return Err(Error::Image("the image is shorter than its size".to_owned()));
    }
    // Straight alpha to the premultiplied pixels the preview draws.
    for px in r.data.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        if a < 255 {
            for c in &mut px[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
    }
    Ok(Image { width: r.width, height: r.height, rgba: r.data })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A label in a font the browser does not have (our own drawing, issue #5).
    const LABEL: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="60">
<rect width="240" height="60" fill="white"/>
<text x="10" y="42" font-family="Roboto, Arial" font-size="32" fill="black">Label</text>
<text x="150" y="42" font-family="sans-serif" font-size="32" fill="black">Ab</text>
</svg>"#;

    /// Dark pixels in the left part (the named family's label) and the right (the generic one's).
    fn ink(fonts: usvg::fontdb::Database) -> (usize, usize) {
        let figure = svg_with(LABEL, 1.0, 1024.0, &|_| None, Arc::new(fonts)).unwrap();
        let w = figure.image.width as usize;
        let mut ink = (0, 0);
        for (i, px) in figure.image.rgba.chunks_exact(4).enumerate() {
            if px[0] < 128 {
                if i % w < 140 { ink.0 += 1 } else { ink.1 += 1 }
            }
        }
        ink
    }

    #[test]
    fn svg_text_is_drawn_with_the_fonts_the_host_adds_when_the_system_has_none() {
        // The browser: no system fonts. Without any, the text is left out.
        assert_eq!(ink(database(false, Vec::new())), (0, 0));
        let hack: FontBytes = Arc::new(epaint_default_fonts::HACK_REGULAR);
        let (named, generic) = ink(database(false, vec![hack]));
        assert!(named > 100, "a family the browser lacks drew no text");
        assert!(generic > 50, "sans-serif drew no text");
    }
}
