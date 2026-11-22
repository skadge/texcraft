//! Text keeps 4.5:1 against the surfaces it is drawn on, outlines 3:1 (standards/accessibility.md).

use texcraft_ui_egui::theme::{Tokens, contrast};

#[test]
fn text_and_outlines_keep_their_contrast_in_both_themes() {
    for t in [Tokens::dark(), Tokens::light()] {
        for surface in [t.panel, t.input, t.background] {
            for (name, fg) in [
                ("text", t.text),
                ("text_dim", t.text_dim),
                ("error", t.error),
                ("warning", t.warning),
                ("command", t.syntax_command),
                ("comment", t.syntax_comment),
                ("group", t.syntax_group),
                ("math", t.syntax_math),
                ("environment", t.syntax_environment),
            ] {
                assert!(contrast(fg, surface) >= 4.5, "{:?}: {name} on {surface:?} is {:.2}:1", t.scheme, contrast(fg, surface));
            }
            assert!(contrast(t.border, surface) >= 3.0, "{:?}: border on {surface:?}", t.scheme);
        }
        assert!(contrast(t.on_accent, t.accent) >= 4.5, "{:?}: text on the accent", t.scheme);
    }
}
