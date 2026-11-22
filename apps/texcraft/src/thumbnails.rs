//! The start page's thumbnails on disk: `<data folder>/TeXCraft/thumbnails/<hash>.rgba`, one per
//! project folder. A file is `TCTH`, the width and height as little-endian u32, then premultiplied
//! RGBA8. Reading checks the sizes, so a damaged or foreign file is ignored.

use std::path::PathBuf;

use texcraft_ui_egui::Thumbnail;

const MAGIC: &[u8; 4] = b"TCTH";
const MAX_SIDE: u32 = 2048;

fn dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("TeXCraft").join("thumbnails"))
}

/// FNV-1a of the project's folder, as the file name.
fn file(project: &str) -> Option<PathBuf> {
    let hash = project.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    Some(dir()?.join(format!("{hash:016x}.rgba")))
}

pub fn load(project: &str) -> Option<Thumbnail> {
    let bytes = std::fs::read(file(project)?).ok()?;
    let (head, rgba) = bytes.split_at_checked(12)?;
    if &head[..4] != MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(head[4..8].try_into().ok()?);
    let height = u32::from_le_bytes(head[8..12].try_into().ok()?);
    let expected = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || rgba.len() != expected {
        return None;
    }
    Some(Thumbnail { project: project.to_owned(), width, height, rgba: rgba.to_vec() })
}

pub fn store(thumbnail: &Thumbnail) {
    let Some(path) = file(&thumbnail.project) else { return };
    let mut bytes = Vec::with_capacity(12 + thumbnail.rgba.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&thumbnail.width.to_le_bytes());
    bytes.extend_from_slice(&thumbnail.height.to_le_bytes());
    bytes.extend_from_slice(&thumbnail.rgba);
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, bytes));
    if let Err(e) = written {
        log::warn!("saving the thumbnail {}: {e}", path.display());
    }
}
