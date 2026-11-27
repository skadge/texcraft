//! TeX Live packages a desktop build lacked, and what TeXCraft does about them
//! (`build.missing_packages`, Settings › Building › Missing packages).
//!
//! retex 2.5 downloads a package only in its worker's build processes (the desktop's screen and
//! print builds): their lookup of a file TeX Live has but the installed tree lacks installs the
//! package from TeX Live's own repository of the installed year (tlnet through
//! `mirrors.ctan.org` for the current year, the frozen `tlnet-final` of the historic archive for a
//! past one), so downloaded packages always match the installed TeX Live. Whether it may is
//! [`retex::Downloads`]: `Always` for [`MissingPackages::Auto`], `Never` otherwise. Builds retex
//! runs inside TeXCraft's own process (accessible PDFs, exports, the CLI) neither download nor
//! find downloaded packages.
//!
//! A build that lacked a package says so in an error whose message and notes retex writes
//! (`package `x` not found`, `x.sty is in the TeX Live package `y``): [`lack_of`] reads them, and
//! [`advise`] replaces retex's own advice about its settings with TeXCraft's.

/// What a desktop build does when a document needs a TeX Live package that is not installed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MissingPackages {
    /// Not chosen yet: the window asks the first time a package is missing; until then, and
    /// without a window, nothing is downloaded.
    #[default]
    Ask,
    /// Downloaded from TeX Live's repository of the installed year, during the build.
    Auto,
    /// Not downloaded; the build's message says how to install the package.
    Manual,
    /// Not downloaded.
    Off,
}

impl MissingPackages {
    pub const ALL: [MissingPackages; 4] = [MissingPackages::Ask, MissingPackages::Auto, MissingPackages::Manual, MissingPackages::Off];

    /// `ask`, `auto`, `manual` or `off`.
    pub fn parse(s: &str) -> Result<MissingPackages, String> {
        MissingPackages::ALL
            .into_iter()
            .find(|m| m.name() == s.trim())
            .ok_or_else(|| format!("`{}` is not a choice for missing packages: give ask, auto, manual or off.", s.trim()))
    }

    pub fn name(self) -> &'static str {
        match self {
            MissingPackages::Ask => "ask",
            MissingPackages::Auto => "auto",
            MissingPackages::Manual => "manual",
            MissingPackages::Off => "off",
        }
    }

    /// Whether builds download what is missing.
    pub fn downloads(self) -> bool {
        self == MissingPackages::Auto
    }
}

/// What a build lacked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Lack {
    /// No TeX Live at all: retex makes its own tree only with downloads on.
    TexLive,
    /// A file of TeX Live, and the package that holds it when retex knows (it knows once it has
    /// TeX Live's package database).
    File { file: String, package: Option<String> },
}

impl Lack {
    /// The package's name, else the file's; `None` for TeX Live as a whole.
    pub fn name(&self) -> Option<&str> {
        match self {
            Lack::TexLive => None,
            Lack::File { file, package } => Some(package.as_deref().unwrap_or(file)),
        }
    }
}

/// The text between the first pair of backquote and quote after `prefix` in `s`.
fn quoted_after<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = &s[s.find(prefix)? + prefix.len()..];
    let rest = rest.strip_prefix('`')?;
    rest.split_once('`').or_else(|| rest.split_once('\'')).map(|(name, _)| name)
}

/// What an error of retex's says was missing: a TeX Live file and its package, or TeX Live itself;
/// `None` for other errors, and for a file that no package of TeX Live has (a typo, or the
/// project's own file).
pub fn lack_of(message: &str, notes: &[String]) -> Option<Lack> {
    if message == "no TeX Live found" || message.starts_with("cannot make retex's own tree of TeX Live") {
        return Some(Lack::TexLive);
    }
    if notes.iter().any(|n| n.starts_with("no package of TeX Live")) {
        return None;
    }
    // `x.sty is in the TeX Live package `y``.
    let held = notes.iter().find_map(|n| {
        let (file, rest) = n.split_once(" is in the TeX Live package ")?;
        Some((file.to_owned(), quoted_after(rest, "")?.to_owned()))
    });
    if let Some((file, package)) = held {
        return Some(Lack::File { file, package: Some(package) });
    }
    // Without TeX Live's database retex cannot tell: a class or package (`\usepackage{x} loads the
    // file x.sty, which retex cannot find`) is taken to be TeX Live's.
    let loaded = notes.iter().find_map(|n| {
        let (_, rest) = n.split_once(" loads the file ")?;
        let (file, _) = rest.split_once(", which retex cannot find")?;
        Some(file.to_owned())
    });
    if let Some(file) = loaded {
        return Some(Lack::File { file, package: None });
    }
    let stem = quoted_after(message, "package ").filter(|_| message.ends_with("not found")).map(|s| format!("{s}.sty"));
    let class = || quoted_after(message, "document class ").filter(|_| message.ends_with("not found")).map(|s| format!("{s}.cls"));
    stem.or_else(class).map(|file| Lack::File { file, package: None })
}

