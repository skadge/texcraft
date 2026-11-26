//! Push and pull for project folders, and Save Project to GitLab or GitHub (docs/forges.md, step
//! 5), against the fake forge and the git server in memory: no test reaches the network.

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

const TOKEN: &str = "ghp-engine-git-test";

fn temp(name: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("texcraft-it-{}-git-{name}", std::process::id()));
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

fn write(s: &mut Session, path: &str, text: &str) -> Result<Value, String> {
    s.execute("file.write", &json!({"path": path, "text": text}))
}

fn text(s: &Session, path: &str) -> Option<String> {
    let path = ProjectPath::new(path).ok()?;
    s.project().text(&path).ok().map(str::to_owned)
}

fn head(s: &mut Session) -> Result<String, String> {
    s.execute("history.status", &json!({}))?["head"].as_str().map(str::to_owned).ok_or_else(|| "no head".to_owned())
}

/// A server whose pushes need GitHub's token as basic authentication, over a new bare repository.
fn server(dir: &Path) -> Result<Server, String> {
    let bare = dir.join("server.git");
    std::fs::create_dir_all(&bare).map_err(|e| e.to_string())?;
    let out = bare_init(&bare)?;
    Ok(Server::new(&out).with_auth("x-access-token", TOKEN))
}

/// A bare repository at `dir`: a repository's `.git` folder alone, as `git init --bare` makes.
fn bare_init(dir: &Path) -> Result<PathBuf, String> {
    for sub in ["objects/pack", "objects/info", "refs/heads", "refs/tags"] {
        std::fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
    }
    std::fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").map_err(|e| e.to_string())?;
    std::fs::write(dir.join("config"), "[core]\n\trepositoryformatversion = 0\n\tbare = true\n").map_err(|e| e.to_string())?;
    Ok(dir.to_path_buf())
}

