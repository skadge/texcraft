//! Where the browser keeps sign-ins and tokens (docs/forges.md): the site's local storage, which
//! every tab of the site shares and which outlives the browser's restarts, while "Remember
//! sign-ins in this browser" is on (the default); this tab's session storage while it is off, for
//! a shared computer.
//!
//! The decision and its trade-off: a refresh token in local storage stays in the browser's
//! profile after the tab is closed, for as long as GitLab honours it (months), and any script the
//! page runs could read it, as it could the session storage's. The page runs only its own code
//! (no third-party script, no handwritten JavaScript), so keeping sign-ins across tabs and days is
//! worth it by default; turned off, nothing outlives the tab.
//!
//! The rules, over any key-value storage ([`KeyValue`]: the browser's two, or fakes in tests):
//!
//! - `get` looks in local storage, then in this tab's; one found in the tab's while remembering is
//!   moved to local storage (a sign-in made before remembering was on, or by an older version).
//! - `put` writes to local storage while remembering, to the tab's otherwise, and removes the
//!   other copy.
//! - `remove` removes it from both.
//! - Turning remembering off moves everything kept in local storage into this tab's (so other
//!   tabs and later visits forget it); turning it on moves this tab's into local storage. The
//!   setting is a marker in local storage ([`MARKER`]: present when off), read each time, so that
//!   every tab follows it at once.
//! - The GitLab sign-in waiting for the page to come back ([`crate::cmd::forge_auth::PENDING`])
//!   always stays in the tab: the round trip comes back to it, and two tabs signing in at once
//!   must not take each other's.

use crate::ai::{SecretString, Secrets};

/// The entry in local storage that says remembering is off (absent: on).
pub const MARKER: &str = "texcraft.remember";

/// The entries' prefixes this store keeps: sign-ins, and git servers' tokens.
const PREFIXES: [&str; 2] = ["texcraft.forge.", "texcraft.git."];

/// A key-value storage: the browser's local or session storage, or a fake.
pub trait KeyValue: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    /// Removes it; already gone is success.
    fn remove(&self, key: &str) -> Result<(), String>;
    /// Every key it holds.
    fn keys(&self) -> Result<Vec<String>, String>;
}

/// Sign-ins and tokens kept in local storage (`local`) or this tab's (`tab`), as "Remember
/// sign-ins in this browser" says.
pub struct BrowserSecrets<L, T> {
    local: L,
    tab: T,
}

/// Whether `key` is one of this store's (and not one kept for the tab only).
fn movable(key: &str) -> bool {
    PREFIXES.iter().any(|p| key.starts_with(p)) && key != crate::cmd::forge_auth::PENDING
}

/// Whether a change of the entry `key` in local storage (none: all of it was cleared) is one
/// another tab should re-read its sign-ins for.
pub fn concerns(key: Option<&str>) -> bool {
    key.is_none_or(|k| k == MARKER || movable(k))
}

impl<L: KeyValue, T: KeyValue> BrowserSecrets<L, T> {
    /// The store over `local` and `tab`.
    pub fn new(local: L, tab: T) -> Self {
        Self { local, tab }
    }

    /// Remembering, unless local storage's marker says not (a storage that cannot be read: on).
    fn on(&self) -> bool {
        !matches!(self.local.get(MARKER), Ok(Some(v)) if v == "off")
    }

    /// Moves every entry of this store from `from` to `to`.
    fn move_all(from: &dyn KeyValue, to: &dyn KeyValue) -> Result<(), String> {
        for key in from.keys()?.into_iter().filter(|k| movable(k)) {
            if let Some(value) = from.get(&key)? {
                to.set(&key, &value)?;
            }
            from.remove(&key)?;
        }
        Ok(())
    }
}

impl<L: KeyValue, T: KeyValue> Secrets for BrowserSecrets<L, T> {
    fn get(&self, handle: &str) -> Result<Option<SecretString>, String> {
        if !movable(handle) {
            return Ok(self.tab.get(handle)?.map(SecretString::new));
        }
        if let Some(value) = self.local.get(handle)? {
            return Ok(Some(SecretString::new(value)));
        }
        let Some(value) = self.tab.get(handle)? else { return Ok(None) };
        // Remembering: what this tab kept goes where every tab sees it. If local storage refuses
        // it, the tab keeps it.
        if self.on() && self.local.set(handle, &value).is_ok() {
            self.tab.remove(handle)?;
        }
        Ok(Some(SecretString::new(value)))
    }

    fn put(&self, handle: &str, value: &SecretString) -> Result<(), String> {
        if !movable(handle) {
            return self.tab.set(handle, value.expose());
        }
        if self.on() {
            self.local.set(handle, value.expose())?;
            self.tab.remove(handle)
        } else {
            self.tab.set(handle, value.expose())?;
            self.local.remove(handle)
        }
    }

    fn remove(&self, handle: &str) -> Result<(), String> {
        let local = self.local.remove(handle);
        let tab = self.tab.remove(handle);
        local.and(tab)
    }

