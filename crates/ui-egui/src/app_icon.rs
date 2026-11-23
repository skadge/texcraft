//! The app icon (assets/app-icon, rendered from texcraft.svg) as a texture: the mark in the top
//! bar, and the icon on the start page and in About.

use std::io::Cursor;

/// The 128 px icon; drawn at 24 to 64 points, so it stays sharp on high-density screens.
const PNG: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/io.gitlab.severin_lemaignan.TeXCraft.png");

fn decode() -> Option<egui::ColorImage> {
    let mut reader = png::Decoder::new(Cursor::new(PNG)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let size = [info.width as usize, info.height as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, buf.get(..info.buffer_size())?))
}

/// The icon's texture, made on first use.
pub fn texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("texcraft-app-icon");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(handle);
    }
    let handle = ctx.load_texture("texcraft-app-icon", decode()?, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    Some(handle)
}

/// Paints the icon in `rect`; nothing when it cannot be decoded.
pub fn paint(ui: &egui::Ui, rect: egui::Rect) {
    if let Some(texture) = texture(ui.ctx()) {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(texture.id(), rect, uv, egui::Color32::WHITE);
    }
}
