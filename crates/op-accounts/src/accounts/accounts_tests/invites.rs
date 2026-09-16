//! Invitations as links: issuing one, spending it once, and what survives the
//! accounts involved.

use super::*;
use crate::accounts::{hash_hex, hash_token, AccountsError, InviteWithdrawal, NewInvite};

#[test]
fn an_invite_is_found_by_its_token() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    let issued = db
        .create_invite(
            &NewInvite::new(&["editor"], Some("u1"), 86_400).with_email("new@example.com"),
            NOW,
        )
        .expect("issue");

    assert_eq!(
        db.find_invite(&issued.token).expect("find"),
        Some(issued.invite.clone())
    );
    assert_eq!(issued.invite.email.as_deref(), Some("new@example.com"));
    assert_eq!(issued.invite.roles, vec!["editor".to_string()]);
    assert_eq!(issued.invite.created_by.as_deref(), Some("u1"));
    assert_eq!(issued.invite.created_at, NOW);
    assert_eq!(issued.invite.expires_at, NOW + 86_400);
    assert_eq!(issued.invite.accepted_by, None);
    assert_eq!(issued.invite.accepted_at, None);
    assert!(issued.invite.is_redeemable_at(NOW));
}

#[test]
fn the_database_holds_the_hash_of_the_invite_and_not_the_link() {
    let (dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");

    assert_eq!(stored_invite_hash(&db), hash_token(&issued.token).to_vec());
    assert!(
        !appears_in(&file_bytes(&dir), &issued.token),
        "an invite table that leaked is a list of invitations nobody can accept"
    );
}

#[test]
fn redeeming_an_invite_records_who_and_when() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let issued = db
        .create_invite(&NewInvite::new(&["editor"], Some("u1"), 86_400), NOW)
        .expect("issue");

    let redeemed = db
        .redeem_invite(&issued.token, "u2", NOW + 60)
        .expect("redeem");

    assert_eq!(redeemed.accepted_by.as_deref(), Some("u2"));
    assert_eq!(redeemed.accepted_at, Some(NOW + 60));
    assert!(!redeemed.is_redeemable_at(NOW + 61));
    assert_eq!(
        db.find_invite(&issued.token).expect("find"),
        Some(redeemed),
        "spent invites are still readable: an operator asking what happened gets an answer"
    );
}

#[test]
fn an_invite_cannot_be_redeemed_twice() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    invited_user(&db, "u3", "carol");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    db.redeem_invite(&issued.token, "u2", NOW + 60)
        .expect("redeem");

    assert_eq!(
        db.redeem_invite(&issued.token, "u3", NOW + 61)
            .expect_err("a second acceptance"),
        AccountsError::InviteAlreadyAccepted,
        "one link makes one account"
    );
    assert_eq!(
        db.find_invite(&issued.token)
            .expect("find")
            .expect("a row")
            .accepted_by
            .as_deref(),
        Some("u2"),
        "and the second attempt did not overwrite who accepted it"
    );
}

#[test]
fn an_expired_invite_cannot_be_redeemed() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 3600), NOW)
        .expect("issue");

    assert_eq!(
        db.redeem_invite(&issued.token, "u2", NOW + 3600)
            .expect_err("past the window"),
        AccountsError::InviteExpired
    );
}

#[test]
fn an_unknown_token_is_not_an_invite() {
    let (_dir, db) = store();

    assert_eq!(db.find_invite("not-a-token").expect("find"), None);
    assert_eq!(
        db.redeem_invite("not-a-token", "u2", NOW)
            .expect_err("nothing to redeem"),
        AccountsError::InviteNotFound
    );
}

#[test]
fn an_invite_with_no_address_and_no_inviter_is_still_an_invite() {
    // This product sends no mail: a link handed over by hand is the normal
    // case, and an operator who knows neither the address nor which account is
    // issuing it must still be able to create one.
    let (_dir, db) = store();
    invited_user(&db, "u2", "bob");

    let issued = db
        .create_invite(&NewInvite::new(&["admin"], None, 86_400), NOW)
        .expect("issue");

    assert_eq!(issued.invite.email, None);
    assert_eq!(issued.invite.created_by, None);
    assert_eq!(
        db.redeem_invite(&issued.token, "u2", NOW + 1)
            .expect("redeem")
            .accepted_by
            .as_deref(),
        Some("u2")
    );
}