/// Copies folder `from` (a project with its `.git`) to `to`, as a clone would make it.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A project saved in `dir` twice (two commits), then edited and not saved.
fn project(s: &mut Session, dir: &Path) -> Result<(), String> {
    write(s, "main.tex", "\\documentclass{article}\n\\begin{document}\nOne\n\\end{document}\n")?;
    s.execute("project.save_as", &json!({"path": dir.display().to_string()}))?;
    write(s, "main.tex", "\\documentclass{article}\n\\begin{document}\nTwo\n\\end{document}\n")?;
    s.execute("project.save", &json!({}))?;
    write(s, "main.tex", "\\documentclass{article}\n\\begin{document}\nThree\n\\end{document}\n")?;
    Ok(())
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
fn saving_to_github_makes_the_repository_sets_origin_and_pushes_the_whole_history() -> R {
    let dir = temp("save")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    let folder = dir.join("paper");
    project(&mut s, &folder)?;
    assert!(s.is_dirty());

    let saved = s.execute("forge.create_repository", &json!({"service": "github", "name": "paper", "token": TOKEN}))?;
    assert_eq!(saved["pushed"], true, "{saved}");
    assert_eq!(saved["remote"], "origin");
    assert_eq!(saved["web_url"], "https://github.com/ada/paper");
    assert!(!saved.to_string().contains(TOKEN), "the token is in no result");
    // A: made private and empty; C: the edit not saved was committed first, and everything pushed.
    let body = &fake.created()[0];
    assert_eq!((body["private"].clone(), body["auto_init"].clone()), (json!(true), json!(false)));
    assert!(!s.is_dirty());
    let tip = head(&mut s)?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(tip.clone()));
    let log = s.execute("history.list", &json!({}))?;
    assert!(log["commits"].as_array().map_or(0, Vec::len) >= 3, "the whole history: {log}");
    assert!(folder.join(".gitignore").is_file(), "the build files are ignored");

    // B: the folder's origin, as git sees it, and the branch following it, up to date.
    let config = std::fs::read_to_string(folder.join(".git/config"))?;
    assert!(config.contains("https://github.com/ada/paper.git"), "{config}");
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!(remotes["upstream"], json!({"remote": "origin", "branch": "main"}));
    assert_eq!((remotes["ahead"].clone(), remotes["behind"].clone()), (json!(0), json!(0)));
    assert_eq!(remotes["web_url"], "https://github.com/ada/paper");

    // A second save is refused: the project has its remote.
    assert!(s.execute("forge.create_repository", &json!({"service": "github", "name": "other", "token": TOKEN})).is_err());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_name_taken_is_said_and_nothing_is_set_or_pushed() -> R {
    let dir = temp("taken")?;
    let fake = Fake::account(Service::GitLab, "https://gitlab.example", "ada", TOKEN);
    fake.take("ada/paper");
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    project(&mut s, &dir.join("paper"))?;
    let refused = s
        .execute("forge.create_repository", &json!({"service": "gitlab", "instance": "https://gitlab.example", "name": "paper", "token": TOKEN}))
        .unwrap_err();
    assert!(refused.contains("choose another name"), "{refused}");
    assert_eq!(s.execute("git.remotes", &json!({}))?["remotes"], json!([]));
    assert_eq!(server.count("POST"), 0);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn pull_fast_forwards_merges_and_push_refuses_over_commits_not_pulled() -> R {
    let dir = temp("pull")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let (a_dir, b_dir) = (dir.join("a"), dir.join("b"));
    let mut a = session(&fake, &server);
    write(&mut a, "main.tex", "one\ntwo\nthree\nfour\nfive\n")?;
    a.execute("file.new", &json!({"path": "notes.tex", "text": "notes\n"}))?;
    a.execute("project.save_as", &json!({"path": a_dir.display().to_string()}))?;
    a.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"}))?;
    let pushed = a.execute("git.push", &json!({"token": TOKEN}))?;
    assert_eq!(pushed["old"], Value::Null);

    // B is a clone of A.
    copy(&a_dir, &b_dir)?;
    let mut b = session(&fake, &server);
    b.execute("project.open", &json!({"path": b_dir.display().to_string()}))?;

    // A pushes a change; B pulls it, fast-forward.
    write(&mut a, "main.tex", "ONE\ntwo\nthree\nfour\nfive\n")?;
    a.execute("project.save", &json!({}))?;
    a.execute("git.push", &json!({"token": TOKEN}))?;
    assert_eq!(b.execute("git.remotes", &json!({}))?["behind"], 0, "not known before a pull");
    let pulled = b.execute("git.pull", &json!({"token": TOKEN}))?;
    assert_eq!(pulled["fast_forward"], true, "{pulled}");
    assert_eq!(text(&b, "main.tex").as_deref(), Some("ONE\ntwo\nthree\nfour\nfive\n"));
    assert_eq!(std::fs::read_to_string(b_dir.join("main.tex"))?, "ONE\ntwo\nthree\nfour\nfive\n");
    assert!(!b.is_dirty());

    // Both change different lines; B pushes first, A's push is refused until it pulls and merges.
    write(&mut b, "main.tex", "ONE\ntwo\nthree\nfour\nFIVE\n")?;
    b.execute("project.save", &json!({}))?;
    b.execute("git.push", &json!({"token": TOKEN}))?;
    write(&mut a, "notes.tex", "more notes\n")?;
    write(&mut a, "main.tex", "ONE\nTWO\nthree\nfour\nfive\n")?;
    a.execute("project.save", &json!({}))?;
    let refused = a.execute("git.push", &json!({"token": TOKEN})).unwrap_err();
    assert!(refused.contains("pull first"), "{refused}");
    let merged = a.execute("git.pull", &json!({"token": TOKEN}))?;
    assert_eq!(merged["merged"], true, "{merged}");
    assert_eq!(text(&a, "main.tex").as_deref(), Some("ONE\nTWO\nthree\nfour\nFIVE\n"));
    assert_eq!(text(&a, "notes.tex").as_deref(), Some("more notes\n"));
    let remotes = a.execute("git.remotes", &json!({}))?;
    assert_eq!((remotes["ahead"].clone(), remotes["behind"].clone()), (json!(2), json!(0)), "{remotes}");
    a.execute("git.push", &json!({"token": TOKEN}))?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut a)?));

    // The same line changed on both sides: the pull says which file, and changes nothing.
    b.execute("git.pull", &json!({"token": TOKEN}))?;
    write(&mut b, "main.tex", "ONE\nTWO\nthree\nfour\nB\n")?;
    b.execute("project.save", &json!({}))?;
    b.execute("git.push", &json!({"token": TOKEN}))?;
    write(&mut a, "main.tex", "ONE\nTWO\nthree\nfour\nA\n")?;
    a.execute("project.save", &json!({}))?;
    let before = head(&mut a)?;
    let conflict = a.execute("git.pull", &json!({"token": TOKEN})).unwrap_err();
    assert!(conflict.contains("main.tex"), "{conflict}");
    assert_eq!(head(&mut a)?, before);
    assert_eq!(text(&a, "main.tex").as_deref(), Some("ONE\nTWO\nthree\nfour\nA\n"));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_pull_or_a_push_waits_for_the_project_to_be_saved_and_runs_in_the_background() -> R {
    let dir = temp("background")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    write(&mut s, "main.tex", "one\n")?;
    s.execute("project.save_as", &json!({"path": dir.join("p").display().to_string()}))?;
    s.execute("git.set_remote", &json!({"name": "origin", "url": "git@github.com:ada/paper.git"}))?;
    write(&mut s, "main.tex", "two\n")?;
    assert!(s.execute("git.push", &json!({"token": TOKEN})).unwrap_err().contains("Save the project first"));
    assert!(s.execute("git.pull", &json!({"token": TOKEN})).unwrap_err().contains("Save the project first"));
    s.execute("project.save", &json!({}))?;

    let started = s.execute("git.push", &json!({"token": TOKEN, "background": true}))?;
    assert_eq!(started["started"], true);
    assert!(s.execute("git.pull", &json!({})).is_err(), "one operation at a time");
    let done = wait(&mut s)?;
    assert_eq!(done["operation"], "git_push");
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut s)?));
    // An SSH address of the server was reached over HTTPS.
    assert!(server.requests().iter().all(|(_, url, _)| url.starts_with("https://github.com/ada/paper.git/")));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_wrong_token_is_said_in_words_and_never_shown() -> R {
    let dir = temp("auth")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    write(&mut s, "main.tex", "one\n")?;
    s.execute("project.save_as", &json!({"path": dir.join("p").display().to_string()}))?;
    s.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"}))?;
    let wrong = "ghp-not-the-right-one";
    let refused = s.execute("git.push", &json!({"token": wrong})).unwrap_err();
    assert!(refused.contains("did not accept"), "{refused}");
    assert!(!refused.contains(wrong));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn in_the_browser_saving_to_gitlab_commits_the_files_and_links_the_project() -> R {
    let instance = "https://gitlab.example";
    let fake = Fake::account(Service::GitLab, instance, "ada", TOKEN);
    let dir = temp("browser")?;
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    s.use_library(Library::default());
    write(&mut s, "main.tex", "\\documentclass{article}\n")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    let saved = s.execute("forge.create_repository", &json!({"service": "gitlab", "instance": instance, "name": "paper", "token": TOKEN}))?;
    assert_eq!(saved["linked"], true, "{saved}");
    let status = s.execute("forge.status", &json!({}))?;
    assert_eq!(status["linked"], true);
    assert_eq!(status["project"], "ada/paper");
    assert_eq!(status["changes"], json!([]), "nothing left to commit");
    assert!(fake.files("main").contains_key("main.tex"));
    assert_eq!(server.count("POST") + server.count("GET"), 0, "no git protocol in the browser's way");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// A clone of `a_dir` (a project pushed to `server`) opened in a new session.
fn clone_of(a_dir: &Path, b_dir: &Path, fake: &Fake, server: &Server) -> Result<Session, Box<dyn std::error::Error>> {
    copy(a_dir, b_dir)?;
    let mut b = session(fake, server);
    b.execute("project.open", &json!({"path": b_dir.display().to_string()}))?;
    Ok(b)
}

fn commits(s: &mut Session) -> Result<usize, String> {
    Ok(s.execute("history.list", &json!({"limit": 1000}))?["commits"].as_array().map_or(0, Vec::len))
}

#[test]
fn sync_makes_a_version_takes_in_the_remote_then_sends_everything() -> R {
    let dir = temp("sync")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let (a_dir, b_dir) = (dir.join("a"), dir.join("b"));
    let mut a = session(&fake, &server);
    write(&mut a, "main.tex", "one\ntwo\nthree\nfour\nfive\n")?;
    a.execute("project.save_as", &json!({"path": a_dir.display().to_string()}))?;
    assert!(a.execute("project.sync", &json!({"token": TOKEN})).is_err(), "no remote: nothing to sync with");
    a.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"}))?;
    let first = a.execute("project.sync", &json!({"token": TOKEN}))?;
    assert_eq!(first["operation"], "sync", "{first}");
    assert_eq!(first["pulled"]["empty"], true, "an empty remote: nothing to take in, everything sent");
    let mut b = clone_of(&a_dir, &b_dir, &fake, &server)?;

    // B changes a line and syncs; A changes another, written but in no version, and syncs.
    write(&mut b, "main.tex", "one\ntwo\nthree\nfour\nFIVE\n")?;
    b.execute("project.write", &json!({}))?;
    b.execute("project.sync", &json!({"token": TOKEN}))?;
    write(&mut a, "main.tex", "ONE\ntwo\nthree\nfour\nfive\n")?;
    a.execute("project.write", &json!({}))?;
    let before = commits(&mut a)?;
    let done = a.execute("project.sync", &json!({"token": TOKEN}))?;
    assert!(done["version"]["commit"].is_string(), "a version of the change first: {done}");
    assert_eq!(done["pulled"]["merged"], true, "{done}");
    assert_eq!(text(&a, "main.tex").as_deref(), Some("ONE\ntwo\nthree\nfour\nFIVE\n"));
    assert!(commits(&mut a)? > before + 1, "the version, B's commit and the merge");
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut a)?), "and all of it sent");
    let remotes = a.execute("git.remotes", &json!({}))?;
    assert_eq!((remotes["ahead"].clone(), remotes["behind"].clone()), (json!(0), json!(0)), "{remotes}");
    assert!(!a.unversioned());

    // In the background, as the window runs it: one operation, polled to its end.
    write(&mut b, "main.tex", "one\ntwo\nTHREE\nfour\nFIVE\n")?;
    let started = b.execute("project.sync", &json!({"background": true}))?;
    assert_eq!(started["started"], true, "{started}");
    assert_eq!(b.git_busy(), Some("sync"));
    let done = wait(&mut b)?;
    assert_eq!(done["operation"], "sync", "{done}");
    assert_eq!(text(&b, "main.tex").as_deref(), Some("ONE\ntwo\nTHREE\nfour\nFIVE\n"), "A's line came in");
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut b)?));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_sync_in_conflict_lists_the_files_and_the_choice_made_settles_them() -> R {
    let dir = temp("sync-conflict")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let (a_dir, b_dir) = (dir.join("a"), dir.join("b"));
    let mut a = session(&fake, &server);
    write(&mut a, "main.tex", "one\ntwo\n")?;
    a.execute("project.save_as", &json!({"path": a_dir.display().to_string()}))?;
    a.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"}))?;
    a.execute("project.sync", &json!({"token": TOKEN}))?;
    let mut b = clone_of(&a_dir, &b_dir, &fake, &server)?;
    write(&mut b, "main.tex", "one\nB\n")?;
    b.execute("project.sync", &json!({"token": TOKEN}))?;
    write(&mut a, "main.tex", "one\nA\n")?;

    let refused = a.execute("project.sync", &json!({})).unwrap_err();
    assert!(refused.contains("main.tex"), "{refused}");
    let status = a.execute("forge.status", &json!({}))?;
    assert_eq!(status["conflicts"][0]["path"], "main.tex", "the window's dialog lists it: {status}");
    assert_eq!(text(&a, "main.tex").as_deref(), Some("one\nA\n"), "nothing changed");

    a.execute("project.sync", &json!({"resolve": {"main.tex": "mine"}}))?;
    assert_eq!(text(&a, "main.tex").as_deref(), Some("one\nA\n"));
    assert!(a.forge_conflicts().is_empty());
    b.execute("project.sync", &json!({}))?;
    assert_eq!(text(&b, "main.tex").as_deref(), Some("one\nA\n"), "mine went to the remote");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn a_sync_merges_edits_to_other_sentences_of_the_same_paragraph_without_asking() -> R {
    let dir = temp("sync-words")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let server = server(&dir)?;
    let (a_dir, b_dir) = (dir.join("a"), dir.join("b"));
    let paragraph =
        |first: &str, last: &str| format!("\\section{{Results}}\nThe model {first} the baseline \\cite{{x}}. Training takes {last} on one GPU.\n");
    let mut a = session(&fake, &server);
    write(&mut a, "main.tex", &paragraph("beats", "a day"))?;
    a.execute("project.save_as", &json!({"path": a_dir.display().to_string()}))?;
    a.execute("git.set_remote", &json!({"name": "origin", "url": "https://github.com/ada/paper.git"}))?;
    a.execute("project.sync", &json!({"token": TOKEN}))?;
    let mut b = clone_of(&a_dir, &b_dir, &fake, &server)?;
    // B edits the second sentence and syncs; A edits the first sentence of the same line.
    write(&mut b, "main.tex", &paragraph("beats", "two days"))?;
    b.execute("project.sync", &json!({"token": TOKEN}))?;
    write(&mut a, "main.tex", &paragraph("matches", "a day"))?;

    let done = a.execute("project.sync", &json!({"token": TOKEN}))?;
    assert_eq!(done["pulled"]["merged"], true, "{done}");
    assert_eq!(text(&a, "main.tex"), Some(paragraph("matches", "two days")));
    assert!(a.forge_conflicts().is_empty(), "nothing asked");
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut a)?), "and sent");
    b.execute("project.sync", &json!({"token": TOKEN}))?;
    assert_eq!(text(&b, "main.tex"), Some(paragraph("matches", "two days")));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn an_overleaf_remote_is_named_and_reached_with_its_git_token() -> R {
    const OVERLEAF_TOKEN: &str = "olp_engine0test0token";
    let dir = temp("overleaf")?;
    let fake = Fake::account(Service::GitHub, "", "ada", TOKEN);
    let bare = dir.join("server.git");
    std::fs::create_dir_all(&bare)?;
    let server = Server::new(&bare_init(&bare)?).with_auth("git", OVERLEAF_TOKEN);
    let mut s = session(&fake, &server);
    write(&mut s, "main.tex", "one\n")?;
    s.execute("project.save_as", &json!({"path": dir.join("p").display().to_string()}))?;
    s.execute("git.set_remote", &json!({"name": "origin", "url": "https://git@git.overleaf.com/69cb8d545624606190d85c57"}))?;
    let remotes = s.execute("git.remotes", &json!({}))?;
    assert_eq!((remotes["hosting"].as_str(), remotes["hosting_name"].as_str()), (Some("overleaf"), Some("Overleaf")), "{remotes}");

    // No token yet: asked for, in words, with nothing sent.
    let asked = s.execute("project.sync", &json!({})).unwrap_err();
    assert!(asked.contains("Overleaf") && asked.contains("token"), "{asked}");
    assert!(matches!(s.forge_problem(), Some(texcraft_engine::ForgeProblem::NeedsToken { .. })));
    assert_eq!(server.requests().len(), 0);

    s.execute("project.sync", &json!({"token": OVERLEAF_TOKEN}))?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut s)?));
    // Over HTTPS at the project's address, the user name out of the URL and in the credentials.
    assert!(server.requests().iter().all(|(_, url, _)| url.starts_with("https://git.overleaf.com/69cb8d545624606190d85c57/")));
    // The token is kept for the session: the next Sync needs none.
    write(&mut s, "main.tex", "two\n")?;
    s.execute("project.sync", &json!({}))?;
    assert_eq!(server.get("refs/heads/main").map(|id| id.to_string()), Some(head(&mut s)?));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn in_the_browser_sync_commits_the_changes_to_the_linked_branch_saying_what_changed() -> R {
    let instance = "https://gitlab.example";
    let fake = Fake::account(Service::GitLab, instance, "ada", TOKEN);
    let dir = temp("browser-sync")?;
    let server = server(&dir)?;
    let mut s = session(&fake, &server);
    s.use_library(Library::default());
    write(&mut s, "main.tex", "\\documentclass{article}\n")?;
    s.execute("project.save_as", &json!({"path": "Paper"}))?;
    s.execute("forge.create_repository", &json!({"service": "gitlab", "instance": instance, "name": "paper", "token": TOKEN}))?;

    s.execute("file.new", &json!({"path": "intro.tex", "text": "Hello\n"}))?;
    let done = s.execute("project.sync", &json!({}))?;
    assert_eq!(done["sync"], true, "{done}");
    assert!(done["commit"].is_string(), "{done}");
    assert_eq!(fake.files("main").get("intro.tex").map(Vec::as_slice), Some(&b"Hello\n"[..]));
    assert!(s.execute("forge.status", &json!({}))?["changes"].as_array().is_some_and(Vec::is_empty));
    // Nothing left to send: a Sync takes in the branch only.
    let again = s.execute("project.sync", &json!({}))?;
    assert_eq!(again["operation"], "pull", "{again}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
