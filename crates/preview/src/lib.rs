//! The PDF a build wrote, as pixels for the preview pane. Rendering goes through craft-pdf's
//! bounded rasteriser (premultiplied RGBA8, at most `craft_pdf::raster::MAX_SIDE` pixels a side),
//! which builds for wasm32 too, so the desktop and the web preview are drawn by the same code.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;

use craft_pdf::raster::{self, Options, RenderCache};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the PDF could not be read: {0}")]
    Open(String),
    #[error("the PDF has no page {0}")]
    NoSuchPage(usize),
    #[error("the page could not be drawn: {0}")]
    Render(String),
    #[error("the image could not be read: {0}")]
    Image(String),
}

pub mod figure;
mod links;
mod text;

/// Whether a link's URI may be opened: web and mail addresses only (craft-pdf's check).
pub use craft_pdf::safe_url;
pub use links::{Link, Target};
pub use text::{Glyph, glyph_at, text_between};

/// A page drawn at some scale.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8, row by row.
    pub rgba: Vec<u8>,
}

/// An opened PDF.
pub struct Document {
    pdf: craft_pdf::syntax::Pdf,
}

impl Document {
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Self, Error> {
        craft_pdf::open(bytes, None).map(|pdf| Self { pdf }).map_err(|e| Error::Open(format!("{e:?}")))
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Page sizes in PDF points (1/72 in), as displayed (after rotation).
    pub fn page_sizes(&self) -> Vec<[f32; 2]> {
        self.pdf
            .pages()
            .iter()
            .map(|p| {
                let (w, h) = p.render_dimensions();
                [w, h]
            })
            .collect()
    }

    /// The links of page `index` (from 0): where each is on the page, where it goes.
    pub fn links(&self, index: usize) -> Vec<Link> {
        links::links(&self.pdf, index)
    }

    /// The glyphs of page `index` (from 0), in the order drawn, for selecting its text.
    pub fn glyphs(&self, index: usize) -> Result<Vec<Glyph>, Error> {
        text::glyphs(&self.pdf, index).map_err(Error::Render)
    }

    /// Page `index` (from 0) at `scale` device pixels per point, on white.
    pub fn render(&self, index: usize, scale: f32) -> Result<Image, Error> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(Error::NoSuchPage(index))?;
        let options = Options { scale: [scale, scale], background: [255, 255, 255, 255], ..Options::default() };
        let raster = raster::render(page, &RenderCache::new(), &Default::default(), &options).map_err(|e| Error::Render(e.to_string()))?;
        Ok(Image { width: raster.width, height: raster.height, rgba: raster.bytes().to_vec() })
    }

    /// Page `index` at `scale` as ink: white, its alpha how much the page is covered (a
    /// premultiplied mask to draw in the colour of the text around it, on any background). The
    /// page's own colours are lost.
    pub fn render_ink(&self, index: usize, scale: f32) -> Result<Image, Error> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(Error::NoSuchPage(index))?;
        let options = Options { scale: [scale, scale], background: [0, 0, 0, 0], ..Options::default() };
        let raster = raster::render(page, &RenderCache::new(), &Default::default(), &options).map_err(|e| Error::Render(e.to_string()))?;
        let mut rgba = raster.bytes().to_vec();
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3];
            px.copy_from_slice(&[a, a, a, a]);
        }
        Ok(Image { width: raster.width, height: raster.height, rgba })
    }
}
