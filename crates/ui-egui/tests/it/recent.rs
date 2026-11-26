use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::{Prefs, RECENT_MAX};

#[test]
fn opening_a_project_puts_it_first_once_and_keeps_the_list_bounded() -> std::io::Result<()> {
    let base = std::env::temp_dir().join(format!("texcraft-recent-{}", std::process::id()));
    let folders: Vec<_> = (0..RECENT_MAX + 2).map(|i| base.join(format!("p{i}"))).collect();
    for f in &folders {
        std::fs::create_dir_all(f)?;
        std::fs::write(f.join("main.tex"), "\\documentclass{article}")?;
    }
    let mut app = App::with_prefs(Session::with_backend(Box::new(Unavailable("tests".into()))), Prefs::default());
    for f in &folders {
        app.open_project(&f.display().to_string());
    }
    let first = folders[0].display().to_string();
    app.open_project(&first);
    let recent = app.take_changed_prefs().map(|p| p.recent).unwrap_or_default();
    std::fs::remove_dir_all(&base)?;
    assert_eq!(recent.first(), Some(&first));
    assert_eq!(recent.iter().filter(|p| **p == first).count(), 1);
    assert!(recent.len() <= RECENT_MAX);
    Ok(())
}

#[test]
fn a_folder_with_a_file_too_large_to_load_opens_and_says_its_size_in_units() -> std::io::Result<()> {
    // Issue #28: the status line gave a raw byte count, and the project did not open.
    use egui_kittest::kittest::{NodeT, Queryable};
    let dir = std::env::temp_dir().join(format!("texcraft-too-large-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("main.tex"), "\\documentclass{article}")?;
    let big = texcraft_engine::project::MAX_FILE_BYTES as u64 + 1;
    // Sparse: as large as asked, without the disk.
    std::fs::File::create(dir.join("atlas.pdf"))?.set_len(big)?;
    let mut app = crate::support::app(Session::with_backend(Box::new(Unavailable("tests".into()))));
    app.open_project(&dir.display().to_string());
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    let opened = h.state().session().dir().is_some();
    let said = h.query_by_label_contains("atlas.pdf (64").map(|n| n.accesskit_node().label().unwrap_or_default().to_string());
    std::fs::remove_dir_all(&dir)?;
    assert!(opened, "the project did not open");
    let said = said.unwrap_or_default();
    assert!(said.contains("MB") && !said.contains(&big.to_string()), "the status line does not give the size in units: {said:?}");
    Ok(())
}
