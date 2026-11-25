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
    /// `files`, `outline`, `review` or empty for a hidden side panel.
    pub side: String,
    /// Project folders, most recent first.
    pub recent: Vec<String>,
    /// The user's shortcut changes, as craft-keymap stores them.
    #[serde(with = "craft_keymap::keymap::overrides_list")]
    pub shortcuts: craft_keymap::Overrides,
    /// The editor's key bindings are Vim's.
    pub vim: bool,
    /// A wide editor shows the open file's comments in a margin beside the text.
    pub review_margin: bool,
    /// The editor closes brackets and environments as they are typed.
    pub auto_close: bool,
    /// The desktop window has the system's title bar, not the window controls in the top bar.
    pub system_title_bar: bool,
    /// The engine, stopping at the first error, draft images.
    pub build: texcraft_engine::BuildOptions,
    /// Who the user's comments are from; made on the first comment.
    pub identity: Option<Identity>,
    /// The AI connection (its token is in the keychain, never here), model and agreements.
    pub ai: texcraft_engine::ai::Saved,
    /// Spell checking: on, the language, comments.
    pub spelling: texcraft_engine::SpellingOptions,
    /// The user's words (Add to Dictionary), for every project.
    pub words: Vec<String>,
    /// The symbols inserted last, by command, the latest first.
    pub recent_symbols: Vec<String>,
    /// After a pause in typing, the model's next words are shown after the caret (AI3).
    pub ai_complete: bool,
}

/// The name and key a person's comments carry (docs/collaboration-roadmap.md, R1). No accounts:
/// the key pair is made on first use. The secret key is kept here, in the preferences file, until
/// the system's key store is used (docs/review-format.md).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub name: String,
    /// The ed25519 public key, in hex.
    pub key: String,
    /// The ed25519 secret key, in hex.
    pub secret: String,
}

impl Identity {
    /// A new identity named `name`, with a new key pair.
    pub fn new(name: &str) -> Result<Self, String> {
        let (key, secret) = texcraft_engine::new_key_pair()?;
        Ok(Self { name: name.trim().to_owned(), key, secret })
    }

    pub fn author(&self) -> texcraft_engine::review::Author {
        texcraft_engine::review::Author::new(self.name.clone(), self.key.clone())
    }
}

/// The name the system knows the user by, to start the identity's name from.
pub fn system_user_name() -> String {
    ["USER", "USERNAME", "LOGNAME"].iter().find_map(|v| std::env::var(v).ok().filter(|n| !n.trim().is_empty())).unwrap_or_default()
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
            vim: false,
            review_margin: true,
            auto_close: true,
            system_title_bar: false,
            build: texcraft_engine::BuildOptions::default(),
            identity: None,
            ai: texcraft_engine::ai::Saved::default(),
            spelling: texcraft_engine::SpellingOptions::default(),
            words: Vec::new(),
            recent_symbols: Vec::new(),
            ai_complete: false,
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
            "review" => Some(Side::Review),
            "search" => Some(Side::Search),
            "history" => Some(Side::History),
            "symbols" => Some(Side::Symbols),
            "ask" => Some(Side::Ask),
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
            Some(Side::Review) => "review",
            Some(Side::Search) => "search",
            Some(Side::History) => "history",
            Some(Side::Symbols) => "symbols",
            Some(Side::Ask) => "ask",
            None => "",
        }
        .to_owned();
    }
}
