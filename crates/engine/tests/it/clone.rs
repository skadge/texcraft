//! Import a Project's git side (docs/design/import-dialog.md): a real clone into a folder, the
//! check of an address, the credential remembered, and stopping, against the git server in
//! memory: no test reaches the network.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::ai::{SecretString, Secrets};
use texcraft_engine::gitwire::fake::Server;
use texcraft_engine::gitwire::{Http, Reply};
use texcraft_engine::project::ProjectPath;

type R = Result<(), Box<dyn std::error::Error>>;

const TOKEN: &str = "ghp-engine-clone-test";

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-clone-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn session(http: impl Fn() -> Box<dyn Http> + Send + Sync + 'static) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_git_http(http);
    s
}

fn over(server: &Server) -> Session {
    let server = server.clone();
    session(move || Box::new(server.clone()))
}

/// A bare repository at `dir`, as `git init --bare` makes it.
fn bare(dir: &Path) -> Result<PathBuf, String> {
    for sub in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        std::fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
    }
    std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").map_err(|e| e.to_string())?;
    std::fs::write(dir.join("config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n").map_err(|e| e.to_string())?;
    Ok(dir.to_path_buf())
}

fn write(s: &mut Session, path: &str, text: &str) -> Result<Value, String> {
    s.execute("file.write", &json!({"path": path, "text": text}))
}

fn text(s: &Session, path: &str) -> Option<String> {
    s.project().text(&ProjectPath::new(path).ok()?).ok().map(str::to_owned)
}

fn commits(s: &mut Session) -> Result<usize, String> {
    Ok(s.execute("history.list", &json!({"limit": 1000}))?["commits"].as_array().map_or(0, Vec::len))
}

fn head(s: &mut Session) -> Result<String, String> {
    s.execute("history.status", &json!({}))?["head"].as_str().map(str::to_owned).ok_or_else(|| "no head".to_owned())
}

/// A server at `url`'s place holding a project of three commits, pushed by another session.
fn published(dir: &Path, server: &Server, url: &str, token: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let mut a = over(server);
    for (i, line) in ["One", "Two", "Three"].iter().enumerate() {
        write(&mut a, "main.tex", &format!("\\documentclass{{article}}\n\\begin{{document}}\n{line}\n\\end{{document}}\n"))?;
        if i == 0 {
            a.execute("project.save_as", &json!({"path": dir.join("a").display().to_string()}))?;
            a.execute("git.set_remote", &json!({"name": "origin", "url": url}))?;
        } else {
            a.execute("project.save", &json!({}))?;
        }
    }
    a.execute("git.push", &json!({"token": token}))?;
    Ok(commits(&mut a)?)
}

