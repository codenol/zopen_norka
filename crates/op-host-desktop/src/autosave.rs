//! Autosave for the desktop window.
//!
//! Same promise as the browser's (`op_host_web::web_autosave`): a change
//! reaches disk without a command. The desktop differs in two ways, and both
//! are why this is not the same code:
//!
//! - the write already exists — `SaveSession` coalesces, skips an unchanged
//!   document and acknowledges by identity — so this only decides *when*;
//! - the window idles on `WaitUntil`, so a per-frame check is not enough on
//!   its own: the deadline has to reach the event loop, or a document left
//!   alone after an edit would never be saved.
//!
//! A document with no path is not touched. Where a draft for it should live is
//! the same open question as on the web (issue #16), and the desktop's
//! `SaveFork` action opens a dialog — a modal appearing on a timer is not a
//! background save.

use std::time::{Duration, Instant};

use crate::DesktopApp;

/// Quiet period after the last change before a write happens.
const DEBOUNCE: Duration = Duration::from_secs(3);

/// Floor between two autosave attempts.
const MIN_INTERVAL: Duration = Duration::from_secs(15);

/// What the tick remembers between frames.
#[derive(Default)]
pub(crate) struct AutosaveClock {
    /// `document_revision` seen last time — a change bumps it.
    revision: Option<u64>,
    /// When that revision first appeared.
    changed_at: Option<Instant>,
    /// When the last attempt was made.
    attempted_at: Option<Instant>,
}

impl DesktopApp {
    /// Run one autosave check. Called once per painted frame.
    pub(crate) fn autosave_tick(&mut self) {
        let Some(_) = self.current_path.clone() else {
            self.autosave.changed_at = None;
            return;
        };
        let (revision, dirty) = {
            let state = self.host.editor_state();
            (state.document_revision(), state.is_dirty())
        };
        if !dirty {
            self.autosave.changed_at = None;
            self.autosave.revision = Some(revision);
            return;
        }
        let now = Instant::now();
        if self.autosave.revision != Some(revision) {
            self.autosave.revision = Some(revision);
            self.autosave.changed_at = Some(now);
            return;
        }
        let Some(changed_at) = self.autosave.changed_at else {
            self.autosave.changed_at = Some(now);
            return;
        };
        if now.duration_since(changed_at) < DEBOUNCE {
            return;
        }
        if self
            .autosave
            .attempted_at
            .is_some_and(|attempted| now.duration_since(attempted) < MIN_INTERVAL)
        {
            return;
        }
        self.autosave.attempted_at = Some(now);
        // Pending inputs are part of what the user would expect to be saved.
        self.host.commit_pending_input_pub();
        // `request_background_save` already refuses when the collaboration
        // policy says no, so no gate is duplicated here.
        self.request_background_save();
    }

    /// When the event loop should next wake for autosave, if at all.
    pub(crate) fn autosave_deadline(&self) -> Option<Instant> {
        if self.current_path.is_none() {
            return None;
        }
        let changed_at = self.autosave.changed_at?;
        let due = changed_at + DEBOUNCE;
        let due = match self.autosave.attempted_at {
            Some(attempted) => due.max(attempted + MIN_INTERVAL),
            None => due,
        };
        Some(due)
    }
}
