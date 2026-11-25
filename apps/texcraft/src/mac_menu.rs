//! The macOS menu bar: TeXCraft's menus ([`texcraft_ui_egui::App::system_menu_layout`]) in the
//! system's menu bar, through craft-menu's AppKit backend. Everything else (the menus, running a
//! chosen entry, keeping them in step) is platform-free, in texcraft-ui-egui.

use texcraft_ui_egui::App;

/// Puts the menus in the system's menu bar and hands its driver to `app`, which hides the
/// in-window menu bar. When AppKit refuses them, the in-window menus stay.
pub(crate) fn install(mut app: App, ctx: &egui::Context) -> App {
    let layout = app.system_menu_layout();
    for c in &layout.clashes {
        log::info!("menu: {} is {} on macOS and {} in TeXCraft; {}", c.shortcut, c.system, c.command.id, c.resolution);
    }
    match craft_menu::mac::install(ctx, &layout.bar) {
        Some(menu) => app.set_system_menu(menu),
        None => log::warn!("the system menu bar could not be installed; the menus stay in the window"),
    }
    app
}