#[test]
fn an_invite_outlives_the_account_that_issued_it() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");

    db.delete_user("u1").expect("delete the inviter");

    let invite = db
        .find_invite(&issued.token)
        .expect("find")
        .expect("the invite is still there");
    assert_eq!(
        invite.created_by, None,
        "the id goes with the account, and the record of the invitation does not"
    );
    // A link already in somebody's hands must still work: the inviter leaving
    // is not the recipient's fault.
    assert!(db.redeem_invite(&issued.token, "u2", NOW + 60).is_ok());
}

#[test]
fn a_spent_invite_stays_spent_after_the_accepting_account_is_deleted() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    db.redeem_invite(&issued.token, "u2", NOW + 60)
        .expect("redeem");

    db.delete_user("u2").expect("delete the accepted account");

    let invite = db
        .find_invite(&issued.token)
        .expect("find")
        .expect("the invite is still there");
    assert_eq!(invite.accepted_by, None, "the id goes with the account");
    assert_eq!(
        invite.accepted_at,
        Some(NOW + 60),
        "the fact that the link was used is not the account's to take with it"
    );
    assert_eq!(
        db.redeem_invite(&issued.token, "u1", NOW + 120)
            .expect_err("the link is still spent"),
        AccountsError::InviteAlreadyAccepted,
        "reading `accepted_by` instead of `accepted_at` here would hand the link back"
    );
}

#[test]
fn a_withdrawn_invite_is_gone_and_an_accepted_one_is_not_withdrawable() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let unused = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    let used = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    db.redeem_invite(&used.token, "u2", NOW + 1)
        .expect("redeem");

    assert!(db.revoke_invite(&unused.token).expect("withdraw"));
    assert_eq!(db.find_invite(&unused.token).expect("find"), None);
    assert!(
        !db.revoke_invite(&used.token)
            .expect("withdraw a spent invite"),
        "withdrawing cannot un-create the account the link made"
    );
    assert!(db.find_invite(&used.token).expect("find").is_some());
}

#[test]
fn an_invite_naming_an_account_that_does_not_exist_is_refused() {
    let (_dir, db) = store();
    invited_user(&db, "u2", "bob");

    assert_eq!(
        db.create_invite(&NewInvite::new(&[], Some("nobody"), 86_400), NOW)
            .expect_err("no such inviter"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u2"), 86_400), NOW)
        .expect("issue");
    assert_eq!(
        db.redeem_invite(&issued.token, "nobody", NOW)
            .expect_err("no such accepting account"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
    assert_eq!(
        db.find_invite(&issued.token)
            .expect("find")
            .expect("a row")
            .accepted_at,
        None,
        "a refused acceptance must not have spent the link"
    );
}

#[test]
fn an_invitation_is_named_by_its_stored_hash_and_never_by_its_token() {
    // The listing is the only surface that has to NAME a row, and the name it
    // has is the hash: one-way, useless as a credential on every path this
    // product has, and enough for an operator to act on the row they are
    // looking at.
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .create_invite(&NewInvite::new(&["qa"], Some("u1"), 86_400), NOW)
        .expect("issue");

    assert_eq!(
        issued.id,
        hash_hex(&hash_token(&issued.token)),
        "the issuance answer and the listing name the row the same way"
    );
    let listed = db.list_invites(10, 0).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, issued.id);
    assert_eq!(listed[0].invite, issued.invite);
    // And never the token, on any of it.
    assert!(!listed[0].id.contains(&issued.token));
}

#[test]
fn the_listing_is_newest_first_and_paged() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let mut ids = Vec::new();
    for offset in 0..3 {
        ids.push(
            db.create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW + offset)
                .expect("issue")
                .id,
        );
    }

    let all = db.list_invites(10, 0).expect("list");
    assert_eq!(all.len(), 3);
    assert_eq!(
        all.iter()
            .map(|listed| listed.id.clone())
            .collect::<Vec<_>>(),
        vec![ids[2].clone(), ids[1].clone(), ids[0].clone()],
        "the operator is looking for what they just sent"
    );
    // A page is a continuation: the same order, starting where the last one
    // stopped.
    let page = db.list_invites(1, 1).expect("page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, ids[1]);
    assert_eq!(db.list_invites(1, 99).expect("past the end").len(), 0);
}

#[test]
fn a_spent_or_expired_invitation_is_listed_rather_than_hidden() {
    // The read an operator makes when a link "does not work". A query that
    // answered "no such invitation" for one that was used last week would be a
    // query that hides the answer.
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let spent = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    db.redeem_invite(&spent.token, "u2", NOW + 60)
        .expect("redeem");
    let stale = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 60), NOW)
        .expect("issue");

    let listed = db.list_invites(10, 0).expect("list");
    assert_eq!(listed.len(), 2);
    let accepted = db.find_invite(&spent.token).expect("find").expect("a row");
    assert_eq!(accepted.accepted_at, Some(NOW + 60));
    assert!(!accepted.is_redeemable_at(NOW + 61));
    let expired = db.find_invite(&stale.token).expect("find").expect("a row");
    assert!(!expired.is_redeemable_at(NOW + 61));
    assert!(listed.iter().any(|row| row.id == stale.id));
}

