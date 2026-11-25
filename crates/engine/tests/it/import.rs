//! Bringing many files in at once: a folder (`file.import` of a folder), a zip (`file.unzip`), a
//! new project from either (`project.import`). Zips are made here with the writer, so no binary
//! fixture is needed.

use base64::Engine as _;
use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::import::{Gathered, Limits, Skip};

type R = Result<(), String>;

fn session() -> Session {
    Session::with_backend(Box::new(Unavailable("tests".into())))
}

fn paths(s: &Session) -> Vec<String> {
    s.project().paths().map(|p| p.as_str().to_owned()).collect()
}

fn reasons(result: &Value) -> Vec<(String, String)> {
    let list = result["skipped"].as_array().cloned().unwrap_or_default();
    list.iter().map(|s| (s["path"].as_str().unwrap_or_default().to_owned(), s["reason"].as_str().unwrap_or_default().to_owned())).collect()
}

fn reason_of(result: &Value, path: &str) -> Option<String> {
    reasons(result).into_iter().find(|(p, _)| p == path).map(|(_, r)| r)
}

/// A scratch folder of its own for each test.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!("texcraft-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(Scratch(dir))
    }

    fn write(&self, rel: &str, text: &str) -> std::io::Result<()> {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

fn zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut w = craft_zip::Writer::new();
    for (name, data) in entries {
        w.add(name, data, craft_zip::Compression::Deflated).map_err(|e| e.to_string())?;
    }
    w.finish().map_err(|e| e.to_string())
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Marks the central directory entry `name` of `bytes` as a Unix symbolic link.
fn make_symlink(bytes: &mut [u8], name: &str) {
    let mut at = 0;
    while let Some(found) = bytes.get(at..).and_then(|b| b.windows(4).position(|w| w == b"PK\x01\x02")) {
        let start = at + found;
        let len = |o: usize| usize::from(u16::from_le_bytes([bytes[start + o], bytes[start + o + 1]]));
        let name_len = len(28);
        if bytes.get(start + 46..start + 46 + name_len) == Some(name.as_bytes()) {
            bytes[start + 5] = 3; // made on Unix
            bytes[start + 38..start + 42].copy_from_slice(&((0o120_777u32) << 16).to_le_bytes());
        }
        at = start + 4;
    }
}

#[test]
fn a_folder_comes_in_with_its_structure_without_hidden_files_or_build_outputs() -> R {
    let dir = Scratch::new("folder").map_err(io)?;
    let paper = dir.0.join("paper");
    for (rel, text) in [
        ("paper/main.tex", "\\documentclass{article}"),
        ("paper/chapters/intro.tex", "intro"),
        ("paper/figures/plot.svg", "<svg/>"),
        ("paper/main.aux", "aux"),
        ("paper/main.synctex.gz", "x"),
        ("paper/.DS_Store", "x"),
        ("paper/.git/config", "x"),
        ("outside.tex", "secret"),
    ] {
        dir.write(rel, text).map_err(io)?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.0.join("outside.tex"), paper.join("leak.tex")).map_err(io)?;
        std::os::unix::fs::symlink(paper.join("chapters/intro.tex"), paper.join("again.tex")).map_err(io)?;
    }
    let mut s = session();
    let result = s.execute("file.import", &json!({"source": paper.display().to_string()}))?;
    let have = paths(&s);
    for kept in ["paper/main.tex", "paper/chapters/intro.tex", "paper/figures/plot.svg"] {
        assert!(have.iter().any(|p| p == kept), "{kept} is missing: {have:?}");
    }
    for left in ["paper/main.aux", "paper/main.synctex.gz", "paper/.DS_Store", "paper/.git/config"] {
        assert!(!have.iter().any(|p| p == left), "{left} came in: {have:?}");
    }
    assert_eq!(reason_of(&result, "paper/main.aux").as_deref(), Some("build_output"), "{result}");
    assert_eq!(reason_of(&result, "paper/.git").as_deref(), Some("hidden"), "{result}");
    #[cfg(unix)]
    {
        assert_eq!(reason_of(&result, "paper/leak.tex").as_deref(), Some("link"), "a link out of the folder was followed: {result}");
        assert!(have.iter().any(|p| p == "paper/again.tex"), "a link inside the folder was not read: {have:?}");
    }
    Ok(())
}

