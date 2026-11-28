//! Link to a Repository (docs/design/add-remote.md): an open project linked to a repository that
//! exists, against the git server in memory and the fake forge; no test reaches the network.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::forge::Service;
use texcraft_engine::forge::fake::Fake;
use texcraft_engine::gitwire::fake::Server;
use texcraft_engine::library::Library;
use texcraft_engine::project::ProjectPath;

type R = Result<(), Box<dyn std::error::Error>>;

const TOKEN: &str = "ghp-engine-link-test";
const URL: &str = "https://github.com/ada/paper.git";

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-link-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn session(fake: &Fake, server: &Server) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let fake = fake.clone();
    s.set_forge_transport(move || fake.transport());
    let server = server.clone();
    s.set_git_http(move || Box::new(server.clone()));
    s
}

/// A server over a new bare repository whose git requests need GitHub's token.
fn server(dir: &Path) -> Result<Server, String> {
    let bare = dir.join("server.git");
    for sub in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        std::fs::create_dir_all(bare.join(sub)).map_err(|e| e.to_string())?;
    }
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").map_err(|e| e.to_string())?;
    std::fs::write(bare.join("config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n").map_err(|e| e.to_string())?;
    Ok(Server::new(&bare).with_auth("x-access-token", TOKEN))
}

/// Writes `path`, making it when the project has none.
fn write(s: &mut Session, path: &str, text: &str) -> Result<Value, String> {
    s.execute("file.write", &json!({"path": path, "text": text})).or_else(|_| s.execute("file.new", &json!({"path": path, "text": text})))
}

fn text(s: &Session, path: &str) -> Option<String> {
    let path = ProjectPath::new(path).ok()?;
    s.project().text(&path).ok().map(str::to_owned)
}

/// A project of `files` saved in folder `dir`.
fn project(s: &mut Session, dir: &Path, files: &[(&str, &str)]) -> Result<(), String> {
    for (path, body) in files {
        write(s, path, body)?;
    }
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    Ok(())
}

/// Another history on the server: a project of `files` of its own, pushed there.
fn others(dir: &Path, fake: &Fake, server: &Server, files: &[(&str, &str)]) -> Result<(), String> {
    let mut b = session(fake, server);
    project(&mut b, &dir.join("theirs"), files)?;
    b.execute("git.set_remote", &json!({"name": "origin", "url": URL}))?;
    b.execute("git.push", &json!({"token": TOKEN}))?;
    Ok(())
}

fn commits(s: &mut Session) -> Result<usize, String> {
    Ok(s.execute("history.list", &json!({"limit": 1000}))?["commits"].as_array().map_or(0, Vec::len))
}

