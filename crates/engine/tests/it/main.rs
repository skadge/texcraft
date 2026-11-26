//! Integration tests for texcraft-engine.

mod ai;
mod build_options;
mod command_sweep;
mod deleted;
mod draft;
mod edits;
mod export;
mod file_access;
mod files;
mod find;
mod floats;
mod folders;
mod forge;
mod forge_pick;
mod forge_servers;
mod forge_sign_in;
mod git;
// Offline grammar needs Harper (the `grammar` feature, on by default).
#[cfg(feature = "grammar")]
mod grammar;
mod history;
mod image;
mod import;
mod language;
mod library;
mod numbers;
mod rename;
mod review;
mod search;
mod spelling;
mod sync;
mod templates;
