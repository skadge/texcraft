//! Which engine a build of [`Engine::Auto`] runs (Settings › Building, "Chosen from the
//! document"): read from the project's sources, without typesetting, so the choice is known
//! before the build and can be shown.
//!
//! retex's own choice in its library runs LuaLaTeX's mode first and pdfLaTeX's only after an
//! error that shows a pdfTeX document, and not at all in the browser; a document for pdfLaTeX
//! is named here instead: a `% !TEX program = pdflatex` line (or `TS-program`) in the main
//! file's first lines, or a pdfTeX primitive that LuaLaTeX's format does not define
//! (`\pdfmapfile`, `\pdfstrcmp`) used outside a test for the engine. A document that also
//! loads a package for LuaTeX only (fontspec) or calls Lua is left to retex.

use std::collections::BTreeSet;

use texcraft_project::ProjectPath;

use crate::{Engine, Job};

/// The engine chosen for a document, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chosen {
    pub engine: Engine,
    pub why: Why,
}

/// Why an engine was chosen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Why {
    /// The main file asks for it: its comment line (`% !TEX program = pdflatex`), from 1.
    Asked { comment: String, line: u32 },
    /// A pdfTeX primitive LuaLaTeX lacks (`\pdfmapfile`), where it is first used.
    Primitive { command: String, file: String, line: u32 },
}

/// How many of the main file's first lines may hold the `% !TEX program` line.
const MAGIC_LINES: usize = 20;

/// The pdfTeX primitives (without the backslash) that LuaLaTeX's format does not define: those
/// LuaTeX lacks, and those it has under another name (`\pdfextension mapfile`).
const PDFTEX_ONLY: &[&str] = &[
    "pdfadjustinterwordglue",
    "pdfadjustspacing",
    "pdfannot",
    "pdfappendkern",
    "pdfcatalog",
    "pdfcolorstack",
    "pdfcolorstackinit",
    "pdfcompresslevel",
    "pdfcopyfont",
    "pdfcreationdate",
    "pdfdecimaldigits",
    "pdfdest",
    "pdfdestmargin",
    "pdfdraftmode",
    "pdfeachlinedepth",
    "pdfeachlineheight",
    "pdfelapsedtime",
    "pdfendlink",
    "pdfendthread",
    "pdfescapehex",
    "pdfescapename",
    "pdfescapestring",
    "pdffakespace",
    "pdffiledump",
    "pdffilemoddate",
    "pdffilesize",
    "pdffirstlineheight",
    "pdffontattr",
    "pdffontexpand",
    "pdffontname",
    "pdffontobjnum",
    "pdffontsize",
    "pdfgamma",
    "pdfgentounicode",
    "pdfglyphtounicode",
    "pdfhorigin",
    "pdfignoreddimen",
    "pdfimageapplygamma",
    "pdfimagegamma",
    "pdfimagehicolor",
    "pdfimageresolution",
    "pdfincludechars",
    "pdfinclusioncopyfonts",
    "pdfinclusionerrorlevel",
    "pdfinfo",
    "pdfinfoomitdate",
    "pdfinsertht",
    "pdfinterwordspaceoff",
    "pdfinterwordspaceon",
    "pdflastannot",
    "pdflastlinedepth",
    "pdflastlink",
    "pdflastmatch",
    "pdflastobj",
    "pdflastxform",
    "pdflastximage",
    "pdflastximagecolordepth",
    "pdflastximagepages",
    "pdflastxpos",
    "pdflastypos",
    "pdflinkmargin",
    "pdfliteral",
    "pdfmajorversion",
    "pdfmapfile",
    "pdfmapline",
    "pdfmatch",
    "pdfmdfivesum",
    "pdfminorversion",
    "pdfnames",
    "pdfnobuiltintounicode",
    "pdfnoligatures",
    "pdfnormaldeviate",
    "pdfobj",
    "pdfobjcompresslevel",
    "pdfomitcharset",
    "pdfomitinfodict",
    "pdfoptionpdfminorversion",
    "pdfoutline",
    "pdfoutput",
    "pdfpageattr",
    "pdfpagebox",
    "pdfpageheight",
    "pdfpageref",
    "pdfpageresources",
    "pdfpagesattr",
    "pdfpagewidth",
    "pdfpkmode",
    "pdfpkresolution",
    "pdfprependkern",
    "pdfprimitive",
    "pdfprotrudechars",
    "pdfptexuseunderscore",
    "pdfpxdimen",
    "pdfrandomseed",
    "pdfrefobj",
    "pdfrefxform",
    "pdfrefximage",
    "pdfresettimer",
    "pdfrestore",
    "pdfretval",
    "pdfrunninglinkoff",
    "pdfrunninglinkon",
    "pdfsave",
    "pdfsavepos",
    "pdfsetmatrix",
    "pdfsetrandomseed",
    "pdfshellescape",
    "pdfstartlink",
    "pdfstartthread",
    "pdfstrcmp",
    "pdfsuppressptexinfo",
    "pdfsuppresswarningdupdest",
    "pdfsuppresswarningdupmap",
    "pdfsuppresswarningpagegroup",
    "pdftexbanner",
    "pdftexrevision",
    "pdftexversion",
    "pdfthread",
    "pdfthreadmargin",
    "pdftracingfonts",
    "pdftrailer",
    "pdftrailerid",
    "pdfunescapehex",
    "pdfuniformdeviate",
    "pdfuniqueresname",
    "pdfvorigin",
    "pdfxform",
    "pdfxformname",
    "pdfximage",
    "pdfximagebbox",
];

