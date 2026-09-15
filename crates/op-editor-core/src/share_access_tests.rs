//! Levels, grant ceilings and the wire shapes the Share dialog reads.

use super::*;

#[test]
fn the_four_levels_are_the_operators_four_and_descend_in_authority() {
    assert_eq!(
        ShareLevel::ALL.map(ShareLevel::wire),
        ["admin", "editor", "commenter", "viewer"]
    );
    for (index, level) in ShareLevel::ALL.iter().enumerate() {
        let next = ShareLevel::ALL.get(index + 1);
        if let Some(next) = next {
            assert!(
                level.rank() > next.rank(),
                "{level:?} must outrank {next:?}"
            );
        }
    }
}

#[test]
fn commenting_and_inviting_share_one_level_because_the_matrix_does() {
    // The whole reason the dialog is not Figma's view/edit: this level carries
    // a right (invite) that no view/edit pair can express, and it is the level
    // the five contributor roles hold.
    let commenter = ShareLevel::Commenter.document_rights();
    assert!(commenter.can_comment());
    assert!(commenter.can_invite());
    assert!(!commenter.can_edit());
    assert_eq!(
        ShareLevel::Commenter.role_wires(),
        ["software", "analyst", "frontend", "backend", "qa"]
    );
}

#[test]
fn a_viewer_is_an_account_with_no_product_role_at_all() {
    // The view-only floor is what "no roles" means; an invitation at this
    // level therefore asks for no roles rather than for a viewer role that
    // does not exist in the hub's vocabulary.
    assert!(ShareLevel::Viewer.roles().is_empty());
    assert_eq!(ShareLevel::Viewer.document_rights(), Rights::VIEW_ONLY);
}

#[test]
fn no_document_grant_reaches_the_deployments_account_list() {
    // The load-bearing exclusion: ManageUsers is a deployment right, and a
    // level granted on a file must not be a way to reach it.
    for level in ShareLevel::ALL {
        assert!(
            !level.document_rights().can_manage_users(),
            "{level:?} leaked the account list into a document grant"
        );
        assert_eq!(
            level.manages_this_document(),
            level == ShareLevel::Admin,
            "{level:?} answered the wrong question about this document's access list"
        );
    }
    // …while the level itself still names the operator's admin bucket, which
    // is what a caller must hold before it may hand it out.
    assert!(ShareLevel::Admin.role_rights().can_manage_users());
}

#[test]
fn a_level_reads_back_out_of_the_rights_it_names() {
    assert_eq!(ShareLevel::from_rights(Rights::ADMIN), ShareLevel::Admin);
    assert_eq!(ShareLevel::from_rights(Rights::EDITOR), ShareLevel::Editor);
    assert_eq!(
        ShareLevel::from_rights(Rights::CONTRIBUTOR),
        ShareLevel::Commenter
    );
    assert_eq!(
        ShareLevel::from_rights(Rights::VIEW_ONLY),
        ShareLevel::Viewer
    );
    // No roles at all still reads as the floor, never as something stronger.
    assert_eq!(ShareLevel::from_rights(Rights::NONE), ShareLevel::Viewer);
}

#[test]
fn you_may_not_hand_out_a_level_you_do_not_hold() {
    // A contributor may add a viewer or a commenter…
    assert!(ShareLevel::Viewer.is_grantable_by(Rights::CONTRIBUTOR));
    assert!(ShareLevel::Commenter.is_grantable_by(Rights::CONTRIBUTOR));
    // …and may not create an editor or an admin.
    assert!(!ShareLevel::Editor.is_grantable_by(Rights::CONTRIBUTOR));
    assert!(!ShareLevel::Admin.is_grantable_by(Rights::CONTRIBUTOR));
    // Only a right that carries the account list may hand out the admin level.
    assert!(ShareLevel::Admin.is_grantable_by(Rights::ADMIN));
    assert!(!ShareLevel::Admin.is_grantable_by(Rights::EDITOR));
}

