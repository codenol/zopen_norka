//! The Share dialog's geometry, hit-test, model and press flow.

use op_editor_core::access::Rights;
use op_editor_core::editor_ui_state::share::{
    ShareAction, ShareIssuedInvite, ShareLevelTarget, ShareNotice, ShareRow, ShareUiState,
};
use op_editor_core::{EditorState, ShareGrant, ShareInviteRefusal, ShareLevel, ShareListSnapshot};
use op_i18n::Locale;

use crate::widgets::share_dialog::{
    apply_share_hit, invite_char_allowed, invite_field_backspace, invite_field_paste,
    invite_field_submit, invite_field_text, ShareDialog, ShareDialogHit,
};
use crate::widgets::share_dialog_layout::{self, MAX_PERSON_ROWS};
use crate::widgets::share_dialog_model::ShareDialogModel;
use crate::Point2D;

/// A viewport the card fits in.
const VW: f32 = 1280.0;
const VH: f32 = 800.0;

/// An editor state with the dialog open and a list loaded.
fn state_with(rights: Rights, people: usize, links: usize) -> EditorState {
    let mut state = EditorState::starter();
    // The product ships Chinese as the default locale; these tests assert the
    // English catalog so a missing translation is a failure rather than a
    // silent fallback that happens to read fine.
    state.editor_ui.locale = Locale::EnUs;
    state.editor_ui.host_locale_override = None;
    let share: &mut ShareUiState = &mut state.editor_ui.share;
    share.own_rights = rights;
    share.rights_known = true;
    share.is_owner = true;
    share.self_account = Some("userA".to_string());
    share.link = Some("http://host/f/key".to_string());
    share.list = ShareListSnapshot {
        shared_with: (0..people)
            .map(|index| ShareGrant {
                account: format!("user{index}"),
                level: if index % 2 == 0 {
                    ShareLevel::Viewer
                } else {
                    ShareLevel::Commenter
                },
                invited_by: Some("userA".to_string()),
                // The directory knows these people; the dialog must paint the
                // name rather than the account id (issue #119).
                display_name: Some(format!("Person {index}")),
                username: Some(format!("user{index}")),
            })
            .collect(),
        shared_with_me: Vec::new(),
        available: true,
    };
    let link = share.link.clone();
    share.open_with(link);
    if links > 0 {
        share.notice = Some(ShareNotice::Issued(
            (0..links)
                .map(|index| ShareIssuedInvite {
                    email: format!("person{index}@example.test"),
                    path: format!("/invite/tok{index}"),
                })
                .collect(),
        ));
    }
    state
}

fn dialog(state: &EditorState) -> ShareDialog<'_> {
    ShareDialog::for_editor(state, VW, VH).expect("the dialog is open")
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

#[test]
fn a_closed_dialog_is_not_a_dialog_at_all() {
    let mut state = EditorState::starter();
    assert!(ShareDialog::for_editor(&state, VW, VH).is_none());
    state.editor_ui.share.open_with(None);
    assert!(ShareDialog::for_editor(&state, VW, VH).is_some());
}

#[test]
fn the_card_is_centred_and_never_wider_than_the_window() {
    let state = state_with(Rights::ADMIN, 2, 0);
    let dialog = dialog(&state);
    let card = dialog.rect();
    assert!((card.origin.x - (VW - card.size.x) / 2.0).abs() < 0.01);
    assert!((card.origin.y - (VH - card.size.y) / 2.0).abs() < 0.01);
    assert_eq!(card.size.x, share_dialog_layout::CARD_W);

    // On a phone the card shrinks to the viewport instead of centring itself
    // off both edges.
    let narrow = ShareDialog::for_editor(&state, 360.0, 640.0).expect("open");
    assert!(narrow.rect().size.x <= 360.0);
    assert!(narrow.rect().origin.x >= 0.0);
}

#[test]
fn the_card_grows_with_the_list_and_the_invitation_links() {
    let small = dialog(&state_with(Rights::ADMIN, 0, 0)).rect().size.y;
    let with_people = dialog(&state_with(Rights::ADMIN, 2, 0)).rect().size.y;
    let with_links = dialog(&state_with(Rights::ADMIN, 2, 2)).rect().size.y;
    assert!(with_people > small, "person rows did not add height");
    assert!(
        with_links > with_people,
        "invitation links did not add height"
    );
    // Each person row is one fixed height — that is what makes the hit-test
    // arithmetic instead of a text measurement.
    let one = dialog(&state_with(Rights::ADMIN, 1, 0)).rect().size.y;
    assert!((with_people - one - 46.0).abs() < 0.01);
}

