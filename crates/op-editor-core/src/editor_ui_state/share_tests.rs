//! The Share dialog's state: what Invite would do, and what it refuses.

use super::*;

fn state_with(rights: Rights) -> ShareUiState {
    ShareUiState {
        own_rights: rights,
        is_owner: true,
        self_account: Some("userA".to_string()),
        ..ShareUiState::default()
    }
}

/// An open dialog owned by an administrator.
///
/// Admin rather than editor because the tests below exercise BOTH invite
/// paths, and the email one needs the account list — the level is the subject
/// of its own tests, not a property of this fixture.
fn opening() -> ShareUiState {
    let mut state = state_with(Rights::ADMIN);
    state.open_with(Some("http://host/f/key?node-id=3".to_string()));
    state
}

#[test]
fn the_dialog_starts_closed_and_opens_over_a_link() {
    let mut state = ShareUiState::default();
    assert!(!state.open);
    assert_eq!(state.link, None);
    // The default level a fresh press would hand out is the weakest one.
    assert_eq!(state.invite_level, ShareLevel::Viewer);
    assert!(!state.link_enabled);
    state.open_with(Some("http://host/f/key".to_string()));
    assert!(state.open);
    assert_eq!(state.link.as_deref(), Some("http://host/f/key"));
    state.close();
    assert!(!state.open);
    assert_eq!(state.link.as_deref(), Some("http://host/f/key"));
}

#[test]
fn closing_drops_the_notice_because_it_belonged_to_that_press() {
    let mut state = opening();
    state.notice = Some(ShareNotice::CopiedLink);
    state.close();
    assert_eq!(state.notice, None);
}

#[test]
fn an_empty_field_is_refused_before_anything_else_is_asked() {
    let state = opening();
    assert_eq!(state.plan_invite(), Err(ShareInviteRefusal::EmptyField));
    let mut state = state;
    state.invite_input.set_text("  ,, \n ");
    assert_eq!(state.plan_invite(), Err(ShareInviteRefusal::EmptyField));
}

#[test]
fn invite_without_the_right_refuses_with_the_reason_no_request_is_made() {
    // The view-only floor: an account that may look at the document and
    // nothing else. The dialog must say which right is missing rather than
    // pretend a link was created.
    let mut state = state_with(Rights::VIEW_ONLY);
    state.open_with(None);
    state.invite_input.set_text("ada@example.test");
    assert_eq!(state.plan_invite(), Err(ShareInviteRefusal::NoInviteRight));
    assert!(!state.can_invite());
    // The viewer level is the only one these rights cover — and it is still
    // not handable, because `can_invite` is false. The control is dimmed and
    // the press refuses, which is what the two answers together mean.
    assert_eq!(state.grantable_levels(), vec![ShareLevel::Viewer]);
    // And the same for a named account, not only for an email.
    state.invite_input.set_text("userB");
    assert_eq!(state.plan_invite(), Err(ShareInviteRefusal::NoInviteRight));
}

#[test]
fn a_commenter_may_add_people_but_not_make_editors() {
    let mut state = state_with(Rights::CONTRIBUTOR);
    state.invite_input.set_text("userB");
    assert_eq!(
        state.plan_invite(),
        Ok(ShareInvitePlan {
            invitations: Vec::new(),
            grants: vec!["userB".to_string()],
        })
    );
    assert_eq!(
        state.set_invite_level(ShareLevel::Editor),
        Err(ShareInviteRefusal::LevelAboveOwn {
            level: ShareLevel::Editor,
            own: ShareLevel::Commenter,
        })
    );
    assert_eq!(
        state.invite_level,
        ShareLevel::Viewer,
        "the level did not move"
    );
    assert_eq!(
        state.set_invite_level(ShareLevel::Commenter),
        Ok(()),
        "a commenter may hand out its own level"
    );
    assert_eq!(
        state.grantable_levels(),
        vec![ShareLevel::Commenter, ShareLevel::Viewer]
    );
}

#[test]
fn an_email_entry_is_an_invitation_and_an_account_entry_is_a_grant() {
    let mut state = opening();
    state
        .invite_input
        .set_text("ada@example.test, userB, ada@example.test");
    let plan = state.plan_invite().expect("a plan");
    assert_eq!(plan.invitations, ["ada@example.test"]);
    assert_eq!(plan.grants, ["userB"]);
    assert!(!plan.is_empty());
}

