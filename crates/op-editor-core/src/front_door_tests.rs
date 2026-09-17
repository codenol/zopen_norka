//! The front door's rules: which screen an address shows, and what a session
//! that arrives afterwards has to finish.

use super::*;
use crate::editor_ui_state::EmbedHost;

fn address(path: &str, query: &str) -> Address {
    Address::new(path, query, EmbedHost::None)
}

/// Every session a tab can be in, so the landing table can be asserted for all
/// of them instead of for one.
const SESSIONS: [Session; 4] = [
    Session::Pending,
    Session::NoAccounts,
    Session::Anonymous,
    Session::SignedIn,
];

#[test]
fn the_root_is_the_front_door_and_that_is_the_file_list() {
    // The product change: a signed-in tab that lands on the app root gets the
    // file list. It used to get an editor holding a starter document nobody
    // asked for — the empty canvas the operator reported.
    for session in SESSIONS {
        let mut door = FrontDoor::default();
        assert_eq!(
            door.observe(&address("/", ""), session, None),
            FrontDoorStep::Show(AppScreen::Files),
            "root, {session:?}"
        );
        // `/index.html` is the same place, spelled the way a static server
        // serves it.
        let mut door = FrontDoor::default();
        assert_eq!(
            door.observe(&address("/index.html", ""), session, None),
            FrontDoorStep::Show(AppScreen::Files)
        );
    }
}

#[test]
fn the_file_list_keeps_its_own_path() {
    for session in SESSIONS {
        let mut door = FrontDoor::default();
        assert_eq!(
            door.observe(&address("/files", ""), session, None),
            FrontDoorStep::Show(AppScreen::Files),
            "{session:?}"
        );
        let mut door = FrontDoor::default();
        assert_eq!(
            door.observe(&address("/files/", "?sort=name"), session, None),
            FrontDoorStep::Show(AppScreen::Files)
        );
    }
}

#[test]
fn signing_in_never_moves_a_tab_to_another_screen() {
    // The screen is a function of the ADDRESS. A screen that changed with the
    // session would make one URL mean two things, and would flash the wrong one
    // while the status answer was still in flight.
    for (path, query, expected) in [
        ("/", "", Landing::Files),
        ("/files", "", Landing::Files),
        ("/f/abc", "", Landing::Editor),
        ("/f/abc/spisok-tokenov", "?node=n42", Landing::Editor),
        ("/invite/tok-1", "", Landing::Hold),
        ("/api/files", "", Landing::Hold),
    ] {
        for session in SESSIONS {
            assert_eq!(
                address(path, query).landing(),
                expected,
                "{path}{query} for {session:?}"
            );
        }
    }
}

#[test]
fn an_address_that_names_a_document_opens_that_document() {
    let link = address("/f/01hqx/kommutatory", "?node=n42&page=3");
    assert_eq!(link.landing(), Landing::Editor);
    assert_eq!(link.document_key().as_deref(), Some("01hqx"));
    // The parameters a deep link carries do not change the screen, and a
    // collaboration or tenant parameter is simply not ours to read here.
    assert_eq!(
        address("/f/01hqx", "?tenant=acme&node=n42").landing(),
        Landing::Editor
    );
}

#[test]
fn the_root_still_opens_the_editor_when_it_points_inside_a_document() {
    // A selection link for a document with no server key is `/?node=…`
    // (`route::selection_link`), and it means the editor — dropping a visitor
    // on the file list would throw the link away.
    assert_eq!(address("/", "?node=n42").landing(), Landing::Editor);
    assert_eq!(address("/", "?page=2").landing(), Landing::Editor);
    assert_eq!(address("/", "?node=n42").document_key(), None);
}

#[test]
fn an_embedded_host_and_foreign_paths_are_left_alone() {
    // An embed opens its document through the bridge; the page address belongs
    // to the host page, so the front door has no opinion about it.
    let embedded = Address::new("/", "", EmbedHost::VsCode);
    assert_eq!(embedded.landing(), Landing::Hold);
    let embedded_link = Address::new("/f/abc", "?embed=vscode", EmbedHost::VsCode);
    assert_eq!(embedded_link.landing(), Landing::Hold);
    // An invitation is a page of its own, and an API path is not a screen.
    for path in [
        "/invite/tok-1",
        "/api/files",
        "/pkg/op_host_web.js",
        "/files2",
    ] {
        assert_eq!(address(path, "").landing(), Landing::Hold, "{path}");
    }
}