#[test]
fn a_folder_goes_into_the_folder_asked_for() -> R {
    let dir = Scratch::new("into").map_err(io)?;
    dir.write("figs/a.png", "png").map_err(io)?;
    let mut s = session();
    s.execute("file.import", &json!({"source": dir.0.join("figs").display().to_string(), "into": "assets"}))?;
    assert!(paths(&s).iter().any(|p| p == "assets/figs/a.png"), "{:?}", paths(&s));
    Ok(())
}

#[test]
fn an_import_past_its_limits_says_what_it_left_out() -> R {
    let dir = Scratch::new("limits").map_err(io)?;
    for i in 0..5 {
        dir.write(&format!("many/f{i}.tex"), "0123456789").map_err(io)?;
    }
    let few = Gathered::from_folder(&dir.0.join("many"), Limits { files: 3, ..Limits::default() }, false, None)?;
    assert_eq!(few.files.len(), 3);
    assert_eq!(few.skipped.iter().filter(|s| s.reason == Skip::TooMany).count(), 2, "{:?}", few.skipped);
    let small = Gathered::from_folder(&dir.0.join("many"), Limits { bytes: 25, ..Limits::default() }, false, None)?;
    assert!(small.files.len() < 5);
    assert!(small.skipped.iter().any(|s| s.reason == Skip::TotalTooLarge), "{:?}", small.skipped);
    Ok(())
}

#[test]
fn files_already_in_the_project_are_replaced_kept_beside_or_skipped_as_asked() -> R {
    let dir = Scratch::new("existing").map_err(io)?;
    dir.write("doc/main.tex", "new").map_err(io)?;
    dir.write("doc/fresh.tex", "fresh").map_err(io)?;
    let source = dir.0.join("doc").display().to_string();
    let text = |s: &mut Session, p: &str| s.execute("file.read", &json!({"path": p})).map(|r| r["text"].as_str().unwrap_or_default().to_owned());

    let mut s = session();
    s.execute("file.new", &json!({"path": "doc/main.tex", "text": "old"}))?;
    let checked = s.execute("file.import", &json!({"source": source, "check": true}))?;
    assert_eq!(checked["existing"], json!(["doc/main.tex"]), "{checked}");
    assert_eq!(text(&mut s, "doc/main.tex")?, "old", "a check changed the project");
    assert!(!paths(&s).iter().any(|p| p == "doc/fresh.tex"), "a check added a file");

    s.execute("file.import", &json!({"source": source, "existing": "skip"}))?;
    assert_eq!(text(&mut s, "doc/main.tex")?, "old");
    assert!(paths(&s).iter().any(|p| p == "doc/fresh.tex"), "the other files were not added");

    let both = s.execute("file.import", &json!({"source": source, "existing": "keep_both"}))?;
    assert_eq!(text(&mut s, "doc/main.tex")?, "old");
    let renamed = both["renamed"].as_array().cloned().unwrap_or_default();
    let new_main = renamed.iter().find(|r| r["from"] == "doc/main.tex").and_then(|r| r["to"].as_str()).ok_or(both.to_string())?.to_owned();
    assert_ne!(new_main, "doc/main.tex");
    assert_eq!(text(&mut s, &new_main)?, "new");

    s.execute("file.import", &json!({"source": source}))?;
    assert_eq!(text(&mut s, "doc/main.tex")?, "new", "the default replaces, as adding one file does");
    assert!(s.execute("file.import", &json!({"source": source, "existing": "merge"})).is_err());
    Ok(())
}

