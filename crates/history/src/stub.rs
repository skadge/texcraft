//! The browser has no git on disk: a folder is never in a repository there (projects of the
//! library keep their history in [`crate::store`]).

use std::path::Path;

use crate::{Entry, Owner, Person, Result, Snapshot};

const NOT_YET: &str = "A folder's git repository cannot be reached in the browser.";

/// Never made in the browser: [`History::find`] finds none and the rest refuse.
pub struct History(());

impl History {
    pub fn find(_dir: &Path) -> Result<Option<Self>> {
        Ok(None)
    }

    pub fn open_or_init(_dir: &Path) -> Result<Self> {
        Err(NOT_YET.to_owned())
    }

    pub fn owner(&self) -> Owner {
        Owner::Theirs
    }

    pub fn git_dir(&self) -> &Path {
        Path::new("")
    }

    pub fn work_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::new()
    }

    pub fn prefix(&self) -> &str {
        ""
    }

    pub fn branch(&self) -> Option<String> {
        None
    }

    pub fn head(&self) -> Result<Option<String>> {
        Err(NOT_YET.to_owned())
    }

    pub fn commit(&self, _: &Snapshot, _: &str, _: Option<&Person>, _: chrono::DateTime<chrono::Utc>) -> Result<Option<String>> {
        Err(NOT_YET.to_owned())
    }

    pub fn log(&self, _limit: usize) -> Result<Vec<Entry>> {
        Err(NOT_YET.to_owned())
    }

    pub fn file_at(&self, _commit: &str, _path: &str) -> Result<Option<Vec<u8>>> {
        Err(NOT_YET.to_owned())
    }

    pub fn parent_of(&self, _commit: &str) -> Result<Option<String>> {
        Err(NOT_YET.to_owned())
    }

    pub fn files_at(&self, _commit: &str) -> Result<Vec<(String, Vec<u8>)>> {
        Err(NOT_YET.to_owned())
    }
}
