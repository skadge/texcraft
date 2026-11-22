//! The interface icons: a subset of Lucide (ISC AND MIT, `assets/icons/`, ATTRIBUTION.md), drawn
//! with a 1.75 px stroke and tinted at draw time with a theme token (docs/design.md, Space and
//! shape).

use std::sync::OnceLock;

use craft_ui::icons::{SvgIconSet, SvgStyle};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

fn set() -> &'static SvgIconSet {
    static SET: OnceLock<SvgIconSet> = OnceLock::new();
    SET.get_or_init(|| SvgIconSet::new(ICONS, SvgStyle { current_color: "white", stroke_width: Some("1.75") }, "bytes://texcraft/icons/", "file"))
}

/// Installs the SVG loader the icons need; once per context.
pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// Icon `name` at `size` points, tinted `color`.
pub fn image(name: &str, size: f32, color: egui::Color32) -> egui::Image<'static> {
    egui::Image::new(set().source(name)).fit_to_exact_size(egui::vec2(size, size)).tint(color)
}

/// Paints icon `name` centred in `rect`.
pub fn paint(ui: &egui::Ui, rect: egui::Rect, name: &str, size: f32, color: egui::Color32) {
    let target = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
    image(name, size, color).paint_at(ui, target);
}
