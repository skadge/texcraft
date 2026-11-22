//! A project's graphics as pixels, for the editor pane: what `\includegraphics` takes with retex.
//! Raster images go through retex's own decoders (`retex-image`: PNG, GIF, WebP, TIFF, JPEG XL),
//! JPEG through the decoder the PDF renderer uses (`zune-jpeg`; retex embeds JPEG as it is and
//! has none), PDF figures through the PDF renderer (their first page). SVG is drawn by the
//! interface itself.

use std::sync::Arc;

use retex_image::Kind as Format;

use crate::{Document, Error, Image};

/// The largest side a figure is drawn at, in pixels.
pub const MAX_SIDE: f32 = 2400.0;

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

/// A raster or PDF figure's pixels (premultiplied RGBA8), and its size in pixels (for a PDF, its
/// page in points).
pub fn decode(name: &str, bytes: &[u8]) -> Result<(Image, [f32; 2]), Error> {
    if kind(name) == Some(Kind::Pdf) {
        let doc = Document::open(Arc::new(bytes.to_vec()))?;
        let [w, h] = doc.page_sizes().first().copied().ok_or(Error::NoSuchPage(0))?;
        let scale = (MAX_SIDE / w.max(h).max(1.0)).min(4.0);
        return Ok((doc.render(0, scale)?, [w, h]));
    }
    let bad = |e: &dyn std::fmt::Display| Error::Image(e.to_string());
    // The decoders may panic on a damaged file: that is an error in the file.
    let image = retex_image::contained(|| match retex_image::kind_of(bytes) {
        Some(Format::Jpeg) => jpeg(bytes),
        Some(Format::Png) => retex_image::png::decode(bytes).map_err(|e| bad(&e)).and_then(|r| rgba(&r)),
        Some(f @ (Format::Gif | Format::Webp | Format::Tiff | Format::Jxl)) => {
            retex_image::raster::decode(f, bytes).map_err(|e| bad(&e)).and_then(|r| rgba(&r))
        }
        _ => Err(Error::Image("not an image retex can include".to_owned())),
    })
    .ok_or_else(|| Error::Image("the file is damaged".to_owned()))??;
    let size = [image.width as f32, image.height as f32];
    Ok((image, size))
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