#[test]
fn an_invitation_is_withdrawn_by_the_id_a_listing_gives() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");

    assert_eq!(
        db.revoke_invite_by_id(&issued.id).expect("withdraw"),
        InviteWithdrawal::Revoked
    );
    assert_eq!(db.find_invite(&issued.token).expect("find"), None);
    // Idempotent from the operator's side: asking twice is a list that is out
    // of date, not a fault.
    assert_eq!(
        db.revoke_invite_by_id(&issued.id).expect("withdraw again"),
        InviteWithdrawal::NotFound
    );
}

#[test]
fn the_two_ways_of_withdrawing_reach_the_same_row_and_the_same_rule() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");
    let unused = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    let used = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");
    db.redeem_invite(&used.token, "u2", NOW + 1)
        .expect("redeem");

    assert_eq!(
        db.revoke_invite_by_id(&unused.id).expect("withdraw"),
        InviteWithdrawal::Revoked
    );
    // The rule is the store's, and both doors lead to it: an accepted
    // invitation is not withdrawable, because doing so cannot un-create the
    // account it made.
    assert_eq!(
        db.revoke_invite_by_id(&used.id)
            .expect("withdraw a spent invite"),
        InviteWithdrawal::AlreadyAccepted
    );
    assert!(!db.revoke_invite(&used.token).expect("withdraw by token"));
    assert!(db.find_invite(&used.token).expect("find").is_some());
}

#[test]
fn an_id_that_is_not_a_hash_is_no_such_invitation() {
    // A token pasted where an id was asked for, a truncated copy, an id from
    // some other system: all of them are "nothing by that name", which is the
    // answer the caller can act on, rather than a malformed-request error they
    // cannot.
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");

    for not_an_id in [
        "",
        "not-an-id",
        &issued.token,
        &issued.id[..32],
        &format!("{}0", issued.id),
        &format!("g{}", &issued.id[1..]),
    ] {
        assert_eq!(
            db.revoke_invite_by_id(not_an_id).expect("withdraw"),
            InviteWithdrawal::NotFound,
            "{not_an_id}"
        );
    }
    // The row is untouched by any of them.
    assert!(db.find_invite(&issued.token).expect("find").is_some());
}

#[test]
fn an_id_is_read_whatever_case_it_was_copied_in() {
    // An operator copying an id out of a listing by hand is the case this
    // tolerates: hex has one meaning in either case, and refusing the capital
    // form would be refusing the same value.
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let issued = db
        .create_invite(&NewInvite::new(&[], Some("u1"), 86_400), NOW)
        .expect("issue");

    assert_eq!(
        db.revoke_invite_by_id(&issued.id.to_uppercase())
            .expect("withdraw"),
        InviteWithdrawal::Revoked
    );
}
