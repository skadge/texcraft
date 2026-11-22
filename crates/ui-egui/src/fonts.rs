//! Interface faces (docs/design.md, Type): Inter for the interface, in regular, semibold and
//! italic, and a monospace face for source text, embedded from craft-fonts by `build.rs`. They
//! head craft-ui's font stack, which adds faces for other scripts and emoji behind them, and on the
//! desktop the installed fonts last. Without the craft-fonts checkout, egui's bundled faces stand in.

use craft_ui::fonts::{I18nScripts, UiFonts};
use egui::{FontData, FontFamily};

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
    let mut fonts = UiFonts::new(&I18nScripts);
    let all = [FontFamily::Proportional, semibold(), italic(), FontFamily::Monospace];
    if UI_FACES.is_empty() {
        // egui's bundled faces stand in for craft-fonts' (docs/design.md, Type).
        let text = [FontFamily::Proportional, semibold(), italic()];
        fonts = fonts.egui_face("Ubuntu-Light", &text).egui_face("Hack", &[FontFamily::Monospace]);
    }
    for (name, bytes) in UI_FACES {
        let families = match *name {
            "Inter-SemiBold" => vec![semibold()],
            "Inter-Italic" => vec![italic()],
            "Mono" => vec![FontFamily::Monospace],
            _ => vec![FontFamily::Proportional],
        };
        fonts = fonts.app_face(name, FontData::from_static(bytes), &families);
    }
    // Arrows and other symbols come from egui's Hack, emoji (which documents contain, also in
    // source text) from egui's emoji faces; installed fonts follow on the desktop, for what none
    // of these draw.
    fonts = fonts.egui_face("Hack", &all).egui_face("Ubuntu-Light", &all).egui_face("NotoEmoji-Regular", &all).egui_face("emoji-icon-font", &all);
    fonts.install(ctx);
}
