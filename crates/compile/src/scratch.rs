//! A scratch document typeset on its own (docs/ROADMAP.md, AI4): LaTeX a model wrote, after the
//! project's preamble, built once in process in a copy of the project (so its packages, inputs
//! and images are found), apart from the shown build's files. Its PDF is the preview; when it does
//! not typeset, retex's messages say why, with their lines counted in the body that was given.

use std::path::Path;
use std::sync::Arc;

use texcraft_project::ProjectPath;

/// The name of the scratch document in the copy of the project, beside the main document (TeX
/// writes no file whose name starts with a dot).
const NAME: &str = "texcraft-ai-preview.tex";

/// What to typeset: the project's files, the main document's folder (`None` at the root), the
/// preamble and the body.
pub struct Scratch {
    pub files: Vec<(ProjectPath, Arc<[u8]>)>,
    pub main_dir: Option<String>,
    pub preamble: String,
    pub body: String,
}

impl Scratch {
    /// The document built: the preamble, then the body on pages without headers or numbers.
    pub fn document(&self) -> String {
        format!("{}\n\\begin{{document}}\n\\pagestyle{{empty}}\n{}\n\\end{{document}}\n", self.preamble.trim_end(), self.body.trim_end())
    }

    /// The line of the document where the body starts (from 1).
    fn body_line(&self) -> u32 {
        u32::try_from(self.preamble.trim_end().lines().count()).unwrap_or(u32::MAX).saturating_add(3)
    }
}

/// Typesets `scratch` in `dir` (emptied first): its PDF, or retex's messages for the user.
pub fn typeset(dir: &Path, scratch: &Scratch) -> Result<Vec<u8>, Vec<String>> {
    let one = |e: String| vec![e];
    let src = dir.join("src");
    match std::fs::remove_dir_all(&src) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(one(format!("deleting {}: {e}", src.display()))),
        _ => {}
    }
    for (path, bytes) in &scratch.files {
        let target = src.join(path.as_str());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| one(format!("creating {}: {e}", parent.display())))?;
        }
        std::fs::write(&target, bytes).map_err(|e| one(format!("writing {}: {e}", target.display())))?;
    }
    let folder = match &scratch.main_dir {
        Some(d) => src.join(d),
        None => src.clone(),
    };
    std::fs::create_dir_all(&folder).map_err(|e| one(format!("creating {}: {e}", folder.display())))?;
    let main = folder.join(NAME);
    std::fs::write(&main, scratch.document()).map_err(|e| one(format!("writing {}: {e}", main.display())))?;
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        retex::Retex::new().passes(1).halt_on_error(true).keep_files(dir.join("out")).compile_file(&main)
    }));
    let doc = match built {
        Ok(Ok(doc)) => doc,
        Ok(Err(retex::Error::Failed { diagnostics, .. })) => return Err(messages(&diagnostics, scratch.body_line())),
        Ok(Err(e)) => return Err(one(e.to_string())),
        Err(_) => return Err(one("The typesetting engine stopped unexpectedly.".to_owned())),
    };
    let errors = messages(doc.diagnostics(), scratch.body_line());
    if !errors.is_empty() {
        return Err(errors);
    }
    if doc.pages() == 0 {
        return Err(one("It typeset to nothing: there is no page.".to_owned()));
    }
    Ok(doc.pdf().to_vec())
}

/// The errors among `diagnostics`, those in the scratch document with their line in the body.
fn messages(diagnostics: &[retex::Diagnostic], body_line: u32) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.severity == retex::Severity::Error)
        .map(|d| {
            let ours = d.file.as_deref().is_none_or(|f| f.ends_with(NAME));
            match d.line {
                Some(l) if ours && l >= body_line => format!("line {}: {}", l - body_line + 1, d.message),
                _ => d.message.clone(),
            }
        })
        .collect()
}