#[test]
fn an_email_invitation_needs_the_account_list_whatever_level_it_names() {
    // The email path creates an ACCOUNT, which is a power of the deployment's
    // account list and not of a document — so it needs `can_manage_users` even
    // though the editor level may invite people who already exist.
    let mut state = state_with(Rights::EDITOR);
    state.invite_input.set_text("ada@example.test");
    assert!(!state.can_invite_by_email());
    assert_eq!(
        state.plan_invite(),
        Err(ShareInviteRefusal::NotAnAdministratorForEmail)
    );
    // A viewer invitation mints an account too — still the same one power.
    state.invite_level = ShareLevel::Viewer;
    assert_eq!(
        state.plan_invite(),
        Err(ShareInviteRefusal::NotAnAdministratorForEmail)
    );
    // An administrator may.
    let mut admin = state_with(Rights::ADMIN);
    admin.invite_input.set_text("ada@example.test");
    assert!(admin.can_invite_by_email());
    assert_eq!(admin.plan_invite().expect("a plan").invitations.len(), 1);
    // And a named account is still just a grant, which editing rights allow.
    admin.invite_input.set_text("userB");
    assert_eq!(admin.plan_invite().expect("a plan").grants, ["userB"]);
}

#[test]
fn too_many_addresses_are_refused_with_the_real_count() {
    let many: String = (0..MAX_INVITE_ENTRIES + 3)
        .map(|index| format!("user{index},"))
        .collect();
    let mut state = opening();
    state.invite_input.set_text(many);
    assert_eq!(
        state.plan_invite(),
        Err(ShareInviteRefusal::TooManyEntries {
            count: MAX_INVITE_ENTRIES + 3,
        })
    );
}

#[test]
fn inviting_yourself_is_caught_here_rather_than_by_the_server() {
    let mut state = opening();
    state.invite_input.set_text("userA");
    assert_eq!(
        state.plan_invite(),
        Err(ShareInviteRefusal::AlreadyOnList {
            account: "userA".to_string(),
        })
    );
}

#[test]
fn an_unreadable_access_list_is_reported_and_never_shown_as_empty() {
    let mut state = opening();
    state.apply_list(ShareListSnapshot {
        shared_with: vec![ShareGrant::unattributed("userB", ShareLevel::Editor)],
        shared_with_me: Vec::new(),
        available: true,
    });
    assert_eq!(state.people().len(), 1);

    // A refusal body, a truncated answer, a network error page — all mean
    // "we do not know". The list stays, and the dialog says so.
    state.apply_list(ShareListSnapshot::parse(r#"{"ok":false}"#));
    assert_eq!(state.people().len(), 1, "the known list was discarded");
    assert_eq!(state.notice, Some(ShareNotice::ListUnavailable));
    assert!(state.notice.as_ref().expect("a notice").is_refusal());

    // A later good answer clears the warning.
    state.apply_list(ShareListSnapshot::parse(
        r#"{"ok":true,"sharedWith":[],"sharedWithMe":[]}"#,
    ));
    assert_eq!(state.notice, None);
    assert!(state.people().is_empty());
}

#[test]
fn the_people_list_groups_by_level_and_counts() {
    let mut state = opening();
    state.apply_list(ShareListSnapshot::parse(
        r#"{"ok":true,"sharedWith":[
             {"account":"userB","level":"viewer","invitedBy":"userA"},
             {"account":"userC","level":"viewer","invitedBy":"userA"},
             {"account":"userD","level":"editor","invitedBy":null}
           ],"sharedWithMe":[]}"#,
    ));
    assert_eq!(state.count_at(ShareLevel::Viewer), 2);
    assert_eq!(state.count_at(ShareLevel::Editor), 1);
    assert_eq!(state.count_at(ShareLevel::Admin), 0);
}

#[test]
fn attribution_says_who_added_somebody_and_admits_when_nobody_recorded_it() {
    let state = opening();
    let locale = Locale::EnUs;
    let mine = ShareGrant {
        account: "userB".to_string(),
        level: ShareLevel::Viewer,
        invited_by: Some("userA".to_string()),
    };
    assert_eq!(
        state.attribution_label(locale, &mine),
        op_i18n::translate(locale, "share.row.invitedByYou")
    );
    let theirs = ShareGrant {
        invited_by: Some("userE".to_string()),
        ..mine.clone()
    };
    assert!(state.attribution_label(locale, &theirs).contains("userE"));
    // A grant from before attribution existed says so rather than naming the
    // owner by default — which would be a guess printed as a fact.
    let unknown = ShareGrant {
        invited_by: None,
        ..mine.clone()
    };
    assert_eq!(
        state.attribution_label(locale, &unknown),
        op_i18n::translate(locale, "share.row.invitedUnknown")
    );
}

#[test]
fn the_callers_own_row_says_you_and_everybody_else_shows_their_account() {
    let state = opening();
    let locale = Locale::EnUs;
    let me = ShareGrant::unattributed("userA", ShareLevel::Editor);
    let other = ShareGrant::unattributed("userB", ShareLevel::Viewer);
    assert_eq!(state.person_label(locale, &me), "You");
    assert_eq!(state.person_label(locale, &other), "userB");
}

