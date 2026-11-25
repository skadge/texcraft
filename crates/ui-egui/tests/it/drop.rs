//! A zip file dropped on the window asks whether to unzip it into the project (the default, which
//! has the keyboard) or to add the zip itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn harness() -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "\\documentclass{article}\n"})).ok();
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness
}

/// A zip with one folder holding two files, written to a scratch file named `name`.
fn zip_file(name: &str) -> Result<PathBuf, String> {
    let mut w = craft_zip::Writer::new();
    w.add("paper/intro.tex", b"Hello.", craft_zip::Compression::Deflated).map_err(|e| e.to_string())?;
    w.add("paper/figures/plot.svg", b"<svg/>", craft_zip::Compression::Deflated).map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join(format!("texcraft-drop-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, w.finish().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path)
}

fn drop_file(harness: &mut Harness<'_, App>, path: &Path) {
    harness.input_mut().dropped_files.push(Arc::new(Dropped(path.to_path_buf())));
    harness.run();
}

fn paths(harness: &mut Harness<'_, App>) -> Vec<String> {
    harness.state_mut().session().project().paths().map(|p| p.as_str().to_owned()).collect()
}

#[test]
fn a_dropped_zip_is_unzipped_with_the_keyboard_by_default() -> Result<(), String> {
    let zip = zip_file("paper.zip")?;
    let mut harness = harness();
    drop_file(&mut harness, &zip);
    assert!(harness.query_by_label_contains("Unzip paper.zip?").is_some(), "no question about the zip");
    assert!(harness.get_by_label("Unzip into the Project").is_focused(), "the default answer does not have the keyboard");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let have = paths(&mut harness);
    assert!(have.iter().any(|p| p == "intro.tex") && have.iter().any(|p| p == "figures/plot.svg"), "not unzipped without its folder: {have:?}");
    assert!(!have.iter().any(|p| p.ends_with(".zip")), "the zip itself was added: {have:?}");
    assert!(harness.query_by_label_contains("2 files brought in").is_some(), "the status line does not say what came in");
    let _ = std::fs::remove_dir_all(zip.parent().unwrap_or(&zip));
    Ok(())
}

#[test]
fn a_dropped_zip_can_be_added_as_a_file_or_left_out() -> Result<(), String> {
    let zip = zip_file("data.zip")?;
    let mut harness = harness();
    drop_file(&mut harness, &zip);
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.query_by_label_contains("Unzip data.zip?").is_none(), "Cancel left the question open");
    assert_eq!(paths(&mut harness), ["main.tex"], "Cancel changed the project");
    drop_file(&mut harness, &zip);
    harness.get_by_label("Add the Zip as a File").click();
    harness.run();
    assert_eq!(paths(&mut harness), ["data.zip", "main.tex"]);
    // Dropped again, the zip is in the project already: one question for it.
    drop_file(&mut harness, &zip);
    harness.get_by_label("Add the Zip as a File").click();
    harness.run();
    assert!(harness.query_by_label_contains("1 file is already in the project").is_some(), "no question about the file in the project");
    harness.get_by_label("Keep Both").click();
    harness.run();
    assert_eq!(paths(&mut harness), ["data-2.zip", "data.zip", "main.tex"]);
    let _ = std::fs::remove_dir_all(zip.parent().unwrap_or(&zip));
    Ok(())
}
