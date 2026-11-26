//! TeXCraft in the browser. Build with `trunk build --release` from this folder.
//!
//! Builds run retex's WebAssembly module in a Web Worker (`engine-worker`, docs/ROADMAP.md);
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
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use texcraft_engine::library::{Change, Library};
    use texcraft_ui_egui::{App, BrowserStorage, Pick};
    use wasm_bindgen::JsCast;

    use crate::store::Store;

    /// Files chosen in the browser's file picker, waiting for the next frame.
    type Chosen = Rc<RefCell<Vec<(String, Vec<u8>)>>>;
    /// Errors of the browser's storage, for the status line.
    type Errors = Rc<RefCell<Vec<String>>>;
    /// The window's context once it draws, for work done outside a frame to ask for one.
    type Repaint = Rc<RefCell<Option<egui::Context>>>;
    /// The names of what a drop of folders brought, read by [`read_dropped_tree`]: eframe hands
    /// the same drop over as files, which are left out.
    type Taken = Rc<RefCell<Vec<String>>>;
    /// Writes the browser's storage refused (the change, and why), for the next frame.
    type Refused = Rc<RefCell<Vec<(Change, String)>>>;
    /// What the browser last said of the room for the projects, for the next frame.
    type Room = Rc<RefCell<Option<BrowserStorage>>>;
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

    /// Whether `promise` (of `navigator.storage`) resolved to true, false, or said nothing.
    async fn flag(promise: Result<js_sys::Promise, wasm_bindgen::JsValue>) -> Option<bool> {
        wasm_bindgen_futures::JsFuture::from(promise.ok()?).await.ok()?.as_bool()
    }

    /// What `navigator.storage` says of the site's data: whether the browser keeps it until the
    /// user deletes it, first asking it to (`ask`) when it does not, and the room used and allowed.
    /// A browser without the Storage API (or a page that is not a secure context) says nothing.
    async fn storage(ask: bool) -> BrowserStorage {
        let Some(manager) = web_sys::window().map(|w| w.navigator().storage()) else { return BrowserStorage::default() };
        let mut persisted = flag(manager.persisted()).await;
        if ask && persisted == Some(false) {
            persisted = flag(manager.persist()).await.or(persisted);
        }
        let estimate = match manager.estimate() {
            Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise).await.ok().map(|v| v.unchecked_into::<web_sys::StorageEstimate>()),
            Err(_) => None,
        };
        let used = estimate.as_ref().and_then(|e| e.get_usage()).map(|u| u as u64);
        let quota = estimate.as_ref().and_then(|e| e.get_quota()).map(|q| q as u64);
        BrowserStorage { persisted, used, quota }
    }

    struct Shell {
        app: App,
        chosen: Chosen,
        /// Zip files chosen for Import a Zip File.
        zips: Chosen,
        taken: Taken,
        started: bool,
        store: Option<Store>,
        errors: Errors,
        fetched: Fetched,
        repaint: Repaint,
        /// Whether the session has unsaved changes, for the page's `beforeunload` listener.
        unsaved: Rc<Cell<bool>>,
        refused: Refused,
        room: Room,
        /// Whether the browser was asked to keep the projects for good (at the first save).
        asked_to_keep: bool,
    }

    impl Shell {
        /// Stores what changed this frame: the projects saved or deleted, the preferences and the
        /// thumbnails. Each write runs on its own; a failed one is said on the status line.
        fn keep(&mut self, ctx: &egui::Context) {
            let changes = self.app.take_library_changes();
            let prefs = self.app.take_changed_prefs();
            let thumbnails = self.app.take_thumbnails();
            if let Some(store) = &self.store {
                // The first save of the page asks the browser to keep the projects until the user
                // deletes them; the start page says whether it agreed.
                if !self.asked_to_keep && changes.iter().any(|c| matches!(c, Change::Saved(..))) {
                    self.asked_to_keep = true;
                    let (room, ctx) = (self.room.clone(), ctx.clone());
                    wasm_bindgen_futures::spawn_local(async move {
                        *room.borrow_mut() = Some(storage(true).await);
                        ctx.request_repaint();
                    });
                }
                for change in changes {
                    let (store, refused, room, ctx) = (store.clone(), self.refused.clone(), self.room.clone(), ctx.clone());
                    wasm_bindgen_futures::spawn_local(async move {
                        let result = match &change {
                            Change::Saved(name, project) => store.put_project(name, project).await,
                            Change::Deleted(name) => store.delete_project(name).await,
                            Change::Versions { project, versions, blobs } => store.put_versions(project, versions, blobs).await,
                            Change::Thread { project, file, text } => store.put_thread(project, file, text.as_deref()).await,
                        };
                        match result {
                            Err(e) => refused.borrow_mut().push((change, e)),
                            // The room the projects take now.
                            Ok(()) if matches!(change, Change::Saved(..) | Change::Deleted(_)) => *room.borrow_mut() = Some(storage(false).await),
                            Ok(()) => {}
                        }
                        ctx.request_repaint();
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
            // A refused write leaves its project unsaved, and says why (rule 15).
            for (change, error) in std::mem::take(&mut *self.refused.borrow_mut()) {
                self.app.not_stored(&change, &error);
            }
            if let Some(room) = self.room.borrow_mut().take() {
                self.app.set_browser_storage(room);
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
                    Pick::ImportZip => {
                        let zips = self.zips.clone();
                        let ctx = ctx.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let dialog = rfd::AsyncFileDialog::new().set_title(&request.title).add_filter("zip", &["zip"]);
                            let Some(handles) = dialog.pick_files().await else { return };
                            for handle in handles {
                                let bytes = handle.read().await;
                                zips.borrow_mut().push((handle.file_name(), bytes));
                            }
                            ctx.request_repaint();
                        });
                    }
                    _ => {
                        self.app.pick_failed(texcraft_ui_egui::i18n::t("Folders and paths on disk are not available in the browser yet.").to_owned())
                    }
                }
            }
            let taken = std::mem::take(&mut *self.taken.borrow_mut());
            for file in self.app.take_dropped_files() {
                // Read already, with its folders, from the drop's entries.
                if taken.iter().any(|name| file.path().as_os_str() == name.as_str()) {
                    continue;
                }
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
                            let what = texcraft_ui_egui::i18n::t(
                                "The browser did not let TeXCraft read {file}: {error}. If it is a folder, this browser does not let pages read dropped folders: drop a zip file of it instead.",
                            );
                            errors.borrow_mut().push(what.replace("{file}", &name).replace("{error}", &e));
                        }
                    }
                    ctx.request_repaint();
                });
            }
            for (name, bytes) in std::mem::take(&mut *self.zips.borrow_mut()) {
                self.app.unzip_file(name, bytes);
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

    /// The largest texture the browser build asks for, glyph atlas included (issue #12).
    const MAX_TEXTURE_SIDE: usize = 2048;

    impl eframe::App for Shell {
        /// Phones' GPUs can interpolate texture coordinates at half precision: in egui's glyph
        /// atlas, as wide as the largest texture (8192 px under WebGL 2), the glyphs past its middle
        /// were then read four texels at a time and drawn garbled. In an atlas of 2048 px a step
        /// is at most one texel, which the linear filter smooths.
        fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
            raw_input.max_texture_side = Some(raw_input.max_texture_side.map_or(MAX_TEXTURE_SIDE, |s| s.min(MAX_TEXTURE_SIDE)));
        }

        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            if !self.started {
                self.started = true;
                *self.repaint.borrow_mut() = Some(ui.ctx().clone());
                hide_loading_screen();
            }
            self.app.frame(ui);
            self.unsaved.set(self.app.session().is_dirty());
            let ctx = ui.ctx().clone();
            self.picks(&ctx);
            self.keep(&ctx);
            for (name, data) in self.app.take_exported_files() {
                if let Err(e) = download(&name, &data) {
                    let what = texcraft_ui_egui::i18n::t("The browser did not download {file}: {error}");
                    self.app.report(what.replace("{file}", &name).replace("{error}", &e), true);
                }
            }
        }
    }

    /// The page's loading screen (index.html) goes once the first frame is drawn.
    fn hide_loading_screen() {
        if let Some(e) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("loading")) {
            e.remove();
        }
    }

    /// Hands `data` to the browser as a download named `name` (a link to a blob, clicked).
    fn download(name: &str, data: &[u8]) -> Result<(), String> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(data));
        let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(|e| format!("{e:?}"))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| format!("{e:?}"))?;
        let link: web_sys::HtmlAnchorElement =
            document.create_element("a").map_err(|e| format!("{e:?}"))?.dyn_into().map_err(|_| "no link element")?;
        link.set_href(&url);
        link.set_download(name);
        link.click();
        let _ = web_sys::Url::revoke_object_url(&url);
        Ok(())
    }

    /// Drops that bring no file to eframe: its handler takes `dataTransfer.files` only, which some
    /// drags leave empty (Firefox, from a file manager) while their items hold the file. Those
    /// files are read from the items here; a drop with no file at all is said on the status line,
    /// with what it carried. Listens before eframe (capture), and leaves drops with files to it.
    ///
    /// A drop with a folder is read here too, through the drop's entries (`webkitGetAsEntry`):
    /// eframe's files have no folders. The whole drop is then read here, and eframe's copy of it
    /// left out ([`Taken`]).
    fn watch_drops(canvas: &web_sys::HtmlCanvasElement, chosen: &Chosen, errors: &Errors, repaint: &Repaint, taken: &Taken) {
        let (chosen, errors, repaint, taken) = (chosen.clone(), errors.clone(), repaint.clone(), taken.clone());
        let ondrop = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::DragEvent)>::new(move |event: web_sys::DragEvent| {
            let Some(transfer) = event.data_transfer() else { return };
            let items = transfer.items();
            // The entries can be read only while the event is handled; their files later.
            let entries: Vec<web_sys::FileSystemEntry> = (0..items.length())
                .filter_map(|i| items.get(i))
                .filter(|item| item.kind() == "file")
                .filter_map(|item| item.webkit_get_as_entry().ok().flatten())
                .collect();
            if entries.iter().any(|e| e.is_directory()) {
                *taken.borrow_mut() = entries.iter().map(|e| e.name()).collect();
                let (chosen, errors, repaint) = (chosen.clone(), errors.clone(), repaint.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    let (files, problems) = read_dropped_tree(entries).await;
                    chosen.borrow_mut().extend(files);
                    errors.borrow_mut().extend(problems);
                    if let Some(ctx) = &*repaint.borrow() {
                        ctx.request_repaint();
                    }
                });
                return;
            }
            if transfer.files().is_some_and(|f| f.length() > 0) {
                return;
            }
            let files: Vec<web_sys::File> = (0..items.length())
                .filter_map(|i| items.get(i))
                .filter(|item| item.kind() == "file")
                .filter_map(|item| item.get_as_file().ok().flatten())
                .collect();
            log::info!("a drop with no files: {} items, types {:?}", items.length(), transfer.types().to_vec());
            if files.is_empty() {
                let types: Vec<String> = transfer.types().iter().filter_map(|t| t.as_string()).collect();
                let what = texcraft_ui_egui::i18n::t("The browser passed no file with this drop (it carried: {types}). Add Files can add it.");
                errors.borrow_mut().push(what.replace("{types}", &types.join(", ")));
                if let Some(ctx) = &*repaint.borrow() {
                    ctx.request_repaint();
                }
                return;
            }
            for file in files {
                let (chosen, errors, repaint) = (chosen.clone(), errors.clone(), repaint.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                        Ok(buffer) => chosen.borrow_mut().push((file.name(), js_sys::Uint8Array::new(&buffer).to_vec())),
                        Err(e) => {
                            let what = texcraft_ui_egui::i18n::t("The browser did not let TeXCraft read {file}: {error}");
                            errors.borrow_mut().push(what.replace("{file}", &file.name()).replace("{error}", &format!("{e:?}")));
                        }
                    }
                    if let Some(ctx) = &*repaint.borrow() {
                        ctx.request_repaint();
                    }
                });
            }
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_capture(true);
        if let Err(e) = canvas.add_event_listener_with_callback_and_add_event_listener_options("drop", ondrop.as_ref().unchecked_ref(), &options) {
            log::warn!("drops cannot be watched: {e:?}");
        }
        ondrop.forget();
    }

    /// The files of dropped folders and files, by their path from the drop (`paper/figures/a.png`),
    /// with what could not be read. Hidden entries and build outputs are not read: they are
    /// passed on by name, empty, for the import to report as left out. The import's limits stop
    /// the reading; what is left unread is said.
    async fn read_dropped_tree(roots: Vec<web_sys::FileSystemEntry>) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
        use texcraft_engine::import::{Limits, skip_reason};
        use texcraft_ui_egui::i18n::t;
        const MAX_DEPTH: usize = 32;
        let limits = Limits::default();
        let max_file = texcraft_engine::project::MAX_FILE_BYTES as f64;
        let (mut files, mut problems) = (Vec::new(), Vec::new());
        let (mut bytes, mut count, mut unread) = (0u64, 0usize, 0usize);
        let mut stack: Vec<(web_sys::FileSystemEntry, String, usize)> = roots.into_iter().rev().map(|e| (e.clone(), e.name(), 0)).collect();
        while let Some((entry, path, depth)) = stack.pop() {
            if skip_reason(&path).is_some() {
                files.push((path, Vec::new()));
                continue;
            }
            // By what the entries say they are: the interfaces' names differ between browsers.
            if entry.is_directory() {
                let dir: &web_sys::FileSystemDirectoryEntry = entry.unchecked_ref();
                if depth >= MAX_DEPTH {
                    unread += 1;
                    continue;
                }
                match entries_of(dir, limits.files.saturating_mul(2)).await {
                    Ok(children) => stack.extend(children.into_iter().rev().map(|c| {
                        let name = c.name();
                        (c, format!("{path}/{name}"), depth + 1)
                    })),
                    Err(e) => {
                        problems.push(t("The browser did not let TeXCraft read {file}: {error}").replace("{file}", &path).replace("{error}", &e))
                    }
                }
                continue;
            }
            if !entry.is_file() {
                continue;
            }
            let entry: &web_sys::FileSystemFileEntry = entry.unchecked_ref();
            let file = match file_of(entry).await {
                Ok(file) => file,
                Err(e) => {
                    problems.push(t("The browser did not let TeXCraft read {file}: {error}").replace("{file}", &path).replace("{error}", &e));
                    continue;
                }
            };
            let size = file.size();
            if count >= limits.files || size > max_file || bytes as f64 + size > limits.bytes as f64 {
                unread += 1;
                continue;
            }
            match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => {
                    let data = js_sys::Uint8Array::new(&buffer).to_vec();
                    bytes += data.len() as u64;
                    count += 1;
                    files.push((path, data));
                }
                Err(e) => problems
                    .push(t("The browser did not let TeXCraft read {file}: {error}").replace("{file}", &path).replace("{error}", &format!("{e:?}"))),
            }
        }
        if unread > 0 {
            let said = t("{n} dropped files were not read: past the limit of 5,000 files or 500 MB, larger than 64 MB, or nested too deep.");
            problems.push(said.replace("{n}", &unread.to_string()));
        }
        (files, problems)
    }

    /// The entries of folder `dir`, at most `max`. The browser gives them in batches until one is
    /// empty.
    async fn entries_of(dir: &web_sys::FileSystemDirectoryEntry, max: usize) -> Result<Vec<web_sys::FileSystemEntry>, String> {
        let reader = dir.create_reader();
        let mut all = Vec::new();
        while all.len() < max {
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                if let Err(e) = reader.read_entries_with_callback_and_callback(&resolve, &reject) {
                    let _ = reject.call1(&wasm_bindgen::JsValue::NULL, &e);
                }
            });
            let batch = wasm_bindgen_futures::JsFuture::from(promise).await.map_err(|e| format!("{e:?}"))?;
            let batch: js_sys::Array = batch.dyn_into().map_err(|_| "the browser listed no entries".to_owned())?;
            if batch.length() == 0 {
                break;
            }
            all.extend(batch.iter().map(|v| v.unchecked_into::<web_sys::FileSystemEntry>()));
        }
        Ok(all)
    }

    /// The file of a dropped folder's entry.
    async fn file_of(entry: &web_sys::FileSystemFileEntry) -> Result<web_sys::File, String> {
        let promise = js_sys::Promise::new(&mut |resolve, reject| entry.file_with_callback_and_callback(&resolve, &reject));
        let file = wasm_bindgen_futures::JsFuture::from(promise).await.map_err(|e| format!("{e:?}"))?;
        file.dyn_into().map_err(|_| "not a file".to_owned())
    }

    /// Asks the browser to warn before the page is left or reloaded while `unsaved` is set (the
    /// browser shows its own question; pages cannot word it).
    fn warn_before_leaving(unsaved: Rc<Cell<bool>>) {
        let Some(window) = web_sys::window() else { return };
        let listener = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(move |event: web_sys::BeforeUnloadEvent| {
            if unsaved.get() {
                event.prevent_default();
                // Older browsers warn only when `returnValue` is set.
                event.set_return_value("unsaved");
            }
        });
        if let Err(e) = window.add_event_listener_with_callback("beforeunload", listener.as_ref().unchecked_ref()) {
            log::warn!("no warning before leaving the page: {e:?}");
        }
        // The listener lives as long as the page.
        listener.forget();
    }

    /// Sign-ins to GitLab (docs/forges.md, step 4), kept in the tab's session storage: a reload
    /// and GitLab's round trip keep them, closing the tab forgets them. IndexedDB or local storage
    /// would keep a refresh token, which works for months, in the browser's profile after the tab
    /// is gone; any script of the page could read either, and the page loads only its own.
    struct TabStorage;

    impl TabStorage {
        fn storage() -> Result<web_sys::Storage, String> {
            web_sys::window()
                .ok_or("no window")?
                .session_storage()
                .map_err(|e| format!("{e:?}"))?
                .ok_or_else(|| "this browser has no session storage".to_owned())
        }
    }

    impl texcraft_engine::ai::Secrets for TabStorage {
        fn get(&self, handle: &str) -> Result<Option<texcraft_engine::ai::SecretString>, String> {
            Ok(Self::storage()?.get_item(handle).map_err(|e| format!("{e:?}"))?.map(texcraft_engine::ai::SecretString::new))
        }
        fn put(&self, handle: &str, value: &texcraft_engine::ai::SecretString) -> Result<(), String> {
            Self::storage()?.set_item(handle, value.expose()).map_err(|e| format!("{e:?}"))
        }
        fn remove(&self, handle: &str) -> Result<(), String> {
            Self::storage()?.remove_item(handle).map_err(|e| format!("{e:?}"))
        }
        fn lasting(&self) -> bool {
            true
        }
    }

    /// The page's address without its query (GitLab's redirect URI: the origin and `/`), and the
    /// query GitLab sent the page back with, if it did (`code` and `state`, or `error`); the query
    /// is taken out of the address bar at once, so the code is in no history entry.
    fn sign_in_return() -> (String, Option<serde_json::Value>) {
        let Some(window) = web_sys::window() else { return (String::new(), None) };
        let location = window.location();
        let origin = location.origin().unwrap_or_default();
        let page = format!("{origin}/");
        let search = location.search().unwrap_or_default();
        let pairs = texcraft_engine::forge::oauth::parse_query(&search);
        let has = |k: &str| pairs.iter().any(|(n, _)| n == k);
        if !(has("state") && (has("code") || has("error"))) {
            return (page, None);
        }
        let path = location.pathname().unwrap_or_else(|_| "/".to_owned());
        if let Ok(history) = window.history()
            && let Err(e) = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path))
        {
            log::warn!("the sign-in's code could not be taken out of the address: {e:?}");
        }
        let params: serde_json::Map<String, serde_json::Value> = pairs
            .into_iter()
            .filter(|(k, _)| matches!(k.as_str(), "code" | "state" | "error" | "error_description"))
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        (page, Some(params.into()))
    }

    /// The page's query when it is a link that proposes a server (`?gitlab=…&client_id=…`,
    /// docs/forges.md); it is taken out of the address bar, so a reload does not ask again.
    fn server_link() -> Option<String> {
        let window = web_sys::window()?;
        let location = window.location();
        let search = location.search().ok()?;
        let pairs = texcraft_engine::forge::oauth::parse_query(&search);
        if !pairs.iter().any(|(k, _)| k == "gitlab" || k == "github") {
            return None;
        }
        let path = location.pathname().unwrap_or_else(|_| "/".to_owned());
        if let Ok(history) = window.history()
            && let Err(e) = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path))
        {
            log::warn!("the server link could not be taken out of the address: {e:?}");
        }
        Some(search)
    }

    /// The engine worker's script, made by Trunk (`index.html`), under its plain name.
    const WORKER: &str = "engine-worker_loader.js";

    /// The engine worker's script: the name the build gave it, named after the engine's contents
    /// so that a browser never starts an old one after an update (`<meta name="texcraft-engine">`,
    /// written by `cargo xtask version-worker`), or its plain name in a build without that step.
    fn worker_url() -> String {
        web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector("meta[name=texcraft-engine]").ok().flatten())
            .and_then(|m| m.get_attribute("content"))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| WORKER.to_owned())
    }

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
            let mut session = texcraft_engine::Session::in_browser(&worker_url());
            let (page, returned) = sign_in_return();
            let link = if returned.is_none() { server_link() } else { None };
            session.set_forge_secrets(std::sync::Arc::new(TabStorage));
            session.set_forge_redirect(&page);
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
            if store.is_some() {
                app.set_browser_storage(storage(false).await);
            }
            app.set_native_pickers(true);
            app.show_start_page();
            if let Some(params) = returned {
                app.forge_sign_in_returned(params);
            }
            if let Some(query) = link {
                app.forge_propose_server(&query);
            }
            let chosen = Chosen::default();
            let repaint = Repaint::default();
            let taken = Taken::default();
            watch_drops(&canvas, &chosen, &errors, &repaint, &taken);
            let unsaved = Rc::new(Cell::new(false));
            warn_before_leaving(unsaved.clone());
            let shell = Shell {
                app,
                chosen,
                zips: Chosen::default(),
                taken,
                started: false,
                store,
                errors,
                fetched: Fetched::default(),
                repaint,
                unsaved,
                refused: Refused::default(),
                room: Room::default(),
                asked_to_keep: false,
            };
            let started = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(|_cc| Ok(Box::new(shell)))).await;
            if let Err(e) = started {
                log::error!("TeXCraft could not start: {e:?}");
            }
        });
    }
}
