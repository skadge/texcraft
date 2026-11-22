//! Interface faces (docs/design.md, Type): Inter for the interface, in regular, semibold and
//! italic, and a monospace face for source text, embedded from craft-fonts by `build.rs`. They
//! head craft-ui's font stack, which adds faces for other scripts and emoji behind them. Without
//! the craft-fonts checkout, egui's bundled faces stand in and the named families map to them.

use craft_ui::fonts::{I18nScripts, UiFonts};
use egui::{FontData, FontDefinitions, FontFamily};

include!(concat!(env!("OUT_DIR"), "/ui_faces.rs"));

/// The semibold interface face, for titles and the active tab.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// The italic interface face.
pub fn italic() -> FontFamily {
    FontFamily::Name("italic".into())
}

pub fn install(ctx: &egui::Context) {
    if UI_FACES.is_empty() {
        let mut defs = FontDefinitions::default();
        let proportional = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        defs.families.insert(semibold(), proportional.clone());
        defs.families.insert(italic(), proportional);
        ctx.set_fonts(defs);
        return;
    }
    let mut fonts = UiFonts::new(&I18nScripts);
    for (name, bytes) in UI_FACES {
        let families = match *name {
            "Inter-SemiBold" => vec![semibold()],
            "Inter-Italic" => vec![italic()],
            "Mono" => vec![FontFamily::Monospace],
            _ => vec![FontFamily::Proportional],
        };
        fonts = fonts.app_face(name, FontData::from_static(bytes), &families);
    }
    // Arrows and other symbols Inter lacks come from egui's Hack, ahead of system fonts.
    let all = [FontFamily::Proportional, semibold(), italic(), FontFamily::Monospace];
    fonts = fonts.egui_face("Hack", &all).egui_face("Ubuntu-Light", &all);
    fonts.install(ctx);
}
