//! Interface translations. Strings in code are English and are the lookup keys (AGENTS.md rule
//! 16); craft-i18n maps them to the current language at render time. Command ids, document text,
//! the CLI and the agent tools never see translated text. A string without a translation shows in
//! English. Translations are added in storytold/craft-i18n (`locales/<code>/texcraft.tsv`), never
//! here.

use std::cell::Cell;
use std::sync::LazyLock;

use craft_i18n::{I18n, Lang};

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// The languages the interface offers. Right-to-left languages wait for mirrored layout.
pub const LANGUAGES: &[&str] = &["en", "fr", "de", "es", "it", "pt", "nl", "ja", "zh-Hans"];

static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let (i18n, problems) = I18n::new(CRAFT_I18N, LANGUAGES);
    for p in problems {
        log::warn!("translation catalog: {p}");
    }
    i18n
});

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// `source` in the current language.
pub fn t(source: &'static str) -> &'static str {
    I18N.t(CURRENT.get(), source)
}

/// Follows the system language.
pub fn use_system_language() {
    CURRENT.set(I18N.system_lang());
}

/// Translates an English string literal (the key).
#[macro_export]
macro_rules! tl {
    ($s:expr) => {
        $crate::i18n::t($s)
    };
}
