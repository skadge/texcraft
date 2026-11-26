//! Interface translations. Strings in code are English and are the lookup keys (AGENTS.md rule
//! 16); the catalogs map them to the current language at render time. Command ids, document text,
//! the CLI and the agent tools never see translated text. A string without a translation shows in
//! English.
//!
//! Until TeXCraft's cutover to storytold/craft-i18n, its catalogs are `i18n/<locale>.tsv` in this
//! repository, in craft-i18n's catalog format (build.rs embeds them, with craft-i18n's shared
//! catalogs when `CRAFT_I18N_DIR` is set). `cargo xtask i18n-report` lists the English each one
//! lacks.
//!
//! - [`tl!`] a plain string; [`tf!`] a template with `{name}` placeholders, filled in one pass;
//!   [`trn`] a count (`@plural` rows); [`tid`] a command's label by its id (`@id` rows); [`tn!`]
//!   marks a string in a table that is translated where it is shown.
//! - [`msg`] an engine message: the engine and its crates speak English to the CLI and agents, and
//!   the interface shows their messages through the catalogs' `@msg` rows, exact or as templates.
//! - [`ago`], [`date`], [`time`] and [`file_size`] write times and sizes for the region.
//! - The pseudo locale (debug builds) marks every string `⟦…⟧` and pads it by 40 %, to show text
//!   that bypasses translation and layouts that clip longer languages.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex, PoisonError};

use craft_i18n::{I18n, Lang};

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// A language the interface offers: its code (lowercase BCP 47, the catalog's file name) and its
/// name in itself.
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

/// The languages the interface offers. English is British English, the source text itself.
/// Right-to-left languages wait for mirrored layout.
pub const LANGUAGES: &[Language] = &[
    Language { code: "en", name: "English" },
    Language { code: "de", name: "Deutsch" },
    Language { code: "es", name: "Español" },
    Language { code: "fr", name: "Français" },
    Language { code: "it", name: "Italiano" },
    Language { code: "nl", name: "Nederlands" },
    Language { code: "pt", name: "Português" },
];

/// The code of the pseudo locale (debug builds only).
pub const PSEUDO: &str = "pseudo";

static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let offered: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
    let (i18n, problems) = I18n::new(CRAFT_I18N, &offered);
    for p in problems {
        log::warn!("translation catalog: {p}");
    }
    i18n
});

/// The `@msg` templates of each embedded locale, by code, read on first use.
static TEMPLATES: LazyLock<HashMap<String, Vec<Template>>> = LazyLock::new(|| templates(CRAFT_I18N));

/// Padded pseudo-locale strings by source text, leaked so lookups can hand out `&'static str`
/// (debug builds and tests only; one per distinct string).
static PADDED: LazyLock<Mutex<BTreeMap<String, &'static str>>> = LazyLock::new(Mutex::default);

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

fn pseudo() -> bool {
    CURRENT.get() == Lang::PSEUDO
}

/// `text` as the pseudo locale shows it: between ⟦ and ⟧, padded by 40 % (at least one) with `~`,
/// so a layout that fits English but not a longer language shows it.
pub fn pad(text: &str) -> String {
    let extra = (text.chars().count() * 2).div_ceil(5).max(1);
    format!("⟦{text}{}⟧", "~".repeat(extra))
}

fn padded(source: &str) -> &'static str {
    let mut cache = PADDED.lock().unwrap_or_else(PoisonError::into_inner);
    cache.entry(source.to_owned()).or_insert_with(|| Box::leak(pad(source).into_boxed_str()))
}