#[test]
fn a_session_is_read_from_the_last_status_answer() {
    let mut ui = EditorUiState::default();
    assert_eq!(Session::of(&ui), Session::Pending);

    // An answer for a deployment with no accounts: nothing to sign in to.
    ui.account_entry.status_received = true;
    assert_eq!(Session::of(&ui), Session::NoAccounts);

    // Accounts exist and nobody is signed in: the gate goes up.
    ui.account_ui_available = true;
    assert_eq!(Session::of(&ui), Session::Anonymous);
    assert!(Session::of(&ui).gate());
    assert!(Session::of(&ui).may_be_refused());

    ui.account = crate::AccountState::signed_in_account(
        "Дизайнер".to_string(),
        Some("designer".to_string()),
        Some("subject-1".to_string()),
    );
    assert_eq!(Session::of(&ui), Session::SignedIn);
    assert!(!Session::of(&ui).gate());
    assert!(Session::of(&ui).reaches_documents());
}

#[test]
fn a_signed_in_load_of_a_document_link_shows_the_editor() {
    let mut door = FrontDoor::default();
    assert_eq!(
        door.observe(&address("/f/abc", ""), Session::SignedIn, None),
        FrontDoorStep::Show(AppScreen::Editor)
    );
    // The next frame: the install opened the document, and the front door has
    // nothing to add.
    assert_eq!(
        door.observe(&address("/f/abc", ""), Session::SignedIn, None),
        FrontDoorStep::Hold
    );
}

#[test]
fn the_screen_is_decided_once_per_address_not_once_per_frame() {
    // The file list is ALSO reached from the editor's own file menu, and in the
    // frame after that click the address still names the document. A rule that
    // re-asserted the address's screen every frame would drag the user straight
    // back into the document they just left.
    let mut door = FrontDoor::default();
    let link = address("/f/abc", "");
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Show(AppScreen::Editor)
    );
    // The screen changed under the address; the address itself has not moved,
    // so the front door has no further opinion about it.
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Hold
    );
    // A new address is a new decision.
    assert_eq!(
        door.observe(&address("/files", ""), Session::SignedIn, None),
        FrontDoorStep::Show(AppScreen::Files)
    );
    // Even back to the same address, once it is a change again.
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Show(AppScreen::Editor)
    );
}

#[test]
fn a_link_opened_before_signing_in_lands_on_its_document_afterwards() {
    // The whole point of remembering: the daemon refuses the open for an
    // anonymous caller, and the link must survive that refusal.
    let mut door = FrontDoor::default();
    let link = address("/f/abc/kommutatory", "");
    assert_eq!(
        door.observe(&link, Session::Pending, None),
        FrontDoorStep::Show(AppScreen::Editor)
    );
    assert_eq!(
        door.observe(&link, Session::Anonymous, None),
        FrontDoorStep::Hold
    );
    // Signed in at last: the document the link names, not the list.
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Open("abc".to_string())
    );
    // And once only — a second frame must not re-open it.
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Hold
    );
}

#[test]
fn signing_out_and_back_in_through_a_link_reopens_that_document() {
    // `reset_for_new_identity` rebuilds the tab from the starter document while
    // the address (and `file_key`) still name the old one, so a tab whose key
    // matches still has to load it again — the guard is the gate, not the key.
    let mut door = FrontDoor::default();
    let link = address("/f/abc", "");
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Show(AppScreen::Editor)
    );
    assert_eq!(
        door.observe(&link, Session::Anonymous, None),
        FrontDoorStep::Hold
    );
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Open("abc".to_string())
    );
}

#[test]
fn signing_in_from_the_root_lands_on_the_file_list() {
    let mut door = FrontDoor::default();
    let root = address("/", "");
    assert_eq!(
        door.observe(&root, Session::Pending, None),
        FrontDoorStep::Show(AppScreen::Files)
    );
    assert_eq!(
        door.observe(&root, Session::Anonymous, None),
        FrontDoorStep::Hold
    );
    assert_eq!(
        door.observe(&root, Session::SignedIn, None),
        FrontDoorStep::Hold,
        "no document was ever named, so the screen already showing is the answer"
    );
}

