//! The app-wide hostile-input sweep (standards/testing.md): every command, with junk parameters,
//! returns a result and leaves a usable session. A new command is covered here without changes.

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::file_access::AutomationRoots;
use texcraft_engine::{Session, cmd};

fn junk() -> Vec<Value> {
    vec![
        Value::Null,
        json!([]),
        json!({"path": 7}),
        json!({"path": "../../etc/passwd", "text": "x", "from": "/", "to": "", "on": "yes", "wait": "no"}),
        json!({"path": "a\u{0}b", "text": null}),
        json!({"path": "x".repeat(5000)}),
        json!({"from": "main.tex", "to": "main.tex"}),
    ]
}

#[test]
fn every_command_survives_junk_parameters() -> std::io::Result<()> {
    // Confined to a scratch folder, so junk paths given to commands that write cannot reach real files.
    let root = std::env::temp_dir().join(format!("texcraft-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let roots = AutomationRoots::new(std::slice::from_ref(&root), std::slice::from_ref(&root)).map_err(std::io::Error::other)?;
    for spec in cmd::all() {
        for params in junk() {
            let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
            s.confine(roots.clone());
            let _ = s.execute(spec.id, &params);
            // The session still answers queries.
            assert!(s.execute("project.inspect", &json!({})).is_ok(), "{} broke the session with {params}", spec.id);
        }
    }
    std::fs::remove_dir_all(&root)
}