#[test]
fn the_default_level_fails_closed() {
    // A client older than this model, or a body that forgot to say, must not
    // be able to hand out editing by omission.
    assert_eq!(ShareLevel::DEFAULT, ShareLevel::Viewer);
    assert_eq!(ShareLevel::DEFAULT.rank(), 0);
}

#[test]
fn levels_parse_from_the_spellings_three_writers_use() {
    for (raw, level) in [
        ("admin", ShareLevel::Admin),
        ("Admin", ShareLevel::Admin),
        ("админ", ShareLevel::Admin),
        ("can-edit", ShareLevel::Editor),
        ("can_edit", ShareLevel::Editor),
        ("ux/ui", ShareLevel::Editor),
        ("can comment", ShareLevel::Commenter),
        ("inviter", ShareLevel::Commenter),
        ("guest", ShareLevel::Viewer),
    ] {
        assert_eq!(ShareLevel::from_wire(raw), Ok(level), "{raw}");
    }
    assert_eq!(ShareLevel::from_wire("  "), Err(ShareLevelError::Blank));
    assert_eq!(
        ShareLevel::from_wire("superuser"),
        Err(ShareLevelError::Unknown {
            raw: "superuser".to_string()
        })
    );
}

#[test]
fn a_grant_reads_both_wire_shapes_and_the_older_one_fails_closed() {
    let current = serde_json::json!({
        "account": "userB",
        "level": "editor",
        "invitedBy": "userA",
    });
    assert_eq!(
        ShareGrant::from_json(&current),
        Some(ShareGrant {
            account: "userB".to_string(),
            level: ShareLevel::Editor,
            invited_by: Some("userA".to_string()),
            display_name: None,
            username: None,
        })
    );
    // Every list written before levels existed is an array of account ids.
    assert_eq!(
        ShareGrant::from_json(&serde_json::json!("userB")),
        Some(ShareGrant::unattributed("userB", ShareLevel::Viewer))
    );
    // An entry that names no account is dropped rather than read as one.
    assert_eq!(ShareGrant::from_json(&serde_json::json!("   ")), None);
    assert_eq!(
        ShareGrant::from_json(&serde_json::json!({"level": "editor"})),
        None
    );
}

#[test]
fn an_unknown_persisted_level_reads_as_the_floor_not_as_an_error() {
    // A level this build does not know belongs to a future writer. Reading it
    // as "unknown, so whatever" would be the fail-open direction; reading it
    // as Viewer keeps the account on the list with the least it could mean.
    let grant = ShareGrant::from_json(&serde_json::json!({
        "account": "userB",
        "level": "wizard",
    }))
    .expect("a grant");
    assert_eq!(grant.level, ShareLevel::Viewer);
}

#[test]
fn a_grant_round_trips_through_its_wire_object() {
    let grant = ShareGrant {
        account: "userB".to_string(),
        level: ShareLevel::Commenter,
        invited_by: Some("userA".to_string()),
        display_name: None,
        username: None,
    };
    assert_eq!(ShareGrant::from_json(&grant.to_json()), Some(grant));
}

#[test]
fn the_list_snapshot_parses_a_success_and_refuses_to_guess_at_anything_else() {
    let body = serde_json::json!({
        "ok": true,
        "sharedWith": [
            {"account": "userB", "level": "editor", "invitedBy": "userA"},
            {"account": "userC", "level": "viewer", "invitedBy": null},
            "userD",
        ],
        "sharedWithMe": ["userE"],
    })
    .to_string();
    let snapshot = ShareListSnapshot::parse(&body);
    assert!(snapshot.available);
    assert_eq!(snapshot.shared_with.len(), 3);
    assert_eq!(
        snapshot.shared_with_me,
        [SharedOwner {
            owner: "userE".to_string(),
            level: ShareLevel::DEFAULT,
            display_name: None,
            username: None,
        }]
    );
    assert_eq!(snapshot.level_of("userB"), Some(ShareLevel::Editor));
    assert_eq!(snapshot.level_of("userD"), Some(ShareLevel::Viewer));
    assert_eq!(snapshot.count_at(ShareLevel::Viewer), 2);
    assert_eq!(snapshot.count_at(ShareLevel::Admin), 0);

    // "We do not know who has access" must never render as "nobody does".
    for body in [
        "",
        "not json",
        r#"{"ok":false,"error":"tenant-not-shared"}"#,
        r#"{"sharedWith":["userB"]}"#,
    ] {
        assert!(!ShareListSnapshot::parse(body).available, "{body}");
    }
}

