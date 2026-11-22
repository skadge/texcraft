//! The user's preferences: appearance, building, layout and the recent projects. The interface
//! reads and changes them; the shell stores them (the desktop app in craft-settings' `config.toml`,
//! saved atomically) and hands them over at start.

use serde::{Deserialize, Serialize};

use crate::{Layout, Side};

/// Most recent projects remembered.
pub const RECENT_MAX: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemePref,
    /// An interface language code, or `auto` for the system's.
    pub language: String,
    /// The source text's size, in points.
    pub editor_font_size: f32,
    pub auto_compile: bool,
    pub auto_compile_delay_ms: u64,
    /// `split`, `editor` or `preview`.
    pub layout: String,
    /// `files`, `outline` or empty for a hidden side panel.
    pub side: String,
    /// Project folders, most recent first.
    pub recent: Vec<String>,
    /// The user's shortcut changes, as craft-keymap stores them.
    #[serde(with = "craft_keymap::keymap::overrides_list")]
    pub shortcuts: craft_keymap::Overrides,
    /// The start page has offered to download every template, and the user answered.
    pub templates_offered: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            language: "auto".to_owned(),
            editor_font_size: 13.5,
            auto_compile: true,
            auto_compile_delay_ms: 800,
            layout: "split".to_owned(),
            side: "files".to_owned(),
            recent: Vec::new(),
            shortcuts: craft_keymap::Overrides::default(),
            templates_offered: false,
        }
    }
}

impl Prefs {
    pub(crate) fn layout(&self) -> Layout {
        match self.layout.as_str() {
            "editor" => Layout::EditorOnly,
            "preview" => Layout::PreviewOnly,
            _ => Layout::Split,
        }
    }

    pub(crate) fn side(&self) -> Option<Side> {
        match self.side.as_str() {
            "outline" => Some(Side::Outline),
            "" => None,
            _ => Some(Side::Files),
        }
    }

    pub(crate) fn set_view(&mut self, layout: Layout, side: Option<Side>) {
        self.layout = match layout {
            Layout::Split => "split",
            Layout::EditorOnly => "editor",
            Layout::PreviewOnly => "preview",
        }
        .to_owned();
        self.side = match side {
            Some(Side::Files) => "files",
            Some(Side::Outline) => "outline",
            None => "",
        }
        .to_owned();
    }
}
