//! Going between the source and the PDF (docs/ROADMAP.md), through the SyncTeX data of the
//! last build that wrote some (`retex-synctex`).
//!
//! SyncTeX names a file as the document named it, relative to the main document's folder
//! (`sections/intro`, `sections/intro.tex`); the project names it from the project's root. This
//! module turns one into the other.

use retex_synctex::Synctex;
use texcraft_project::{Project, ProjectPath};

/// A rectangle of a PDF page, in PDF points from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PageRect {
    /// From 0.
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The parsed data of a build.
pub(crate) struct Sync {
    data: Synctex,
}

/// The folder of the main document, as a prefix (`paper/`), empty at the project's root.
fn main_dir(project: &Project) -> String {
    project.main().and_then(|m| m.as_str().rsplit_once('/')).map(|(d, _)| format!("{d}/")).unwrap_or_default()
}

impl Sync {
    pub fn parse(data: &[u8]) -> Option<Self> {
        Synctex::parse(data).ok().map(|data| Self { data })
    }

    /// The project file and line (from 1) that made point (`x`, `y`) of `page` (from 0).
    pub fn source(&self, project: &Project, page: u32, x: f32, y: f32) -> Option<(ProjectPath, u32)> {
        let place = self.data.inverse(page, x, y)?;
        let name = place.file.trim_start_matches("./");
        let dir = main_dir(project);
        let candidates = [format!("{dir}{name}"), format!("{dir}{name}.tex"), name.to_owned(), format!("{name}.tex")];
        let path = candidates.iter().filter_map(|c| ProjectPath::new(c).ok()).find(|p| project.get(p).is_some())?;
        Some((path, place.line))
    }

    /// Where line `line` (from 1) of `path` is in the PDF.
    pub fn locate(&self, project: &Project, path: &ProjectPath, line: u32) -> Vec<PageRect> {
        let dir = main_dir(project);
        let name = path.as_str().strip_prefix(&dir).unwrap_or(path.as_str());
        let mut found = self.data.forward(name, line, None);
        if found.is_empty()
            && let Some(bare) = name.strip_suffix(".tex")
        {
            found = self.data.forward(bare, line, None);
        }
        found.into_iter().map(|(page, r)| PageRect { page, x: r.x, y: r.y, width: r.width, height: r.height }).collect()
    }
}
