//! Choosing files and folders. The interface does not show native pickers itself: it queues a
//! [`PickRequest`], the shell shows it (the desktop app through craft-dialogs, which never blocks
//! the event loop) and hands the answer back with [`App::picked`]. A shell without pickers (the
//! web app, for now) leaves them off, and the interface asks for a typed path instead.

use std::path::PathBuf;

use serde_json::json;

use crate::App;

/// What a pick is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    OpenProject,
    SaveProjectAs,
    SavePdf,
    AddFiles,
}

/// The kind of native dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Folder,
    OpenFiles,
    SaveFile,
}

#[derive(Debug, Clone)]
pub struct PickRequest {
    pub pick: Pick,
    pub kind: PickKind,
    /// Translated.
    pub title: String,
    pub directory: Option<PathBuf>,
    pub file_name: Option<String>,
    /// (translated name, extensions) of a file-type filter.
    pub filter: Option<(String, Vec<&'static str>)>,
}

impl App {
    /// Whether the shell shows native pickers for [`App::take_picks`].
    pub fn set_native_pickers(&mut self, on: bool) {
        self.native_pickers = on;
    }

    /// The picks to show, oldest first.
    pub fn take_picks(&mut self) -> Vec<PickRequest> {
        std::mem::take(&mut self.picks)
    }

    /// The answer to a pick: the paths chosen (never empty).
    pub fn picked(&mut self, pick: Pick, paths: Vec<PathBuf>) {
        let Some(first) = paths.first().map(|p| p.display().to_string()) else { return };
        match pick {
            Pick::OpenProject => self.open_project(&first),
            Pick::SaveProjectAs => {
                if self.run("project.save_as", json!({"path": first})).is_some() {
                    self.remember_project();
                }
            }
            Pick::SavePdf => {
                self.run("build.save_pdf", json!({"path": first}));
            }
            Pick::AddFiles => {
                for path in paths {
                    self.run("file.import", json!({"source": path.display().to_string()}));
                }
                self.view.last_edit = Some(0.0);
            }
        }
    }

    /// A pick the shell could not show: its reason goes to the status line.
    pub fn pick_failed(&mut self, reason: String) {
        self.view.status = Some((reason, true));
    }

    /// Queues `pick` with the project's folder as the starting place.
    pub(crate) fn request_pick(&mut self, pick: Pick) {
        let directory = self.session.dir().cloned();
        let (kind, title, file_name, filter) = match pick {
            Pick::OpenProject => (PickKind::Folder, tl!("Open Project Folder"), None, None),
            Pick::SaveProjectAs => (PickKind::Folder, tl!("Save Project to Folder"), None, None),
            Pick::SavePdf => {
                let stem = self
                    .session
                    .project()
                    .main()
                    .map_or("document".to_owned(), |m| m.file_name().rsplit_once('.').map_or(m.file_name(), |(s, _)| s).to_owned());
                (PickKind::SaveFile, tl!("Save PDF As"), Some(format!("{stem}.pdf")), Some((tl!("PDF documents").to_owned(), vec!["pdf"])))
            }
            Pick::AddFiles => (PickKind::OpenFiles, tl!("Add Files to the Project"), None, None),
        };
        self.picks.push(PickRequest { pick, kind, title: title.to_owned(), directory, file_name, filter });
    }
}