/// Conditionals that test for the engine or for a command: what is inside them is written for
/// either engine.
const GUARDS: &[&str] = &[
    "ifx",
    "ifdefined",
    "ifcsname",
    "@ifundefined",
    "ifpdf",
    "ifPDFTeX",
    "ifpdftex",
    "ifluatex",
    "ifLuaTeX",
    "iftutex",
    "ifxetex",
    "IfFormatAtLeastTF",
    "sys_if_engine_pdftex:TF",
    "sys_if_engine_luatex:TF",
];

/// Commands after which a conditional's name is an argument, not a test that opens.
const NAMING: &[&str] = &["newif", "ifx", "let", "def", "gdef", "edef", "xdef", "global", "csname", "expandafter", "noexpand", "ifdefined"];

/// Packages that run in LuaTeX only (or XeTeX's, which retex runs as LuaTeX).
const LUATEX_PACKAGES: &[&str] =
    &["fontspec", "unicode-math", "luacode", "luatexja", "luaotfload", "luatexbase", "luamplib", "lua-ul", "luacolor", "luatexko", "polyglossia"];

/// Environments whose lines are shown or written elsewhere, not run.
const VERBATIM: &[&str] =
    &["verbatim", "verbatim*", "Verbatim", "BVerbatim", "LVerbatim", "lstlisting", "minted", "comment", "filecontents", "filecontents*"];

/// How many files the scan follows from the main one, at most.
const MAX_FILES: usize = 64;

impl Job {
    /// For [`Engine::Auto`], the engine the document asks for, which the job's options then name;
    /// none (and the options left to retex) when the document does not say, or the settings
    /// name an engine.
    pub fn choose_engine(&mut self) -> Option<Chosen> {
        if self.options.engine != Engine::Auto {
            return None;
        }
        let chosen = choose(&self.files, &self.main)?;
        self.options.engine = chosen.engine;
        Some(chosen)
    }
}

/// The engine the project's main document `main` asks for: a `% !TEX program` line, else a
/// pdfTeX primitive used in it or in the files it reads from the project; none when nothing
/// says, or when it also uses LuaTeX's packages.
pub fn choose<B: AsRef<[u8]>>(files: &[(ProjectPath, B)], main: &ProjectPath) -> Option<Chosen> {
    choose_with(main, |path| files.iter().find(|(p, _)| p == path).map(|(_, b)| String::from_utf8_lossy(b.as_ref()).into_owned()))
}

/// [`choose`] over the project's files as `text` gives them (none: no such file, or not text).
pub fn choose_with(main: &ProjectPath, text: impl Fn(&ProjectPath) -> Option<String>) -> Option<Chosen> {
    let main_text = text(main)?;
    if let Some(asked) = asked(&main_text) {
        return Some(asked);
    }
    let mut seen: BTreeSet<ProjectPath> = BTreeSet::new();
    let mut queue = vec![(main.clone(), main_text)];
    let mut first: Option<Why> = None;
    while let Some((path, source)) = queue.pop() {
        if !seen.insert(path.clone()) || seen.len() > MAX_FILES {
            continue;
        }
        let found = scan(&source);
        if found.lua {
            return None;
        }
        if first.is_none()
            && let Some((command, line)) = found.primitive
        {
            first = Some(Why::Primitive { command: format!("\\{command}"), file: path.as_str().to_owned(), line });
        }
        for name in found.reads.iter().rev() {
            if let Some((candidate, t)) = candidates(&path, name).into_iter().find_map(|c| text(&c).map(|t| (c, t))) {
                queue.push((candidate, t));
            }
        }
    }
    first.map(|why| Chosen { engine: Engine::PdfLatex, why })
}