/// `source` in the current language.
pub fn t(source: &'static str) -> &'static str {
    if pseudo() {
        return padded(source);
    }
    I18N.t(CURRENT.get(), source)
}

/// `source` with a `context` that tells apart two meanings of the same English (`menu`); the plain
/// row when the context has none.
pub fn tc(context: &str, source: &'static str) -> &'static str {
    if pseudo() {
        return padded(source);
    }
    I18N.tc(CURRENT.get(), context, source)
}

/// A command's label by its id (`@id` rows), else the label's plain row, else the English label.
pub fn tid(id: &str, label: &'static str) -> &'static str {
    if pseudo() {
        return padded(label);
    }
    I18N.tid(CURRENT.get(), id, label)
}

/// `one` or `other` (English keys, each with `{n}`) for the count `n`, in the current language's
/// plural forms, the number written for the region.
pub fn trn(n: usize, one: &'static str, other: &'static str) -> String {
    let text = I18N.trn(if pseudo() { Lang::EN } else { CURRENT.get() }, n as u64, one, other);
    if pseudo() { pad(&text) } else { text }
}

/// A translated template's `{name}` placeholders filled from `args`, in one pass (a value holding
/// `{other}` stays as it is), each value isolated when the text is right to left.
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    craft_i18n::fmt(template, args)
}

/// The languages the picker offers: (code, name in that language), English first; in debug
/// builds the pseudo locale last.
pub fn languages() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = I18N.languages().into_iter().map(|l| (I18N.code(l).to_owned(), I18N.name(l).to_owned())).collect();
    if cfg!(debug_assertions) {
        out.push((PSEUDO.to_owned(), "⟦Pseudo⟧".to_owned()));
    }
    out
}

/// Shows the interface in `code` ("auto" follows the system); unknown codes keep the current one.
pub fn set_language(code: &str) {
    if code == "auto" {
        use_system_language();
    } else if let Some(lang) = I18N.from_code(code) {
        CURRENT.set(lang);
    }
}

/// The interface language's code (`en`, `fr`); English for the pseudo locale, which only marks
/// English.
pub fn current_code() -> &'static str {
    if pseudo() { "en" } else { I18N.code(CURRENT.get()) }
}

/// The language spelling is checked in when the document names none: the interface's, with the
/// system's region when the region's locale is in that language (`en-GB` for the English
/// interface on a British system, issue #36), so its spellings are the region's.
pub fn spelling_fallback() -> String {
    with_region(current_code(), &REGION.numbers)
}

/// `code` with the region of `locale` when `locale` is in that language.
fn with_region(code: &str, locale: &str) -> String {
    match language_region(locale) {
        (language, Some(region)) if language == code => format!("{code}-{}", region.to_ascii_uppercase()),
        _ => code.to_owned(),
    }
}

/// Follows the system language (on the web, the browser's `navigator.languages`).
pub fn use_system_language() {
    CURRENT.set(I18N.system_lang());
}

/// The shared Keyboard Shortcuts dialog's words, in the current language.
pub fn keymap_text() -> craft_keymap::ui::I18nText<'static> {
    craft_keymap::ui::I18nText { i18n: &I18N, lang: CURRENT.get() }
}

/// An `@msg` row whose English has placeholders: its literal parts and placeholder names, in order,
/// and the translation.
#[derive(Debug)]
struct Template {
    /// One more than `names`: the text before each placeholder, then the text after the last.
    literals: Vec<String>,
    names: Vec<String>,
    translation: String,
}

impl Template {
    fn parse(english: &str, translation: &str) -> Option<Template> {
        let mut literals = Vec::new();
        let mut names = Vec::new();
        let mut rest = english;
        while let Some(open) = rest.find('{') {
            let close = rest.get(open..)?.find('}')? + open;
            let name = rest.get(open + 1..close)?;
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            literals.push(rest.get(..open)?.to_owned());
            names.push(name.to_owned());
            rest = rest.get(close + 1..)?;
        }
        literals.push(rest.to_owned());
        // No placeholder, no literal text, or two placeholders in a row: it could match anything.
        let inner_empty = literals.iter().skip(1).take(names.len().saturating_sub(1)).any(String::is_empty);
        if names.is_empty() || literals.iter().all(String::is_empty) || inner_empty {
            return None;
        }
        Some(Template { literals, names, translation: translation.to_owned() })
    }

    /// The values of the placeholders when `s` is an instance of this template.
    fn captures<'s>(&self, s: &'s str) -> Option<Vec<&'s str>> {
        let (first, rest) = self.literals.split_first()?;
        let (last, middle) = rest.split_last()?;
        let mut rest = s.strip_prefix(first.as_str())?.strip_suffix(last.as_str())?;
        let mut values = Vec::with_capacity(self.names.len());
        for literal in middle {
            let at = rest.find(literal.as_str())?;
            values.push(rest.get(..at)?);
            rest = rest.get(at + literal.len()..)?;
        }
        values.push(rest);
        values.iter().all(|v| !v.is_empty()).then_some(values)
    }

    /// Ordering among matching templates: text before a placeholder first, then the longest
    /// leading text, then the most literal text.
    fn rank(&self) -> (bool, usize, usize) {
        let lead = self.literals.first().map_or(0, String::len);
        (lead > 0, lead, self.literals.iter().map(String::len).sum())
    }
}