#[test]
fn a_recorded_grant_updates_the_row_in_place_and_clears_the_field() {
    let mut state = opening();
    state.invite_input.set_text("userB");
    state.record_granted(vec![ShareGrant {
        account: "userB".to_string(),
        level: ShareLevel::Commenter,
        invited_by: Some("userA".to_string()),
    }]);
    assert_eq!(state.invite_input.text(), "");
    assert_eq!(state.people().len(), 1);
    assert_eq!(state.count_at(ShareLevel::Commenter), 1);

    // Granting the same account again at another level replaces the row rather
    // than listing the account twice.
    state.record_granted(vec![ShareGrant {
        account: "userB".to_string(),
        level: ShareLevel::Editor,
        invited_by: Some("userA".to_string()),
    }]);
    assert_eq!(state.people().len(), 1);
    assert_eq!(state.count_at(ShareLevel::Editor), 1);
    assert_eq!(state.count_at(ShareLevel::Commenter), 0);
}

#[test]
fn issued_invitations_are_reported_with_their_links_and_never_as_delivered() {
    let mut state = opening();
    state.record_issued(vec![ShareIssuedInvite {
        email: "ada@example.test".to_string(),
        path: "/invite/tok123".to_string(),
    }]);
    let notice = state.notice.clone().expect("a notice");
    assert!(!notice.is_refusal());
    assert_eq!(notice.i18n_key(), "share.notice.issued");
    match notice {
        ShareNotice::Issued(invites) => {
            assert_eq!(invites[0].path, "/invite/tok123");
            assert_eq!(invites[0].email, "ada@example.test");
        }
        other => panic!("unexpected notice: {other:?}"),
    }
    // The invitation went nowhere by itself — the field is cleared because the
    // person now has a link to carry, not because a message was sent.
    assert_eq!(state.invite_input.text(), "");
}

#[test]
fn re_leveling_somebody_is_the_admins_right_and_not_the_editors() {
    let grants = || {
        vec![ShareGrant {
            account: "userB".to_string(),
            level: ShareLevel::Viewer,
            invited_by: Some("userA".to_string()),
        }]
    };
    // An editor who does NOT own this document may not re-level anybody: the
    // document's access list is the admin level's one extra power.
    let mut visitor = state_with(Rights::EDITOR);
    visitor.is_owner = false;
    visitor.apply_list(ShareListSnapshot {
        shared_with: grants(),
        shared_with_me: Vec::new(),
        available: true,
    });
    assert_eq!(
        visitor.set_person_level(0, ShareLevel::Editor),
        Err(ShareInviteRefusal::NoInviteRight)
    );

    // The owner may — the document is theirs — and only up to its own level.
    let mut owner = state_with(Rights::EDITOR);
    owner.apply_list(ShareListSnapshot {
        shared_with: grants(),
        shared_with_me: Vec::new(),
        available: true,
    });
    assert!(!ShareLevel::from_rights(Rights::EDITOR).manages_this_document());
    assert_eq!(owner.set_person_level(0, ShareLevel::Editor), Ok(()));
    assert_eq!(
        owner.set_person_level(0, ShareLevel::Admin),
        Err(ShareInviteRefusal::LevelAboveOwn {
            level: ShareLevel::Admin,
            own: ShareLevel::Editor,
        })
    );
    assert_eq!(owner.people()[0].level, ShareLevel::Editor);

    let mut admin = state_with(Rights::ADMIN);
    admin.apply_list(ShareListSnapshot {
        shared_with: grants(),
        shared_with_me: Vec::new(),
        available: true,
    });
    assert!(ShareLevel::from_rights(Rights::ADMIN).manages_this_document());
    assert_eq!(admin.set_person_level(0, ShareLevel::Admin), Ok(()));
    assert_eq!(admin.people()[0].level, ShareLevel::Admin);
}

#[test]
fn revoking_forgets_the_row_only_after_the_server_confirmed_it() {
    let mut state = opening();
    state.apply_list(ShareListSnapshot {
        shared_with: vec![
            ShareGrant::unattributed("userB", ShareLevel::Viewer),
            ShareGrant::unattributed("userC", ShareLevel::Editor),
        ],
        shared_with_me: Vec::new(),
        available: true,
    });
    state.forget_person(0);
    assert_eq!(state.people().len(), 1);
    assert_eq!(state.people()[0].account, "userC");
    // An index past the end is a no-op rather than a panic: the list may have
    // been replaced by a fresher answer between the press and the confirmation.
    state.forget_person(9);
    assert_eq!(state.people().len(), 1);
}

#[test]
fn a_refusal_from_the_server_is_shown_the_same_way_as_a_local_one() {
    let mut state = opening();
    state.busy = true;
    state.record_refusal(ShareInviteRefusal::NoInviteRight);
    assert!(!state.busy);
    let notice = state.notice.clone().expect("a notice");
    assert!(notice.is_refusal());
    assert_eq!(notice.i18n_key(), "share.invite.refused.noRight");
}
