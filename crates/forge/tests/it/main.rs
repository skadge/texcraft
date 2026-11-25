//! Integration tests for texcraft-forge, against the fake GitLab: no test reaches the network.

mod address;
mod commit;
mod errors;
mod github;
mod merge;
mod oauth;
mod open;
mod pull;
mod servers;

use texcraft_forge::fake::{Fake, FakeGitLab};
use texcraft_forge::{Address, Client, GitLab, Service, Token};

pub const INSTANCE: &str = "https://gitlab.example";
pub const PROJECT: &str = "group/paper";
pub const TOKEN: &str = "glpat-test-token";

/// A client of the fake's project, GitLab or GitHub as the fake is.
pub fn client(fake: &Fake) -> texcraft_forge::Result<Client> {
    Ok(Client::new(fake.transport(), fake.address(), Token::new(TOKEN)?))
}

/// A client of the fake GitLab's project through `transport`, sending `token`.
pub fn client_with(transport: Box<dyn texcraft_forge::Transport>, token: &str) -> texcraft_forge::Result<GitLab> {
    let address = Address { instance: INSTANCE.to_owned(), path: PROJECT.to_owned(), service: Service::GitLab };
    Ok(GitLab::new(transport, address, Token::new(token)?))
}

/// The same project on a fake GitLab and a fake GitHub, for the behaviour they share.
pub fn both(files: &[(&str, &[u8])]) -> [Fake; 2] {
    [FakeGitLab::new(INSTANCE, PROJECT, TOKEN, files), Fake::github(PROJECT, TOKEN, files)]
}

const PAPER: &[(&str, &[u8])] = &[
    ("main.tex", b"\\documentclass{article}\n\\begin{document}\nHi\n\\end{document}\n"),
    ("chapters/intro.tex", b"Intro\n"),
    ("fig/plot.png", b"\x89PNG\0\x01"),
];

/// A paper on a fake GitLab.
pub fn paper() -> FakeGitLab {
    FakeGitLab::new(INSTANCE, PROJECT, TOKEN, PAPER)
}

/// The paper on a fake GitLab and on a fake GitHub.
pub fn papers() -> [Fake; 2] {
    both(PAPER)
}

pub use texcraft_forge::task::block_on;