/// The `@msg` templates of every embedded locale, by code, best-ranked first.
fn templates(embedded: &craft_i18n::Embedded) -> HashMap<String, Vec<Template>> {
    let mut out = HashMap::new();
    for (code, shared, app) in embedded {
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<Template> = Vec::new();
        // The app's rows come first, so they win over the shared ones.
        for text in [app, shared] {
            for line in text.lines() {
                let mut cols = line.trim_end_matches('\r').splitn(3, '\t');
                let (Some("@msg"), Some(english), Some(translation)) = (cols.next(), cols.next(), cols.next()) else { continue };
                let english = unescape(english);
                if seen.insert(english.clone()) {
                    list.extend(Template::parse(&english, &unescape(translation)));
                }
            }
        }
        list.sort_by_key(|t| std::cmp::Reverse(t.rank()));
        out.insert(code.to_ascii_lowercase(), list);
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// How deep a message's values are translated in turn (a reason inside a message).
const DEPTH: usize = 3;

/// An engine message (English, filled in) in the current language: its exact `@msg` row, else the
/// first `@msg` template it is an instance of, with the values translated in turn unless they are
/// quoted (file names, user text); a message with no row stays English. The engine's own text is
/// English by design: the CLI and agents read it. Text the interface translated itself passes
/// through unchanged, so a place that shows both kinds can call it on either.
// ponytail: craft-libs' newer `I18n::message` does this in the crate; use it when the craft-libs
// rev moves (its own change, AGENTS.md rule 10).
pub fn msg(s: &str) -> String {
    if pseudo() {
        // Text the interface translated already is marked; an engine message is marked here.
        return if s.starts_with('⟦') || s.is_empty() { s.to_owned() } else { pad(s) };
    }
    message(&I18N, CURRENT.get(), &TEMPLATES, s, DEPTH)
}

fn message(i18n: &I18n, lang: Lang, templates: &HashMap<String, Vec<Template>>, s: &str, depth: usize) -> String {
    if s.is_empty() || lang == Lang::EN {
        return s.to_owned();
    }
    if let Some(exact) = i18n.get(lang, "@msg", s) {
        return exact.to_owned();
    }
    // Along the locale's chain: `pt-br`, then `pt`.
    let code = i18n.code(lang);
    let chain = std::iter::once(code).chain(code.rsplit_once('-').map(|(base, _)| base));
    for code in chain {
        let Some(list) = templates.get(code) else { continue };
        for template in list {
            let Some(values) = template.captures(s) else { continue };
            let shown: Vec<String> = values
                .iter()
                .map(|v| if depth <= 1 || quoted(s, v) { (*v).to_owned() } else { message(i18n, lang, templates, v, depth - 1) })
                .collect();
            let args: Vec<(&str, &str)> = template.names.iter().map(String::as_str).zip(shown.iter().map(String::as_str)).collect();
            return fill(&template.translation, &args);
        }
    }
    s.to_owned()
}

/// Whether quotes or backticks surround `value` at its place in `s` (`value` is a slice of `s`).
fn quoted(s: &str, value: &str) -> bool {
    let Some(at) = (value.as_ptr() as usize).checked_sub(s.as_ptr() as usize) else { return false };
    let (Some(before), Some(after)) = (s.get(..at), s.get(at + value.len()..)) else { return false };
    [('"', '"'), ('“', '”'), ('‘', '’'), ('\'', '\''), ('`', '`')].iter().any(|(a, b)| before.ends_with(*a) && after.starts_with(*b))
}

/// How long ago `time` was, from `now`: "just now", "5 minutes ago", "yesterday", "3 days ago",
/// then the date.
pub fn ago(time: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> String {
    let seconds = (now - time).num_seconds().max(0);
    let n = |d: i64| usize::try_from(seconds / d).unwrap_or(0);
    match seconds {
        0..60 => t("just now").to_owned(),
        60..3600 => trn(n(60), "{n} minute ago", "{n} minutes ago"),
        3600..86_400 => trn(n(3600), "{n} hour ago", "{n} hours ago"),
        86_400..172_800 => t("yesterday").to_owned(),
        172_800..2_592_000 => trn(n(86_400), "{n} day ago", "{n} days ago"),
        _ => date(time.with_timezone(&chrono::Local).date_naive()),
    }
}

/// The language and the region of a BCP 47 or POSIX tag, lowercase.
fn language_region(locale: &str) -> (String, Option<String>) {
    let lower = locale.to_ascii_lowercase().replace('_', "-");
    let lower = lower.split(['.', '@']).next().unwrap_or_default().to_owned();
    let mut parts = lower.split('-');
    let language = parts.next().unwrap_or_default().to_owned();
    let region = parts.find(|p| p.len() == 2).map(str::to_owned);
    (language, region)
}

/// The order and separator of a numeric date in `locale` (the region's time locale).
fn date_pattern(locale: &str) -> &'static str {
    let (language, region) = language_region(locale);
    match (language.as_str(), region.as_deref()) {
        ("en", Some("us" | "ph") | None) => "%-m/%-d/%Y",
        ("en", Some("ca" | "za")) | ("ja" | "zh" | "ko" | "hu" | "lt" | "sv", _) => "%Y-%m-%d",
        ("de" | "ru" | "pl" | "cs" | "sk" | "fi" | "nb" | "nn" | "no" | "da" | "tr" | "uk" | "ro" | "hr" | "sl" | "sr" | "bg" | "et" | "lv", _) => {
            "%d.%m.%Y"
        }
        ("nl", _) => "%d-%m-%Y",
        _ => "%d/%m/%Y",
    }
}

/// Whether `locale` writes times on a 12-hour clock.
fn twelve_hour(locale: &str) -> bool {
    let (language, region) = language_region(locale);
    language == "en" && matches!(region.as_deref(), Some("us" | "ca" | "au" | "nz" | "ph" | "in") | None)
}

static REGION: LazyLock<craft_i18n::Region> = LazyLock::new(craft_i18n::Region::system);

/// A calendar date, numeric, as the system's region writes it (`25/11/2026`, `25.11.2026`).
pub fn date(day: chrono::NaiveDate) -> String {
    day.format(date_pattern(&REGION.time)).to_string()
}

/// A time of day, hours and minutes, on the region's clock (`14:05`, `2:05 PM`).
pub fn time(at: chrono::NaiveTime) -> String {
    let twelve = match REGION.hour_cycle {
        Some(craft_i18n::HourCycle::H12) => true,
        Some(craft_i18n::HourCycle::H23) => false,
        None => twelve_hour(&REGION.time),
    };
    at.format(if twelve { "%-I:%M %p" } else { "%H:%M" }).to_string()
}

/// A size in bytes for people: `512 bytes`, `1.5 MB`, with the region's decimal separator and the
/// interface language's unit names.
pub fn file_size(bytes: u64) -> String {
    let lang = if pseudo() { Lang::EN } else { CURRENT.get() };
    let text = I18N.file_size(lang, bytes, craft_i18n::FileSize::BINARY);
    if pseudo() { pad(&text) } else { text }
}

/// A measure in `unit`, an English template with `{n}` (`{n} pt`, `{n} s`): the number as the
/// region writes it, the unit as the language does (`unit` context rows).
pub fn measure(value: f64, decimals: usize, unit: &'static str) -> String {
    let lang = if pseudo() { Lang::EN } else { CURRENT.get() };
    let text = I18N.measure(lang, value, decimals, unit);
    if pseudo() { pad(&text) } else { text }
}

/// A number with `decimals` digits after the separator, as the region writes it (`1,5`).
pub fn number(value: f64, decimals: usize) -> String {
    I18N.format().number(value, decimals)
}

/// Translates an English string literal (the key).
#[macro_export]
macro_rules! tl {
    ($s:expr) => {
        $crate::i18n::t($s)
    };
}

/// Marks an English string literal in a table that is translated where it is shown (with
/// `i18n::t`), so `cargo xtask i18n-report` finds it; the literal itself.
#[macro_export]
macro_rules! tn {
    ($s:literal) => {
        $s
    };
}

/// A template (English, the key) translated and its `{name}` placeholders filled:
/// `tf!("Committed {files} to {branch}.", files = n, branch = b)`.
#[macro_export]
macro_rules! tf {
    ($s:expr $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($s), &[$((stringify!($k), ::std::string::ToString::to_string(&$v).as_str())),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_falls_back_to_the_interface_language_as_the_systems_region_writes_it() {
        assert_eq!(with_region("en", "en_GB.UTF-8"), "en-GB");
        assert_eq!(with_region("en", "en-AU"), "en-AU");
        // A region of another language says nothing about this one's spelling.
        assert_eq!(with_region("en", "fr_FR.UTF-8"), "en");
        assert_eq!(with_region("fr", "C"), "fr");
    }

    /// A French catalog written for these tests (our own wording).
    const FR: &str = "@id\tlanguage.name\tFrançais\n\
        \tSave\tEnregistrer\n\
        @plural\t{n} file|{n} files\t{n} fichier|{n} fichiers\n\
        @id\tfile.save\tEnregistrer le fichier\n\
        @msg\tCould not open {path}: {why}\tImpossible d’ouvrir {path} : {why}\n\
        @msg\t{path} is not a folder\t{path} n’est pas un dossier\n\
        @msg\tpermission denied\taccès refusé\n";
    static FIXTURE: &craft_i18n::Embedded = &[("fr", "", FR)];

    fn fixture() -> (I18n, Lang, HashMap<String, Vec<Template>>) {
        let (i18n, _) = I18n::new(FIXTURE, &["fr"]);
        let fr = i18n.from_code("fr").unwrap();
        (i18n, fr, templates(FIXTURE))
    }

    #[test]
    fn counts_take_the_plural_form_of_the_language() {
        let (i18n, fr, _) = fixture();
        // French counts 0 as singular, English as plural.
        assert!(!i18n.trn(fr, 0, "{n} file", "{n} files").ends_with('s'));
        assert!(i18n.trn(fr, 2, "{n} file", "{n} files").ends_with('s'));
        assert!(i18n.trn(Lang::EN, 0, "{n} file", "{n} files").ends_with('s'));
        assert_ne!(i18n.tid(fr, "file.save", "Save"), i18n.t(fr, "Save"), "the command's @id row is not chosen over the plain row");
    }

    #[test]
    fn engine_messages_are_translated_by_their_template_and_values_in_turn() {
        let (i18n, fr, templates) = fixture();
        let shown = message(&i18n, fr, &templates, "Could not open notes.tex: permission denied", DEPTH);
        assert!(shown.starts_with("Impossible") && shown.contains("notes.tex") && shown.ends_with("accès refusé"), "{shown}");
        // A quoted value is the user's text and stays as it is, even when it reads like a message.
        let shown = message(&i18n, fr, &templates, "Could not open `permission denied`: permission denied", DEPTH);
        assert!(shown.contains("`permission denied`") && shown.ends_with("accès refusé"), "{shown}");
        // A value holding a placeholder's name is not filled in again.
        let shown = message(&i18n, fr, &templates, "{why} is not a folder", DEPTH);
        assert!(shown.starts_with("{why}") && shown != "{why} is not a folder", "{shown}");
        // Without a row, the English.
        let english = "The build stopped.";
        assert_eq!(message(&i18n, fr, &templates, english, DEPTH), english);
    }

    #[test]
    fn templates_that_could_match_anything_are_left_out() {
        assert!(Template::parse("{a}{b}", "x").is_none());
        assert!(Template::parse("{a}", "x").is_none());
        assert!(Template::parse("Open {a", "x").is_none());
        assert!(Template::parse("Open {a} and {b}", "x").is_some());
    }

    #[test]
    fn the_pseudo_locale_marks_and_lengthens_every_string() {
        let text = "Compile";
        let shown = pad(text);
        assert!(shown.starts_with('⟦') && shown.ends_with('⟧') && shown.contains(text));
        assert!(shown.chars().count() as f32 >= text.chars().count() as f32 * 1.4, "{shown}");
    }

    #[test]
    fn times_read_as_how_long_ago() {
        let now = chrono::Utc::now();
        let earlier = |s: i64| now - chrono::Duration::seconds(s);
        assert_eq!(ago(earlier(5), now), "just now");
        assert!(ago(earlier(600), now).contains("10"));
        let old = ago(earlier(4_000_000), now);
        assert!(!old.contains("ago") && old.chars().any(|c| c.is_ascii_digit()), "an old time is not a date: {old}");
    }

    #[test]
    fn dates_follow_the_regions_order() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
        let us = day.format(date_pattern("en-US")).to_string();
        let gb = day.format(date_pattern("en_GB.UTF-8")).to_string();
        let de = day.format(date_pattern("de-DE")).to_string();
        assert_ne!(us, gb, "the month and day are not swapped between the US and the UK");
        assert_ne!(gb, de, "German dates are not written apart from British ones");
        assert!(twelve_hour("en-US") && !twelve_hour("fr-FR") && !twelve_hour("en-GB"));
    }
}