    fn lasting(&self) -> bool {
        true
    }

    fn remembering(&self) -> Option<bool> {
        Some(self.on())
    }

    fn remember(&self, on: bool) -> Result<(), String> {
        if on {
            Self::move_all(&self.tab, &self.local)?;
            self.local.remove(MARKER)
        } else {
            // The marker first: the other tabs, told of each entry leaving, already read it off.
            self.local.set(MARKER, "off")?;
            Self::move_all(&self.local, &self.tab)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, PoisonError};

    use super::*;

    /// A storage in memory; clones share it (two tabs share local storage).
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<BTreeMap<String, String>>>);

    impl Memory {
        fn has(&self, key: &str) -> bool {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).contains_key(key)
        }
    }

    impl KeyValue for Memory {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_owned(), value.to_owned());
            Ok(())
        }
        fn remove(&self, key: &str) -> Result<(), String> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
            Ok(())
        }
        fn keys(&self) -> Result<Vec<String>, String> {
            Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).keys().cloned().collect())
        }
    }

    const GRANT: &str = "texcraft.forge.gitlab.com";
    const TOKEN: &str = "texcraft.git.github.com";

    fn secret(s: &str) -> SecretString {
        SecretString::new(s.to_owned())
    }

    fn read(store: &impl Secrets, key: &str) -> Option<String> {
        store.get(key).ok().flatten().map(|s| s.expose().to_owned())
    }

    fn store() -> (BrowserSecrets<Memory, Memory>, Memory, Memory) {
        let (local, tab) = (Memory::default(), Memory::default());
        (BrowserSecrets::new(local.clone(), tab.clone()), local, tab)
    }

    #[test]
    fn remembering_is_on_by_default_and_puts_where_every_tab_reads() -> Result<(), String> {
        let (store, local, tab) = store();
        assert_eq!(store.remembering(), Some(true));
        store.put(GRANT, &secret("a"))?;
        assert!(local.has(GRANT) && !tab.has(GRANT));
        // Another tab: the same local storage, its own session storage.
        let other = BrowserSecrets::new(local.clone(), Memory::default());
        assert_eq!(read(&other, GRANT).as_deref(), Some("a"));
        Ok(())
    }

    #[test]
    fn off_puts_in_the_tab_only() -> Result<(), String> {
        let (store, local, tab) = store();
        store.remember(false)?;
        store.put(TOKEN, &secret("t"))?;
        assert!(tab.has(TOKEN) && !local.has(TOKEN));
        let other = BrowserSecrets::new(local.clone(), Memory::default());
        assert_eq!(read(&other, TOKEN), None, "another tab sees it");
        assert_eq!(other.remembering(), Some(false), "another tab does not follow the setting");
        other.put(GRANT, &secret("b"))?;
        assert!(!local.has(GRANT), "another tab still keeps for every tab");
        Ok(())
    }

    #[test]
    fn a_tab_sign_in_is_still_read_and_moves_to_local_storage() -> Result<(), String> {
        let (store, local, tab) = store();
        tab.set(GRANT, "old")?;
        assert_eq!(read(&store, GRANT).as_deref(), Some("old"));
        assert!(local.has(GRANT) && !tab.has(GRANT), "not moved");
        Ok(())
    }

    #[test]
    fn turning_off_moves_into_the_tab_and_on_moves_back() -> Result<(), String> {
        let (store, local, tab) = store();
        store.put(GRANT, &secret("a"))?;
        store.put(TOKEN, &secret("t"))?;
        store.remember(false)?;
        assert!(tab.has(GRANT) && tab.has(TOKEN) && !local.has(GRANT) && !local.has(TOKEN));
        assert_eq!(read(&store, GRANT).as_deref(), Some("a"), "this tab forgot it");
        store.remember(true)?;
        assert!(local.has(GRANT) && local.has(TOKEN) && !tab.has(GRANT) && !tab.has(TOKEN));
        assert!(!local.has(MARKER));
        Ok(())
    }

    #[test]
    fn remove_clears_both() -> Result<(), String> {
        let (store, local, tab) = store();
        local.set(GRANT, "a")?;
        tab.set(GRANT, "b")?;
        store.remove(GRANT)?;
        assert!(!local.has(GRANT) && !tab.has(GRANT));
        Ok(())
    }

    #[test]
    fn the_pending_sign_in_stays_in_the_tab() -> Result<(), String> {
        let (store, local, tab) = store();
        let pending = crate::cmd::forge_auth::PENDING;
        store.put(pending, &secret("p"))?;
        assert!(tab.has(pending) && !local.has(pending));
        store.remember(false)?;
        store.remember(true)?;
        assert!(tab.has(pending) && !local.has(pending), "moved with the sign-ins");
        assert!(!concerns(Some(pending)) && concerns(Some(GRANT)) && concerns(Some(MARKER)) && concerns(None));
        assert!(!concerns(Some("texcraft.something.else")));
        Ok(())
    }
}