/// The `% !TEX program = …` (or `TS-program`) line of the first lines, when it names an engine
/// retex has: pdfLaTeX's or LuaLaTeX's.
fn asked(text: &str) -> Option<Chosen> {
    for (n, line) in text.lines().take(MAGIC_LINES).enumerate() {
        let Some(rest) = line.trim_start().strip_prefix('%') else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.get(..4).filter(|s| s.eq_ignore_ascii_case("!tex")).and_then(|_| rest.get(4..)) else { continue };
        let rest = rest.trim_start();
        let lower = rest.to_ascii_lowercase();
        let after = ["ts-program", "program"].iter().find_map(|k| lower.strip_prefix(k).map(|r| rest.len() - r.len()));
        let Some(value) = after.and_then(|at| rest.get(at..)).and_then(|r| r.trim_start().strip_prefix('=')) else { continue };
        let program = value.trim().to_ascii_lowercase();
        let engine = match program.as_str() {
            "pdflatex" | "pdftex" | "latex" => Engine::PdfLatex,
            "lualatex" | "luatex" | "lualatex-dev" => Engine::LuaLatex,
            _ => continue,
        };
        return Some(Chosen { engine, why: Why::Asked { comment: line.trim().to_owned(), line: u32::try_from(n + 1).unwrap_or(u32::MAX) } });
    }
    None
}

/// What one file says of its engine.
#[derive(Default)]
struct Found {
    /// The first pdfTeX-only primitive used outside a test for the engine, and its line.
    primitive: Option<(String, u32)>,
    /// It loads a package for LuaTeX, or calls Lua.
    lua: bool,
    /// The files it reads (`\input`, `\include`, `\usepackage`, `\documentclass`), as written.
    reads: Vec<String>,
}

/// The text of a line before its comment (an unescaped `%`).
fn code(line: &str) -> &str {
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            '\\' => escaped = !escaped,
            '%' if !escaped => return &line[..i],
            _ => escaped = false,
        }
    }
    line
}

/// The control sequences of `code`, in order, without their backslash (letters, `@`, `_`, `:`).
fn control_sequences(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(i) = rest.find('\\') {
        rest = &rest[i + 1..];
        let end = rest.find(|c: char| !(c.is_ascii_alphabetic() || c == '@' || c == '_' || c == ':')).unwrap_or(rest.len());
        if end == 0 {
            // A control symbol (`\\`, `\%`): skip its character.
            rest = rest.get(rest.chars().next().map_or(0, char::len_utf8)..).unwrap_or("");
            continue;
        }
        out.push(&rest[..end]);
        rest = &rest[end..];
    }
    out
}

/// The `{…}` argument after `command` (and an optional `[…]`) at each place `command` is in
/// `code`, split at commas.
fn arguments(code: &str, command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(i) = rest.find(command) {
        rest = &rest[i + command.len()..];
        if rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let mut after = rest.trim_start();
        if let Some(opt) = after.strip_prefix('[') {
            after = opt.split_once(']').map_or("", |(_, r)| r).trim_start();
        }
        if let Some(arg) = after.strip_prefix('{').and_then(|a| a.split_once('}')).map(|(a, _)| a) {
            out.extend(arg.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned));
        }
    }
    out
}

