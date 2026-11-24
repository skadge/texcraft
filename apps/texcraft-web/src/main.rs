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
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use texcraft_ui_egui::{App, Pick};
    use wasm_bindgen::JsCast;

    /// Files chosen in the browser's file picker, waiting for the next frame.
    type Chosen = Rc<RefCell<Vec<(String, Vec<u8>)>>>;

    struct Shell {
        app: App,
        chosen: Chosen,
        started: bool,
    }

    impl Shell {
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
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match file.bytes_async().await {
                        Ok(bytes) => chosen.borrow_mut().push((file.path().display().to_string(), bytes)),
                        Err(e) => log::warn!("a dropped file could not be read: {e}"),
                    }
                    ctx.request_repaint();
                });
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
            let session = texcraft_engine::Session::in_browser(WORKER);
            let mut app = App::new(session);
            app.set_native_pickers(true);
            let shell = Shell { app, chosen: Chosen::default(), started: false };
            let started = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(|_cc| Ok(Box::new(shell)))).await;
            if let Err(e) = started {
                log::error!("TeXCraft could not start: {e:?}");
            }
        });
    }
}
