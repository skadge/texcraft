//! Text keeps 4.5:1 against the surfaces it is drawn on; control outlines and the accent keep
//! 3:1 (standards/accessibility.md, docs/design.md).

use texcraft_ui_egui::theme::{Tokens, blend, contrast};

#[test]
fn text_and_controls_keep_their_contrast_in_both_themes() {
    for t in [Tokens::dark(), Tokens::light()] {
        // Rows and buttons are drawn on hovered and selected backgrounds too.
        let surfaces = [t.frame, t.surface, t.raised, blend(t.frame, t.hover), blend(t.frame, t.accent_soft), blend(t.surface, t.current_line)];
        for surface in surfaces {
            for (name, fg) in [
                ("text", t.text),
                ("text_muted", t.text_muted),
                ("accent_muted", t.accent_muted),
                ("error", t.error),
                ("warning", t.warning),
                ("success", t.success),
            ] {
                assert!(contrast(fg, surface) >= 4.5, "{:?}: {name} on {surface:?} is {:.2}:1", t.scheme, contrast(fg, surface));
            }
            for (name, fg) in [("control_border", t.control_border), ("accent", t.accent), ("spelling", t.spelling), ("grammar", t.grammar)] {
                assert!(contrast(fg, surface) >= 3.0, "{:?}: {name} on {surface:?} is {:.2}:1", t.scheme, contrast(fg, surface));
            }
        }
        // Formulas in the formatted view sit on a pressed surface.
        let code = [t.surface, blend(t.surface, t.current_line), blend(t.surface, t.pressed)];
        for surface in code {
            for (name, fg) in [
                ("command", t.syntax_command),
                ("comment", t.syntax_comment),
                ("group", t.syntax_group),
                ("math", t.syntax_math),
                ("environment", t.syntax_environment),
                ("special", t.syntax_special),
            ] {
                assert!(contrast(fg, surface) >= 4.5, "{:?}: {name} is {:.2}:1", t.scheme, contrast(fg, surface));
            }
        }
        assert!(contrast(t.on_accent, t.accent) >= 4.5, "{:?}: text on the accent", t.scheme);
    }
}

/// A tone by its name, and how to take it from the tokens.
type Tone = (&'static str, fn(&Tokens) -> egui::Color32);

/// A state's badge (Settings › Accounts) is read on its tint, and the tint is seen in the dark
/// theme as well as in the light one (it was so faint there that only the words told it).
#[test]
fn badge_tints_keep_their_text_readable_and_show_in_both_themes() {
    let (dark, light) = (Tokens::dark(), Tokens::light());
    for t in [dark, light] {
        for (name, tone) in [("accent", t.accent), ("success", t.success), ("warning", t.warning), ("error", t.error), ("muted", t.text_muted)] {
            let tint = t.tint(tone);
            assert!(contrast(t.text, tint) >= 4.5, "{:?}: text on the {name} tint is {:.2}:1", t.scheme, contrast(t.text, tint));
        }
    }
    // How far the tint is from the surface, as a colour: a dark surface's tint must move as far
    // as a white one's to be seen as one (its luminance contrast is no measure of a tint's hue).
    let apart = |a: egui::Color32, b: egui::Color32| {
        let d = |x: u8, y: u8| (f32::from(x) - f32::from(y)).powi(2);
        (d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b())).sqrt()
    };
    let tones: [Tone; 3] = [("accent", |t| t.accent), ("success", |t| t.success), ("warning", |t| t.warning)];
    for (name, tone) in tones {
        let seen = |t: &Tokens| apart(t.tint(tone(t)), t.surface);
        assert!(
            seen(&dark) >= seen(&light),
            "the {name} tint is fainter in the dark theme ({:.0}) than in the light ({:.0})",
            seen(&dark),
            seen(&light)
        );
    }
}
