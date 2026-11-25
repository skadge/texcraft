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
    /// The folder to make a folder named as the untitled project in, and save it there.
    SaveProjectIn,
    SavePdf,
    /// Where to save one of the last build's files (`build.save_output`), by its kind.
    SaveOutput(&'static str),
    /// Where to write an export of the project.
    Export(texcraft_engine::Export),
    AddFiles,
    /// A new or empty folder for a project made from a template.
    TemplateFolder,
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
                    if let Some(next) = self.after_save.take() {
                        self.proceed(next);
                    }
                }
            }
            Pick::SaveProjectIn => {
                let Some(name) = self.pending_name.take() else { return };
                let folder = std::path::Path::new(&first).join(&name);
                if folder.exists() {
                    let said = tl!("There is already a folder named {name} there: choose another place or name.");
                    self.view.status = Some((said.replace("{name}", &name), true));
                } else if self.run("project.save_as", json!({"path": folder.display().to_string()})).is_some() {
                    self.remember_project();
                    self.view.status = Some((tl!("The project is saved as {name}.").replace("{name}", &name), false));
                }
            }
            Pick::SavePdf => {
                self.run("build.save_pdf", json!({"path": first}));
            }
            Pick::Export(format) => self.export_to(format, &first),
            Pick::SaveOutput(kind) => self.save_output(kind, &first),
            Pick::TemplateFolder => self.template_folder_chosen(first),
            Pick::AddFiles => {
                for path in paths {
                    self.run("file.import", json!({"source": path.display().to_string()}));
                }
                self.view.last_edit = Some(0.0);
            }
        }
    }

    /// Files the user chose or dropped, by name and contents (the browser has no paths): each is
    /// added to the project at the top level, replacing a file of the same name.
    pub fn add_files(&mut self, files: Vec<(String, Vec<u8>)>) {
        use base64::Engine as _;
        let mut added = 0;
        for (name, bytes) in files {
            let name = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_owned();
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            if self.run("file.import", json!({"path": name, "data": data})).is_some() {
                added += 1;
            }
        }
        if added > 0 {
            self.view.last_edit = Some(0.0);
            self.view.status = Some((tl!("Files added to the project: {n}").replace("{n}", &added.to_string()), false));
        }
    }

    /// Files dropped on the window this frame: added to the project (from their path on the
    /// desktop, their contents in the browser). While files are dragged over, says what a drop does.
    pub(crate) fn dropped_files(&mut self, ctx: &egui::Context) {
        let (dropped, hovering) = ctx.input(|i| (i.raw.dropped_files.clone(), !i.raw.hovered_files.is_empty()));
        if self.start {
            return;
        }
        if hovering {
            let t = self.tokens;
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop-files")));
            painter.rect_filled(screen, 0.0, t.canvas.gamma_multiply(0.85));
            painter.rect_stroke(screen.shrink(12.0), t.corner(), egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
            painter.text(screen.center(), egui::Align2::CENTER_CENTER, tl!("Drop to add the files to the project"), t.title(), t.text);
        }
        for file in dropped {
            // On the desktop a dropped file has its path on disk; in the browser only a name, and
            // its contents are read asynchronously by the shell (`take_dropped_files`).
            if cfg!(target_arch = "wasm32") {
                self.dropped.push(file);
            } else {
                let imported = self.run("file.import", json!({"source": file.path().display().to_string()}));
                self.view.last_edit = Some(0.0);
                if let Some(path) = imported.as_ref().and_then(|r| r["path"].as_str()) {
                    self.figure_for_drop(path);
                }
            }
        }
    }

    /// An image dropped while the formatted view is shown becomes a figure after the block holding
    /// the caret (`figure.insert`), its caption left for the figure dialog.
    fn figure_for_drop(&mut self, image: &str) {
        let Some(path) = self.session.current().cloned() else { return };
        let is_image = texcraft_engine::project::ProjectPath::new(image).is_ok_and(|p| texcraft_engine::cmd::is_image(&p));
        if !is_image || !self.editor.formatted.shows(&path) {
            return;
        }
        crate::editor::send_edits(self);
        let Ok(text) = self.session.project().text(&path).map(str::to_owned) else { return };
        let caret = crate::formatted_edit::caret_byte(self).min(text.len());
        let blocks = texcraft_latex::view::view(&text);
        let at = blocks.iter().find(|b| b.range.end >= caret).map_or(text.len(), |b| b.range.end);
        let stem = image.rsplit('/').next().unwrap_or(image).rsplit_once('.').map_or(image, |(s, _)| s).to_owned();
        let label: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')).collect();
        let chars = text.get(..at).map_or(0, |s| s.chars().count());
        let params = json!({"path": path.as_str(), "at": chars, "image": image, "width": 0.8, "placement": "htbp", "label": format!("fig:{label}")});
        if self.run("figure.insert", params).is_some() {
            let said = tl!("{image} is in a new figure: give it a caption with Edit Figure….").replace("{image}", image);
            self.view.status = Some((said, false));
        }
    }

    /// Files dropped in the browser, for the shell to read and give to [`App::add_files`].
    pub fn take_dropped_files(&mut self) -> Vec<egui::DroppedFileHandle> {
        std::mem::take(&mut self.dropped)
    }

    /// A pick the shell could not show: its reason goes to the status line.
    pub fn pick_failed(&mut self, reason: String) {
        self.view.status = Some((reason, true));
    }

    /// The main document's name without its extension, for a suggested file name.
    pub(crate) fn main_stem(&self) -> String {
        self.session.project().main().map_or("document".to_owned(), |m| m.file_name().rsplit_once('.').map_or(m.file_name(), |(s, _)| s).to_owned())
    }

    /// Exports the project to `path` and says on the status line what was written. It blocks
    /// while retex builds and converts the document.
    pub(crate) fn export_to(&mut self, format: texcraft_engine::Export, path: &str) {
        // On its thread: the window stays usable; `poll_export` says when it is done.
        if self.run("build.export", json!({"format": format, "path": path, "background": true})).is_some() {
            let label = crate::preview::export_label(format);
            self.view.status = Some((tl!("Exporting as {format}…").replace("{format}", label), false));
        }
    }

    /// Saves the last build's output of `kind` to `path` (in the browser, downloads it under that
    /// name) and says so on the status line.
    pub(crate) fn save_output(&mut self, kind: &str, path: &str) {
        let Some(done) = self.run("build.save_output", json!({"output": kind, "path": path})) else { return };
        let written = done["path"].as_str().unwrap_or(path);
        let said = if done["download"].as_bool() == Some(true) {
            tl!("Downloading {file}.").replace("{file}", written)
        } else {
            tl!("Saved to {path}.").replace("{path}", written)
        };
        self.view.status = Some((said, false));
    }

    /// Queues `pick` with the project's folder as the starting place.
    pub(crate) fn request_pick(&mut self, pick: Pick) {
        let directory = self.session.dir().cloned();
        let (kind, title, file_name, filter) = match pick {
            Pick::OpenProject => (PickKind::Folder, tl!("Open Project Folder"), None, None),
            Pick::SaveProjectAs => (PickKind::Folder, tl!("Save Project to Folder"), None, None),
            Pick::SaveProjectIn => (PickKind::Folder, tl!("Choose Where to Make the Project's Folder"), self.pending_name.clone(), None),
            Pick::SavePdf => {
                let stem = self.main_stem();
                (PickKind::SaveFile, tl!("Save PDF As"), Some(format!("{stem}.pdf")), Some((tl!("PDF documents").to_owned(), vec!["pdf"])))
            }
            Pick::Export(format) => {
                let (stem, ext) = (self.main_stem(), format.extension());
                (
                    PickKind::SaveFile,
                    tl!("Export As"),
                    Some(format!("{stem}.{ext}")),
                    Some((crate::preview::export_label(format).to_owned(), vec![ext])),
                )
            }
            Pick::SaveOutput(kind) => {
                let name = self.session.outputs().into_iter().find(|o| o.kind == kind).map(|o| o.name);
                let ext = name.as_deref().and_then(|n| n.rsplit_once('.')).map(|(_, e)| e.to_owned()).unwrap_or_default();
                let filter = (!ext.is_empty()).then(|| (tl!("Files of this kind").to_owned(), vec![output_extension(&ext)]));
                (PickKind::SaveFile, tl!("Save Build Output As"), name, filter)
            }
            Pick::AddFiles => (PickKind::OpenFiles, tl!("Add Files to the Project"), None, None),
            Pick::TemplateFolder => (PickKind::Folder, tl!("Choose a New or Empty Folder for the Project"), None, None),
        };
        self.picks.push(PickRequest { pick, kind, title: title.to_owned(), directory, file_name, filter });
    }
}

/// An output's extension as a `'static` file-type filter.
fn output_extension(ext: &str) -> &'static str {
    match ext {
        "pdf" => "pdf",
        "synctex" => "synctex",
        _ => "log",
    }
}