#[test]
fn the_address_wins_over_a_document_it_named_earlier() {
    // A visitor follows a link, gives up on it, walks to the list, and signs in
    // there: the list is where they are, and the list is where they stay.
    let mut door = FrontDoor::default();
    assert_eq!(
        door.observe(&address("/f/abc", ""), Session::Anonymous, None),
        FrontDoorStep::Show(AppScreen::Editor)
    );
    let list = address("/files", "");
    assert_eq!(
        door.observe(&list, Session::Anonymous, None),
        FrontDoorStep::Show(AppScreen::Files)
    );
    assert_eq!(
        door.observe(&list, Session::SignedIn, None),
        FrontDoorStep::Hold
    );
}

#[test]
fn a_deployment_without_accounts_never_gates_anything() {
    for address in [address("/", ""), address("/f/abc", "")] {
        let expected = if address.document_key().is_some() {
            FrontDoorStep::Show(AppScreen::Editor)
        } else {
            FrontDoorStep::Show(AppScreen::Files)
        };
        let mut door = FrontDoor::default();
        assert_eq!(door.observe(&address, Session::NoAccounts, None), expected);
    }
}

#[test]
fn an_invitation_leaves_the_screen_where_it_is() {
    let mut door = FrontDoor::default();
    let invite = address("/invite/tok-1", "");
    assert_eq!(
        door.observe(&invite, Session::Anonymous, None),
        FrontDoorStep::Hold
    );
    // Accepting it signs the visitor in; the address still names the invitation
    // until the form clears its token, and the screen stays where it was.
    assert_eq!(
        door.observe(&invite, Session::SignedIn, None),
        FrontDoorStep::Hold
    );
    // Then the router writes the editor's own address, and that is the front
    // door: the file list.
    assert_eq!(
        door.observe(&address("/", ""), Session::SignedIn, None),
        FrontDoorStep::Show(AppScreen::Files)
    );
}

#[test]
fn an_embed_is_never_rerouted() {
    let mut door = FrontDoor::default();
    let embed = Address::new("/", "", EmbedHost::VsCode);
    assert_eq!(
        door.observe(&embed, Session::SignedIn, None),
        FrontDoorStep::Hold
    );
}

#[test]
fn the_address_bar_keeps_a_document_it_cannot_open_yet() {
    // The router asks this before it writes the editor's own route over the
    // address. Without it, a link naming a document would be replaced by `/` —
    // the state of an editor holding nothing — the moment that state painted,
    // and the visitor's link would be gone before they could sign in.
    let mut door = FrontDoor::default();
    let link = address("/f/abc", "");
    assert!(!door.waiting_for_a_document(), "nothing named yet");
    door.observe(&link, Session::Pending, None);
    assert!(door.waiting_for_a_document(), "the link names a document");

    // The document arrives: the address is the state's again.
    door.observe(&link, Session::SignedIn, Some("abc"));
    assert!(!door.waiting_for_a_document());
}

#[test]
fn a_document_that_never_arrives_stops_holding_the_address() {
    // A link to a document that is gone (or that the daemon refuses) must not
    // pin the address bar for the rest of the page's life — the same five-second
    // discipline `route_sync` applies to a node that is not in the document.
    let mut door = FrontDoor::default();
    let dead = address("/f/gone", "");
    door.observe(&dead, Session::SignedIn, None);
    assert!(door.waiting_for_a_document());
    for _ in 0..GIVE_UP_FRAMES {
        door.observe(&dead, Session::SignedIn, None);
    }
    assert!(
        !door.waiting_for_a_document(),
        "after the give-up clock the address belongs to the state again"
    );
    // And it does not simply start again on the next frame.
    door.observe(&dead, Session::SignedIn, None);
    assert!(!door.waiting_for_a_document());
}

#[test]
fn a_gate_holds_a_link_for_as_long_as_the_sign_in_takes() {
    // The give-up clock is about the DAEMON not answering. Somebody taking ten
    // minutes over a password is not the link failing, so the clock does not run
    // while the gate is up.
    let mut door = FrontDoor::default();
    let link = address("/f/abc", "");
    for frame in 0..(GIVE_UP_FRAMES * 2) {
        let session = if frame % 2 == 0 {
            Session::Anonymous
        } else {
            Session::Pending
        };
        door.observe(&link, session, None);
    }
    assert!(
        door.waiting_for_a_document(),
        "the link is still the visitor's to sign in to"
    );
    assert_eq!(
        door.observe(&link, Session::SignedIn, None),
        FrontDoorStep::Open("abc".to_string())
    );
}
