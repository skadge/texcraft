use std::path::PathBuf;

use texcraft_engine::file_access::{Access, AutomationRoots};

type R = Result<(), Box<dyn std::error::Error>>;

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn an_agent_session_cannot_start_without_both_roots() -> R {
    let d = temp("roots")?;
    assert!(AutomationRoots::new(std::slice::from_ref(&d), &[]).is_err());
    assert!(AutomationRoots::new(&[], &[d]).is_err());
    Ok(())
}

#[test]
fn paths_that_escape_the_roots_are_refused() -> R {
    let base = temp("escape")?;
    let read = base.join("read");
    let write = base.join("write");
    std::fs::create_dir_all(&read)?;
    std::fs::create_dir_all(&write)?;
    std::fs::write(base.join("secret.tex"), "x")?;
    let roots = AutomationRoots::new(std::slice::from_ref(&read), std::slice::from_ref(&write))?;

    assert!(roots.check(&write.join("new/file.tex"), Access::Write).is_ok());
    assert!(roots.check(&read, Access::Write).is_err(), "a read root is not writable");
    assert!(roots.check(&write.join("../secret.tex"), Access::Read).is_err());
    assert!(roots.check(&PathBuf::from("../secret.tex"), Access::Read).is_err(), "relative paths start at the write root");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(base.join("secret.tex"), write.join("link.tex"))?;
        assert!(roots.check(&write.join("link.tex"), Access::Read).is_err(), "a link out of the roots");
        std::fs::hard_link(base.join("secret.tex"), write.join("hard.tex"))?;
        assert!(roots.check(&write.join("hard.tex"), Access::Read).is_err(), "a second hard link");
    }
    Ok(())
}
