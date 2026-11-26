//! Files, folders and zip files dropped on the window, chosen with Add Files or Import a Zip File.
//!
//! A drop becomes a queue of jobs. Files and folders are added through `file.import`; a zip file
//! is asked about first (unzip it into the project, which is the default, or add the zip itself).
//! Before anything is added the import is checked (`check`): when files of the same paths are in
//! the project already, one question for the whole drop says Replace, Keep Both or Skip. The
//! status line then says what came in and what was left out, and why.
//!
//! On the start page a dropped folder opens as a project (on the desktop), and a zip file or a
//! folder the browser read becomes a new project (`project.import`), which is then saved where the
//! user chooses: a folder on the desktop, a name in the browser.

use std::collections::VecDeque;
use std::path::PathBuf;

use egui::RichText;
use serde_json::{Value, json};

use crate::App;
use crate::dialogs::{Dialog, Next, actions, title};
use crate::theme::Tokens;

/// Something dropped or chosen.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    /// A file or folder on disk (the desktop).
    Path(PathBuf),
    /// A file by its path and contents (the browser: a folder's files have the folder's name
    /// and their subfolders in their path).
    Bytes(String, Vec<u8>),
}

impl Item {
    /// A zip file at the top level of the drop (a zip inside a dropped folder is just a file).
    fn zip_name(&self) -> Option<String> {
        let name = match self {
            Item::Path(path) if path.is_file() => path.file_name()?.to_str()?.to_owned(),
            Item::Bytes(name, _) if !name.contains('/') => name.clone(),
            _ => return None,
        };
        name.to_ascii_lowercase().ends_with(".zip").then_some(name)
    }
}

