//! The words a document prints that the formatted view shows: the names its class and language
//! give to what LaTeX makes (`\refname` or `\bibname` over the bibliography, `\contentsname`,
//! `\enclname`), as `\renewcommand` changes them, and the date `\today` writes.
//!
//! The language is the main one babel or polyglossia is given (`\usepackage[main=french]{babel}`,
//! the last language of babel's and the class's options, `\setdefaultlanguage{german}`); those
//! the view knows are English, French, German, Spanish, Italian, Dutch and Portuguese, and any
//! other prints as English here.

use crate::tree::{self, Kind, Node};

/// A document's language, as far as the view knows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    English,
    /// British English: its `\today` is "27th November 2026".
    British,
    French,
    German,
    Spanish,
    Italian,
    Dutch,
    Portuguese,
}

impl Language {
    /// The language babel or polyglossia calls `name`.
    pub fn named(name: &str) -> Option<Language> {
        Some(match name.trim() {
            "english" | "american" | "USenglish" | "canadian" => Language::English,
            "british" | "UKenglish" | "australian" | "newzealand" => Language::British,
            "french" | "francais" | "frenchb" | "acadian" | "canadien" => Language::French,
            "german" | "ngerman" | "austrian" | "naustrian" | "swissgerman" | "nswissgerman" => Language::German,
            "spanish" | "spanishmx" => Language::Spanish,
            "italian" => Language::Italian,
            "dutch" => Language::Dutch,
            "portuguese" | "portuges" | "brazil" | "brazilian" => Language::Portuguese,
            _ => return None,
        })
    }
}

/// The names a document prints.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Names {
    pub language: Language,
    /// Over the table of contents (`\contentsname`).
    pub contents: String,
    /// `\listfigurename`.
    pub figures: String,
    /// `\listtablename`.
    pub tables: String,
    /// Over the bibliography: `\bibname` in a class with chapters (book, report), else `\refname`.
    pub bibliography: String,
    /// `\indexname`.
    pub index: String,
    /// Before a letter's enclosures (`\enclname`, "encl").
    pub enclosure: String,
    /// Before a letter's copies (`\ccname`, "cc").
    pub copies: String,
}

impl Default for Names {
    fn default() -> Self {
        Names::of_language(Language::English, false)
    }
}

/// `\contentsname`, `\listfigurename`, `\listtablename`, `\refname`, `\bibname`, `\indexname`,
/// `\enclname`, `\ccname` in a language.
fn words(language: Language) -> [&'static str; 8] {
    match language {
        Language::English | Language::British => {
            ["Contents", "List of Figures", "List of Tables", "References", "Bibliography", "Index", "encl", "cc"]
        }
        Language::French => {
            ["Table des matières", "Table des figures", "Liste des tableaux", "Références", "Bibliographie", "Index", "P.J.", "Copie à"]
        }
        Language::German => [
            "Inhaltsverzeichnis",
            "Abbildungsverzeichnis",
            "Tabellenverzeichnis",
            "Literatur",
            "Literaturverzeichnis",
            "Index",
            "Anlage(n)",
            "Verteiler",
        ],
        Language::Spanish => {
            ["Índice", "Índice de figuras", "Índice de cuadros", "Referencias", "Bibliografía", "Índice alfabético", "Adjunto", "Copia a"]
        }
        Language::Italian => [
            "Indice",
            "Elenco delle figure",
            "Elenco delle tabelle",
            "Riferimenti bibliografici",
            "Bibliografia",
            "Indice analitico",
            "Allegati",
            "e p.c.",
        ],
        Language::Dutch => ["Inhoudsopgave", "Lijst van figuren", "Lijst van tabellen", "Referenties", "Bibliografie", "Index", "Bijlage(n)", "cc"],
        Language::Portuguese => {
            ["Conteúdo", "Lista de Figuras", "Lista de Tabelas", "Referências", "Bibliografia", "Índice Remissivo", "Anexo", "Com cópia a"]
        }
    }
}

impl Names {
    fn of_language(language: Language, chapters: bool) -> Self {
        let [contents, figures, tables, refname, bibname, index, enclosure, copies] = words(language).map(str::to_owned);
        Names { language, contents, figures, tables, bibliography: if chapters { bibname } else { refname }, index, enclosure, copies }
    }
}

/// The classes with chapters, whose bibliography is headed `\bibname`.
const WITH_CHAPTERS: &[&str] = &["book", "report", "memoir", "scrbook", "scrreprt", "ctexbook", "ctexrep", "thesis"];

/// Whether the class of a document whose main file is `main` has chapters (book, report).
pub fn has_chapters(main: &str) -> bool {
    class(main).is_some_and(|(c, _)| WITH_CHAPTERS.contains(&c.as_str()))
}

/// The class `\documentclass` names, and its options.
fn class(main: &str) -> Option<(String, Vec<String>)> {
    let tree = tree::parse(main);
    tree.iter().find_map(|n| match &n.kind {
        Kind::Command { name, args, .. } if name == "documentclass" => {
            let class = args.iter().find(|a| !a.optional).and_then(|a| main.get(a.inner.clone()))?.trim().to_owned();
            let options = args
                .iter()
                .filter(|a| a.optional)
                .filter_map(|a| main.get(a.inner.clone()))
                .flat_map(|o| o.split(','))
                .map(|o| o.trim().to_owned())
                .collect();
            Some((class, options))
        }
        _ => None,
    })
}

