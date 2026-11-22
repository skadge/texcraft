//! A project's graphics as pixels, for the editor pane: what `\includegraphics` takes with retex.
//!
//! - Raster images go through retex's own decoders (`retex-image`: PNG, GIF, WebP, TIFF, JPEG XL),
//!   JPEG through the decoder the PDF renderer uses (`zune-jpeg`; retex embeds JPEG as it is).
//! - PDF figures through the PDF renderer, any page.
//! - SVG through resvg, with the images it places looked up among the project's files (the host
//!   says how, [`Lookup`]) and the system's fonts.
//!
//! PDF and SVG are drawn at the scale asked for, so a zoomed figure stays sharp.

use std::sync::{Arc, OnceLock};

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

/// The system's fonts, loaded once, for text in SVG figures.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn svg(bytes: &[u8], scale: f32, max_side: f32, lookup: Lookup<'_>) -> Result<Figure, Error> {
    let bad = |e: &dyn std::fmt::Display| Error::Image(e.to_string());
    let mut options = usvg::Options { fontdb: fonts(), ..usvg::Options::default() };
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
    let bad = |e: &dyn std::fmt::Display| Error::Image(e.to_string());
    // The decoders may panic on a damaged file: that is an error in the file.
    retex_image::contained(|| match retex_image::kind_of(bytes) {
        Some(Format::Jpeg) => jpeg(bytes),
        Some(Format::Png) => retex_image::png::decode(bytes).map_err(|e| bad(&e)).and_then(|r| rgba(&r)),
        Some(f @ (Format::Gif | Format::Webp | Format::Tiff | Format::Jxl)) => {
            retex_image::raster::decode(f, bytes).map_err(|e| bad(&e)).and_then(|r| rgba(&r))
        }
        _ => Err(Error::Image("not an image retex can include".to_owned())),
    })
    .ok_or_else(|| Error::Image("the file is damaged".to_owned()))?
}

fn jpeg(bytes: &[u8]) -> Result<Image, Error> {
    let options = zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(bytes), options);
    let rgba = decoder.decode().map_err(|e| Error::Image(format!("{e:?}")))?;
    let (width, height) = decoder.dimensions().ok_or_else(|| Error::Image("no size".to_owned()))?;
    Ok(Image { width: width as u32, height: height as u32, rgba })
}

/// A decoded raster (grey, RGB or CMYK, 8 or 16 bits, alpha apart) as premultiplied RGBA8.
fn rgba(r: &retex_image::png::Raster) -> Result<Image, Error> {
    let n = (r.width as usize) * (r.height as usize);
    let comps = usize::from(r.components);
    let bytes = if r.bits == 16 { 2 } else { 1 };
    let sample = |data: &[u8], i: usize| -> u8 { data.get(i * bytes).copied().unwrap_or(0) };
    if r.data.len() < n * comps * bytes {
        return Err(Error::Image("the image is shorter than its size".to_owned()));
    }
    let mut out = Vec::with_capacity(n * 4);
    for p in 0..n {
        let at = p * comps;
        let (red, green, blue) = match comps {
            1 => {
                let g = sample(&r.data, at);
                (g, g, g)
            }
            4 => {
                // CMYK, ink amounts: no ink is white.
                let ink = |i: usize| u16::from(255 - sample(&r.data, at + i));
                let k = ink(3);
                ((ink(0) * k / 255) as u8, (ink(1) * k / 255) as u8, (ink(2) * k / 255) as u8)
            }
            _ => (sample(&r.data, at), sample(&r.data, at + 1), sample(&r.data, at + 2)),
        };
        let a = r.alpha.as_ref().map_or(255, |alpha| sample(alpha, p));
        let pre = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        out.extend_from_slice(&[pre(red), pre(green), pre(blue), a]);
    }
    Ok(Image { width: r.width, height: r.height, rgba: out })
}
