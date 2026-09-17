//! The Share dialog's host half, tested where it is pure: parsing, and the
//! mapping from a daemon code onto the sentence this build has for it.

use super::*;

#[test]
fn a_link_access_answer_is_read_in_both_directions() {
    assert_eq!(
        parse_link_access(r#"{"ok":true,"linkAccess":"editor"}"#),
        Some((true, ShareLevel::Editor))
    );
    assert_eq!(
        parse_link_access(r#"{"ok":true,"linkAccess":null}"#),
        Some((false, ShareLevel::DEFAULT))
    );
    assert_eq!(parse_link_access("not json"), None);
}

#[test]
fn the_shared_with_array_survives_both_entry_shapes() {
    let grants = parse_shared_with(
        r#"{"ok":true,"sharedWith":["userB",{"account":"userC","level":"editor","invitedBy":"userA"}]}"#,
    );
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0].account, "userB");
    assert_eq!(grants[0].level, ShareLevel::DEFAULT);
    assert_eq!(grants[1].account, "userC");
    assert_eq!(grants[1].level, ShareLevel::Editor);
    assert_eq!(grants[1].invited_by.as_deref(), Some("userA"));
}

/// Every code the daemon answers this dialog with, and the sentence this
/// build has for it.
///
/// The string on the left of each pair is the DAEMON's spelling — the same
/// literal `share_routes_tests` asserts the daemon puts on the wire — so
/// the two halves of one refusal are pinned in two places and a rename on
/// one side shows up on the other (issue #142: they read `share-with-self`
/// against a daemon that answers `cannot-share-with-self`, which left the
/// "that account is you" sentence unreachable).
#[test]
fn a_refusal_code_maps_onto_the_sentence_this_build_has() {
    // What the share routes answer a caller with no invite right
    // (`AccessRefusal::ReadOnly`), beside the dialog's own spelling of it.
    for code in ["read-only-role", "invite-role-required"] {
        assert_eq!(
            refusal_for(Some(code), &ShareAction::LoadList, ""),
            ShareInviteRefusal::NoInviteRight,
            "{code}"
        );
    }
    // What the invitation route answers (`AccessRefusal::NotAnAdministrator`).
    for code in ["admin-role-required", "admin-role-required-for-email"] {
        assert_eq!(
            refusal_for(Some(code), &ShareAction::LoadList, ""),
            ShareInviteRefusal::NotAnAdministratorForEmail,
            "{code}"
        );
    }
    // `ShareError::SelfShare` in `share_routes`, which is the code this
    // whole issue is about.
    for code in ["cannot-share-with-self", "already-has-access"] {
        assert_eq!(
            refusal_for(
                Some(code),
                &ShareAction::Grant {
                    account: "userB".to_string(),
                    level: ShareLevel::Viewer,
                },
                ""
            ),
            ShareInviteRefusal::AlreadyOnList {
                account: "userB".to_string()
            },
            "{code}"
        );
    }
    assert_eq!(
        refusal_for(
            Some("level-above-your-own"),
            &ShareAction::Grant {
                account: "userB".to_string(),
                level: ShareLevel::Editor,
            },
            ""
        ),
        ShareInviteRefusal::LevelAboveOwn {
            level: ShareLevel::Editor,
            own: ShareLevel::DEFAULT,
        }
    );
    // A typo in the invite field, and the account that arrives one over the
    // list's ceiling: the two refusals a person is most likely to meet, and
    // both used to be shown as the code itself (issue #146).
    assert_eq!(
        refusal_for(
            Some("unknown-account"),
            &ShareAction::Grant {
                account: "collegue".to_string(),
                level: ShareLevel::Viewer,
            },
            r#"{"ok":false,"error":"unknown-account","message":"this deployment has no account with that id, name or address"}"#
        ),
        ShareInviteRefusal::UnknownAccount {
            account: "collegue".to_string()
        }
    );
    assert_eq!(
        refusal_for(
            Some("share-limit-reached"),
            &ShareAction::Grant {
                account: "one-too-many".to_string(),
                level: ShareLevel::Viewer,
            },
            r#"{"ok":false,"error":"share-limit-reached","limit":256,"message":"this document is already shared with 256 accounts"}"#
        ),
        ShareInviteRefusal::ShareLimitReached { limit: 256 }
    );
    // A refusal about an entry this dialog cannot see, and a limit no body
    // carried, are carried as codes rather than answered with a sentence
    // that would have to name nobody or invent a number.
    assert_eq!(
        refusal_for(Some("unknown-account"), &ShareAction::LoadList, ""),
        ShareInviteRefusal::RefusedByServer {
            code: "unknown-account".to_string()
        }
    );
    assert_eq!(
        refusal_for(
            Some("share-limit-reached"),
            &ShareAction::LoadList,
            r#"{"ok":false,"error":"share-limit-reached"}"#
        ),
        ShareInviteRefusal::RefusedByServer {
            code: "share-limit-reached".to_string()
        }
    );
    // A code with no sentence of its own is carried, not guessed at — the
    // daemon's remaining share-route codes included.
    for code in [
        "share-not-persisted",
        "missing-document",
        "payload-too-large",
        "malformed-share-request",
        "account-lookup-unavailable",
        "tenant-not-shared",
    ] {
        assert_eq!(
            refusal_for(Some(code), &ShareAction::LoadList, ""),
            ShareInviteRefusal::RefusedByServer {
                code: code.to_string()
            },
            "{code}"
        );
    }
    // Nothing readable at all: the same catch-all, named.
    assert_eq!(
        refusal_for(None, &ShareAction::LoadList, ""),
        ShareInviteRefusal::RefusedByServer {
            code: "unknown".to_string()
        }
    );
}

#[test]
fn an_invitation_link_is_made_absolute() {
    assert_eq!(
        link_with_base("http://127.0.0.1:3100", "/invite/abc"),
        "http://127.0.0.1:3100/invite/abc"
    );
    assert_eq!(
        link_with_base("http://127.0.0.1:3100", "https://x/invite/abc"),
        "https://x/invite/abc"
    );
}

#[test]
fn an_error_body_yields_its_code() {
    assert_eq!(
        error_code(r#"{"ok":false,"error":"share-not-persisted"}"#).as_deref(),
        Some("share-not-persisted")
    );
    assert_eq!(error_code("<html>500</html>"), None);
}
