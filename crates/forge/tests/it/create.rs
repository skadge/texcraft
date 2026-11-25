//! Making a repository for a project that has none (Save Project to GitHub or GitLab).

use texcraft_forge::create::{Creator, NewRepository, Owner, check_name, suggest_name};
use texcraft_forge::fake::Fake;
use texcraft_forge::task::block_on;
use texcraft_forge::{Error, GITHUB_COM, Service, Token};

const TOKEN: &str = "secret-token";

fn creator(fake: &Fake, service: Service, instance: &str) -> texcraft_forge::Result<Creator> {
    Ok(Creator::new(fake.transport(), service, instance, Token::new(TOKEN)?))
}

fn new(name: &str, owner: Option<Owner>) -> NewRepository {
    NewRepository { owner, name: name.to_owned(), private: true, description: Some("A paper".into()), initialize: false }
}

#[test]
fn names_are_checked_and_suggested_from_a_folder() {
    assert!(check_name("paper-2026").is_ok());
    assert!(check_name("").is_err());
    assert!(check_name("my paper").is_err());
    assert!(check_name("-paper").is_err());
    assert!(check_name("paper.git").is_err());
    assert!(check_name(&"a".repeat(101)).is_err());
    for folder in ["My Thesis (final)", "été 2026", "  ...", "paper.git", "a/b"] {
        let name = suggest_name(folder);
        assert!(check_name(&name).is_ok(), "{folder:?} gives {name:?}");
    }
    assert_eq!(suggest_name("My Thesis"), "My-Thesis");
}

#[test]
fn the_owners_are_the_account_then_its_organisations_or_groups() {
    for (service, instance) in [(Service::GitHub, GITHUB_COM), (Service::GitLab, "https://gitlab.example")] {
        let fake = Fake::account(service, instance, "ada", TOKEN);
        fake.add_group(11, "zeta-lab");
        fake.add_group(12, "alpha-lab");
        let owners = block_on(creator(&fake, service, instance).unwrap().owners()).unwrap();
        let paths: Vec<&str> = owners.iter().map(|o| o.path.as_str()).collect();
        assert_eq!(paths, ["ada", "alpha-lab", "zeta-lab"], "{service:?}");
        assert!(owners[0].personal && !owners[1].personal);
        if service == Service::GitLab {
            assert_eq!(owners[1].namespace_id, Some(12));
        }
    }
}

#[test]
fn a_github_repository_is_made_private_and_empty_for_the_account_or_an_organisation() {
    let fake = Fake::account(Service::GitHub, GITHUB_COM, "ada", TOKEN);
    fake.add_group(5, "lab");
    let made = block_on(creator(&fake, Service::GitHub, GITHUB_COM).unwrap().create(&new("paper", None))).unwrap();
    assert_eq!(made.address.path, "ada/paper");
    assert_eq!(made.clone_url, "https://github.com/ada/paper.git");
    assert_eq!(made.web_url, "https://github.com/ada/paper");
    let body = &fake.created()[0];
    assert_eq!(body["private"], true);
    assert_eq!(body["auto_init"], false, "empty, for the history to be pushed into");
    assert_eq!(body["description"], "A paper");
    assert!(fake.requests().iter().any(|r| r == "POST https://api.github.com/user/repos"));

    let lab = Owner { path: "lab".into(), personal: false, namespace_id: None };
    let made = block_on(creator(&fake, Service::GitHub, GITHUB_COM).unwrap().create(&new("notes", Some(lab)))).unwrap();
    assert_eq!(made.address.path, "lab/notes");
    assert!(fake.requests().iter().any(|r| r == "POST https://api.github.com/orgs/lab/repos"));
}

#[test]
fn a_gitlab_project_is_made_in_its_group_private_without_a_readme() {
    let instance = "https://gitlab.example";
    let fake = Fake::account(Service::GitLab, instance, "ada", TOKEN);
    fake.add_group(31, "lab/papers");
    let group = Owner { path: "lab/papers".into(), personal: false, namespace_id: Some(31) };
    let made = block_on(creator(&fake, Service::GitLab, instance).unwrap().create(&new("paper", Some(group)))).unwrap();
    assert_eq!(made.address.path, "lab/papers/paper");
    assert_eq!(made.clone_url, "https://gitlab.example/lab/papers/paper.git");
    let body = &fake.created()[0];
    assert_eq!(body["namespace_id"], 31);
    assert_eq!(body["visibility"], "private");
    assert_eq!(body["initialize_with_readme"], false);
}

#[test]
fn a_name_taken_is_said_as_such_on_both_forges() {
    for (service, instance) in [(Service::GitHub, GITHUB_COM), (Service::GitLab, "https://gitlab.example")] {
        let fake = Fake::account(service, instance, "ada", TOKEN);
        fake.take("ada/paper");
        let refused = block_on(creator(&fake, service, instance).unwrap().create(&new("paper", None))).unwrap_err();
        assert!(matches!(&refused, Error::NameTaken { name, .. } if name == "paper"), "{service:?}: {refused:?}");
        assert!(refused.to_string().contains("choose another name"));
        assert!(!refused.to_string().contains(TOKEN));
        assert!(fake.created().is_empty());
    }
}

#[test]
fn a_bad_name_is_refused_before_anything_is_sent() {
    let fake = Fake::account(Service::GitHub, GITHUB_COM, "ada", TOKEN);
    let refused = block_on(creator(&fake, Service::GitHub, GITHUB_COM).unwrap().create(&new("my paper", None))).unwrap_err();
    assert!(matches!(refused, Error::Name(_)));
    assert!(fake.requests().is_empty());
}

#[test]
fn in_the_browser_the_files_are_the_first_commit_and_the_project_is_linked_to_it() {
    let files = vec![("main.tex".to_owned(), b"\\documentclass{article}".to_vec()), ("figures/a.txt".to_owned(), b"a".to_vec())];
    for (service, instance) in [(Service::GitHub, GITHUB_COM), (Service::GitLab, "https://gitlab.example")] {
        let fake = Fake::account(service, instance, "ada", TOKEN);
        let mut repo = new("paper", None);
        // GitHub's API cannot commit to an empty repository: it is made with a README first.
        repo.initialize = service == Service::GitHub;
        let made = block_on(creator(&fake, service, instance).unwrap().create(&repo)).unwrap();
        let link = block_on(creator(&fake, service, instance).unwrap().first_commit(&made, &files, "Saved from TeXCraft")).unwrap();
        assert_eq!(link.branch, "main");
        assert_eq!(fake.head("main").as_deref(), Some(link.base.as_str()));
        assert_eq!(fake.parent(&link.base), None, "{service:?}: the first commit has no parent");
        let on_branch = fake.files("main");
        assert_eq!(on_branch.keys().collect::<Vec<_>>(), ["figures/a.txt", "main.tex"], "{service:?}: the README is gone");
        assert_eq!(link.files.len(), 2);
        assert_eq!(texcraft_forge::changes(&link, &files).unwrap(), Vec::new(), "nothing is changed after it");
    }
}
