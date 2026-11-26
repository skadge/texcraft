//! Saving by itself (docs/ROADMAP.md, C9): the project's changes are written shortly after typing
//! stops, at once when the window loses the keyboard focus and before the project closes, to its
//! folder (in the browser, to its storage). A version of the history is not made on every write:
//! one is made after a longer pause in typing when something changed since the latest, and when the
//! project closes, in any repository; each amends the automatic one before it while this session
//! made that one and did not push it, so they stay few (`history.version`). The top bar says where
//! the writing stands; a write that failed is said in words, and the project has unsaved changes
//! again (AGENTS.md rule 15), so leaving it asks first.
//!
//! Everything goes through `project.write` and `history.version`; the timing is the window's, on
//! egui's clock.

use serde_json::json;

use crate::App;

/// Seconds without typing before the changes are written.
pub(crate) const PAUSE: f64 = 1.5;

/// Seconds without typing before a version is made of the changes written since the latest one.
pub(crate) const VERSION_PAUSE: f64 = 300.0;

/// Where writing the project stands.
#[derive(Default)]
pub(crate) struct Autosave {
    /// The file and review revisions last seen, and when they last changed (egui's time).
    seen: Option<(u64, u64)>,
    changed_at: f64,
    /// The window had the keyboard focus last frame.
    focused: bool,
    /// Why the last write failed, and the revisions it was tried at: it is not tried again until
    /// the project changes, or the user saves.
    failed: Option<(String, (u64, u64))>,
    /// The revisions an automatic version was last tried at.
    version_tried: Option<(u64, u64)>,
}

/// What the top bar says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum State {
    Saved,
    /// Changes wait for the pause in typing.
    Saving,
    /// The last write failed: why.
    Failed(String),
}

impl Autosave {
    /// The last write failed, in words, while the project has changes it did not write.
    pub(crate) fn failure(&self) -> Option<&str> {
        self.failed.as_ref().map(|(why, _)| why.as_str())
    }

    /// A save worked: nothing failed since.
    pub(crate) fn clear_failure(&mut self) {
        self.failed = None;
    }

    /// The shell could not keep what was written (the browser's storage refused it).
    pub(crate) fn not_kept(&mut self, why: String, revisions: (u64, u64)) {
        self.failed = Some((why, revisions));
    }
}

fn revisions(app: &App) -> (u64, u64) {
    (app.session.revision(), app.session.review_revision())
}

/// Whether the window saves this project by itself: the setting is on and the project has a
/// place (a folder, or a name in the browser). Agents' sessions save when they say so.
pub(crate) fn applies(app: &App) -> bool {
    app.session.settings.autosave && app.session.dir().is_some() && app.session.roots().is_none()
}

/// What the top bar says, when the window saves this project by itself.
pub(crate) fn state(app: &App) -> Option<State> {
    if !applies(app) {
        return None;
    }
    Some(match (app.session.is_dirty(), app.autosave.failure()) {
        (false, _) => State::Saved,
        (true, Some(why)) => State::Failed(why.to_owned()),
        (true, None) => State::Saving,
    })
}

/// Each frame: writes the changes once typing has paused (or the window lost the focus), and makes
/// a version after the longer pause.
pub(crate) fn tick(app: &mut App, ctx: &egui::Context) {
    let (now, focused) = ctx.input(|i| (i.time, i.focused));
    let at = revisions(app);
    if app.autosave.seen != Some(at) {
        app.autosave.seen = Some(at);
        app.autosave.changed_at = now;
    }
    let lost_focus = std::mem::replace(&mut app.autosave.focused, focused) && !focused;
    if app.start || !applies(app) {
        return;
    }
    let quiet = now - app.autosave.changed_at;
    if app.session.is_dirty() {
        if app.autosave.failed.as_ref().is_some_and(|(_, tried)| *tried == at) {
            return;
        }
        if lost_focus || quiet >= PAUSE {
            write(app);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(PAUSE - quiet));
        }
        return;
    }
    if !app.session.unversioned() || app.autosave.version_tried == Some(at) {
        return;
    }
    if quiet >= VERSION_PAUSE {
        version(app);
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(VERSION_PAUSE - quiet));
    }
}

/// Writes the changes now. Whether they are written (on a failure the status line says why).
pub(crate) fn write(app: &mut App) -> bool {
    let at = revisions(app);
    match app.session.execute("project.write", &json!({})) {
        Ok(done) => {
            app.autosave.failed = None;
            app.saved(&done);
            true
        }
        Err(e) => {
            let said = tl!("Your changes are not saved: {why}").replace("{why}", &crate::i18n::msg(&e));
            app.autosave.failed = Some((crate::i18n::msg(&e), at));
            app.view.status = Some((said, true));
            false
        }
    }
}

/// Makes a version of its own of the changes written, when there are some.
pub(crate) fn version(app: &mut App) {
    app.autosave.version_tried = Some(revisions(app));
    if !app.session.unversioned() {
        return;
    }
    if let Err(e) = app.session.execute("history.version", &json!({"automatic": true})) {
        let said = tl!("Your changes are saved, but no version of them could be made: {why}").replace("{why}", &crate::i18n::msg(&e));
        app.view.status = Some((said, true));
    }
}

/// Before something rewrites the project's files (a restore, accepting all suggestions): its
/// changes written and kept in a version of TeXCraft's own, when the window saves by itself.
pub(crate) fn keep_a_version(app: &mut App) {
    if applies(app) && (!app.session.is_dirty() || write(app)) {
        version(app);
    }
}

/// Before the project closes (another opens, or the window closes): its changes written and a
/// version made, when the window saves it by itself. Whether nothing is left unsaved; when
/// something is (no place, the setting off, a write that failed) the user is asked.
pub(crate) fn before_leaving(app: &mut App) -> bool {
    if !applies(app) {
        return !app.session.is_dirty();
    }
    if app.session.is_dirty() && !write(app) {
        return false;
    }
    version(app);
    true
}
