//! What the tests share.

use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

/// The window over `s`, not compiling by itself. With the default settings a project compiles
/// once it has been open for the auto-compile delay, which a few frames of a test reach (each is a
/// quarter second): the build runs on a thread, and until it ends its spinner asks for frame after
/// frame, so `Harness::run` gives up when the machine is busy and the thread late. A test of
/// building turns compiling on or compiles itself.
pub(crate) fn app(s: Session) -> App {
    App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() })
}

/// The text runs of the formatted view labelled `view` (each of its lines, in a paragraph, an
/// item or a cell of it): their node, rect on screen and text.
pub(crate) fn view_lines(h: &egui_kittest::Harness<'_, App>, view: &str) -> Vec<(egui::accesskit::NodeId, egui::Rect, String)> {
    use egui_kittest::kittest::{NodeT, Queryable, by};
    h.query_all(by().role(egui::accesskit::Role::TextRun))
        .filter(|n| {
            let mut up = n.accesskit_node().parent();
            while let Some(p) = up {
                if p.label().is_some_and(|l| l == view) {
                    return true;
                }
                up = p.parent();
            }
            false
        })
        .map(|n| (n.accesskit_node().locate().0, n.rect(), n.accesskit_node().value().unwrap_or_default()))
        .collect()
}

/// The caret of the formatted view labelled `view`: its line's rect and text, and where in it.
pub(crate) fn view_caret(h: &egui_kittest::Harness<'_, App>, view: &str) -> Option<(egui::Rect, String, usize)> {
    use egui_kittest::kittest::{NodeT, Queryable};
    let node = h.query_by_label(view)?;
    let sel = node.accesskit_node().data().text_selection()?;
    let (_, rect, value) = view_lines(h, view).into_iter().find(|(id, ..)| *id == sel.focus.node)?;
    Some((rect, value, sel.focus.character_index))
}
