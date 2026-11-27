//! The documents the property runs edit: TeXCraft's own templates and examples, the crate's
//! fixtures (`formatted-*.tex` written for these runs: lists, tables, figures, theorems, styles,
//! formulas, references; theorems, deep lists, tables with merged cells, code, a bibliography, a
//! beamer talk; a talk with overlays, columns and blocks, a report with a title page, macros and a
//! bibliography), and the `.tex` files under `TEXCRAFT_FUZZ_CORPUS`, read at run time and
//! never committed.

use std::path::Path;

pub struct Doc {
    pub name: String,
    pub text: String,
}

fn doc(name: &str, text: &str) -> Doc {
    Doc { name: name.to_owned(), text: text.to_owned() }
}

/// The committed documents.
pub fn committed() -> Vec<Doc> {
    vec![
        doc("mixed", include_str!("../../fixtures/formatted-mixed.tex")),
        doc("lists", include_str!("../../fixtures/formatted-lists.tex")),
        doc("fragment", include_str!("../../fixtures/formatted-fragment.tex")),
        doc("structures", include_str!("../../fixtures/formatted-structures.tex")),
        doc("beamer", include_str!("../../fixtures/formatted-beamer.tex")),
        doc("talk", include_str!("../../fixtures/formatted-talk.tex")),
        doc("report", include_str!("../../fixtures/formatted-report.tex")),
        doc("article", include_str!("../../../../../templates/builtin/article/main.tex")),
        doc("example", include_str!("../../../../../templates/builtin/example/main.tex")),
        doc("example/introduction", include_str!("../../../../../templates/builtin/example/sections/introduction.tex")),
        doc("thesis", include_str!("../../../../../templates/builtin/thesis/main.tex")),
        doc("thesis/introduction", include_str!("../../../../../templates/builtin/thesis/chapters/introduction.tex")),
        doc("thesis/background", include_str!("../../../../../templates/builtin/thesis/chapters/background.tex")),
        doc("homework", include_str!("../../../../../templates/builtin/homework/main.tex")),
        doc("letter", include_str!("../../../../../templates/builtin/letter/main.tex")),
        doc("cv", include_str!("../../../../../templates/builtin/cv/main.tex")),
        doc("slides", include_str!("../../../../../templates/builtin/slides/main.tex")),
        doc("examples/article", include_str!("../../../../../examples/article/main.tex")),
        doc("examples/intro", include_str!("../../../../../examples/article/sections/intro.tex")),
        doc("frontmatter", include_str!("../../fixtures/frontmatter.tex")),
    ]
}

/// The `.tex` files under `dir` (at any depth, up to `limit` of them, each under 400 KB).
pub fn local(dir: &Path, limit: usize) -> Vec<Doc> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if out.len() >= limit {
                return out;
            }
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|e| e == "tex")
                && std::fs::metadata(&p).is_ok_and(|m| m.len() < 400_000)
                && let Ok(bytes) = std::fs::read(&p)
            {
                let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
                out.push(Doc { name: p.display().to_string(), text });
            }
        }
    }
    out
}