/// Commands to run for one part of a drop, each with its parameters.
type Runs = Vec<(&'static str, Value)>;

enum Job {
    /// A zip file: unzip it (these parameters for `file.unzip`) or add it as a file (these for
    /// `file.import`)?
    Zip { name: String, unzip: Value, as_file: Value },
    /// Imports to run; asked about first when some of their files are in the project already.
    Import { runs: Runs, figure: bool },
}

enum Step {
    Zip,
    /// Files of the project the imports would replace.
    Existing(Vec<String>),
}

/// The drop dialog: the job it asks about and the ones after it.
pub(crate) struct DropForm {
    jobs: VecDeque<Job>,
    step: Step,
    focus: bool,
}

/// What the imports of one drop did, added up.
#[derive(Default)]
struct Tally {
    files: usize,
    replaced: usize,
    renamed: usize,
    skipped: Vec<String>,
    /// The one file added, for a figure when it is an image.
    only: Option<String>,
}

impl App {
    /// Brings `items` in: into the project, or on the start page as a project.
    pub(crate) fn import_items(&mut self, items: Vec<Item>, figure: bool) {
        if items.is_empty() {
            return;
        }
        if self.start {
            self.start_with(items);
            return;
        }
        let mut jobs = VecDeque::new();
        let mut runs: Runs = Vec::new();
        let mut folder_files = Vec::new();
        let mut zips = Vec::new();
        for item in items {
            if let Some(name) = item.zip_name() {
                zips.push(zip_job(name, item));
                continue;
            }
            match item {
                Item::Path(path) => runs.push(("file.import", json!({"source": path.display().to_string()}))),
                Item::Bytes(name, bytes) if name.contains('/') => folder_files.push(json!({"path": name, "data": base64(&bytes)})),
                Item::Bytes(name, bytes) => runs.push(("file.import", json!({"path": name, "data": base64(&bytes)}))),
            }
        }
        if !folder_files.is_empty() {
            runs.push(("file.import", json!({"files": folder_files})));
        }
        if !runs.is_empty() {
            jobs.push_back(Job::Import { runs, figure });
        }
        jobs.extend(zips);
        self.continue_drop(jobs);
    }

    /// Unzips chosen zip files into the project, without asking whether to (Import a Zip File).
    pub(crate) fn unzip_items(&mut self, items: Vec<Item>) {
        let runs: Runs = items
            .into_iter()
            .map(|item| match item {
                Item::Path(path) => ("file.unzip", json!({"source": path.display().to_string()})),
                Item::Bytes(_, bytes) => ("file.unzip", json!({"data": base64(&bytes)})),
            })
            .collect();
        self.continue_drop(VecDeque::from([Job::Import { runs, figure: false }]));
    }

    /// A zip file the shell read for Import a Zip File (the browser): unzipped into the project.
    pub fn unzip_file(&mut self, name: String, bytes: Vec<u8>) {
        self.unzip_items(vec![Item::Bytes(name, bytes)]);
    }

    /// Runs the jobs that need no question, until one does: it is then asked in the dialog.
    fn continue_drop(&mut self, mut jobs: VecDeque<Job>) {
        while let Some(job) = jobs.front_mut() {
            match job {
                Job::Zip { .. } => {
                    self.view.dialog = Some(Dialog::Drop(Box::new(DropForm { jobs, step: Step::Zip, focus: true })));
                    return;
                }
                Job::Import { runs, figure } => {
                    let existing = self.existing_files(runs);
                    if !existing.is_empty() {
                        self.view.dialog = Some(Dialog::Drop(Box::new(DropForm { jobs, step: Step::Existing(existing), focus: true })));
                        return;
                    }
                    let (runs, figure) = (std::mem::take(runs), *figure);
                    jobs.pop_front();
                    self.run_imports(runs, "replace", figure);
                }
            }
        }
    }

    /// The project's files `runs` would replace, from each run's check.
    fn existing_files(&mut self, runs: &Runs) -> Vec<String> {
        let mut existing = Vec::new();
        for (command, params) in runs {
            let mut checked = params.clone();
            checked["check"] = json!(true);
            // A check that fails says why on the status line; the import itself then fails the same way.
            if let Ok(done) = self.session.execute(command, &checked) {
                existing.extend(done["existing"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)));
            }
        }
        existing
    }

    /// Runs the imports with `existing` (`replace`, `keep_both` or `skip`) and says what they did.
    fn run_imports(&mut self, runs: Runs, existing: &str, figure: bool) {
        let mut tally = Tally::default();
        let mut failed = None;
        for (command, mut params) in runs {
            params["existing"] = json!(existing);
            match self.run(command, params) {
                Some(done) => tally.add(&done),
                None => failed = self.view.status.take(),
            }
        }
        self.view.last_edit = Some(0.0);
        match (tally.files + tally.skipped.len(), failed) {
            (0, Some(failed)) => self.view.status = Some(failed),
            (_, failed) => {
                let (mut said, error) = tally.said();
                if let Some((why, _)) = failed {
                    said = format!("{said} {}", crate::i18n::msg(&why));
                }
                self.view.status = Some((said, error));
            }
        }
        if figure
            && tally.files == 1
            && let Some(only) = tally.only
        {
            self.figure_for_drop(&only);
        }
    }

    /// The start page: a folder opens as a project; a zip file, or a folder's files from the
    /// browser, make a new project.
    fn start_with(&mut self, items: Vec<Item>) {
        let folder = items.iter().find_map(|i| match i {
            Item::Path(p) if p.is_dir() => Some(p.display().to_string()),
            _ => None,
        });
        if let Some(folder) = folder {
            self.guard(Next::OpenProject(folder));
            return;
        }
        if let Some(item) = items.iter().find(|i| i.zip_name().is_some()) {
            let name = item.zip_name().unwrap_or_default();
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s).to_owned();
            let params = match item {
                Item::Path(path) => json!({"source": path.display().to_string()}),
                Item::Bytes(_, bytes) => json!({"data": base64(bytes)}),
            };
            self.guard(Next::ImportProject(params, stem));
            return;
        }
        let files: Vec<Value> = items
            .iter()
            .filter_map(|i| match i {
                Item::Bytes(name, bytes) if name.contains('/') => Some(json!({"path": name, "data": base64(bytes)})),
                _ => None,
            })
            .collect();
        if let Some(Item::Bytes(name, _)) = items.iter().find(|i| matches!(i, Item::Bytes(n, _) if n.contains('/'))) {
            let stem = name.split('/').next().unwrap_or(name).to_owned();
            self.guard(Next::ImportProject(json!({"files": files}), stem));
            return;
        }
        let said = tl!("Drop a folder or a zip file here to make a project of it; files are added to a project once one is open.");
        self.view.status = Some((said.to_owned(), true));
    }

    /// Import a Project's Zip File in the browser: the zip the browser read (its name and
    /// contents) becomes a new project, kept in the browser under its name.
    pub fn import_zip_project(&mut self, name: String, bytes: Vec<u8>) {
        if matches!(self.view.dialog, Some(Dialog::Import(_))) {
            self.view.dialog = None;
        }
        let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s).to_owned();
        self.import_project(json!({"data": base64(&bytes)}), stem);
    }

    /// Import a Project's Folder in the browser: the files of the folder the browser read, by
    /// their path from the folder's name, become a new project kept in the browser.
    pub fn import_folder_project(&mut self, files: Vec<(String, Vec<u8>)>) {
        if matches!(self.view.dialog, Some(Dialog::Import(_))) {
            self.view.dialog = None;
        }
        let name = files.first().and_then(|(p, _)| p.replace('\\', "/").split('/').next().map(str::to_owned)).unwrap_or_else(|| "Project".to_owned());
        let files: Vec<Value> = files.iter().map(|(path, bytes)| json!({"path": path.replace('\\', "/"), "data": base64(bytes)})).collect();
        if files.is_empty() {
            self.view.status = Some((tl!("The folder chosen is empty: there is nothing to import.").to_owned(), true));
            return;
        }
        self.import_project(json!({"files": files}), name);
    }

    /// Makes the new project (`project.import`), then asks where to keep it, as `name`.
    pub(crate) fn import_project(&mut self, params: Value, name: String) {
        let Some(done) = self.run("project.import", params) else { return };
        self.start = false;
        self.pdf = crate::pdf_view::PdfView::default();
        self.view.last_edit = Some(0.0);
        let mut tally = Tally { files: done["files"].as_u64().unwrap_or(0) as usize, ..Tally::default() };
        tally.add_skipped(&done);
        let (said, error) = tally.said();
        self.view.status = Some((said, error));
        if self.session.library().is_none()
            && let Some(parent) = self.zip_into.take()
        {
            // Import a Project's Zip File: into a new folder where new projects go.
            let folder = texcraft_engine::free_folder(&parent, &name);
            if self.run("project.save_as", json!({"path": folder.display().to_string()})).is_some() {
                self.remember_project();
                let said = tl!("Imported {name} into {folder}.").replace("{name}", &name).replace("{folder}", &folder.display().to_string());
                self.view.status = Some((said, error));
            }
            return;
        }
        if self.session.library().is_some() {
            let prompt = crate::dialogs::Prompt {
                title: tl!("Keep the New Project"),
                label: tl!("A name for the project, kept in this browser"),
                value: name,
                action: tl!("Save"),
                command: "project.save_as",
                params: Box::new(|v| json!({"path": v})),
                focus: true,
            };
            self.view.dialog = Some(Dialog::Prompt(prompt));
        } else if self.native_pickers {
            self.pending_name = Some(name);
            self.request_pick(crate::picks::Pick::SaveProjectIn);
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn zip_job(name: String, item: Item) -> Job {
    match item {
        Item::Path(path) => {
            let source = path.display().to_string();
            Job::Zip { name, unzip: json!({"source": source}), as_file: json!({"source": source}) }
        }
        Item::Bytes(file, bytes) => {
            let data = base64(&bytes);
            Job::Zip { name, unzip: json!({"data": data}), as_file: json!({"path": file, "data": data}) }
        }
    }
}

impl Tally {
    fn add(&mut self, done: &Value) {
        let files = done["files"].as_array().map_or(0, Vec::len);
        if files == 1 {
            self.only = done["files"][0].as_str().map(str::to_owned);
        }
        self.files += files;
        self.replaced += done["replaced"].as_u64().unwrap_or(0) as usize;
        self.renamed += done["renamed"].as_array().map_or(0, Vec::len);
        self.add_skipped(done);
    }

    fn add_skipped(&mut self, done: &Value) {
        let reasons = done["skipped"].as_array().into_iter().flatten().filter_map(|s| s["reason"].as_str().map(str::to_owned));
        self.skipped.extend(reasons);
    }

    /// What the status line says, and whether it is a problem (nothing came in, or a limit cut
    /// the import short).
    fn said(&self) -> (String, bool) {
        let mut said = tl!("{files} brought in.").replace("{files}", &crate::i18n::trn(self.files, "{n} file", "{n} files"));
        if self.replaced > 0 {
            said.push(' ');
            said.push_str(&tl!("Replaced: {n}.").replace("{n}", &self.replaced.to_string()));
        }
        if self.renamed > 0 {
            said.push(' ');
            said.push_str(&tl!("Kept beside a file of the same name: {n}.").replace("{n}", &self.renamed.to_string()));
        }
        let count = |reasons: &[&str]| self.skipped.iter().filter(|r| reasons.contains(&r.as_str())).count();
        let groups = [
            (count(&["hidden"]), crate::i18n::trn(count(&["hidden"]), "{n} hidden", "{n} hidden")),
            (count(&["build_output"]), crate::i18n::trn(count(&["build_output"]), "{n} build output", "{n} build outputs")),
            (count(&["link"]), crate::i18n::trn(count(&["link"]), "{n} link", "{n} links")),
            (count(&["unsafe_name"]), crate::i18n::trn(count(&["unsafe_name"]), "{n} unsafe name", "{n} unsafe names")),
            (count(&["exists"]), crate::i18n::trn(count(&["exists"]), "{n} already in the project", "{n} already in the project")),
            (count(&["too_large"]), crate::i18n::trn(count(&["too_large"]), "{n} larger than 64 MB", "{n} larger than 64 MB")),
            (
                count(&["too_many", "total_too_large"]),
                crate::i18n::trn(
                    count(&["too_many", "total_too_large"]),
                    "{n} past the limit of 5,000 files or 500 MB",
                    "{n} past the limit of 5,000 files or 500 MB",
                ),
            ),
            (
                count(&["clash", "unreadable", "refused", "too_deep"]),
                crate::i18n::trn(
                    count(&["clash", "unreadable", "refused", "too_deep"]),
                    "{n} that could not be read or added",
                    "{n} that could not be read or added",
                ),
            ),
        ];
        let left: Vec<String> = groups.into_iter().filter(|(n, _)| *n > 0).map(|(_, s)| s).collect();
        if !left.is_empty() {
            said.push(' ');
            said.push_str(&tl!("Left out: {reasons}.").replace("{reasons}", &left.join(", ")));
        }
        let limited = count(&["too_many", "total_too_large", "too_large"]) > 0;
        (said, self.files == 0 || limited)
    }
}

/// The drop dialog, when it is open.
pub(crate) fn dialog(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let mut choice = None;
    let modal = crate::sheet::show(ctx, &t, "texcraft-drop", 440.0, crate::sheet::Leave::Close, |ui| {
        if let Some(Dialog::Drop(form)) = app.view.dialog.as_mut() {
            choice = ask(ui, &t, form);
        }
    });
    if modal.close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        choice = Some(Choice::Cancel);
    }
    let Some(choice) = choice else { return };
    let Some(Dialog::Drop(form)) = app.view.dialog.take() else { return };
    let mut jobs = form.jobs;
    let Some(job) = jobs.pop_front() else { return };
    match (job, choice) {
        (_, Choice::Cancel) => {}
        (Job::Zip { unzip, .. }, Choice::Unzip) => jobs.push_front(Job::Import { runs: vec![("file.unzip", unzip)], figure: false }),
        (Job::Zip { as_file, .. }, Choice::AsFile) => jobs.push_front(Job::Import { runs: vec![("file.import", as_file)], figure: false }),
        (Job::Import { runs, figure }, Choice::Existing(existing)) => app.run_imports(runs, existing, figure),
        // A choice another step offers: nothing to do.
        _ => {}
    }
    app.continue_drop(jobs);
}

enum Choice {
    Unzip,
    AsFile,
    /// `replace`, `keep_both` or `skip`.
    Existing(&'static str),
    Cancel,
}

fn ask(ui: &mut egui::Ui, t: &Tokens, form: &mut DropForm) -> Option<Choice> {
    let mut choice = None;
    let focus = std::mem::take(&mut form.focus);
    match (&form.step, form.jobs.front()) {
        (Step::Zip, Some(Job::Zip { name, .. })) => {
            title(ui, t, &tl!("Unzip {file}?").replace("{file}", name));
            ui.label(tl!("Its files and folders can be added to the project, or the zip file itself."));
            ui.add_space(t.unit * 4.0);
            actions(ui, |ui| {
                if primary(ui, t, tl!("Unzip into the Project"), focus).clicked() {
                    choice = Some(Choice::Unzip);
                }
                if ui.add(egui::Button::new(tl!("Add the Zip as a File")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    choice = Some(Choice::AsFile);
                }
                if ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        }
        (Step::Existing(paths), _) => {
            let n = paths.len();
            title(ui, t, &crate::i18n::trn(n, "{n} file is already in the project", "{n} files are already in the project"));
            let shown: Vec<&str> = paths.iter().take(5).map(String::as_str).collect();
            let mut list = shown.join(", ");
            if n > shown.len() {
                list.push(' ');
                list.push_str(&tl!("and {n} more").replace("{n}", &(n - shown.len()).to_string()));
            }
            ui.label(RichText::new(list).color(t.text_muted));
            ui.label(tl!("Replace them with the new ones, keep both (the new ones get another name), or skip the new ones?"));
            ui.add_space(t.unit * 4.0);
            actions(ui, |ui| {
                if primary(ui, t, tl!("Replace"), focus).clicked() {
                    choice = Some(Choice::Existing("replace"));
                }
                if ui.add(egui::Button::new(tl!("Keep Both")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    choice = Some(Choice::Existing("keep_both"));
                }
                if ui.add(egui::Button::new(tl!("Skip")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    choice = Some(Choice::Existing("skip"));
                }
                if ui.add(egui::Button::new(tl!("Cancel")).min_size(egui::vec2(80.0, 30.0))).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        }
        _ => choice = Some(Choice::Cancel),
    }
    choice
}

/// The default answer, which takes the keyboard when the question is asked.
fn primary(ui: &mut egui::Ui, t: &Tokens, label: &str, focus: bool) -> egui::Response {
    let text = RichText::new(label).color(t.on_accent).font(t.strong());
    let response = ui.add(egui::Button::new(text).fill(t.accent).stroke(egui::Stroke::NONE).min_size(egui::vec2(88.0, 30.0)));
    if focus {
        response.request_focus();
    }
    response
}