#[test]
fn the_invite_field_splits_on_what_people_actually_paste() {
    assert_eq!(
        parse_invite_entries(" a@b.c , d@e.f\n g@h.i ;, a@b.c "),
        ["a@b.c", "d@e.f", "g@h.i"]
    );
    assert!(parse_invite_entries("  ,, \n ").is_empty());
    // Past the cap the list stops growing: the dialog refuses the press rather
    // than inviting a prefix of a mailing list.
    let many: String = (0..MAX_INVITE_ENTRIES + 5)
        .map(|index| format!("user{index}@example.test,"))
        .collect();
    assert_eq!(parse_invite_entries(&many).len(), MAX_INVITE_ENTRIES);
    let long = format!("{}@example.test", "a".repeat(400));
    assert_eq!(
        parse_invite_entries(&long)[0].chars().count(),
        MAX_INVITE_ENTRY_CHARS
    );
}

#[test]
fn an_entry_is_an_email_only_when_it_could_be_one() {
    assert!(looks_like_email("ada@example.test"));
    assert!(!looks_like_email("userB"));
    assert!(!looks_like_email("@example.test"));
    assert!(!looks_like_email("ada@"));
    assert!(!looks_like_email("ada@one@two.test"));
    assert!(!looks_like_email("ada lovelace@example.test"));
}

#[test]
fn inviting_without_the_right_says_which_right_is_missing() {
    // The floor every verified account sits on: no invite right.
    assert_eq!(
        ShareInviteRefusal::check_invite_right(Rights::VIEW_ONLY),
        Err(ShareInviteRefusal::NoInviteRight)
    );
    assert_eq!(
        ShareInviteRefusal::check_invite_right(Rights::CONTRIBUTOR),
        Ok(())
    );
    assert_eq!(
        ShareInviteRefusal::check_invite_right(Rights::VIEW_ONLY)
            .expect_err("refused")
            .code(),
        "invite-role-required"
    );
    // And inviting at a level the caller does not hold names both.
    assert_eq!(
        ShareInviteRefusal::check_level(Rights::CONTRIBUTOR, ShareLevel::Editor),
        Err(ShareInviteRefusal::LevelAboveOwn {
            level: ShareLevel::Editor,
            own: ShareLevel::Commenter,
        })
    );
    assert_eq!(
        ShareInviteRefusal::check_level(Rights::EDITOR, ShareLevel::Editor),
        Ok(())
    );
}

#[test]
fn every_refusal_has_a_code_and_a_sentence_of_its_own() {
    let refusals = [
        ShareInviteRefusal::EmptyField,
        ShareInviteRefusal::TooManyEntries { count: 40 },
        ShareInviteRefusal::NoInviteRight,
        ShareInviteRefusal::LevelAboveOwn {
            level: ShareLevel::Admin,
            own: ShareLevel::Commenter,
        },
        ShareInviteRefusal::NotAnAdministratorForEmail,
        ShareInviteRefusal::AlreadyOnList {
            account: "userB".to_string(),
        },
    ];
    let mut codes: Vec<&str> = refusals.iter().map(ShareInviteRefusal::code).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), refusals.len(), "two refusals share a code");
    let mut keys: Vec<&str> = refusals.iter().map(ShareInviteRefusal::i18n_key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), refusals.len(), "two refusals share a message");
    for refusal in &refusals {
        assert!(!refusal.to_string().is_empty());
    }
}
