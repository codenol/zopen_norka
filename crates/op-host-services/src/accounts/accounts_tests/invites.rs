//! Invitations as links: issuing one, spending it once, and what survives the
//! accounts involved.

use super::*;
use crate::accounts::{hash_token, AccountsError, NewInvite};

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
