//! The menus in the system's menu bar (macOS, through craft-menu): the same File, Edit, Insert and
//! Help menus as the in-window bar ([`crate::ui_cmd::MENUS`]), built from the engine registry and
//! the view commands, each entry running the same command id ([`crate::ui_cmd::run_with`]), with
//! its shortcut from the keymap (the user's changes included) and its enabled state. The
//! platform-free part is here, so it is built and tested everywhere; the desktop shell installs
//! the AppKit backend (`craft_menu::mac::install`) and hands the driver to [`App::set_system_menu`].
//! While the system holds the menus, the in-window menu bar is hidden.

use craft_menu::{CommandRef, Gate, Item, ItemRole, Layout, LayoutOptions, Menu, MenuBar, MenuRole, NativeMenu, Node};
use serde_json::json;
use texcraft_engine::cmd;

use crate::App;
use crate::i18n::t;
use crate::ui_cmd::{self, MENUS, UI_COMMANDS};

/// The id of the disabled entry that stands for an empty Open Recent; it runs nothing.
const NO_RECENT: &str = "project.no_recent";

/// The role of the in-window menu titled `title` (English) in the system's menu bar.
fn role(title: &str) -> MenuRole {
    match title {
        "File" => MenuRole::File,
        "Edit" => MenuRole::Edit,
        "Help" => MenuRole::Help,
        _ => MenuRole::Other,
    }
}

impl App {
    /// The menus as a tree, in the in-window bar's order and groups, each entry with its command,
    /// its translated label, its shortcut and whether it can run now. About and Settings are
    /// tagged for the macOS app menu; quitting is the layout's own Quit, which asks the window to
    /// close, so unsaved changes are asked about first.
    pub fn menu_tree(&self) -> MenuBar {
        let mut menus = Vec::with_capacity(MENUS.len());
        for title in MENUS {
            let mut children: Vec<Node> = Vec::new();
            for spec in cmd::all().iter().filter(|c| c.menu.first() == Some(title)) {
                let label = crate::forge::label(self, spec.id).unwrap_or_else(|| t(spec.label));
                children.push(Item::new(spec.id, label).enabled((spec.enabled)(&self.session).is_ok()).into());
            }
            if *title == "File" {
                children.push(Node::Separator);
                children.push(self.recent_submenu());
            }
            let mut last_group = "";
            for c in UI_COMMANDS.iter().filter(|c| c.menu == *title) {
                if !last_group.is_empty() && c.group != last_group {
                    children.push(Node::Separator);
                }
                if *title == "Edit" && c.group == "settings" && last_group != "settings" && crate::ai::offered(self) {
                    children.push(self.rewrite_submenu());
                    children.push(Node::Separator);
                }
                last_group = c.group;
                children.push(Item::new(c.id, t(c.label)).enabled(ui_cmd::enabled(self, c.id).is_ok()).into());
            }
            menus.push(Menu::new(t(title), craft_menu::tidy(children)).role(role(title)));
        }
        let mut bar = MenuBar { menus, documents: Vec::new() };
        bar.tag_item(&"help.about".into(), ItemRole::About);
        bar.tag_item(&"app.settings".into(), ItemRole::Settings);
        craft_menu::apply_keymap(&mut bar, &self.keys.keymap);
        bar
    }

