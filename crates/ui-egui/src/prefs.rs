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

/// The source text's size by default, and its bounds, in points.
pub const TEXT_SIZE_DEFAULT: f32 = 13.5;
pub const TEXT_SIZE_MIN: f32 = 10.0;
pub const TEXT_SIZE_MAX: f32 = 22.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemePref,
    /// An interface language code, or `auto` for the system's.
    pub language: String,
    /// The source text's size, in points, from [`TEXT_SIZE_MIN`] to [`TEXT_SIZE_MAX`]; the
    /// formatted view's text follows it in proportion.
    pub editor_font_size: f32,
    pub auto_compile: bool,
    pub auto_compile_delay_ms: u64,
    /// The project's changes are written shortly after typing stops, and versions made by
    /// themselves (`project.autosave`).
    pub autosave: bool,
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
    /// The GitLab and GitHub Enterprise servers added for signing in: address, service and
    /// application id (sign-ins are in the keychain, never here).
    pub forge_servers: Vec<texcraft_engine::forge::servers::Server>,
    /// Spell checking: on, the language, comments.
    pub spelling: texcraft_engine::SpellingOptions,
    /// Grammar checking: on, the rules and groups switched off.
    pub grammar: texcraft_engine::GrammarOptions,
    /// The user's words (Add to Dictionary), for every project.
    pub words: Vec<String>,
    /// The symbols inserted last, by command, the latest first.
    pub recent_symbols: Vec<String>,
    /// After a pause in typing, the model's next words are shown after the caret (AI3).
    pub ai_complete: bool,
    /// The instructions last given to Ask AI to Change This, the latest first.
    pub ai_instructions: Vec<String>,
    /// The start page shows a tip of the day.
    pub show_tips: bool,
    /// The PDF's pages are dimmed in the dark theme (Settings › Appearance).
    pub dim_pdf: bool,
    /// The first-steps hint above the editor was dismissed (it shows with the first project).
    pub first_steps_seen: bool,
    /// The kinds of build message hidden in every project (Hide Messages Like This).
    pub hidden_messages: Vec<String>,
    /// Where the browser build downloads missing TeX Live packages (Settings › Building).
    pub package_server: texcraft_engine::packages::PackageServer,
    /// What the desktop build does about missing TeX Live packages (Settings › Building).
    pub missing_packages: texcraft_engine::missing::MissingPackages,
    /// The folder Import a Project makes its clones and unzipped projects in, once one was
    /// chosen with Change… (the system's documents folder until then).
    pub clone_folder: Option<String>,
    /// The git servers whose credential is kept in the keychain (Remember on this computer), by
    /// host; the credentials themselves are in the keychain, never here.
    pub git_hosts: Vec<String>,
    /// The browser keeps sign-ins and tokens for every tab and later visits (its storage for the
    /// site), not only for the tab (Settings › Accounts › Remember sign-ins in this browser). On
    /// by default; the desktop keeps them in the keychain whatever this says.
    pub remember_in_browser: bool,
    /// The default connection (Settings › Accounts › Set as Default): a forge's address, such as
    /// `https://gitlab.com`. Import a Project and Save Project to… start from it.
    pub default_connection: Option<String>,
    /// The place New Project's "Where to Keep It" chose last: `local`, or a connection's address.
    pub new_project_place: Option<String>,
    /// The daily update check, and when it last answered.
    pub updates: texcraft_engine::update::Saved,
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
            editor_font_size: TEXT_SIZE_DEFAULT,
            auto_compile: true,
            auto_compile_delay_ms: 800,
            autosave: true,
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
            forge_servers: Vec::new(),
            spelling: texcraft_engine::SpellingOptions::default(),
            grammar: texcraft_engine::GrammarOptions::default(),
            hidden_messages: Vec::new(),
            package_server: Default::default(),
            missing_packages: Default::default(),
            words: Vec::new(),
            recent_symbols: Vec::new(),
            ai_complete: false,
            ai_instructions: Vec::new(),
            show_tips: true,
            dim_pdf: false,
            first_steps_seen: false,
            clone_folder: None,
            git_hosts: Vec::new(),
            remember_in_browser: true,
            default_connection: None,
            new_project_place: None,
            updates: texcraft_engine::update::Saved::default(),
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