#[test]
fn the_controls_do_not_overlap_and_sit_inside_the_card() {
    let state = state_with(Rights::ADMIN, 2, 1);
    let dialog = dialog(&state);
    let layout = dialog.layout();
    let card = layout.card;
    for rect in [
        layout.copy_link,
        layout.close,
        layout.invite_field,
        layout.invite_button,
        layout.invite_level,
        layout.you,
        layout.link_row,
        layout.link_level,
        layout.link_switch,
        layout.footer,
    ] {
        assert!(
            rect.origin.x >= card.origin.x - 0.01
                && rect.origin.x + rect.size.x <= card.origin.x + card.size.x + 0.01
                && rect.origin.y >= card.origin.y - 0.01
                && rect.origin.y + rect.size.y <= card.origin.y + card.size.y + 0.01,
            "a control escaped the card: {rect:?} in {card:?}"
        );
    }
    // Copy link is left of the close button, and does not run under the title.
    assert!(layout.copy_link.origin.x + layout.copy_link.size.x <= layout.close.origin.x);
    assert!(layout.title.origin.x + layout.title.size.x <= layout.copy_link.origin.x);
    // The switch is the right-most control on the link row.
    assert!(layout.link_switch.origin.x > layout.link_level.origin.x);
}

#[test]
fn every_painted_control_is_a_control_that_can_be_pressed() {
    let state = state_with(Rights::ADMIN, 2, 1);
    let dialog = dialog(&state);
    let layout = dialog.layout();
    let centre = |rect: crate::Rect| {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    };
    assert_eq!(
        dialog.hit_test(centre(layout.copy_link)),
        ShareDialogHit::CopyLink
    );
    assert_eq!(dialog.hit_test(centre(layout.close)), ShareDialogHit::Close);
    assert_eq!(
        dialog.hit_test(centre(layout.invite_field)),
        ShareDialogHit::InviteField
    );
    assert_eq!(
        dialog.hit_test(centre(layout.invite_button)),
        ShareDialogHit::Invite
    );
    assert_eq!(
        dialog.hit_test(centre(layout.link_switch)),
        ShareDialogHit::LinkAccess
    );
    assert_eq!(
        dialog.hit_test(centre(layout.link_level)),
        ShareDialogHit::LinkLevel
    );
    assert_eq!(
        dialog.hit_test(centre(layout.footer)),
        ShareDialogHit::OpenSession
    );
    assert_eq!(
        dialog.hit_test(centre(layout.people[1].level)),
        ShareDialogHit::PersonLevel(1)
    );
    assert_eq!(
        dialog.hit_test(centre(layout.people[1].remove)),
        ShareDialogHit::PersonRemove(1)
    );
    assert_eq!(
        dialog.hit_test(centre(layout.invite_links[0].copy)),
        ShareDialogHit::CopyInviteLink(0)
    );
    // A press beside the card is consumed as Outside rather than falling
    // through to the canvas behind a modal.
    assert_eq!(
        dialog.hit_test(Point2D::new(4.0, 4.0)),
        ShareDialogHit::Outside
    );
    assert_eq!(dialog.hit_test(centre(layout.card)), ShareDialogHit::Inside);
}

#[test]
fn the_hover_row_matches_the_hit_so_a_highlighted_control_always_answers() {
    let state = state_with(Rights::ADMIN, 1, 0);
    let dialog = dialog(&state);
    let layout = dialog.layout();
    let centre = |rect: crate::Rect| {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    };
    assert_eq!(
        dialog.row_at(centre(layout.invite_button)),
        Some(ShareRow::Invite)
    );
    assert_eq!(
        dialog.row_at(centre(layout.people[0].remove)),
        Some(ShareRow::PersonRemove(0))
    );
    // Inside and outside highlight nothing: there is no control there.
    assert_eq!(dialog.row_at(Point2D::new(4.0, 4.0)), None);
}

