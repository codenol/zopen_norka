//! The front door: which screen a load shows, and what a session that arrives
//! afterwards still has to finish.
//!
//! Two rules, deliberately kept apart:
//!
//! - **The address decides the screen.** `/f/<key>` is a document, `/files` is
//!   the list, and the bare root is the app's front door — which is the list,
//!   not an editor holding a starter document nobody asked for. A screen that
//!   depended on who is asking would make one URL mean two things and would
//!   flash a wrong screen while the answer was still in flight.
//! - **The session decides the gate**, and finishes what the address asked for.
//!   A visitor who lands on `/f/<key>` while nobody is signed in is refused that
//!   open (the daemon answers `401`), so the request has to be remembered and
//!   repeated once there is a session again — otherwise signing in lands on the
//!   list and the link somebody sent is lost.
//!
//! Nothing here touches a browser: the web host reads `window.location` and
//! hands the result in, and applies what comes back. That is what makes the
//! whole rule testable without a page.

use crate::editor_ui_state::{AppScreen, EditorUiState, EmbedHost};
use crate::route::{self, RouteFile, RoutePath, RouteTarget};

/// Which screen a load shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landing {
    /// The file browser — the front door, and `/files`.
    Files,
    /// The editor, on the document the address names.
    Editor,
    /// Not the editor's business: an invitation (which is a page of its own), an
    /// embedded host (which names its document through the bridge, not through
    /// the address), or a path this app does not serve. The screen stays put.
    Hold,
}

/// What the address bar says, as the front door reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub path: String,
    pub query: String,
    pub embed: EmbedHost,
}

impl Address {
    pub fn new(path: impl Into<String>, query: impl Into<String>, embed: EmbedHost) -> Self {
        Self {
            path: path.into(),
            query: query.into(),
            embed,
        }
    }

    /// The address as a string, for "is this the address I already decided
    /// for?" — the screen is decided once per address, not once per frame.
    pub fn identity(&self) -> String {
        format!("{}{}", self.path, self.query)
    }

    /// The document the address names, when it names one.
    ///
    /// The root counts only when it carries a node or a page: a bare `/` is not
    /// a document, it is the front door.
    pub fn document_key(&self) -> Option<String> {
        match route::parse(&self.path, &self.query) {
            RoutePath::Known(RouteTarget::Document(document)) => document.key().map(str::to_string),
            _ => None,
        }
    }

    /// Which screen this address shows.
    pub fn landing(&self) -> Landing {
        // An embedded host names its document through the bridge; the page
        // address here is the host's own, not the editor's.
        if self.embed != EmbedHost::None {
            return Landing::Hold;
        }
        match route::parse(&self.path, &self.query) {
            RoutePath::Known(RouteTarget::Files) => Landing::Files,
            RoutePath::Known(RouteTarget::Document(document)) => match document.file {
                RouteFile::Key(_) => Landing::Editor,
                // An address that points INSIDE the daemon's own document (a
                // node, a page) still means the editor: that is the shape a
                // selection link takes for a document with no server key.
                RouteFile::Untitled if document.node.is_some() || document.page.is_some() => {
                    Landing::Editor
                }
                // The bare root is the front door: the file list, not an editor
                // holding a starter document nobody asked for.
                RouteFile::Untitled => Landing::Files,
            },
            RoutePath::NotARoute => Landing::Hold,
        }
    }
}

/// Who the tab is, as far as the last `/api/auth/status` answer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    /// No answer yet: nothing is decided, and no gate is up.
    Pending,
    /// The answer said this deployment has no accounts at all. There is nothing
    /// to sign in to, so there is nothing to gate either.
    NoAccounts,
    /// Accounts exist and nobody is signed in: the gate is up, and everything
    /// behind it is unreachable.
    Anonymous,
    SignedIn,
}

impl Session {
    pub fn of(ui: &EditorUiState) -> Self {
        if ui.account.is_signed_in() {
            Self::SignedIn
        } else if !ui.account_entry.status_received {
            Self::Pending
        } else if !ui.account_ui_available {
            Self::NoAccounts
        } else {
            Self::Anonymous
        }
    }

    /// Whether the sign-in surface is up. Its scrim covers the viewport, so the
    /// screen behind it is a decision about what NOT to paint, not about what
    /// the visitor sees.
    pub fn gate(self) -> bool {
        self == Self::Anonymous
    }

    /// Whether this session can reach the daemon's documents.
    pub fn reaches_documents(self) -> bool {
        matches!(self, Self::SignedIn | Self::NoAccounts)
    }