fn scan(text: &str) -> Found {
    let mut found = Found::default();
    // The conditionals open, each whether it tests for the engine or a command.
    let mut open: Vec<bool> = Vec::new();
    let mut verbatim: Option<String> = None;
    for (n, raw) in text.lines().enumerate() {
        if let Some(end) = &verbatim {
            if raw.contains(end.as_str()) {
                verbatim = None;
            }
            continue;
        }
        let c = code(raw);
        if let Some(name) = arguments(c, "\\begin").into_iter().find(|n| VERBATIM.contains(&n.as_str())) {
            let end = format!("\\end{{{name}}}");
            if !c.contains(&end) {
                verbatim = Some(end);
            }
            continue;
        }
        let packages = arguments(c, "\\usepackage").into_iter().chain(arguments(c, "\\RequirePackage"));
        for p in packages {
            if LUATEX_PACKAGES.contains(&p.as_str()) {
                found.lua = true;
            }
            found.reads.push(format!("{p}.sty"));
        }
        for command in ["\\documentclass", "\\LoadClass"] {
            found.reads.extend(arguments(c, command).into_iter().map(|c| format!("{c}.cls")));
        }
        for command in ["\\input", "\\include", "\\subfile"] {
            found.reads.extend(arguments(c, command));
        }
        // A test on the line itself (`\ifx\pdfmapfile\undefined … \fi`) covers it all.
        let sequences = control_sequences(c);
        let guarded_line = sequences.iter().any(|cs| GUARDS.contains(cs));
        for (i, &cs) in sequences.iter().enumerate() {
            // `\newif\ifpdf`, `\ifx\ifpdf\undefined`: a conditional named, not opened.
            let named = i > 0 && NAMING.contains(&sequences[i - 1]);
            match cs {
                "directlua" | "luaexec" | "luadirect" => found.lua = true,
                _ if cs.starts_with("if") && !named => open.push(GUARDS.contains(&cs)),
                "fi" => {
                    open.pop();
                }
                _ if found.primitive.is_none() && !guarded_line && !open.iter().any(|g| *g) && PDFTEX_ONLY.contains(&cs) => {
                    found.primitive = Some((cs.to_owned(), u32::try_from(n + 1).unwrap_or(u32::MAX)));
                }
                _ => {}
            }
        }
    }
    found
}

