//! The front door, applied: the screen a load shows, and finishing what the
//! address asked for once there is a session.
//!
//! The rule itself is `op_editor_core::front_door` — pure, and tested without a
//! page. This module is the browser half: it reads `window.location`, folds one
//! frame into the door's memory, and applies what comes back (switch the screen,
//! or ask for the document the address names).
//!
//! ## Why the front door is not the router
//!
//! `route_sync` owns "the address is the state": it reads the address into the
//! editor and writes the editor back into the address. This module answers a
//! different question — what the app shows when the address says nothing about a
//! document — and it has to answer it in the window between the session arriving
//! and the screen being right, including for a link that was refused while
//! nobody was signed in.

use op_editor_core::front_door::{Address, FrontDoor, FrontDoorStep, Session};
use op_editor_core::{AppScreen, EmbedHost};

use crate::widget_host::WidgetHost;

thread_local! {
    /// The door's page-lifetime memory: which address the screen was decided
    /// for, which document the address named, and whether a gate has been up.
    static DOOR: std::cell::RefCell<FrontDoor> =
        std::cell::RefCell::new(FrontDoor::default());
}

/// The address as the front door reads it, or `None` where there is no browser.
fn current_address(embed: EmbedHost) -> Option<Address> {
    let window = web_sys::window()?;
    let location = window.location();
    Some(Address::new(
        location.pathname().ok()?,
        location.search().unwrap_or_default(),
        embed,
    ))
}

/// Whether the address bar still belongs to the address rather than to the
/// editor state — asked by the router (`route_sync::tick`) before it writes.
///
/// Two cases, one rule, and the invitation is the oldest of them: the token in
/// its address is the credential the form is about to send, so rewriting the
/// address before it has been accepted throws the link away. The second is a
/// link that names a document this tab has not opened — refused for want of a
/// session, or simply still on its way from the daemon — which the router would
/// otherwise replace with the editor's own `/` (the route of an editor holding
/// no document) the moment that state was painted.
pub(crate) fn address_awaits_a_document() -> bool {
    DOOR.with(|door| door.borrow().waiting_for_a_document())
}

/// Fold this frame in and apply what the door says. Returns whether the screen
/// or the pending document changed, so the caller can repaint.
///
/// Called once per frame, before the router writes the state back into the
/// address — otherwise the two would disagree for a frame, and the address would
/// be written for a screen that is about to change.
pub(crate) fn tick(host: &mut WidgetHost) -> bool {
    let (address, session, open_key) = {
        let ui = &host.editor_state().editor_ui;
        let Some(address) = current_address(ui.embed) else {
            return false;
        };
        let open_key = ui.file_key.clone();
        (address, Session::of(ui), open_key)
    };
    let step = DOOR.with(|door| {
        door.borrow_mut()
            .observe(&address, session, open_key.as_deref())
    });
    match step {
        FrontDoorStep::Hold => false,
        FrontDoorStep::Show(screen) => host.set_screen(screen),
        FrontDoorStep::Open(key) => {
            // The editor is where a document is opened, and the frame performs
            // the request itself (`route_sync::tick_files` drains the same field
            // a click on a card fills), so both paths into a document go through
            // one implementation.
            host.set_screen(AppScreen::Editor);
            let ui = &mut host.editor_state_mut().editor_ui;
            ui.server_files_open_request = Some(key);
            // True whatever the screen was: a document is on its way, and the
            // frame that performs the request has to run.
            true
        }
    }
}

/// Forget the file list that belonged to the session this tab is leaving.
///
/// A list the daemon refused for an anonymous caller is not an answer about the
/// account that just signed in, and the screen only asks again while it has no
/// error to show — so leaving the refusal there would strand the new session on
/// "the server refused the list" with no way to retry.
pub(crate) fn forget_previous_session_files(ui: &mut op_editor_core::EditorUiState) {
    ui.server_files.clear();
    ui.server_files_error = None;
    ui.server_files_loading = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_list_is_forgotten_when_the_session_changes() {
        let mut ui = op_editor_core::EditorUiState {
            // What the refused session left behind: an error the screen reports
            // and a request that will never land.
            server_files_error: Some("authentication required".to_string()),
            server_files_loading: true,
            ..op_editor_core::EditorUiState::default()
        };
        ui.server_files.push(op_editor_core::ServerFile {
            key: "old".to_string(),
            name: "Список токенов".to_string(),
            updated_at: 1,
            size: 1,
            has_thumbnail: false,
        });

        forget_previous_session_files(&mut ui);

        assert!(ui.server_files_error.is_none());
        assert!(!ui.server_files_loading);
        assert!(ui.server_files.is_empty());
    }
}