    /// Whether a link that names a document may still be refused for want of a
    /// session — the case the address bar must keep for the sign-in that
    /// follows.
    pub fn may_be_refused(self) -> bool {
        !self.reaches_documents()
    }
}

/// What one frame of the front door should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontDoorStep {
    /// Show this screen.
    Show(AppScreen),
    /// Open this document: the address named it, and this session can reach it.
    Open(String),
    /// Leave everything as it is.
    Hold,
}

/// The front door's page-lifetime memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontDoor {
    /// The address the current screen was decided for, so the decision is made
    /// once per address rather than once per frame.
    decided_for: Option<String>,
    /// The document the address names this frame.
    named: Option<String>,
    /// A document the address names and this tab does not hold. While it is set
    /// the address bar belongs to the address: see [`Self::waiting_for_a_document`].
    opening: Option<String>,
    /// Frames spent waiting for `opening` with a session that can reach the
    /// daemon — the give-up clock, so a link to a document that is gone cannot
    /// pin the address for the rest of the page's life.
    waited: u32,
    /// The document this door stopped waiting for, so it does not start again
    /// the very next frame.
    gave_up: Option<String>,
    /// Whether the gate has been up in this page's life — which is exactly
    /// "a link that named a document was refused for want of a session".
    refused: bool,
}

/// Roughly five seconds at 60 fps. The same discipline `route_sync` applies to a
/// link that names a node this document does not have: an address that cannot be
/// honoured has to stop being honoured.
const GIVE_UP_FRAMES: u32 = 300;

impl FrontDoor {
    /// Whether the address bar still belongs to the address rather than to the
    /// editor state — a document the address names that this tab has not opened.
    ///
    /// This is what the router asks before it writes the editor's own route over
    /// the address. Without it, a link that names a document would be replaced by
    /// `/` (the state of an editor with no document) the moment that state was
    /// painted — which is how a link a visitor had not been able to open yet used
    /// to disappear.
    pub fn waiting_for_a_document(&self) -> bool {
        self.opening.is_some()
    }

    /// Fold one frame in and say what the shell should do about it.
    ///
    /// Called once per frame, so every input is read fresh: the address (which
    /// the router may have rewritten), who the tab is (which a poll may have
    /// changed), and the document it holds.
    ///
    /// The SCREEN is decided once per address, not once per frame. Re-asserting
    /// it every frame would fight the screens the user reaches on their own: the
    /// file list is also opened from the editor's own file menu, and at that
    /// moment the address still names the document for one frame — a per-frame
    /// rule would drag the user straight back into it.
    pub fn observe(
        &mut self,
        address: &Address,
        session: Session,
        open_key: Option<&str>,
    ) -> FrontDoorStep {
        let identity = address.identity();
        let fresh = self.decided_for.as_deref() != Some(identity.as_str());
        self.decided_for = Some(identity);
        self.named = address.document_key();

        // Is the address naming a document this tab does not hold? Then the
        // address is a request in flight, and the router must not write over it.
        match self.named.as_deref() {
            Some(key) if Some(key) == open_key => {
                // The document arrived: the address is the state's again, and a
                // later visit to a dead link gets its own clock.
                self.opening = None;
                self.waited = 0;
                self.gave_up = None;
            }
            Some(key) if Some(key) != self.gave_up.as_deref() => {
                if self.opening.as_deref() != Some(key) {
                    self.opening = Some(key.to_string());
                    self.waited = 0;
                }
            }
            _ => {}
        }

        if session.gate() {
            self.refused = true;
        }
        if session.may_be_refused() {
            // Waiting behind the sign-in form is not the link failing: somebody
            // may take minutes over it, and the address has to survive that.
            self.waited = 0;
        } else if self.opening.is_some() {
            self.waited += 1;
            if self.waited > GIVE_UP_FRAMES {
                self.gave_up = self.opening.take();
                self.waited = 0;
            }
        }

        // The session arrived after the address did: finish what the address
        // asked for. This is deliberately not guarded by the open document's
        // key — the identity reset rebuilds the tab from the starter document,
        // so a tab whose `file_key` still matches is showing an empty document
        // and has to load the real one again.
        if session.reaches_documents() && self.refused {
            self.refused = false;
            if let Some(key) = address.document_key() {
                return FrontDoorStep::Open(key);
            }
        }
        if !fresh {
            return FrontDoorStep::Hold;
        }
        match address.landing() {
            Landing::Files => FrontDoorStep::Show(AppScreen::Files),
            Landing::Editor => FrontDoorStep::Show(AppScreen::Editor),
            Landing::Hold => FrontDoorStep::Hold,
        }
    }
}

#[cfg(test)]
#[path = "front_door_tests.rs"]
mod tests;
