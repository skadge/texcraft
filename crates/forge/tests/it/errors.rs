//! GitLab's refusals, in words a user can act on.

use texcraft_forge::{Error, open};

use crate::{block_on, client, client_with, paper};

#[test]
fn a_wrong_token_is_said_to_be_wrong() {
    let fake = paper();
    let result = block_on(open(&client_with(fake.transport(), "glpat-not-this-one").unwrap(), None));
    assert_eq!(result.err(), Some(Error::Unauthorized));
}

#[test]
fn a_missing_project_or_branch_is_not_found() {
    let fake = paper();
    let result = block_on(open(&client(&fake).unwrap(), Some("no-such-branch")));
    assert!(matches!(&result, Err(Error::NotFound { what }) if what.contains("no-such-branch")), "{result:?}");
    fake.fail_with(404, &[], r#"{"message":"404 Project Not Found"}"#);
    assert!(matches!(block_on(open(&client(&fake).unwrap(), None)), Err(Error::NotFound { .. })));
}

#[test]
fn a_token_without_the_scope_is_forbidden_and_says_which_scopes() {
    let fake = paper();
    fake.fail_with(403, &[], r#"{"error":"insufficient_scope"}"#);
    let err = block_on(open(&client(&fake).unwrap(), None)).unwrap_err();
    assert!(matches!(err, Error::Forbidden { .. }));
    let words = err.to_string();
    assert!(words.contains("read_api") && words.contains("api scope"), "{words}");
}

#[test]
fn the_rate_limit_says_when_to_try_again() {
    let fake = paper();
    fake.fail_with(429, &[("retry-after", "1800")], "Retry later");
    let err = block_on(open(&client(&fake).unwrap(), None)).unwrap_err();
    assert_eq!(err, Error::RateLimited { retry_after: Some(1800) });
    assert!(err.to_string().contains("30 minutes"), "{err}");
    fake.fail_with(429, &[], "Retry later");
    assert_eq!(block_on(open(&client(&fake).unwrap(), None)).unwrap_err(), Error::RateLimited { retry_after: None });
}

#[test]
fn other_refusals_carry_gitlab_s_message() {
    let fake = paper();
    fake.fail_with(400, &[], r#"{"message":"A file with this name already exists"}"#);
    let err = block_on(open(&client(&fake).unwrap(), None)).unwrap_err();
    assert!(err.to_string().contains("A file with this name already exists"), "{err}");
    fake.fail_with(502, &[], "<html>Bad gateway</html>");
    assert_eq!(block_on(open(&client(&fake).unwrap(), None)).unwrap_err(), Error::Server { status: 502 });
}

#[test]
fn the_token_is_never_printed() {
    let token = texcraft_forge::Token::new(crate::TOKEN).unwrap();
    assert!(!format!("{token:?}").contains(crate::TOKEN));
    let request = texcraft_forge::Request {
        method: "GET",
        url: "https://gitlab.example/api/v4/projects".to_owned(),
        headers: vec![("Authorization".to_owned(), format!("Bearer {}", crate::TOKEN))],
        body: None,
        max_body: 1,
    };
    assert!(!format!("{request:?}").contains(crate::TOKEN));
}
