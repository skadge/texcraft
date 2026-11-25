//! Integration tests for texcraft-forge, against the fake GitLab: no test reaches the network.

mod address;
mod commit;
mod errors;
mod merge;
mod open;

use texcraft_forge::fake::FakeGitLab;
use texcraft_forge::{Address, GitLab, Token};

pub const INSTANCE: &str = "https://gitlab.example";
pub const PROJECT: &str = "group/paper";
pub const TOKEN: &str = "glpat-test-token";

/// A client of the fake's project.
pub fn client(fake: &FakeGitLab) -> texcraft_forge::Result<GitLab> {
    client_with(fake.transport(), TOKEN)
}

/// A client of the fake's project through `transport`, sending `token`.
pub fn client_with(transport: Box<dyn texcraft_forge::Transport>, token: &str) -> texcraft_forge::Result<GitLab> {
    let address = Address { instance: INSTANCE.to_owned(), path: PROJECT.to_owned() };
    Ok(GitLab::new(transport, address, Token::new(token)?))
}

pub fn paper() -> FakeGitLab {
    FakeGitLab::new(
        INSTANCE,
        PROJECT,
        TOKEN,
        &[
            ("main.tex", b"\\documentclass{article}\n\\begin{document}\nHi\n\\end{document}\n"),
            ("chapters/intro.tex", b"Intro\n"),
            ("fig/plot.png", b"\x89PNG\0\x01"),
        ],
    )
}

pub use texcraft_forge::task::block_on;
