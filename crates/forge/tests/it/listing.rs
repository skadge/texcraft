//! The projects an account reaches and a project's branches, as the Open dialog suggests and
//! lists them: the requests' shapes, pagination up to its cap, the private marker, the default
//! branch, GitHub's search when nothing listed matches, and the rate limit in words.

use texcraft_forge::fake::Fake;
use texcraft_forge::listing::{Lister, MAX_LISTED, Visibility, find, matching};
use texcraft_forge::{Address, Error, GITHUB_COM, Service, Token};

use crate::{INSTANCE, PROJECT, TOKEN, block_on, paper};

fn lister(fake: &Fake, service: Service) -> texcraft_forge::Result<Lister> {
    let instance = if service == Service::GitHub { GITHUB_COM } else { INSTANCE };
    Ok(Lister::new(fake.transport(), service, instance, Token::new(TOKEN)?))
}

fn github() -> Fake {
    Fake::github(PROJECT, TOKEN, &[("main.tex", b"x\n")])
}

#[test]
fn github_lists_the_user_s_repositories_most_recently_pushed_first_with_their_visibility() {
    let fake = github();
    fake.list("ada/notes", "public", "2026-10-01T08:00:00Z", Some("trunk"));
    let listed = block_on(lister(&fake, Service::GitHub).unwrap().reachable()).unwrap();
    let request = &fake.requests()[0];
    assert!(request.starts_with("GET https://api.github.com/user/repos?"), "{request}");
    for part in ["sort=pushed", "affiliation=owner,collaborator,organization_member", "per_page=100"] {
        assert!(request.contains(part), "{request} lacks {part}");
    }
    let paths: Vec<&str> = listed.items.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, [PROJECT, "ada/notes"]);
    let (own, notes) = (&listed.items[0], &listed.items[1]);
    assert_eq!((own.visibility, notes.visibility), (Visibility::Private, Visibility::Public));
    assert_eq!(notes.default_branch.as_deref(), Some("trunk"));
    assert!(own.activity > notes.activity, "the times were not read");
    assert!(!listed.truncated);
}

#[test]
fn a_long_list_stops_at_its_cap_and_says_there_are_more() {
    let fake = github();
    for i in 0..MAX_LISTED + 20 {
        fake.list(&format!("ada/paper-{i}"), "private", "2026-01-01T00:00:00Z", Some("main"));
    }
    let listed = block_on(lister(&fake, Service::GitHub).unwrap().reachable()).unwrap();
    assert_eq!(listed.items.len(), MAX_LISTED);
    assert!(listed.truncated, "the cap was not said");
    assert!(fake.requests().len() <= MAX_LISTED / 100, "pages past the cap were read: {}", fake.requests().len());
    assert!(fake.requests().len() > 1, "only the first page was read");
}

#[test]
fn github_searches_by_name_only_when_nothing_listed_matches() {
    let fake = github();
    fake.list("ada/thesis-2025", "private", "2026-01-01T00:00:00Z", Some("main"));
    fake.list_public("someone/handbook");
    let l = lister(&fake, Service::GitHub).unwrap();
    let listing = block_on(l.reachable()).unwrap();

    let before = fake.requests().len();
    let found = block_on(find(&l, Some(listing.clone()), "thesis")).unwrap();
    assert_eq!(found.projects.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["ada/thesis-2025"]);
    assert!(!found.searched && fake.requests().len() == before, "a listed match still asked GitHub");

    let found = block_on(find(&l, Some(listing.clone()), "handb")).unwrap();
    assert!(found.searched, "nothing listed matched, yet GitHub's search was not asked");
    assert_eq!(found.projects.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["someone/handbook"]);
    let search = fake.requests().last().cloned().unwrap_or_default();
    assert!(search.starts_with("GET https://api.github.com/search/repositories?q=handb%20in%3Aname"), "{search}");

    // An owner's name searches that owner's repositories.
    block_on(find(&l, Some(listing.clone()), "someone/hand")).unwrap();
    let search = fake.requests().last().cloned().unwrap_or_default();
    assert!(search.contains("user%3Asomeone"), "{search}");

    // Too short to search: nothing, and no request.
    let before = fake.requests().len();
    let found = block_on(find(&l, Some(listing), "zq")).unwrap();
    assert!(found.projects.is_empty() && !found.searched && fake.requests().len() == before);
}

