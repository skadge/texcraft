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
