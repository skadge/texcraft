//! Text keeps 4.5:1 against the surfaces it is drawn on; control outlines and the accent keep
//! 3:1 (standards/accessibility.md, docs/design.md).

use texcraft_ui_egui::theme::{Tokens, blend, contrast};

#[test]
fn text_and_controls_keep_their_contrast_in_both_themes() {
    for t in [Tokens::dark(), Tokens::light()] {
        // Rows and buttons are drawn on hovered and selected backgrounds too.
        let surfaces = [t.frame, t.surface, t.raised, blend(t.frame, t.hover), blend(t.frame, t.accent_soft), blend(t.surface, t.current_line)];
        for surface in surfaces {
            for (name, fg) in [("text", t.text), ("text_muted", t.text_muted), ("error", t.error), ("warning", t.warning), ("success", t.success)] {
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
