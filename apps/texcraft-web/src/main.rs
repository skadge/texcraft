//! TeXCraft in the browser. Build with `trunk build --release` from this folder.
//!
//! The engine does not run in the browser yet: compiling says so (docs/plan.md, phase 3, the WASI
//! engine host). Editing, the outline and the preview of a PDF already work.

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
    use wasm_bindgen::JsCast;

    struct Shell(texcraft_ui_egui::App);

    impl eframe::App for Shell {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.frame(ui);
        }
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
            let session = texcraft_engine::Session::new();
            let started = eframe::WebRunner::new()
                .start(canvas, eframe::WebOptions::default(), Box::new(|_cc| Ok(Box::new(Shell(texcraft_ui_egui::App::new(session))))))
                .await;
            if let Err(e) = started {
                log::error!("TeXCraft could not start: {e:?}");
            }
        });
    }
}