    /// Open Recent: the recent projects by folder name, the open one checked and the ones gone
    /// from disk disabled, then Clear Recent Projects; a disabled placeholder when there are none.
    fn recent_submenu(&self) -> Node {
        if self.prefs.recent.is_empty() {
            return Node::submenu(t("Open Recent"), vec![Item::new(NO_RECENT, t("No recent projects")).enabled(false).into()]);
        }
        let current = self.session.dir().map(|d| d.display().to_string());
        let mut children: Vec<Node> = self
            .prefs
            .recent
            .iter()
            .map(|path| {
                let name = std::path::Path::new(path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                let open = CommandRef::with_params("project.open", json!({ "path": path }));
                let mut item = Item::new(open, name).enabled(self.project_exists(path));
                if current.as_deref() == Some(path.as_str()) {
                    item = item.check(true);
                }
                item.into()
            })
            .collect();
        children.push(Node::Separator);
        children.push(Item::new("project.clear_recent", t("Clear Recent Projects")).into());
        Node::submenu(t("Open Recent"), children)
    }

    /// Rewrite with AI: each way of rewriting the selection, then Translate to… its languages,
    /// each entry `edit.rewrite` with its parameters.
    fn rewrite_submenu(&self) -> Node {
        let enabled = crate::ai::can_rewrite(self).is_ok();
        let mut children: Vec<Node> = crate::ai::REWRITES
            .iter()
            .map(|(how, label)| Item::new(CommandRef::with_params("edit.rewrite", json!({"how": how})), t(label)).enabled(enabled).into())
            .collect();
        let languages = crate::ai::LANGUAGES
            .iter()
            .map(|l| Item::new(CommandRef::with_params("edit.rewrite", json!({"how": "translate", "language": l})), t(l)).enabled(enabled).into())
            .collect();
        children.push(Node::submenu(t("Translate to…"), languages));
        Node::submenu(t("Rewrite with AI"), children)
    }

    /// [`App::menu_tree`] laid out for the macOS menu bar: the app menu (About, Settings, Quit),
    /// then File, Edit, Insert, Window and Help, with the labels the layout adds translated.
    pub fn system_menu_layout(&self) -> Layout {
        let label = |english: &'static str| t(english).to_owned();
        let options = LayoutOptions { label: &label, ..LayoutOptions::new("TeXCraft") };
        craft_menu::mac_layout(&self.menu_tree(), &options)
    }

    /// The system holds the menus from now on, through `menu` (from `craft_menu::mac::install`):
    /// the in-window menu bar is hidden, and each frame runs the entries chosen there.
    pub fn set_system_menu(&mut self, menu: NativeMenu) {
        self.system_menu = Some(menu);
    }

    /// Whether the menus are in the system's menu bar rather than the window.
    pub fn has_system_menu(&self) -> bool {
        self.system_menu.is_some()
    }

    /// Before egui reads the frame's input (`eframe::App::raw_input_hook`): the system menu's key
    /// equivalents become the key presses they were, so the keymap runs them as without it.
    pub fn system_menu_input(&mut self, raw: &mut egui::RawInput) {
        if let Some(menu) = &mut self.system_menu {
            menu.raw_input(raw);
        }
    }

    /// Runs the entries chosen in the system's menus since the last frame, as the in-window menus
    /// would: nothing runs behind a dialog, the gallery or a presentation.
    pub(crate) fn system_menu_clicks(&mut self, ctx: &egui::Context) {
        let Some(menu) = &mut self.system_menu else { return };
        menu.set_gate(Gate {
            modal: self.view.dialog.is_some() || self.gallery.open || self.pdf.presenting().is_some(),
            recording_shortcut: self.keys.dialog.is_recording(),
            text_editing: ctx.egui_wants_keyboard_input(),
        });
        for command in menu.take_clicks(ctx) {
            if command.id != NO_RECENT {
                ui_cmd::run_with(self, &command.id, &command.params);
            }
        }
    }

    /// Brings the system's menus in step with this frame: labels, shortcuts, enabled and checked.
    /// The layout is built each frame and compared with the last, so no change is missed (a
    /// build finishing, a file saved); egui draws frames only on input or when asked.
    pub(crate) fn sync_system_menu(&mut self) {
        let Some(mut menu) = self.system_menu.take() else { return };
        for c in menu.sync(self.system_menu_layout()) {
            log::info!("menu: {} is {} on macOS and {} in TeXCraft; {}", c.shortcut, c.system, c.command.id, c.resolution);
        }
        self.system_menu = Some(menu);
    }
}