/// The project files `name` (as `file` wrote it) may be: from the project's top and from
/// `file`'s folder, as written and with `.tex` added.
fn candidates(file: &ProjectPath, name: &str) -> Vec<ProjectPath> {
    let name = name.trim().trim_start_matches("./");
    let folder = file.as_str().rsplit_once('/').map(|(d, _)| d);
    let mut out = Vec::new();
    for base in [None, folder] {
        let joined = match base {
            Some(d) => format!("{d}/{name}"),
            None => name.to_owned(),
        };
        for path in [joined.clone(), format!("{joined}.tex")] {
            if let Ok(p) = ProjectPath::new(&path)
                && !out.contains(&p)
            {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<(ProjectPath, Vec<u8>)> {
        list.iter().filter_map(|(p, t)| ProjectPath::new(p).ok().map(|p| (p, t.as_bytes().to_vec()))).collect()
    }

    fn chosen(list: &[(&str, &str)]) -> Option<Chosen> {
        let main = ProjectPath::new(list[0].0).ok()?;
        choose(&files(list), &main)
    }

    fn engine(main: &str) -> Option<Engine> {
        chosen(&[("main.tex", main)]).map(|c| c.engine)
    }

    #[test]
    fn a_font_map_makes_a_pdflatex_document() {
        // #52: a document for pdfLaTeX's font maps failed as LuaLaTeX.
        let doc = "\\documentclass{article}\n\\pdfmapfile{+ubuntu-regular.map}\n\\begin{document}\nHi.\n\\end{document}\n";
        let c = chosen(&[("main.tex", doc)]);
        assert_eq!(c.as_ref().map(|c| c.engine), Some(Engine::PdfLatex));
        assert!(matches!(c.map(|c| c.why), Some(Why::Primitive { command, line: 2, .. }) if command == "\\pdfmapfile"));
    }

    #[test]
    fn primitives_lualatex_lacks_choose_pdflatex_and_others_do_not() {
        for primitive in [
            "\\pdfglyphtounicode{f_f}{FB00}",
            "\\pdfcompresslevel=9",
            "\\pdfminorversion=7",
            "\\pdfpagewidth=210mm",
            "\\ifnum\\pdfstrcmp{a}{b}=0 x\\fi",
        ] {
            assert_eq!(engine(&format!("\\documentclass{{article}}\n{primitive}\n")), Some(Engine::PdfLatex), "{primitive}");
        }
        // pdfTeX's names that LuaLaTeX defines too, and legacy encodings, which run in both.
        for both in ["\\pdfstringdef\\x{y}", "\\usepackage[T1]{fontenc}\n\\usepackage[utf8]{inputenc}", "\\pdfbookmark{A}{a}"] {
            assert_eq!(engine(&format!("\\documentclass{{article}}\n{both}\n")), None, "{both}");
        }
    }

    #[test]
    fn a_primitive_behind_a_test_for_the_engine_is_written_for_both() {
        assert_eq!(engine("\\ifx\\pdfmapfile\\undefined\\else\\pdfmapfile{+a.map}\\fi\n"), None);
        assert_eq!(engine("\\ifpdf\n  \\pdfmapfile{+a.map}\n\\else\n  \\relax\n\\fi\n"), None);
        assert_eq!(engine("\\ifdefined\\pdfminorversion\n\\pdfminorversion=7\n\\fi\n"), None);
        // After the test, a primitive counts again; one in an unrelated conditional does too.
        assert_eq!(engine("\\ifpdf\\relax\\fi\n\\ifnum1=1 \\pdfmapfile{+a.map}\\fi\n"), Some(Engine::PdfLatex));
        // Naming a conditional opens none.
        assert_eq!(engine("\\newif\\ifpdf\n\\ifx\\ifpdf\\undefined\\fi\n\\pdfmapfile{+a.map}\n"), Some(Engine::PdfLatex));
    }

    #[test]
    fn comments_and_verbatim_text_do_not_count() {
        assert_eq!(engine("% \\pdfmapfile{+a.map}\nText 50\\% \\pdfmapfile is a command.\n"), Some(Engine::PdfLatex));
        assert_eq!(engine("Text % \\pdfmapfile{+a.map}\n"), None);
        assert_eq!(engine("\\begin{verbatim}\n\\pdfmapfile{+a.map}\n\\end{verbatim}\n"), None);
    }

    #[test]
    fn luatex_packages_leave_the_choice_to_retex() {
        assert_eq!(engine("\\usepackage{fontspec}\n\\pdfmapfile{+a.map}\n"), None);
        assert_eq!(engine("\\directlua{x()}\n\\pdfmapfile{+a.map}\n"), None);
    }

    #[test]
    fn the_main_files_comment_asks_for_an_engine() {
        for line in ["% !TEX program = pdflatex", "%!TeX TS-program = pdflatex", "% !TEX TS-program=pdfLaTeX"] {
            let c = chosen(&[("main.tex", &format!("{line}\n\\documentclass{{article}}\n"))]);
            assert_eq!(c.as_ref().map(|c| c.engine), Some(Engine::PdfLatex), "{line}");
            assert!(matches!(c.map(|c| c.why), Some(Why::Asked { line: 1, .. })), "{line}");
        }
        // An explicit request wins over the signs.
        assert_eq!(engine("% !TEX program = lualatex\n\\pdfmapfile{+a.map}\n"), Some(Engine::LuaLatex));
        assert_eq!(engine("% !TEX program = xelatex\n"), None);
        // Far down, it is not the editor's line.
        assert_eq!(engine(&format!("{}% !TEX program = pdflatex\n", "x\n".repeat(30))), None);
    }

    #[test]
    fn the_files_the_document_reads_count() {
        let main = "\\documentclass{thesis}\n\\usepackage{fonts}\n\\input{chapters/one}\n";
        let in_sty = chosen(&[("main.tex", main), ("fonts.sty", "\\pdfmapline{+x}\n")]);
        assert!(matches!(in_sty.map(|c| c.why), Some(Why::Primitive { file, .. }) if file == "fonts.sty"));
        let in_chapter = chosen(&[("main.tex", main), ("chapters/one.tex", "\n\\pdfinfo{/Title (x)}\n")]);
        assert!(matches!(in_chapter.map(|c| c.why), Some(Why::Primitive { file, line: 2, .. }) if file == "chapters/one.tex"));
        let in_class = chosen(&[("main.tex", main), ("thesis.cls", "\\pdfcompresslevel=9\n")]);
        assert!(in_class.is_some());
        // A file it does not read does not count.
        assert_eq!(chosen(&[("main.tex", main), ("other.tex", "\\pdfmapfile{+a}\n")]), None);
    }

    #[test]
    fn the_settings_choice_stays() {
        let mut job = Job {
            files: files(&[("main.tex", "\\pdfmapfile{+a.map}\n")]).into_iter().map(|(p, b)| (p, std::sync::Arc::from(b))).collect(),
            main: ProjectPath::new("main.tex").unwrap(),
            options: crate::Options { engine: Engine::LuaLatex, ..crate::Options::default() },
            id: 0,
            project: None,
        };
        assert_eq!(job.choose_engine(), None);
        assert_eq!(job.options.engine, Engine::LuaLatex);
        job.options.engine = Engine::Auto;
        assert!(job.choose_engine().is_some());
        assert_eq!(job.options.engine, Engine::PdfLatex);
    }
}
