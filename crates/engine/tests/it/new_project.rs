//! A new project from a template kept on a forge (docs/design/new-project.md): the repository made
//! first, then the template written, committed, set to push to it and pushed (the desktop), or
//! committed to it through the forge's API (the browser). Against the fake forge and the git
//! server in memory: no test reaches the network.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::Service;
use texcraft_engine::forge::fake::Fake;
use texcraft_engine::gitwire::fake::Server;
use texcraft_engine::library::Library;

type R = Result<(), Box<dyn std::error::Error>>;

const TOKEN: &str = "glpat-engine-new-project-test";
const GITLAB: &str = "https://gitlab.example";

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-new-project-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A bare repository on the git server in memory, whose pushes need `user` and the token.
fn server(dir: &Path, user: &str) -> Result<Server, String> {
    let bare = dir.join("server.git");
    for sub in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        std::fs::create_dir_all(bare.join(sub)).map_err(|e| e.to_string())?;
    }
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").map_err(|e| e.to_string())?;
    std::fs::write(bare.join("config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n").map_err(|e| e.to_string())?;
    Ok(Server::new(&bare).with_auth(user, TOKEN))
}

fn session(dir: &Path, fake: &Fake, server: &Server) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.set_template_store(dir.join("store"));
    let fake = fake.clone();
    s.set_forge_transport(move || fake.transport());
    let server = server.clone();
    s.set_git_http(move || Box::new(server.clone()));
    s
}

fn on_gitlab(name: &str) -> Value {
    json!({"service": "gitlab", "instance": GITLAB, "name": name, "token": TOKEN})
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
fn on_gitlab_the_repository_is_made_then_the_template_written_committed_and_pushed() -> R {
    let dir = temp("gitlab")?;
    let fake = Fake::account(Service::GitLab, GITLAB, "ada", TOKEN);
    let server = server(&dir, "oauth2")?;
    let mut s = session(&dir, &fake, &server);
    let folder = dir.join("paper");
    let made =
        s.execute("project.from_template", &json!({"template": "article", "folder": folder.display().to_string(), "forge": on_gitlab("paper")}))?;
    assert_eq!(made["operation"], "new_project");
    assert_eq!(made["project"], "ada/paper");
    assert_eq!(made["pushed"], true, "{made}");
    assert!(!made.to_string().contains(TOKEN), "the token is in no result");
    assert_eq!(fake.created().len(), 1);
    assert_eq!(fake.created()[0]["visibility"], "private", "private unless said otherwise");
    // The template is in the folder, open, committed, and that commit is what the server has.
    let main = made["main"].as_str().ok_or("no main document")?;
    assert!(folder.join(main).is_file());
    assert_eq!(s.project().main().map(|p| p.as_str()), Some(main));
    let head = s.execute("history.status", &json!({}))?["head"].as_str().map(str::to_owned);
    assert!(head.is_some(), "nothing was committed");
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), head, "the server did not get the commit");
    // Sync works at once: origin is the new repository, and the branch follows it.
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["upstream"], json!({"remote": "origin", "branch": "main"}), "{remotes}");
    assert_eq!(remotes["url"], format!("{GITLAB}/ada/paper.git"));
    assert!(s.execute("project.sync", &json!({})).is_ok(), "Sync does not work at once");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_name_taken_ends_it_before_anything_is_written_or_sent() -> R {
    let dir = temp("taken")?;
    let fake = Fake::account(Service::GitLab, GITLAB, "ada", TOKEN);
    fake.take("ada/paper");
    let server = server(&dir, "oauth2")?;
    let mut s = session(&dir, &fake, &server);
    let folder = dir.join("paper");
    let available = s.execute("forge.name_available", &json!({"service": "gitlab", "instance": GITLAB, "name": "paper", "token": TOKEN}))?;
    assert_eq!((available["available"].clone(), available["project"].clone()), (json!(false), json!("ada/paper")));
    let other = s.execute("forge.name_available", &json!({"service": "gitlab", "instance": GITLAB, "name": "thesis", "token": TOKEN}))?;
    assert_eq!(other["available"], true);
    let refused = s
        .execute("project.from_template", &json!({"template": "article", "folder": folder.display().to_string(), "forge": on_gitlab("paper")}))
        .unwrap_err();
    assert!(refused.contains("choose another name"), "{refused}");
    assert!(!folder.exists(), "the folder was made although the repository was not");
    assert_eq!(server.count("POST"), 0);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_push_that_fails_after_the_repository_was_made_keeps_the_project_and_says_why() -> R {
    let dir = temp("push")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir, "x-access-token")?;
    server.refuse("the disk is full");
    let mut s = session(&dir, &fake, &server);
    let folder = dir.join("paper");
    let params = json!({"template": "article", "folder": folder.display().to_string(), "background": true,
        "forge": {"service": "github", "name": "paper", "token": TOKEN}});
    assert_eq!(s.execute("project.from_template", &params)?["started"], true);
    assert_eq!(s.git_busy(), Some("new_project"));
    let made = wait(&mut s)?;
    assert_eq!(made["pushed"], false, "{made}");
    assert!(made["push_error"].as_str().is_some_and(|e| !e.is_empty()), "{made}");
    assert_eq!(made["project"], "ada/paper", "the repository made is said");
    // The project is kept, open, with its remote: Sync sends it later.
    assert!(folder.join(made["main"].as_str().unwrap_or("main.tex")).is_file());
    assert_eq!(s.dir(), Some(&folder));
    assert_eq!(s.execute("git.remotes", &json!({}))?["url"], "https://github.com/ada/paper.git");
    assert_eq!(fake.created().len(), 1, "the repository is not made again, nor removed");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn in_the_browser_the_template_is_the_first_commit_of_the_new_repository() -> R {
    let dir = temp("browser")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir, "x-access-token")?;
    let mut s = session(&dir, &fake, &server);
    s.use_library(Library::default());
    let made = s.execute(
        "project.from_template",
        &json!({"template": "article", "folder": "My paper", "forge": {"service": "github", "name": "my-paper", "token": TOKEN}}),
    )?;
    assert_eq!(made["linked"], true, "{made}");
    assert_eq!(made["folder"], "My paper");
    let status = s.execute("forge.status", &json!({}))?;
    assert_eq!(status["project"], "ada/my-paper");
    assert_eq!(status["changes"], json!([]), "nothing left to commit");
    let main = made["main"].as_str().ok_or("no main document")?;
    let files = fake.files("main");
    assert!(files.contains_key(main), "the template is not the first commit: {:?}", files.keys());
    assert!(!files.contains_key("README.md"), "GitHub's README stayed");
    assert!(s.library().is_some_and(|l| l.contains("My paper")));
    assert_eq!(server.count("POST") + server.count("GET"), 0, "no git protocol in the browser's way");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn internal_is_refused_on_github_and_nothing_is_made() -> R {
    let dir = temp("internal")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir, "x-access-token")?;
    let mut s = session(&dir, &fake, &server);
    let params = json!({"template": "article", "folder": dir.join("paper").display().to_string(),
        "forge": {"service": "github", "name": "paper", "visibility": "internal", "token": TOKEN}});
    assert!(s.execute("project.from_template", &params).is_err());
    assert!(fake.created().is_empty());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