/// The main language of a document whose main file is `main`.
pub fn language(main: &str) -> Language {
    let tree = tree::parse(main);
    let preamble: Vec<&Node> = tree.iter().take_while(|n| !matches!(&n.kind, Kind::Environment { name, .. } if name == "document")).collect();
    let mut found: Option<Language> = None;
    let class_options = class(main).map(|(_, o)| o).unwrap_or_default();
    for (i, n) in preamble.iter().enumerate() {
        let Kind::Command { name, args, .. } = &n.kind else { continue };
        let required = args.iter().find(|a| !a.optional).and_then(|a| main.get(a.inner.clone())).unwrap_or_default();
        match name.as_str() {
            "usepackage" | "RequirePackage" if required.split(',').any(|p| p.trim() == "babel") => {
                let options: Vec<String> = args
                    .iter()
                    .filter(|a| a.optional)
                    .filter_map(|a| main.get(a.inner.clone()))
                    .flat_map(|o| o.split(','))
                    .map(|o| o.trim().to_owned())
                    .collect();
                // `main=` names it; else the last language given (the class's options count too).
                let main_option = options.iter().find_map(|o| o.strip_prefix("main=").and_then(Language::named));
                let last = class_options.iter().chain(&options).rev().find_map(|o| Language::named(o));
                found = main_option.or(last).or(found);
            }
            "setdefaultlanguage" | "setmainlanguage" => {
                // Its argument, parsed or in the group after it (`[variant=british]` before it).
                let after = preamble.get(i + 1..).unwrap_or_default().iter().take(2).find_map(|g| match &g.kind {
                    Kind::Group { .. } => main.get(g.range.start + 1..g.range.end.saturating_sub(1)),
                    _ => None,
                });
                let named = if required.is_empty() { after.unwrap_or_default() } else { required };
                found = Language::named(named).or(found);
            }
            _ => {}
        }
    }
    found.unwrap_or_default()
}

/// The names the document whose main file is `main` prints: those of its language and class, then
/// those `\renewcommand` (or `\def`) sets anywhere in it.
pub fn names(main: &str) -> Names {
    let chapters = has_chapters(main);
    let mut out = Names::of_language(language(main), chapters);
    for (name, value) in renewed(main) {
        let slot = match name.as_str() {
            "contentsname" => &mut out.contents,
            "listfigurename" => &mut out.figures,
            "listtablename" => &mut out.tables,
            "refname" if !chapters => &mut out.bibliography,
            "bibname" if chapters => &mut out.bibliography,
            "indexname" => &mut out.index,
            "enclname" => &mut out.enclosure,
            "ccname" => &mut out.copies,
            _ => continue,
        };
        *slot = value;
    }
    out
}

/// What `\renewcommand{\name}{value}`, `\renewcommand\name{value}` and `\def\name{value}` set, in
/// order, wherever they are (in `\addto\captionsfrench{…}` too); the value's text without braces.
fn renewed(text: &str) -> Vec<(String, String)> {
    fn walk(text: &str, nodes: &[Node], out: &mut Vec<(String, String)>) {
        for (i, n) in nodes.iter().enumerate() {
            match &n.kind {
                Kind::Command { name, args, .. } if matches!(name.as_str(), "renewcommand" | "def" | "providecommand" | "newcommand") => {
                    // `\renewcommand{\refname}{…}`: both parsed as arguments.
                    let parsed = match (args.iter().find(|a| !a.optional), args.iter().rev().find(|a| !a.optional)) {
                        (Some(a), Some(b)) if a.inner != b.inner => {
                            let named = text.get(a.inner.clone()).map(|s| s.trim().trim_start_matches('\\').to_owned());
                            named.zip(text.get(b.inner.clone()).map(str::to_owned))
                        }
                        _ => None,
                    };
                    // `\renewcommand\refname{…}`, `\def\refname{…}`: the name and a group after it.
                    let after = || match (nodes.get(i + 1), nodes.get(i + 2)) {
                        (
                            Some(Node { kind: Kind::Command { name, args: none, .. }, .. }),
                            Some(Node { kind: Kind::Group { closed: true, .. }, range }),
                        ) if none.is_empty() => Some((name.clone(), text.get(range.start + 1..range.end - 1)?.to_owned())),
                        _ => None,
                    };
                    if let Some((name, value)) = parsed.or_else(after) {
                        let value: String = value.chars().filter(|c| !matches!(c, '{' | '}')).collect();
                        out.push((name, value.trim().to_owned()));
                    }
                }
                Kind::Command { args, .. } => args.iter().for_each(|a| walk(text, &a.body, out)),
                Kind::Group { body, .. } | Kind::Environment { body, .. } => walk(text, body, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(text, &tree::parse(text), &mut out);
    out
}

const MONTHS: [[&str; 12]; 7] = [
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
    ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
    ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
    ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"],
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
];

/// The date `\today` prints in `language`, for the calendar date `year`, `month` (1 to 12) and
/// `day` (as a date library gives them).
pub fn today(language: Language, year: i32, month: u32, day: u32) -> String {
    let table = match language {
        Language::English | Language::British => 0,
        Language::French => 1,
        Language::German => 2,
        Language::Spanish => 3,
        Language::Italian => 4,
        Language::Dutch => 5,
        Language::Portuguese => 6,
    };
    let month_name = MONTHS.get(table).and_then(|m| m.get(month.saturating_sub(1) as usize)).copied().unwrap_or_default();
    match language {
        Language::English => format!("{month_name} {day}, {year}"),
        Language::British => {
            let suffix = match (day % 10, day % 100) {
                (1, n) if n != 11 => "st",
                (2, n) if n != 12 => "nd",
                (3, n) if n != 13 => "rd",
                _ => "th",
            };
            format!("{day}{suffix} {month_name} {year}")
        }
        Language::French if day == 1 => format!("1er {month_name} {year}"),
        Language::German => format!("{day}. {month_name} {year}"),
        Language::Spanish | Language::Portuguese => format!("{day} de {month_name} de {year}"),
        Language::French | Language::Italian | Language::Dutch => format!("{day} {month_name} {year}"),
    }
}
