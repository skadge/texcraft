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

/// `one` or `other` (English keys, each with `{n}`) for the count `n`, in the current language's
/// plural forms.
pub fn trn(n: usize, one: &'static str, other: &'static str) -> String {
    I18N.trn(CURRENT.get(), n as u64, one, other)
}

/// The shared Keyboard Shortcuts dialog's words, in the current language.
pub fn keymap_text() -> craft_keymap::ui::I18nText<'static> {
    craft_keymap::ui::I18nText { i18n: &I18N, lang: CURRENT.get() }
}

/// The languages this build can show: (code, name in that language), English first.
pub fn languages() -> Vec<(String, String)> {
    I18N.languages().into_iter().map(|l| (I18N.code(l).to_owned(), I18N.name(l).to_owned())).collect()
}

/// Shows the interface in `code` ("auto" follows the system); unknown codes keep the current one.
pub fn set_language(code: &str) {
    if code == "auto" {
        use_system_language();
    } else if let Some(lang) = I18N.from_code(code) {
        CURRENT.set(lang);
    }
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
