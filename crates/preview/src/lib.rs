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

    /// Page `index` (from 0) at `scale` device pixels per point, on white.
    pub fn render(&self, index: usize, scale: f32) -> Result<Image, Error> {
        let pages = self.pdf.pages();
        let page = pages.get(index).ok_or(Error::NoSuchPage(index))?;
        let options = Options { scale: [scale, scale], background: [255, 255, 255, 255], ..Options::default() };
        let raster = raster::render(page, &RenderCache::new(), &Default::default(), &options).map_err(|e| Error::Render(e.to_string()))?;
        Ok(Image { width: raster.width, height: raster.height, rgba: raster.bytes().to_vec() })
    }
}
