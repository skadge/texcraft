//! `cargo xtask icons`: every app icon file, rendered from `assets/app-icon/texcraft.svg`.
//!
//! The SVG is the master (it places the owner's drawing, `source/blackbird.png`, on TeXCraft's
//! tile). Windows and Linux use the tile edge to edge; macOS icons put it on Apple's grid (the tile
//! is 824/1024 of a transparent canvas). The outputs are committed, so building and packaging need
//! no rendering.

use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};

const ID: &str = "ai.storyteller.texcraft";
const TILE_VIEWBOX: &str = r#"viewBox="0 0 512 512""#;
/// The tile at 824/1024 of the canvas: 512 / (824/1024) = 636.272, centred.
const MAC_VIEWBOX: &str = r#"viewBox="-62.136 -62.136 636.272 636.272""#;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/app-icon")
}

/// The icon at `size`: drawn at a whole multiple of the size of at least 1024 pixels and averaged
/// down, so the engraving's hatching becomes tone rather than speckles at small sizes.
fn render(svg: &str, size: u32, resources: &Path) -> Result<Vec<u8>, String> {
    let k = 1024u32.div_ceil(size).max(1);
    let big = draw(svg, size * k, resources)?;
    let small = if k == 1 { big } else { box_down(&big, k)? };
    small.encode_png().map_err(|e| e.to_string())
}

/// Averages each `k`×`k` block of premultiplied pixels into one.
fn box_down(big: &tiny_skia::Pixmap, k: u32) -> Result<tiny_skia::Pixmap, String> {
    let size = big.width() / k;
    let mut out = tiny_skia::Pixmap::new(size, size).ok_or("empty icon size")?;
    let src = big.data();
    let stride = big.width() as usize * 4;
    let dst = out.data_mut();
    let n = k * k;
    for y in 0..size as usize {
        for x in 0..size as usize {
            let mut sum = [0u32; 4];
            for dy in 0..k as usize {
                let row = (y * k as usize + dy) * stride + x * k as usize * 4;
                for px in src[row..row + k as usize * 4].chunks_exact(4) {
                    for c in 0..4 {
                        sum[c] += u32::from(px[c]);
                    }
                }
            }
            let at = (y * size as usize + x) * 4;
            for c in 0..4 {
                dst[at + c] = u8::try_from((sum[c] + n / 2) / n).unwrap_or(u8::MAX);
            }
        }
    }
    Ok(out)
}

fn draw(svg: &str, size: u32, resources: &Path) -> Result<tiny_skia::Pixmap, String> {
    let options = usvg::Options { resources_dir: Some(resources.to_path_buf()), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| format!("texcraft.svg: {e}"))?;
    // An <image> that cannot be loaded is dropped silently; a tile without the drawing is an error.
    if !has_image(tree.root()) {
        return Err("texcraft.svg: the drawing (source/blackbird.png) was not loaded".to_owned());
    }
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("empty icon size")?;
    let scale = size as f32 / tree.size().width().max(1.0);
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap)
}

fn has_image(group: &usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        usvg::Node::Image(_) => true,
        usvg::Node::Group(g) => has_image(g),
        _ => false,
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A Windows icon file holding PNG images (allowed since Windows Vista).
fn ico(images: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let count = u16::try_from(images.len()).map_err(|_| "too many icon sizes")?;
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(count);
    for (size, png) in images {
        let side = if *size >= 256 { 0 } else { u8::try_from(*size).map_err(|_| "icon size")? };
        let len = u32::try_from(png.len()).map_err(|_| "icon too large")?;
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += len;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    Ok(out)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn run() -> Result<(), String> {
    let dir = dir();
    let master = std::fs::read_to_string(dir.join("texcraft.svg")).map_err(|e| format!("texcraft.svg: {e}"))?;
    if !master.contains(TILE_VIEWBOX) {
        return Err(format!("texcraft.svg must keep {TILE_VIEWBOX}"));
    }
    let mac = master.replace(TILE_VIEWBOX, MAC_VIEWBOX);

    write(&dir.join("texcraft-1024.png"), &render(&master, 1024, &dir)?)?;
    // The window and Dock icon on macOS (embedded by apps/texcraft).
    write(&dir.join("texcraft-macos-512.png"), &render(&mac, 512, &dir)?)?;
    // The Linux icon theme; its 256 is also the window icon on Windows and Linux, its 64 the mark
    // in the top bar.
    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
        write(&dir.join(format!("hicolor/{size}x{size}/apps/{ID}.png")), &render(&master, size, &dir)?)?;
    }
    // The theme's scalable icon is installed alone, so the drawing goes inside it.
    let drawing = std::fs::read(dir.join("source/blackbird.png")).map_err(|e| format!("source/blackbird.png: {e}"))?;
    let embedded = master.replace("\"source/blackbird.png\"", &format!("\"data:image/png;base64,{}\"", base64(&drawing)));
    write(&dir.join(format!("hicolor/scalable/apps/{ID}.svg")), embedded.as_bytes())?;
    let mut images = Vec::new();
    for size in [16, 20, 24, 32, 40, 48, 64, 128, 256] {
        images.push((size, render(&master, size, &dir)?));
    }
    write(&dir.join("texcraft.ico"), &ico(&images)?)?;
    eprintln!("icons written to {}", dir.display());
    Ok(())
}
