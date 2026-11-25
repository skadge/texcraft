//! Reading a project's address as users paste it.

use texcraft_forge::{Address, GITLAB_COM};

#[test]
fn the_forms_a_user_pastes_name_the_same_project() {
    for input in [
        "https://gitlab.com/group/sub/paper",
        "https://gitlab.com/group/sub/paper/",
        "https://gitlab.com/group/sub/paper.git",
        "https://gitlab.com/group/sub/paper/-/tree/main?ref_type=heads",
        "git@gitlab.com:group/sub/paper.git",
        "gitlab.com/group/sub/paper",
        "group/sub/paper",
        "  group/sub/paper  ",
    ] {
        let a = Address::parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!((a.instance.as_str(), a.path.as_str()), (GITLAB_COM, "group/sub/paper"), "{input}");
    }
}

#[test]
fn another_instance_keeps_its_host_and_port() {
    let a = Address::parse("https://git.example.org:8443/team/thesis").unwrap();
    assert_eq!(a.instance, "https://git.example.org:8443");
    assert_eq!(a.api(), "https://git.example.org:8443/api/v4");
    // The API names the project by its encoded path.
    assert_eq!(a.id(), "team%2Fthesis");
}

#[test]
fn the_token_is_not_sent_in_the_clear_except_to_this_computer() {
    assert!(Address::parse("http://gitlab.example.org/team/thesis").is_err());
    assert!(Address::parse("http://localhost:8080/team/thesis").is_ok());
}

#[test]
fn addresses_that_name_no_project_are_refused() {
    for input in
        ["", "paper", "https://gitlab.com/", "https://gitlab.com/paper", "group/../etc", "group/pa per", "https://evil host/a/b", "group//paper"]
    {
        assert!(Address::parse(input).is_err(), "{input:?} was accepted");
    }
}
