//! TeXCraft in the browser. Build with `trunk build --release` from this folder.
//!
//! Builds run retex's WebAssembly module in a Web Worker (`engine-worker`, docs/plan.md, phase 6);
//! the module is TeXCraft's own build of the pinned retex (`cargo xtask retex-wasm`).

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("texcraft-web runs only in the browser: build it with `trunk build` (apps/texcraft-web).");
}

#[cfg(target_arch = "wasm32")]
mod store;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use texcraft_engine::library::{Change, Library};
    use texcraft_ui_egui::{App, Pick};
    use wasm_bindgen::JsCast;

    use crate::store::Store;

    /// Files chosen in the browser's file picker, waiting for the next frame.
    type Chosen = Rc<RefCell<Vec<(String, Vec<u8>)>>>;
    /// Errors of the browser's storage, for the status line.
    type Errors = Rc<RefCell<Vec<String>>>;
    /// Templates fetched (their files, or why not), waiting for the next frame.
    type Fetched = Rc<RefCell<Vec<(String, Result<Vec<(String, Vec<u8>)>, String>)>>>;

    /// The bytes at `url`, relative to the page.
    async fn fetch(url: &str) -> Result<Vec<u8>, String> {
        use wasm_bindgen_futures::JsFuture;
        let window = web_sys::window().ok_or("no window")?;
        let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
            .await
            .map_err(|e| format!("{url}: {e:?}"))?
            .dyn_into()
            .map_err(|_| format!("{url}: no response"))?;
        if !response.ok() {
            return Err(format!("{url}: HTTP {}", response.status()));
        }
        let buffer = JsFuture::from(response.array_buffer().map_err(|e| format!("{url}: {e:?}"))?).await.map_err(|e| format!("{url}: {e:?}"))?;
        Ok(js_sys::Uint8Array::new(&buffer).to_vec())
    }

    /// Template `id`'s files, from the web build's own site: `templates/<id>/files.txt` lists
    /// them (`cargo xtask templates-site`).
    async fn fetch_template(id: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
        let list = fetch(&format!("templates/{id}/files.txt"))
            .await
            .map_err(|e| format!("{} ({e})", texcraft_ui_egui::i18n::t("This build of TeXCraft was published without its templates.")))?;
        let mut files = Vec::new();
        for path in String::from_utf8_lossy(&list).lines().filter(|l| !l.is_empty() && !l.split('/').any(|p| p == "..")) {
            files.push((path.to_owned(), fetch(&format!("templates/{id}/{path}")).await?));
        }
        Ok(files)
    }

    struct Shell {
        app: App,
        chosen: Chosen,
        started: bool,
        store: Option<Store>,
        errors: Errors,
        fetched: Fetched,
    }

    impl Shell {
        /// Stores what changed this frame: the projects saved or deleted, the preferences and the
        /// thumbnails. Each write runs on its own; a failed one is said on the status line.
        fn keep(&mut self, ctx: &egui::Context) {
            let changes = self.app.take_library_changes();
            let prefs = self.app.take_changed_prefs();
            let thumbnails = self.app.take_thumbnails();
            if let Some(store) = &self.store {
                for change in changes {
                    let (store, errors, ctx) = (store.clone(), self.errors.clone(), ctx.clone());
                    wasm_bindgen_futures::spawn_local(async move {
                        use texcraft_ui_egui::i18n::t;
                        let project_failed = t("{project} could not be kept in this browser: {error}");
                        let (name, result, what) = match &change {
                            Change::Saved(name, project) => (name, store.put_project(name, project).await, project_failed),
                            Change::Deleted(name) => (name, store.delete_project(name).await, project_failed),
                            Change::Thread { project, file, text } => (
                                project,
                                store.put_thread(project, file, text.as_deref()).await,
                                t("A comment of {project} could not be kept in this browser: {error}"),
                            ),
                        };
                        if let Err(e) = result {
                            errors.borrow_mut().push(what.replace("{project}", name).replace("{error}", &e));
                            ctx.request_repaint();
                        }
                    });
                }
                if let Some(json) = prefs.and_then(|p| serde_json::to_string(&p).ok()) {
                    let (store, errors) = (store.clone(), self.errors.clone());
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Err(e) = store.put_prefs(&json).await {
                            errors
                                .borrow_mut()
                                .push(format!("{} {e}", texcraft_ui_egui::i18n::t("The preferences could not be kept in this browser:")));
                        }
                    });
                }
                for thumbnail in thumbnails {
                    let store = store.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Err(e) = store.put_thumbnail(&thumbnail).await {
                            log::warn!("a thumbnail could not be kept: {e}");
                        }
                    });
                }
            }
            let errors = std::mem::take(&mut *self.errors.borrow_mut());
            if let Some(last) = errors.into_iter().last() {
                self.app.report(last, true);
            }
        }

        /// The browser's file picker for Add Files; the other picks name folders and paths, which
        /// the browser does not have.
        fn picks(&mut self, ctx: &egui::Context) {
            for request in self.app.take_picks() {
                match request.pick {
                    Pick::AddFiles => {
                        let chosen = self.chosen.clone();
                        let ctx = ctx.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let Some(handles) = rfd::AsyncFileDialog::new().set_title(&request.title).pick_files().await else { return };
                            for handle in handles {
                                let bytes = handle.read().await;
                                chosen.borrow_mut().push((handle.file_name(), bytes));
                            }
                            ctx.request_repaint();
                        });
                    }
                    _ => {
                        self.app.pick_failed(texcraft_ui_egui::i18n::t("Folders and paths on disk are not available in the browser yet.").to_owned())
                    }
                }
            }
            for file in self.app.take_dropped_files() {
                let chosen = self.chosen.clone();
                let errors = self.errors.clone();
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let name = file.path().display().to_string();
                    log::info!("dropped: {name}");
                    match file.bytes_async().await {
                        Ok(bytes) => chosen.borrow_mut().push((name, bytes)),
                        Err(e) => {
                            log::warn!("the dropped file {name} could not be read: {e}");
                            let what = texcraft_ui_egui::i18n::t("The browser did not let TeXCraft read {file}: {error}");
                            errors.borrow_mut().push(what.replace("{file}", &name).replace("{error}", &e));
                        }
                    }
                    ctx.request_repaint();
                });
            }
            for id in self.app.take_template_requests() {
                let fetched = self.fetched.clone();
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = fetch_template(&id).await;
                    fetched.borrow_mut().push((id, result));
                    ctx.request_repaint();
                });
            }
            for (id, result) in std::mem::take(&mut *self.fetched.borrow_mut()) {
                self.app.template_fetched(id, result);
            }
            let files = std::mem::take(&mut *self.chosen.borrow_mut());
            if !files.is_empty() {
                self.app.add_files(files);
            }
        }
    }

    impl eframe::App for Shell {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            if !self.started {
                self.started = true;
                hide_loading_screen();
            }
            self.app.frame(ui);
            let ctx = ui.ctx().clone();
            self.picks(&ctx);
            self.keep(&ctx);
        }
    }

    /// The page's loading screen (index.html) goes once the first frame is drawn.
    fn hide_loading_screen() {
        if let Some(e) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("loading")) {
            e.remove();
        }
    }

    /// The engine worker's script, made by Trunk (`index.html`).
    const WORKER: &str = "engine-worker_loader.js";

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        texcraft_ui_egui::i18n::use_system_language();
        wasm_bindgen_futures::spawn_local(async {
            let Some(canvas) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("texcraft_canvas"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                log::error!("no #texcraft_canvas element");
                return;
            };
            let mut session = texcraft_engine::Session::in_browser(WORKER);
            let errors = Errors::default();
            let store = match Store::open().await {
                Ok(store) => Some(store),
                Err(e) => {
                    let what = texcraft_ui_egui::i18n::t("Projects cannot be kept in this browser:");
                    errors.borrow_mut().push(format!("{what} {e}"));
                    None
                }
            };
            let mut prefs = None;
            let mut thumbnails = Vec::new();
            if let Some(store) = &store {
                let mut library = Library::default();
                match store.projects().await {
                    Ok(projects) => projects.into_iter().for_each(|(name, project)| library.insert(name, project)),
                    Err(e) => {
                        errors.borrow_mut().push(format!("{} {e}", texcraft_ui_egui::i18n::t("The projects kept in this browser could not be read:")))
                    }
                }
                session.use_library(library);
                prefs = store.prefs().await.ok().flatten().and_then(|json| serde_json::from_str(&json).ok());
                thumbnails = store.thumbnails().await.unwrap_or_default();
            }
            let mut app = match prefs {
                Some(prefs) => App::with_prefs(session, prefs),
                None => App::new(session),
            };
            for thumbnail in thumbnails {
                app.set_thumbnail(thumbnail);
            }
            app.set_native_pickers(true);
            app.show_start_page();
            let shell = Shell { app, chosen: Chosen::default(), started: false, store, errors, fetched: Fetched::default() };
            let started = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(|_cc| Ok(Box::new(shell)))).await;
            if let Err(e) = started {
                log::error!("TeXCraft could not start: {e:?}");
            }
        });
    }
}
