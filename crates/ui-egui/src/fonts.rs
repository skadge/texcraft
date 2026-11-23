//! Interface faces (docs/design.md, Type): Inter for the interface, in regular, semibold and
//! italic, a monospace face for source text, and Source Serif 4 for reading in the formatted view,
//! embedded from craft-fonts by `build.rs`. They
//! head craft-ui's font stack, which adds faces for other scripts and emoji behind them, and on the
//! desktop the installed fonts last. Without the craft-fonts checkout, egui's bundled faces stand in.

use craft_ui::fonts::{Family, I18nScripts, UiFonts};
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

/// The formatted view's text face.
pub fn reading() -> FontFamily {
    FontFamily::Name("reading".into())
}

/// The formatted view's italic face.
pub fn reading_italic() -> FontFamily {
    FontFamily::Name("reading-italic".into())
}

/// The formatted view's semibold face, for headings and bold text.
pub fn reading_semibold() -> FontFamily {
    FontFamily::Name("reading-semibold".into())
}

pub fn install(ctx: &egui::Context) {
    // The named families are declared, so they exist with egui's faces when this build has none of
    // craft-fonts' (CI, a checkout without them): craft-ui sets up only the families it is told of.
    let mut fonts = UiFonts::new(&I18nScripts)
        .family(Family::named("semibold").weight(600))
        .family(Family::named("italic"))
        .family(Family::named("reading"))
        .family(Family::named("reading-italic"))
        .family(Family::named("reading-semibold").weight(600));
    let all = [FontFamily::Proportional, semibold(), italic(), FontFamily::Monospace, reading(), reading_italic(), reading_semibold()];
    if UI_FACES.is_empty() {
        // egui's bundled faces stand in for craft-fonts' (docs/design.md, Type).
        let text = [FontFamily::Proportional, semibold(), italic(), reading(), reading_italic(), reading_semibold()];
        fonts = fonts.egui_face("Ubuntu-Light", &text).egui_face("Hack", &[FontFamily::Monospace]);
    }
    for (name, bytes) in UI_FACES {
        let families = match *name {
            "Inter-SemiBold" => vec![semibold()],
            "Inter-Italic" => vec![italic()],
            "Mono" => vec![FontFamily::Monospace],
            "Serif" => vec![reading()],
            "Serif-Italic" => vec![reading_italic()],
            "Serif-Semibold" => vec![reading_semibold()],
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
