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