#[test]
fn the_people_list_is_capped_and_the_rest_are_counted() {
    let state = state_with(Rights::ADMIN, MAX_PERSON_ROWS + 3, 0);
    let dialog = dialog(&state);
    assert_eq!(dialog.layout().people.len(), MAX_PERSON_ROWS);
    assert!(dialog.layout().overflow.is_some());
    let overflow = dialog.model().overflow_text.clone().expect("a count");
    assert!(overflow.contains('3'), "{overflow}");
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[test]
fn the_dialog_names_our_four_levels_and_shows_what_each_one_grants() {
    let state = state_with(Rights::ADMIN, 0, 0);
    let model = ShareDialogModel::for_state(&state);
    // Four levels, and their names carry the two rights Figma's pair cannot:
    // commenting and inviting.
    assert_eq!(model.level_options.len(), 4);
    let labels: Vec<String> = model
        .level_options
        .iter()
        .map(|(_, label, _)| label.clone())
        .collect();
    assert_eq!(
        labels,
        vec![
            op_i18n::translate(Locale::EnUs, "share.level.admin"),
            op_i18n::translate(Locale::EnUs, "share.level.editor"),
            op_i18n::translate(Locale::EnUs, "share.level.commenter"),
            op_i18n::translate(Locale::EnUs, "share.level.viewer"),
        ]
    );
    assert!(labels[2].contains("comment"));
    assert!(labels[2].contains("invite"));
    for (_, _, hint) in &model.level_options {
        assert!(!hint.is_empty(), "a level explained itself with nothing");
    }
}

#[test]
fn a_commenter_is_offered_two_levels_and_not_four() {
    let state = state_with(Rights::CONTRIBUTOR, 0, 0);
    let model = ShareDialogModel::for_state(&state);
    let levels: Vec<ShareLevel> = model
        .level_options
        .iter()
        .map(|(level, _, _)| *level)
        .collect();
    assert_eq!(levels, vec![ShareLevel::Commenter, ShareLevel::Viewer]);
    assert!(
        model.invite_enabled == false,
        "an empty field is not invitable"
    );
}

#[test]
fn the_people_rows_carry_their_level_and_who_added_them() {
    let state = state_with(Rights::ADMIN, 2, 0);
    let model = ShareDialogModel::for_state(&state);
    assert_eq!(model.people.len(), 2);
    assert_eq!(model.people[0].account, "user0");
    // The row a person reads says a NAME. An account id in this column is what
    // made "Who has access" unreadable (issue #119).
    assert_eq!(model.people[0].label, "Person 0");
    assert_eq!(model.people[0].level, ShareLevel::Viewer);
    assert_eq!(model.people[1].level, ShareLevel::Commenter);
    assert_eq!(
        model.people[0].attribution,
        op_i18n::translate(Locale::EnUs, "share.row.invitedByYou")
    );
    assert_eq!(
        model.people[1].level_label,
        op_i18n::translate(Locale::EnUs, "share.level.commenter")
    );
}

#[test]
fn the_dialog_says_there_is_no_mail_before_anything_is_pressed() {
    let state = state_with(Rights::ADMIN, 0, 0);
    let model = ShareDialogModel::for_state(&state);
    assert!(model.notice.is_none());
    assert_eq!(
        model.no_mail_hint,
        op_i18n::translate(Locale::EnUs, "share.invite.noMail")
    );
    // And the caption beside the link switch admits there is no anonymous
    // access, rather than letting the label imply a public URL.
    assert_eq!(
        model.link_caption,
        op_i18n::translate(Locale::EnUs, "share.row.anyoneWithLink.caption")
    );
}

#[test]
fn an_issued_invitation_carries_its_link_into_the_model() {
    let state = state_with(Rights::ADMIN, 0, 2);
    let model = ShareDialogModel::for_state(&state);
    assert_eq!(model.issued_links.len(), 2);
    assert_eq!(model.issued_links[0].1, "/invite/tok0");
    let notice = model.notice.clone().expect("a notice");
    assert!(notice.contains('2'), "{notice}");
    assert!(!model.notice_is_refusal);
}

#[test]
fn a_refusal_is_painted_as_one() {
    let mut state = state_with(Rights::VIEW_ONLY, 0, 0);
    state.editor_ui.share.invite_input.set_text("userB");
    state
        .editor_ui
        .share
        .record_refusal(ShareInviteRefusal::NoInviteRight);
    let model = ShareDialogModel::for_state(&state);
    assert!(model.notice_is_refusal);
    assert_eq!(
        model.notice.as_deref(),
        Some(op_i18n::translate(
            Locale::EnUs,
            "share.invite.refused.noRight"
        ))
    );
    assert!(!model.invite_enabled);
}

// ---------------------------------------------------------------------------
// Press flow
// ---------------------------------------------------------------------------

#[test]
fn opening_the_level_picker_and_choosing_an_option_sets_the_level() {
    let state = state_with(Rights::ADMIN, 0, 0);
    let mut share = state.editor_ui.share.clone();
    // A press on the invite level opens the picker, and a second closes it.
    assert!(apply_share_hit(&mut share, ShareDialogHit::LinkLevel, None));
    assert_eq!(share.level_picker, Some(ShareLevelTarget::Link));
    assert!(apply_share_hit(&mut share, ShareDialogHit::LinkLevel, None));
    assert_eq!(share.level_picker, None);

    // Choosing an option applies it and closes the picker.
    apply_share_hit(&mut share, ShareDialogHit::LinkLevel, None);
    assert!(apply_share_hit(
        &mut share,
        ShareDialogHit::LevelOption(0),
        Some(ShareLevel::Editor)
    ));
    assert_eq!(share.link_level, ShareLevel::Editor);
    assert_eq!(share.level_picker, None);
}

#[test]
fn a_person_row_can_be_re_levelled_and_the_press_reaches_the_server() {
    let state = state_with(Rights::ADMIN, 1, 0);
    let mut share = state.editor_ui.share.clone();
    share.pending.clear();
    apply_share_hit(&mut share, ShareDialogHit::PersonLevel(0), None);
    assert_eq!(share.level_picker, Some(ShareLevelTarget::Person(0)));
    apply_share_hit(
        &mut share,
        ShareDialogHit::LevelOption(1),
        Some(ShareLevel::Editor),
    );
    assert_eq!(share.people()[0].level, ShareLevel::Editor);
    assert!(matches!(
        share.pending.as_slice(),
        [ShareAction::Grant { account, level }] if account == "user0" && *level == ShareLevel::Editor
    ));
}

#[test]
fn invite_without_the_right_queues_nothing_and_says_why() {
    let mut state = state_with(Rights::VIEW_ONLY, 0, 0);
    state
        .editor_ui
        .share
        .invite_input
        .set_text("ada@example.test");
    let mut share = state.editor_ui.share.clone();
    share.pending.clear();
    apply_share_hit(&mut share, ShareDialogHit::Invite, None);
    assert!(
        share.pending.is_empty(),
        "a refused invite must not send anything: {:?}",
        share.pending
    );
    assert!(matches!(
        share.notice,
        Some(ShareNotice::Refused(ShareInviteRefusal::NoInviteRight))
    ));
}

#[test]
fn invite_queues_one_request_per_entry_and_no_request_that_claims_delivery() {
    let mut state = state_with(Rights::ADMIN, 0, 0);
    state
        .editor_ui
        .share
        .invite_input
        .set_text("ada@example.test, userB");
    let mut share = state.editor_ui.share.clone();
    share.pending.clear();
    apply_share_hit(&mut share, ShareDialogHit::Invite, None);
    assert!(share.busy);
    match share.pending.as_slice() {
        [ShareAction::IssueInvitation { email, level }, ShareAction::Grant {
            account,
            level: grant,
        }] => {
            assert_eq!(email, "ada@example.test");
            assert_eq!(account, "userB");
            assert_eq!(*level, ShareLevel::Viewer);
            assert_eq!(*grant, ShareLevel::Viewer);
        }
        other => panic!("unexpected queue: {other:?}"),
    }
}

#[test]
fn the_link_switch_does_not_move_until_the_server_answers() {
    let state = state_with(Rights::ADMIN, 0, 0);
    let mut share = state.editor_ui.share.clone();
    share.pending.clear();
    assert!(!share.link_enabled);
    apply_share_hit(&mut share, ShareDialogHit::LinkAccess, None);
    assert!(
        !share.link_enabled,
        "the switch claimed a state the document does not have"
    );
    assert!(matches!(
        share.pending.as_slice(),
        [ShareAction::SetLinkAccess { enabled: true, .. }]
    ));
    // The host's confirmation is what moves it.
    share.link_enabled = true;
    assert!(share.link_enabled);
}

#[test]
fn copy_link_and_the_session_row_queue_host_work() {
    let state = state_with(Rights::ADMIN, 0, 0);
    let mut share = state.editor_ui.share.clone();
    share.pending.clear();
    apply_share_hit(&mut share, ShareDialogHit::CopyLink, None);
    assert_eq!(share.pending, vec![ShareAction::CopyLink]);

    share.pending.clear();
    apply_share_hit(&mut share, ShareDialogHit::OpenSession, None);
    assert_eq!(share.pending, vec![ShareAction::OpenSession]);
    assert!(
        !share.open,
        "the panel is a different surface, not an overlay"
    );
}

#[test]
fn a_press_on_the_scrim_closes_the_picker_and_not_the_dialog() {
    let mut state = state_with(Rights::ADMIN, 0, 0);
    state
        .editor_ui
        .share
        .invite_input
        .set_text("half-typed@example.test");
    let mut share = state.editor_ui.share.clone();
    apply_share_hit(&mut share, ShareDialogHit::LinkLevel, None);
    assert!(share.level_picker.is_some());
    apply_share_hit(&mut share, ShareDialogHit::Outside, None);
    assert_eq!(share.level_picker, None);
    assert!(share.open);
    assert_eq!(share.invite_input.text(), "half-typed@example.test");
}

#[test]
fn the_close_control_closes_and_drops_the_notice() {
    let state = state_with(Rights::ADMIN, 0, 1);
    let mut share = state.editor_ui.share.clone();
    assert!(share.notice.is_some());
    apply_share_hit(&mut share, ShareDialogHit::Close, None);
    assert!(!share.open);
    assert!(share.notice.is_none());
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

#[test]
fn the_invite_field_owns_the_keys_only_while_it_is_focused() {
    let state = state_with(Rights::ADMIN, 0, 0);
    let mut share = state.editor_ui.share.clone();
    assert_eq!(invite_field_text(&mut share, 'a', 0), None);
    apply_share_hit(&mut share, ShareDialogHit::InviteField, None);
    assert!(share.invite_focused);
    assert_eq!(invite_field_text(&mut share, 'a', 0), Some(true));
    assert_eq!(share.invite_input.text(), "a");
    // Whitespace, commas and the punctuation an address uses all type.
    for character in ["@", ".", ",", "-", "+", "_", " "] {
        assert!(invite_char_allowed(
            character.chars().next().expect("a char")
        ));
    }
    assert_eq!(invite_field_text(&mut share, '<', 0), Some(false));
    assert_eq!(invite_field_text(&mut share, '\n', 0), Some(false));
}

#[test]
fn backspace_and_paste_edit_the_field_and_clear_a_stale_notice() {
    let mut state = state_with(Rights::ADMIN, 0, 0);
    state.editor_ui.share.invite_focused = true;
    let mut share = state.editor_ui.share.clone();
    share.notice = Some(ShareNotice::CopiedLink);
    assert_eq!(
        invite_field_paste(&mut share, "ada@example.test", 1),
        Some(true)
    );
    assert_eq!(share.invite_input.text(), "ada@example.test");
    assert_eq!(share.notice, None, "a stale notice outlived the edit");
    assert_eq!(invite_field_backspace(&mut share, 2), Some(true));
    assert_eq!(
        share.invite_input.text().len(),
        "ada@example.test".len() - 1
    );
}

#[test]
fn enter_in_the_field_is_the_button() {
    let mut state = state_with(Rights::ADMIN, 0, 0);
    state.editor_ui.share.invite_focused = true;
    state.editor_ui.share.invite_input.set_text("userB");
    let mut share = state.editor_ui.share.clone();
    share.pending.clear();
    assert_eq!(invite_field_submit(&mut share), Some(true));
    assert_eq!(
        share.pending,
        vec![ShareAction::Grant {
            account: "userB".to_string(),
            level: ShareLevel::Viewer,
        }]
    );
    // …and it refuses the same way the button does.
    let mut refused = state.editor_ui.share.clone();
    refused.own_rights = Rights::VIEW_ONLY;
    refused.pending.clear();
    assert_eq!(invite_field_submit(&mut refused), Some(true));
    assert!(refused.pending.is_empty());
    assert!(matches!(refused.notice, Some(ShareNotice::Refused(_))));
}
