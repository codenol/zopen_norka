//! The two listings: what was handed out, and who is in the deployment.
//!
//! Fixtures are in the parent module; `use super::*` brings them in.

use super::*;

// ---------------------------------------------------------------------------
// The listings
// ---------------------------------------------------------------------------

#[test]
fn the_invitation_list_says_what_happened_to_each_link() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let pending = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&["qa"])),
    );
    let used = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    let accepted = handle(
        &auth,
        &request(
            "POST",
            op_editor_core::auth_routes::INVITE_ACCEPT,
            &serde_json::json!({
                "token": issued_token(&used),
                "username": "newcomer",
                "password": PASSWORD,
            })
            .to_string(),
        ),
    );
    assert_eq!(accepted.status, "200 OK", "{}", accepted.body);
    let newcomer = auth
        .db()
        .find_user_by_username("newcomer")
        .expect("look up")
        .expect("the account exists");

    let listed = handle(&auth, &as_account(&operator, "GET", INVITES, ""));
    assert_eq!(listed.status, "200 OK");
    let body = json(&listed);
    let invites = body["invites"].as_array().expect("a list of invitations");
    assert_eq!(invites.len(), 2);
    let by_id = |id: &str| {
        invites
            .iter()
            .find(|invite| invite["id"] == id)
            .unwrap_or_else(|| panic!("no invitation with id {id}: {invites:?}"))
            .clone()
    };

    // The pending one, named by the id issuance gave it, so a listing row and
    // the answer that created it agree about which row they mean.
    let still_open = by_id(json(&pending)["id"].as_str().expect("an id"));
    assert_eq!(still_open["state"], "pending");
    assert_eq!(still_open["roles"][0], "qa");
    assert_eq!(still_open["created_by"], operator.id);
    assert!(still_open["accepted_at"].is_null());
    assert!(still_open["accepted_by"].is_null());

    // The spent one: who used it, and when. This is the answer to "the person
    // says the link does not work".
    let spent = by_id(json(&used)["id"].as_str().expect("an id"));
    assert_eq!(spent["state"], "accepted");
    assert_eq!(spent["accepted_by"], newcomer.id);
    assert!(spent["accepted_at"].as_i64().is_some());
}

#[test]
fn an_expired_link_is_listed_as_expired_rather_than_pending() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    auth.db()
        .create_invite(
            &NewInvite::new(&[], Some(&operator.id), -1),
            crate::accounts::now_secs(),
        )
        .expect("issue an invitation that is already past its date");

    let listed = handle(&auth, &as_account(&operator, "GET", INVITES, ""));
    let invites = json(&listed)["invites"].clone();
    let invites = invites.as_array().expect("a list of invitations");
    assert_eq!(invites.len(), 1);
    assert_eq!(
        invites[0]["state"], "expired",
        "a link nobody can accept must not read as outstanding: {invites:?}"
    );
}

#[test]
fn a_listing_reads_the_page_it_was_asked_for() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    for _ in 0..3 {
        handle(
            &auth,
            &as_account(&operator, "POST", INVITES, &issue_body(&[])),
        );
    }
    let mut paged = as_account(&operator, "GET", INVITES, "");
    paged.query = Some("limit=2".into());
    assert_eq!(
        json(&handle(&auth, &paged))["invites"]
            .as_array()
            .expect("a page")
            .len(),
        2
    );
    let mut second = as_account(&operator, "GET", INVITES, "");
    second.query = Some("limit=2&offset=2".into());
    assert_eq!(
        json(&handle(&auth, &second))["invites"]
            .as_array()
            .expect("a page")
            .len(),
        1,
        "the second page is a continuation, not a re-shuffle"
    );
}

#[test]
fn the_account_list_shows_who_is_here_and_when_they_were_last_seen() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let contributor = account(&auth, "contributor", &["qa"]);
    auth.db()
        .touch_last_seen(&contributor.id, crate::accounts::now_secs())
        .expect("record a sighting");

    let listed = handle(&auth, &as_account(&operator, "GET", USERS, ""));
    assert_eq!(listed.status, "200 OK");
    let users = json(&listed)["users"].clone();
    let users = users.as_array().expect("a list of accounts");
    assert_eq!(users.len(), 2);
    let row = users
        .iter()
        .find(|user| user["username"] == "contributor")
        .expect("the contributor");
    assert_eq!(row["id"], contributor.id);
    assert_eq!(row["roles"][0], "qa");
    assert_eq!(row["status"], "active");
    assert_eq!(row["has_password"], true);
    assert!(
        row["last_seen_at"].as_i64().is_some(),
        "when an account was last seen is the difference between unused and unfinished: {row}"
    );
    // And nothing about the credential ever reaches a listing.
    assert!(
        !listed.body.contains("password_hash") && !listed.body.contains("$argon2"),
        "{}",
        listed.body
    );
}
