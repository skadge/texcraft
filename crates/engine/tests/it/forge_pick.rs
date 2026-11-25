//! `forge.projects` and `forge.branches` (C5), against the fake forges: the projects the token
//! reaches with their visibility and default branch, a project's branches with the default one,
//! GitHub's listing kept for the next query, only the last lookup typed answered, errors in words,
//! and the token never in a result. No test reaches the network.

use serde_json::{Value, json};
use texcraft_compile::Unavailable;
use texcraft_engine::forge::fake::Fake;
use texcraft_engine::{ForgeLookup, Session};

const TOKEN: &str = "ghp_engine_pick_secret";

fn session(fake: &Fake) -> Session {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    let fake = fake.clone();
    s.set_forge_transport(move || fake.transport());
    s
}

fn github() -> Fake {
    let fake = Fake::github("ada/paper", TOKEN, &[("main.tex", b"x\n")]);
    fake.list("ada/thesis", "public", "2026-03-01T00:00:00Z", Some("trunk"));
    fake.list_public("someone/handbook");
    fake
}

fn paths(v: &Value) -> Vec<String> {
    v["projects"].as_array().into_iter().flatten().filter_map(|p| p["path"].as_str().map(str::to_owned)).collect()
}

/// Polls until the lookup running ends.
fn wait(s: &mut Session) -> Option<(ForgeLookup, Result<Value, String>)> {
    for _ in 0..500 {
        if let Some(done) = s.poll_forge_lookup() {
            return Some(done);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    None
}

#[test]
fn projects_are_listed_with_their_visibility_and_github_s_listing_answers_the_next_query() -> Result<(), String> {
    let fake = github();
    let mut s = session(&fake);
    let all = s.execute("forge.projects", &json!({"service": "github", "token": TOKEN}))?;
    assert_eq!(paths(&all), ["ada/paper", "ada/thesis"]);
    let paper = &all["projects"][0];
    assert_eq!((paper["private"].as_bool(), paper["default_branch"].as_str()), (Some(true), Some("main")));
    assert_eq!(paper["web_url"], "https://github.com/ada/paper");
    assert!(paper["activity"].as_str().is_some_and(|t| t.starts_with("2026-11-20")), "{paper}");
    assert_eq!(all["projects"][1]["private"], false);

    // The token was given once: it answers the next lookups, and the listing kept does too.
    let asked = fake.requests().len();
    let thesis = s.execute("forge.projects", &json!({"service": "github", "query": "thes"}))?;
    assert_eq!(paths(&thesis), ["ada/thesis"]);
    assert_eq!(fake.requests().len(), asked, "the listing was read again");

    // Nothing listed matches: GitHub's search.
    let found = s.execute("forge.projects", &json!({"service": "github", "query": "handbook"}))?;
    assert_eq!((paths(&found), found["searched"].as_bool()), (vec!["someone/handbook".to_owned()], Some(true)));
    for v in [&all, &thesis, &found] {
        assert!(!v.to_string().contains(TOKEN), "a token in a result: {v}");
    }
    Ok(())
}

#[test]
fn branches_list_the_default_one_by_path_or_by_address_on_a_server() -> Result<(), String> {
    let fake = Fake::new("https://gitlab.example", "group/paper", TOKEN, &[("main.tex", b"x\n")]);
    fake.push("draft", &[("main.tex", Some(b"y\n"))]);
    fake.protect("main");
    let mut s = session(&fake);
    let at = json!({"instance": "https://gitlab.example", "project": "group/paper", "token": TOKEN});
    for params in [at, json!({"project": "https://gitlab.example/group/paper"})] {
        let v = s.execute("forge.branches", &params)?;
        assert_eq!(v["default"], "main", "{params}");
        assert_eq!(v["project"], "group/paper");
        let list = v["branches"].as_array().cloned().unwrap_or_default();
        let names: Vec<&str> = list.iter().filter_map(|b| b["name"].as_str()).collect();
        assert_eq!(names, ["draft", "main"]);
        assert_eq!((list[1]["default"].as_bool(), list[1]["protected"].as_bool()), (Some(true), Some(true)));
        assert_eq!(list[0]["default"], false);
        assert!(!v.to_string().contains(TOKEN));
    }
    let missing = s.execute("forge.branches", &json!({"instance": "https://gitlab.example", "project": "group/other"})).unwrap_err();
    assert!(missing.contains("cannot see it"), "{missing}");
    assert!(!missing.contains(TOKEN));
    Ok(())
}

#[test]
fn without_a_sign_in_or_a_token_a_lookup_says_how_to_get_one() {
    let fake = github();
    let mut s = session(&fake);
    let e = s.execute("forge.projects", &json!({"service": "github", "query": "paper"})).unwrap_err();
    assert!(e.contains("Sign in to GitHub") && e.contains("personal access token"), "{e}");
    assert!(fake.requests().is_empty(), "the forge was asked with no key");
}

#[test]
fn a_lookup_typed_while_one_runs_replaces_it_and_only_the_last_is_answered() -> Result<(), String> {
    let fake = Fake::new("https://gitlab.example", "group/paper", TOKEN, &[("main.tex", b"x\n")]);
    fake.list("group/thesis", "private", "2026-01-01T00:00:00Z", Some("main"));
    let mut s = session(&fake);
    let ask = |q: &str| json!({"instance": "https://gitlab.example", "token": TOKEN, "query": q, "background": true});
    assert_eq!(s.execute("forge.projects", &ask("p"))?["started"], true);
    for q in ["pa", "pap", "thes"] {
        assert_eq!(s.execute("forge.projects", &ask(q))?["queued"], true);
    }
    let (kind, done) = wait(&mut s).ok_or("the lookup did not end")?;
    let done = done?;
    assert_eq!((kind, done["query"].as_str()), (ForgeLookup::Projects, Some("thes")), "an earlier query was answered");
    assert_eq!(paths(&done), ["group/thesis"]);
    assert!(s.poll_forge_lookup().is_none() && !s.forge_lookup_busy());
    // The one running and the last one: the two between were never sent.
    assert_eq!(fake.requests().iter().filter(|r| r.contains("/projects?")).count(), 2, "{:?}", fake.requests());
    Ok(())
}

#[test]
fn a_rate_limit_says_when_to_try_again() {
    let fake = github();
    let mut s = session(&fake);
    fake.fail_with(429, &[("retry-after", "30")], "{}");
    let e = s.execute("forge.projects", &json!({"service": "github", "token": TOKEN})).unwrap_err();
    assert!(e.contains("try again in 30 seconds"), "{e}");
}
