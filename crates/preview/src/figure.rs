//! A project's graphics as pixels, for the editor pane: what `\includegraphics` takes with retex.
//! Raster images (PNG, JPEG, GIF, WebP, TIFF, BMP, JPEG XL) go through craft-image-codecs, PDF
//! figures through the PDF renderer (their first page). SVG is drawn by the interface itself.

use std::sync::Arc;

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
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "tif" | "tiff" | "bmp" | "jxl" => Some(Kind::Raster),
        "pdf" => Some(Kind::Pdf),
        "svg" => Some(Kind::Svg),
        _ => None,
    }
}

/// A raster or PDF figure's pixels (premultiplied RGBA8), and its size in pixels (for a PDF, its
/// page in points).
pub fn decode(name: &str, bytes: &[u8]) -> Result<(Image, [f32; 2]), Error> {
    match kind(name) {
        Some(Kind::Pdf) => {
            let doc = Document::open(Arc::new(bytes.to_vec()))?;
            let [w, h] = doc.page_sizes().first().copied().ok_or(Error::NoSuchPage(0))?;
            let scale = (MAX_SIDE / w.max(h).max(1.0)).min(4.0);
            Ok((doc.render(0, scale)?, [w, h]))
        }
        _ => {
            let image = craft_image_codecs::decode(bytes).map_err(|e| Error::Image(e.to_string()))?;
            let (width, height) = (image.width(), image.height());
            let mut rgba = image.to_rgba8();
            for px in rgba.chunks_exact_mut(4) {
                let a = u16::from(px[3]);
                for c in &mut px[..3] {
                    *c = ((u16::from(*c) * a + 127) / 255) as u8;
                }
            }
            Ok((Image { width, height, rgba }, [width as f32, height as f32]))
        }
    }
}