#[test]
fn an_empty_repository_gets_the_whole_history_and_sync_works_after() -> R {
    let dir = temp("empty")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    project(&mut s, &dir.join("paper"), &[("main.tex", "\\documentclass{article}\n")])?;
    write(&mut s, "main.tex", "\\documentclass{article}\n% more\n")?;
    s.execute("project.save", &json!({}))?;

    let seen = s.execute("git.check_remote", &json!({"url": URL, "token": TOKEN}))?;
    assert_eq!(seen["empty"], true, "{seen}");
    assert_eq!(seen["name_taken"], false);
    assert!(server.get("refs/heads/main").is_none(), "a check changes nothing");

    let done = s.execute("git.add_remote", &json!({"url": URL, "token": TOKEN}))?;
    assert_eq!(done["added"]["name"], "origin", "{done}");
    assert!(!done.to_string().contains(TOKEN), "the token is in no result");
    let tip = s.execute("history.status", &json!({}))?["head"].as_str().map(str::to_owned);
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), tip, "the history went");
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["upstream"]["remote"], "origin", "the branch follows it: {remotes}");
    assert_eq!((remotes["ahead"].clone(), remotes["behind"].clone()), (json!(0), json!(0)));

    write(&mut s, "intro.tex", "Hello\n")?;
    let synced = s.execute("project.sync", &json!({}))?;
    assert_eq!(synced["operation"], "sync", "{synced}");
    assert_ne!(server.get("refs/heads/main").map(|id| id.to_string()), tip, "a later Sync sends more");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn another_history_with_other_files_is_merged_keeping_both_and_sent() -> R {
    let dir = temp("disjoint")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    others(&dir, &fake, &server, &[("README.md", "A repository made on the forge\n")])?;
    let theirs = server.get("refs/heads/main").ok_or("nothing pushed")?.to_string();
    let mut s = session(&fake, &server);
    // Both start from the same blank main.tex; their other files differ.
    project(&mut s, &dir.join("paper"), &[("paper.tex", "\\documentclass{article}\n")])?;
    let before = commits(&mut s)?;

    let seen = s.execute("git.check_remote", &json!({"url": URL, "token": TOKEN}))?;
    assert_eq!((seen["empty"].clone(), seen["related"].clone()), (json!(false), json!(false)), "{seen}");
    assert!(seen["versions"].as_u64() > Some(0), "{seen}");
    assert_eq!(seen["clashes"], json!([]), "no file on both sides: {seen}");
    assert!(seen["only_there"].as_u64() > Some(0) && seen["only_here"].as_u64() > Some(0), "{seen}");
    assert!(text(&s, "README.md").is_none(), "a check takes nothing in");

    s.execute("git.add_remote", &json!({"url": URL, "token": TOKEN, "branch": "main"}))?;
    assert!(text(&s, "README.md").is_some() && text(&s, "paper.tex").is_some(), "both sets of files");
    assert!(dir.join("paper/README.md").is_file(), "on disk too");
    assert!(commits(&mut s)? > before, "their versions and the merge are in the history");
    let tip = server.get("refs/heads/main").ok_or("gone")?.to_string();
    assert_ne!(tip, theirs, "the merge was sent");
    let log = s.execute("history.list", &json!({"limit": 1000}))?;
    assert!(log.to_string().contains(&theirs), "their history is kept, not replaced: {log}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_file_on_both_sides_with_other_contents_is_chosen_per_file() -> R {
    for (choice, kept) in [("mine", "Mine\n"), ("theirs", "Theirs\n")] {
        let dir = temp(&format!("clash-{choice}"))?;
        let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
        let server = server(&dir)?;
        others(&dir, &fake, &server, &[("main.tex", "Theirs\n"), ("refs.bib", "@book{a}\n")])?;
        let mut s = session(&fake, &server);
        project(&mut s, &dir.join("paper"), &[("main.tex", "Mine\n")])?;

        let seen = s.execute("git.check_remote", &json!({"url": URL, "token": TOKEN}))?;
        assert_eq!(seen["clashes"], json!(["main.tex"]), "{seen}");

        let refused = s.execute("git.add_remote", &json!({"url": URL, "token": TOKEN})).unwrap_err();
        assert!(refused.contains("main.tex"), "{refused}");
        let status = s.execute("forge.status", &json!({}))?;
        assert_eq!(status["conflicts"][0]["path"], "main.tex", "the window's dialog lists it: {status}");
        assert_eq!(text(&s, "main.tex").as_deref(), Some("Mine\n"), "nothing changed");
        assert!(text(&s, "refs.bib").is_none(), "nothing taken in");

        // The link stays: Sync with the choice gets through, as its dialog does.
        s.execute("project.sync", &json!({"resolve": {"main.tex": choice}}))?;
        assert_eq!(text(&s, "main.tex").as_deref(), Some(kept), "{choice}");
        assert!(text(&s, "refs.bib").is_some(), "the other file came in");
        assert!(s.forge_conflicts().is_empty());
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[test]
fn a_remote_named_origin_already_asks_for_another_name_and_unlinking_touches_neither_side() -> R {
    let dir = temp("named")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    project(&mut s, &dir.join("paper"), &[("main.tex", "x\n")])?;
    s.execute("git.set_remote", &json!({"name": "origin", "url": "https://gitlab.com/ada/elsewhere.git"}))?;

    let seen = s.execute("git.check_remote", &json!({"url": URL, "token": TOKEN}))?;
    assert_eq!(seen["name_taken"], true, "{seen}");
    let suggested = seen["suggested_name"].as_str().ok_or("no name suggested")?.to_owned();
    assert_ne!(suggested, "origin");

    let refused = s.execute("git.add_remote", &json!({"url": URL, "token": TOKEN})).unwrap_err();
    assert!(refused.contains("origin"), "{refused}");
    let config = std::fs::read_to_string(dir.join("paper/.git/config"))?;
    assert!(config.contains("elsewhere") && !config.contains("ada/paper.git"), "origin as it was: {config}");

    s.execute("git.add_remote", &json!({"url": URL, "token": TOKEN, "name": suggested}))?;
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["remotes"].as_array().map(Vec::len), Some(2), "{remotes}");
    assert_eq!(remotes["upstream"]["remote"], suggested.as_str(), "Sync uses the new one");
    let sent = server.get("refs/heads/main").ok_or("nothing sent")?;

    let removed = s.execute("git.remove_remote", &json!({}))?;
    assert_eq!(removed["name"], suggested.as_str(), "{removed}");
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["remotes"].as_array().map(Vec::len), Some(1), "origin stays: {remotes}");
    assert_eq!(server.get("refs/heads/main"), Some(sent), "the repository is untouched");
    assert_eq!(text(&s, "main.tex").as_deref(), Some("x\n"), "and the project");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_link_that_cannot_start_leaves_the_project_as_it_was() -> R {
    let dir = temp("refused")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    project(&mut s, &dir.join("paper"), &[("main.tex", "x\n")])?;
    assert!(s.execute("git.add_remote", &json!({"url": URL, "token": "wrong", "branch": "main"})).is_err());
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["remotes"], json!([]), "no remote left behind: {remotes}");
    assert!(remotes["upstream"].is_null());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn in_the_browser_a_gitlab_branch_is_linked_bringing_both_sets_of_files_together() -> R {
    let instance = "https://gitlab.example";
    let fake = Fake::new(instance, "ada/paper", TOKEN, &[("README.md", b"Made on GitLab\n"), ("main.tex", b"Theirs\n")]);
    let dir = temp("browser")?;
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    s.use_library(Library::default());
    write(&mut s, "main.tex", "Mine\n")?;
    write(&mut s, "intro.tex", "Hello\n")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    let url = format!("{instance}/ada/paper");

    let seen = s.execute("git.check_remote", &json!({"url": url, "service": "gitlab", "token": TOKEN}))?;
    assert_eq!(seen["clashes"], json!(["main.tex"]), "{seen}");
    assert!(s.forge().is_none(), "a check links nothing");

    let refused = s.execute("git.add_remote", &json!({"url": url, "service": "gitlab", "token": TOKEN})).unwrap_err();
    assert!(refused.contains("main.tex"), "{refused}");
    assert!(s.forge().is_none() && text(&s, "README.md").is_none(), "nothing changed");

    let linked = s.execute("git.add_remote", &json!({"url": url, "service": "gitlab", "token": TOKEN, "resolve": {"main.tex": "mine"}}))?;
    assert_eq!(linked["linked"], true, "{linked}");
    assert!(linked["to_send"].as_u64() > Some(0), "{linked}");
    assert_eq!(text(&s, "README.md").as_deref(), Some("Made on GitLab\n"), "the branch's file came in");
    assert_eq!(text(&s, "main.tex").as_deref(), Some("Mine\n"), "the choice kept");

    s.execute("project.sync", &json!({}))?;
    let files = fake.files("main");
    assert_eq!(files.get("main.tex").map(Vec::as_slice), Some(&b"Mine\n"[..]), "sent");
    assert!(files.contains_key("intro.tex") && files.contains_key("README.md"), "both sets on the branch");
    assert_eq!(server.count("POST") + server.count("GET"), 0, "no git protocol in the browser's way");

    s.execute("git.remove_remote", &json!({}))?;
    assert!(s.forge().is_none(), "unlinked");
    assert!(fake.files("main").contains_key("intro.tex"), "the branch untouched");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