#[test]
fn a_zip_unpacks_without_its_common_folder_or_unsafe_names_or_links() -> R {
    let mut bytes = zip(&[
        ("paper/main.tex", b"\\documentclass{article}"),
        ("paper/sections/one.tex", b"one"),
        ("paper/main.log", b"log"),
        ("paper/../../evil.tex", b"x"),
        ("/etc/evil.tex", b"x"),
        ("C:/evil.tex", b"x"),
        ("paper/link.tex", b"/etc/passwd"),
        ("__MACOSX/paper/._main.tex", b"x"),
    ])?;
    make_symlink(&mut bytes, "paper/link.tex");
    let mut s = session();
    let result = s.execute("file.unzip", &json!({"data": b64(&bytes), "into": "imported"}))?;
    let have = paths(&s);
    assert!(have.iter().any(|p| p == "imported/main.tex"), "the common folder was kept or the file is missing: {have:?}");
    assert!(have.iter().any(|p| p == "imported/sections/one.tex"), "{have:?}");
    assert_eq!(result["folder"], "paper");
    assert!(!have.iter().any(|p| p.contains("evil") || p.contains("link") || p.ends_with(".log") || p.contains("MACOSX")), "{have:?}");
    for name in ["paper/../../evil.tex", "/etc/evil.tex", "C:/evil.tex"] {
        assert_eq!(reason_of(&result, name).as_deref(), Some("unsafe_name"), "{name}: {result}");
    }
    assert_eq!(reason_of(&result, "paper/link.tex").as_deref(), Some("link"), "{result}");
    Ok(())
}

#[test]
fn a_zip_whose_entries_expand_like_a_bomb_is_refused_whole() -> R {
    let zeros = vec![0u8; 4 * 1024 * 1024];
    let mut w = craft_zip::Writer::new();
    w.add("ok.tex", b"fine", craft_zip::Compression::Deflated).map_err(|e| e.to_string())?;
    w.add("zeros.bin", &zeros, craft_zip::Compression::Level(9)).map_err(|e| e.to_string())?;
    let bytes = w.finish().map_err(|e| e.to_string())?;
    let mut s = session();
    let before = paths(&s);
    let refused = s.execute("file.unzip", &json!({"data": b64(&bytes)}));
    assert!(refused.as_ref().is_err_and(|e| e.contains("zip bomb")), "{refused:?}");
    assert_eq!(paths(&s), before, "part of a refused zip came in");
    assert!(s.execute("file.unzip", &json!({"data": b64(b"not a zip")})).is_err());
    Ok(())
}

#[test]
fn a_zip_makes_a_new_unsaved_project_with_its_main_document() -> R {
    let bytes = zip(&[("thesis/thesis.tex", b"\\documentclass{book}"), ("thesis/ch1.tex", b"one")])?;
    let mut s = session();
    let result = s.execute("project.import", &json!({"data": b64(&bytes)}))?;
    assert_eq!(paths(&s), ["ch1.tex", "thesis.tex"]);
    assert_eq!(result["main"], "thesis.tex", "{result}");
    assert!(s.is_dirty(), "a project from a zip is not saved anywhere yet");
    assert!(s.execute("project.import", &json!({"data": b64(&zip(&[(".hidden", b"x")])?)})).is_err(), "an empty project was made");
    Ok(())
}

#[test]
fn files_read_from_a_browser_folder_are_filtered_like_a_folder() -> R {
    let files = json!([
        {"path": "notes/a.tex", "data": b64(b"a")},
        {"path": "notes/a.aux", "data": b64(b"x")},
        {"path": "notes/../b.tex", "data": b64(b"x")}
    ]);
    let mut s = session();
    let result = s.execute("file.import", &json!({"files": files}))?;
    assert!(paths(&s).iter().any(|p| p == "notes/a.tex"), "the folder's name is kept in an open project: {:?}", paths(&s));
    assert_eq!(reason_of(&result, "notes/a.aux").as_deref(), Some("build_output"));
    assert_eq!(reason_of(&result, "notes/../b.tex").as_deref(), Some("unsafe_name"));
    Ok(())
}