#[test]
fn matching_puts_names_that_start_with_the_query_first() {
    let fake = github();
    fake.list("ada/old-paper", "private", "2026-01-01T00:00:00Z", None);
    fake.list("ada/paperwork", "private", "2025-01-01T00:00:00Z", None);
    let listed = block_on(lister(&fake, Service::GitHub).unwrap().reachable()).unwrap();
    let found: Vec<&str> = matching(&listed.items, "PAPER").iter().map(|p| p.path.as_str()).collect();
    // group/paper is the name itself; paperwork starts with it; old-paper only holds it.
    assert_eq!(found, [PROJECT, "ada/paperwork", "ada/old-paper"]);
}

#[test]
fn gitlab_searches_the_user_s_projects_on_the_instance() {
    let fake = paper();
    fake.list("lab/thesis", "internal", "2026-02-01T00:00:00Z", Some("draft"));
    fake.list_public("other/thesis-public");
    let l = lister(&fake, Service::GitLab).unwrap();
    let found = block_on(find(&l, None, "thes")).unwrap();
    let request = &fake.requests()[0];
    assert!(request.starts_with(&format!("GET {INSTANCE}/api/v4/projects?membership=true&order_by=last_activity_at")), "{request}");
    assert!(request.contains("search=thes"), "{request}");
    assert_eq!(found.projects.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["lab/thesis"], "a project the user is no member of");
    assert_eq!((found.projects[0].visibility, found.projects[0].default_branch.as_deref()), (Visibility::Internal, Some("draft")));
    // A group's path searches the namespaces too.
    let found = block_on(find(&l, None, "lab/")).unwrap();
    assert!(fake.requests().last().is_some_and(|r| r.contains("search_namespaces=true")));
    assert_eq!(found.projects.len(), 1);
}

#[test]
fn branches_mark_the_default_and_the_protected_ones_page_after_page() {
    for fake in crate::papers() {
        let service = fake.address().service;
        for i in 0..150 {
            fake.push(&format!("topic-{i:03}"), &[("main.tex", Some(b"y\n"))]);
        }
        fake.protect("topic-001");
        let address = fake.address();
        let (listed, default) = block_on(lister(&fake, service).unwrap().branches(&address)).unwrap();
        assert_eq!(default.as_deref(), Some("main"), "{service:?}");
        assert_eq!(listed.items.len(), 151, "{service:?}: the second page was not read");
        let main = listed.items.iter().find(|b| b.name == "main").unwrap();
        assert!(main.default && !main.protected, "{service:?}");
        assert!(listed.items.iter().find(|b| b.name == "topic-001").is_some_and(|b| b.protected && !b.default), "{service:?}");
        assert_eq!(listed.items.iter().filter(|b| b.default).count(), 1);
    }
}

#[test]
fn a_rate_limit_says_when_to_try_again_and_a_project_not_reached_says_so() {
    let fake = github();
    let reset = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 600).to_string();
    fake.fail_with(403, &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", &reset)], r#"{"message":"API rate limit exceeded"}"#);
    let e = block_on(lister(&fake, Service::GitHub).unwrap().reachable()).unwrap_err();
    assert!(matches!(e, Error::RateLimited { forge: Service::GitHub, retry_after: Some(s) } if s > 500), "{e:?}");
    assert!(e.to_string().contains("try again in 10 minutes"), "{e}");
    fake.heal();
    let elsewhere = Address { instance: GITHUB_COM.to_owned(), path: "someone/secret".to_owned(), service: Service::GitHub };
    let e = block_on(lister(&fake, Service::GitHub).unwrap().branches(&elsewhere)).unwrap_err();
    assert!(matches!(e, Error::NotFound { .. }), "{e:?}");
    assert!(e.to_string().contains("cannot see it"), "{e}");
}