fn wait(s: &mut Session) -> Result<Value, String> {
    for _ in 0..500 {
        if let Some(done) = s.poll_forge() {
            return done;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("the operation did not end".to_owned())
}

#[test]
fn a_clone_has_the_whole_history_follows_origin_and_syncs_at_once() -> R {
    let dir = temp("history")?;
    let server = Server::new(&bare(&dir.join("server.git"))?).with_auth("x-access-token", TOKEN);
    let url = "https://github.com/ada/paper.git";
    let pushed = published(&dir, &server, url, TOKEN)?;

    let mut b = over(&server);
    let folder = dir.join("Documents").join("paper");
    let done = b.execute("project.clone", &json!({"url": url, "folder": folder.display().to_string(), "token": TOKEN}))?;
    assert_eq!((done["branch"].as_str(), done["remote"].as_str()), (Some("main"), Some("origin")), "{done}");
    assert!(!done.to_string().contains(TOKEN), "the token is in no result");
    // The project is open, from its folder, with every commit (no shallow history).
    assert_eq!(b.dir(), Some(&folder));
    assert!(text(&b, "main.tex").is_some_and(|t| t.contains("Three")));
    assert_eq!(commits(&mut b)?, pushed);
    let remotes = b.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["upstream"], json!({"remote": "origin", "branch": "main"}), "{remotes}");
    assert_eq!(remotes["url"], url);
    assert_eq!((remotes["ahead"].clone(), remotes["behind"].clone()), (json!(0), json!(0)));
    let config = std::fs::read_to_string(folder.join(".git/config"))?;
    assert!(!config.contains(TOKEN), "no secret in the repository");

    // Sync works with no further setup: the token given is kept for the session.
    write(&mut b, "main.tex", "\\documentclass{article}\n\\begin{document}\nFour\n\\end{document}\n")?;
    b.execute("project.write", &json!({}))?;
    b.execute("project.sync", &json!({}))?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut b)?));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_folder_that_is_not_empty_is_refused_with_a_free_name() -> R {
    let dir = temp("occupied")?;
    let server = Server::new(&bare(&dir.join("server.git"))?);
    published(&dir, &server, "https://example.org/lab/notes.git", "unused")?;
    let mut s = over(&server);
    let folder = dir.join("notes");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("mine.txt"), "keep me")?;
    let refused =
        s.execute("project.clone", &json!({"url": "https://example.org/lab/notes.git", "folder": folder.display().to_string()})).unwrap_err();
    assert!(refused.contains("notes-2"), "{refused}");
    assert_eq!(s.import_problem(), Some(texcraft_engine::ImportProblem::Folder));
    assert_eq!(std::fs::read_to_string(folder.join("mine.txt"))?, "keep me", "nothing touched");
    // A public repository needs no credentials; an empty folder is taken as it is.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty)?;
    s.execute("project.clone", &json!({"url": "https://example.org/lab/notes.git", "folder": empty.display().to_string()}))?;
    assert!(empty.join("main.tex").is_file() && empty.join(".git").is_dir());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn checking_an_address_lists_its_branches_or_says_it_needs_signing_in() -> R {
    let dir = temp("check")?;
    let open = Server::new(&bare(&dir.join("open.git"))?);
    published(&dir, &open, "https://example.org/lab/notes.git", "unused")?;
    let mut s = over(&open);
    let found = s.execute("git.check_address", &json!({"url": "git clone https://example.org/lab/notes.git"}))?;
    assert_eq!((found["reachable"].clone(), found["needs_sign_in"].clone()), (json!(true), json!(false)), "{found}");
    assert_eq!((found["branches"].clone(), found["default_branch"].clone()), (json!(["main"]), json!("main")));
    assert_eq!((found["host"].as_str(), found["name"].as_str()), (Some("other"), Some("notes")));
    // An SSH address is answered with its HTTPS form, the server not asked.
    let ssh = s.execute("git.check_address", &json!({"url": "git@example.org:lab/notes.git"}))?;
    assert_eq!((ssh["ssh"].clone(), ssh["https_url"].as_str()), (json!(true), Some("https://example.org/lab/notes.git")));

    let dir2 = dir.join("closed");
    std::fs::create_dir_all(&dir2)?;
    let closed = Server::new(&bare(&dir2.join("server.git"))?).with_auth("ada", TOKEN);
    let mut a = over(&closed);
    write(&mut a, "main.tex", "x\n")?;
    a.execute("project.save_as", &json!({"path": dir2.join("a").display().to_string()}))?;
    a.execute("git.set_remote", &json!({"name": "origin", "url": "https://ada@code.example.edu/lab/notes.git"}))?;
    a.execute("git.push", &json!({"token": TOKEN}))?;
    let mut s = over(&closed);
    let needs = s.execute("git.check_address", &json!({"url": "https://code.example.edu/lab/notes.git"}))?;
    assert_eq!(needs["needs_sign_in"], true, "{needs}");
    let wrong =
        s.execute("git.check_address", &json!({"url": "https://code.example.edu/lab/notes.git", "username": "ada", "token": "nope"})).unwrap_err();
    assert!(wrong.contains("code.example.edu did not accept the token"), "{wrong}");
    assert_eq!(s.import_problem(), Some(texcraft_engine::ImportProblem::Unauthorized));
    let right = s.execute("git.check_address", &json!({"url": "https://code.example.edu/lab/notes.git", "username": "ada", "token": TOKEN}))?;
    assert_eq!(right["branches"], json!(["main"]), "{right}");
    // The user name goes into origin's address, so that later pushes send it.
    let folder = dir2.join("b");
    s.execute(
        "project.clone",
        &json!({"url": "https://code.example.edu/lab/notes.git", "folder": folder.display().to_string(), "username": "ada", "token": TOKEN}),
    )?;
    assert_eq!(s.execute("git.remotes", &json!({}))?["url"], "https://ada@code.example.edu/lab/notes.git");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// A keychain in memory that outlives a session.
#[derive(Clone, Default)]
struct Keychain(Arc<Mutex<BTreeMap<String, String>>>);

impl Secrets for Keychain {
    fn get(&self, handle: &str) -> Result<Option<SecretString>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.get(handle).map(|s| SecretString::new(s.clone())))
    }
    fn put(&self, handle: &str, value: &SecretString) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(handle.to_owned(), value.expose().to_owned());
        Ok(())
    }
    fn remove(&self, handle: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(handle);
        Ok(())
    }
    fn lasting(&self) -> bool {
        true
    }
}