/// The guide's page on missing packages.
pub const GUIDE: &str = "https://texcraft-893d51.gitlab.io/guide/troubleshooting.html#desktop-packages";

/// retex's advice about its own settings (`retex config`, `RETEX_OFFLINE`), which a TeXCraft user
/// does not set: TeXCraft's setting decides.
fn about_retex_settings(help: &str) -> bool {
    help.starts_with("or let retex download it")
        || help.contains("RETEX_OFFLINE")
        || help.contains("retex config packages.download")
        || help.contains("--offline")
}

/// The help of an error that lacked `lack`, for a build under `setting`; `in_process` for a build
/// retex runs in TeXCraft's own process, which never downloads. retex's advice about its own
/// settings goes; TeXCraft's follows retex's other advice (its install command for the system).
pub fn advise(help: &mut Vec<String>, lack: &Lack, setting: MissingPackages, in_process: bool) {
    help.retain(|h| !about_retex_settings(h));
    let install = match lack {
        Lack::File { package: Some(p), .. } => {
            format!("install the TeX Live package `{p}`: `tlmgr install {p}`, or the package of your system's TeX Live that has it")
        }
        Lack::File { file, package: None } => {
            format!(
                "`tlmgr search --global --file {file}` names the TeX Live package that has it; install that with tlmgr or your system's package manager"
            )
        }
        Lack::TexLive => "install TeX Live (https://tug.org/texlive), or your system's TeX Live packages".to_owned(),
    };
    let said = match (setting, in_process) {
        (MissingPackages::Auto, true) => {
            format!(
                "accessible PDFs and exports are typeset inside TeXCraft, which neither downloads packages nor finds those it downloaded: {install}"
            )
        }
        (MissingPackages::Auto, false) => format!(
            "TeXCraft could not download it from TeX Live's repository (no network, or the repository did not answer; after a network failure downloads pause for 10 minutes): build again later, or {install}"
        ),
        (MissingPackages::Ask, _) => format!(
            "TeXCraft did not download it: Missing packages in Settings › Building has not been chosen yet. Choose Download automatically there (or the command build.missing_packages with auto) and build again, or {install}"
        ),
        (MissingPackages::Manual, _) => {
            format!("TeXCraft does not download packages (Missing packages in Settings › Building): {install}. See {GUIDE}")
        }
        (MissingPackages::Off, _) => {
            "TeXCraft does not download packages: Missing packages in Settings › Building is set to Don't download".to_owned()
        }
    };
    help.push(said);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_package_retex_knows_names_its_tex_live_package() {
        let lack = lack_of(
            "package `antanilipsum` not found",
            &notes(&[
                "\\usepackage{antanilipsum} loads the file antanilipsum.sty, which retex cannot find",
                "antanilipsum.sty is in the TeX Live package `antanilipsum`",
            ]),
        );
        assert_eq!(lack, Some(Lack::File { file: "antanilipsum.sty".into(), package: Some("antanilipsum".into()) }));
        let class = lack_of("document class `kaobook` not found", &[]);
        assert_eq!(class.as_ref().and_then(Lack::name), Some("kaobook.cls"), "a class without a database is not named by its file");
    }

    #[test]
    fn a_file_in_no_package_or_another_error_is_not_a_missing_package() {
        let typo = lack_of("package `amsmaht` not found", &notes(&["no package of TeX Live 2025 has a file amsmaht.sty"]));
        assert_eq!(typo, None);
        assert_eq!(lack_of("Undefined control sequence", &[]), None);
        assert_eq!(lack_of("file `chapter1.tex` not found", &[]), None, "the project's own file is not TeX Live's");
        assert_eq!(lack_of("no TeX Live found", &[]), Some(Lack::TexLive));
    }

    #[test]
    fn the_advice_follows_the_setting_and_drops_retexs_own() {
        let lack = Lack::File { file: "x.sty".into(), package: Some("x".into()) };
        let retex = vec![
            "install it: `sudo apt install texlive-x`".to_owned(),
            "or let retex download it: package downloads are off (from RETEX_OFFLINE)".to_owned(),
        ];
        for setting in MissingPackages::ALL {
            let mut help = retex.clone();
            advise(&mut help, &lack, setting, false);
            assert!(help.iter().all(|h| !h.contains("RETEX_OFFLINE")), "{setting:?} keeps retex's own setting's advice");
            assert!(help.iter().any(|h| h.contains("apt install")), "{setting:?} drops retex's install command");
            assert!(help.iter().any(|h| h.contains("Settings › Building") || h.contains("download")), "{setting:?} does not say why");
        }
        let mut manual = Vec::new();
        advise(&mut manual, &lack, MissingPackages::Manual, false);
        assert!(
            manual.iter().any(|h| h.contains("tlmgr install x") && h.contains(GUIDE)),
            "Install them myself names neither the command nor the guide"
        );
    }
}
