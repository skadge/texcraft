//! Keyboard shortcuts through craft-keymap. The catalog is every engine command with a menu or a
//! default shortcut and every view and edit command, so a shortcut runs exactly what its menu
//! entry runs. The user's changes (made in the shared Keyboard Shortcuts dialog) are kept in the
//! preferences. While the editor has the keyboard, plain keys and the text-editing chords stay with
//! it; command chords (Ctrl+S, Ctrl+B) still run.

use craft_keymap::egui::Dispatcher;
use craft_keymap::{Active, Bindable, Binding, Keymap, Overrides, Platform, Scope, Scopes};
use texcraft_engine::cmd;

use crate::ui_cmd::UI_COMMANDS;

pub(crate) struct Keys {
    pub keymap: Keymap,
    dispatcher: Dispatcher,
    platform: Platform,
    /// The Keyboard Shortcuts dialog's state while it is open.
    pub dialog: craft_keymap::ui::DialogState,
}

fn bindable(id: &'static str, label: &'static str, menu: &[&str], shortcut: Option<&str>) -> Bindable {
    let mut path: Vec<String> = menu.iter().map(|m| (*m).to_owned()).collect();
    path.push(label.to_owned());
    let b = Bindable::new(id, label, Scope::app()).with_path(path);
    match shortcut.and_then(|s| Binding::parse(s).ok()) {
        Some(binding) => b.with_default(binding),
        None => b,
    }
}

/// Every command a shortcut can run.
fn catalog() -> Vec<Bindable> {
    let engine = cmd::all().iter().filter(|c| !c.menu.is_empty() || c.shortcut.is_some()).map(|c| bindable(c.id, c.label, c.menu, c.shortcut));
    let ui = UI_COMMANDS.iter().map(|c| bindable(c.id, c.label, &[c.menu], c.shortcut));
    engine.chain(ui).collect()
}

impl Keys {
    pub fn new(overrides: Overrides) -> Self {
        let platform = Platform::current();
        Self {
            keymap: Keymap::new(catalog(), Scopes::new(), platform).with_overrides(overrides),
            dispatcher: Dispatcher::new(),
            platform,
            dialog: craft_keymap::ui::DialogState::default(),
        }
    }

    /// The commands the frame's key presses run, removing those presses from the input.
    pub fn dispatch(&mut self, ctx: &egui::Context) -> Vec<String> {
        let typing = ctx.egui_wants_keyboard_input();
        let active = Active { panel: None, contexts: &[], typing };
        let hits = ctx.input_mut(|i| self.dispatcher.dispatch(i, &self.keymap, &active));
        hits.into_iter().map(|h| h.command.key()).collect()
    }

    /// The first shortcut of `id`, as this platform writes it.
    pub fn shortcut_text(&self, id: &str) -> Option<String> {
        self.keymap.bindings(&id.into()).first().map(|b| b.trigger.display(self.platform))
    }
}