#[test]
fn an_overleaf_project_clones_with_its_git_token_remembered_for_later_sessions() -> R {
    const OVERLEAF: &str = "olp_clone0test0token";
    let dir = temp("overleaf")?;
    let server = Server::new(&bare(&dir.join("server.git"))?).with_auth("git", OVERLEAF);
    let url = "https://git.overleaf.com/6f2c1a9e4b7d";
    published(&dir, &server, url, OVERLEAF)?;
    let keychain = Keychain::default();

    let mut b = over(&server);
    b.set_forge_secrets(Arc::new(keychain.clone()));
    let folder = dir.join("thesis");
    let done = b.execute("project.clone", &json!({"url": url, "folder": folder.display().to_string(), "token": OVERLEAF, "remember": true}))?;
    assert_eq!(done["kept"], "keychain", "{done}");
    let (hosts, _) = b.git_hosts_saved();
    assert_eq!(hosts, ["git.overleaf.com"]);
    assert!(keychain.0.lock().map_err(|e| e.to_string())?.values().any(|v| v == OVERLEAF));

    // A later session, with the hosts the preferences kept: Sync needs no token.
    let mut c = over(&server);
    c.set_forge_secrets(Arc::new(keychain.clone()));
    c.restore_git_hosts(vec!["git.overleaf.com".to_owned()]);
    c.execute("project.open", &json!({"path": folder.display().to_string()}))?;
    write(&mut c, "main.tex", "\\documentclass{article}\n\\begin{document}\nFour\n\\end{document}\n")?;
    c.execute("project.write", &json!({}))?;
    c.execute("project.sync", &json!({}))?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut c)?));

    // Forgotten: gone from the keychain and the list.
    c.execute("git.forget_credential", &json!({"host": "git.overleaf.com"}))?;
    assert!(c.git_hosts_saved().0.is_empty() && keychain.0.lock().map_err(|e| e.to_string())?.is_empty());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_github_token_remembered_serves_the_next_import_a_push_and_the_suggestions_until_forgotten() -> R {
    const PAT: &str = "github_pat_kept0engine0test";
    let dir = temp("kept-github")?;
    let server = Server::new(&bare(&dir.join("server.git"))?).with_auth("x-access-token", PAT);
    let url = "https://github.com/ada/paper.git";
    published(&dir, &server, url, PAT)?;
    let keychain = Keychain::default();
    let fake = texcraft_engine::forge::fake::Fake::github("ada/paper", PAT, &[("main.tex", b"x\n")]);
    let with_fake = |s: &mut Session| {
        let fake = fake.clone();
        s.set_forge_transport(move || fake.transport());
    };
    let mut results = Vec::new();

    let mut b = over(&server);
    b.set_forge_secrets(Arc::new(keychain.clone()));
    let first = dir.join("first");
    let done =
        b.execute("project.clone", &json!({"url": url, "folder": first.display().to_string(), "service": "github", "token": PAT, "remember": true}))?;
    assert_eq!(done["kept"], "keychain", "{done}");
    results.push(done);

    // A later session, the hosts as the preferences kept them: nothing asks for the token.
    let mut c = over(&server);
    with_fake(&mut c);
    c.set_forge_secrets(Arc::new(keychain.clone()));
    c.restore_git_hosts(vec!["github.com".to_owned()]);
    assert!(c.forge_reaches("https://github.com", texcraft_engine::forge::Service::GitHub), "the kept token is not known");
    let found = c.execute("forge.projects", &json!({"service": "github", "query": "pap"}))?;
    assert!(found["projects"].as_array().is_some_and(|p| !p.is_empty()), "the suggestions need the token again: {found}");
    results.push(found);
    let second = dir.join("second");
    results.push(c.execute("project.clone", &json!({"url": url, "folder": second.display().to_string(), "service": "github"}))?);
    write(&mut c, "main.tex", "\\documentclass{article}\n\\begin{document}\nFour\n\\end{document}\n")?;
    c.execute("project.save", &json!({}))?;
    results.push(c.execute("git.push", &json!({}))?);
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut c)?), "the push did not go");
    let listed = c.execute("git.credentials", &json!({}))?;
    assert_eq!(listed["credentials"], json!([{"host": "github.com", "service": "github"}]), "{listed}");
    results.push(listed);

    // Replaced: the new token is the one kept.
    results.push(c.execute("git.keep_credential", &json!({"host": "github.com", "token": "github_pat_replaced0token"}))?);
    assert!(keychain.0.lock().map_err(|e| e.to_string())?.values().any(|v| v == "github_pat_replaced0token"));
    assert!(!keychain.0.lock().map_err(|e| e.to_string())?.values().any(|v| v == PAT), "the old token is still kept");

    // Forgotten: gone from the keychain and the list, and the next import asks again.
    results.push(c.execute("git.forget_credential", &json!({"host": "github.com"}))?);
    assert!(c.git_hosts_saved().0.is_empty() && keychain.0.lock().map_err(|e| e.to_string())?.is_empty());
    let mut d = over(&server);
    d.set_forge_secrets(Arc::new(keychain.clone()));
    d.restore_git_hosts(vec!["github.com".to_owned()]);
    assert!(d.git_hosts_saved().0.is_empty(), "a host whose token is gone is still listed");
    let third = dir.join("third");
    let refused = d.execute("project.clone", &json!({"url": url, "folder": third.display().to_string(), "service": "github"}));
    assert!(refused.is_err(), "cloned with a token forgotten");

    // No token in any result.
    for r in &results {
        let text = r.to_string();
        assert!(!text.contains(PAT) && !text.contains("github_pat_replaced"), "a token in a result: {text}");
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// A server that answers every request with `status`, or (none) cannot be reached.
struct Answer(Option<u16>);

impl Http for Answer {
    fn get(&self, _: &str, _: &[(String, String)]) -> Result<Reply, String> {
        self.0.map(|status| Reply { status, content_type: None, body: Vec::new() }).ok_or_else(|| "connection refused".to_owned())
    }
    fn post(&self, url: &str, headers: &[(String, String)], _: Vec<u8>) -> Result<Reply, String> {
        self.get(url, headers)
    }
}

#[test]
fn refusals_and_failures_are_said_in_words_and_leave_no_folder() -> R {
    let dir = temp("errors")?;
    for (status, kind, said) in [
        (Some(403), texcraft_engine::ImportProblem::Forbidden, "This account cannot read lab/notes"),
        (Some(404), texcraft_engine::ImportProblem::NotFound, "No git repository at this address"),
        (None, texcraft_engine::ImportProblem::Network, "Could not reach code.example.edu"),
    ] {
        let mut s = session(move || Box::new(Answer(status)));
        let folder = dir.join(format!("f{}", status.unwrap_or(0)));
        let e = s
            .execute("project.clone", &json!({"url": "https://code.example.edu/lab/notes.git", "folder": folder.display().to_string()}))
            .unwrap_err();
        assert!(e.contains(said), "{status:?}: {e}");
        assert_eq!(s.import_problem(), Some(kind));
        assert!(!folder.exists(), "the folder made is removed");
    }
    // Too large: the history's size and the limit, in words.
    let server = Server::new(&bare(&dir.join("server.git"))?);
    published(&dir, &server, "https://example.org/big.git", "unused")?;
    let mut s = over(&server);
    s.set_clone_limit(64);
    let folder = dir.join("big");
    let e = s.execute("project.clone", &json!({"url": "https://example.org/big.git", "folder": folder.display().to_string()})).unwrap_err();
    assert!(e.contains("TeXCraft imports up to"), "{e}");
    assert_eq!(s.import_problem(), Some(texcraft_engine::ImportProblem::TooLarge));
    assert!(!folder.exists());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn stopping_a_clone_removes_the_folder_it_made() -> R {
    let dir = temp("cancel")?;
    let server = Server::new(&bare(&dir.join("server.git"))?);
    published(&dir, &server, "https://example.org/lab/notes.git", "unused")?;
    let mut s = over(&server);
    let folder = dir.join("notes");
    let started =
        s.execute("project.clone", &json!({"url": "https://example.org/lab/notes.git", "folder": folder.display().to_string(), "background": true}))?;
    assert_eq!(started["started"], true);
    assert_eq!(s.import_busy(), Some("clone"));
    let stopped = s.execute("project.cancel_clone", &json!({}))?;
    assert_eq!(stopped["cancelled"], true, "{stopped}");
    assert_eq!(s.import_busy(), None);
    // The clone's thread ends by itself, and leaves nothing behind either.
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!folder.exists());
    assert!(s.poll_forge().is_none());

    // In the background to its end, as the window runs it.
    s.execute("project.clone", &json!({"url": "https://example.org/lab/notes.git", "folder": folder.display().to_string(), "background": true}))?;
    let done = wait(&mut s)?;
    assert_eq!(done["operation"], "clone", "{done}");
    assert!(folder.join("main.tex").is_file());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
