//! Issuing an invitation: the link, and the one time the token exists.
//!
//! Fixtures are in the parent module; `use super::*` brings them in.

use super::*;

// ---------------------------------------------------------------------------
// Issuing: the link, and the fact that the token exists exactly once
// ---------------------------------------------------------------------------

#[test]
fn an_administrator_issues_a_link_that_acceptance_recognises() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);

    let reply = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&["qa"])),
    );
    assert_eq!(reply.status, "201 Created", "{}", reply.body);
    let body = json(&reply);
    assert_eq!(body["ok"], true);
    assert_eq!(body["roles"][0], "qa");
    let token = issued_token(&reply);
    // A link the browser shell's own parser accepts: the path is built from
    // the shared spelling rather than assembled here.
    let path = body["path"].as_str().expect("a path");
    assert_eq!(path, route::to_invite_path(&token));
    assert_eq!(route::invite_token(path), Some(token.as_str()));
}

#[test]
fn the_token_is_handed_over_once_and_the_database_holds_only_its_hash() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let created = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    let token = issued_token(&created);

    // Nowhere else. The listing is the surface that comes closest, and it is
    // deliberately unable to print a link.
    let listed = handle(&auth, &as_account(&operator, "GET", INVITES, ""));
    assert!(
        !listed.body.contains(&token),
        "the listing must not hand the link back: {}",
        listed.body
    );

    // And the row the listing names is the hash of that token, not the token.
    let id = json(&created)["id"].as_str().expect("an id").to_string();
    assert_eq!(
        id,
        crate::accounts::hash_hex(&crate::accounts::hash_token(&token)),
        "the id is the stored hash in hex"
    );
    let row = auth
        .db()
        .list_invites(10, 0)
        .expect("list")
        .into_iter()
        .find(|listed| listed.id == id)
        .expect("the invitation was written");
    assert!(row.invite.is_redeemable_at(crate::accounts::now_secs()));
}

#[test]
fn an_invitation_expires_when_the_store_says_it_does() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let reply = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    let now = crate::accounts::now_secs();
    assert_eq!(
        json(&reply)["expires_at"].as_i64(),
        Some(now + INVITE_TTL_SECS),
        "the lifetime is the store's policy, not a number this route picked"
    );
}

#[test]
fn an_administrator_may_invite_an_administrator() {
    // Allowed, and deliberately so. The inviter already holds the account
    // list, so handing out the same right is not an escalation — they could
    // reach the same state through `set_roles` a moment later. Refusing it
    // would in fact make a second administrator impossible: `op admin create`
    // only ever makes the FIRST one and refuses a store that already has an
    // account, and the `NORKA_ADMIN_*` variables are ignored for the same
    // reason. So the deployment's second administrator has to come from here.
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);

    let reply = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&["admin"])),
    );
    assert_eq!(reply.status, "201 Created", "{}", reply.body);

    // And the role really arrives: a fresh account accepting this link holds
    // the account list the moment it exists.
    let accepted = handle(
        &auth,
        &request(
            "POST",
            op_editor_core::auth_routes::INVITE_ACCEPT,
            &serde_json::json!({
                "token": issued_token(&reply),
                "username": "second-operator",
                "password": PASSWORD,
            })
            .to_string(),
        ),
    );
    assert_eq!(accepted.status, "200 OK", "{}", accepted.body);
    let created = auth
        .db()
        .find_user_by_username("second-operator")
        .expect("look up")
        .expect("the account exists");
    assert_eq!(created.roles, vec!["admin".to_string()]);
    assert!(auth
        .verifier()
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some(
                accepted
                    .cookies
                    .first()
                    .expect("a session cookie")
                    .split(';')
                    .next()
                    .and_then(|pair| pair.split_once('='))
                    .map(|(_, value)| value.to_string())
                    .expect("a cookie value")
            ),
        })
        .expect("the new session resolves")
        .roles
        .rights()
        .can_manage_users());
}

#[test]
fn an_invitation_may_grant_no_roles_at_all() {
    // A guest invitation: the link admits somebody who may read what they are
    // given and change nothing. Refusing the empty list would make that
    // impossible to express.
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let reply = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    assert_eq!(reply.status, "201 Created", "{}", reply.body);
    assert_eq!(json(&reply)["invite"]["roles"], serde_json::json!([]));
}

#[test]
fn a_role_this_build_does_not_have_is_refused_before_anything_is_written() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let reply = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&["superuser"])),
    );
    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(json(&reply)["error"], "unknown-role");
    let message = json(&reply)["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(message.contains("superuser"), "{message}");
    assert_eq!(auth.db().list_invites(10, 0).expect("list").len(), 0);
}

#[test]
fn the_roles_an_invitation_carries_reach_the_account_it_makes() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    // An alias, on purpose: what an operator types is folded onto the wire
    // spelling before it is stored, so the invitation carries one role and not
    // four spellings of one.
    let created = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&["UX/UI", "qa"])),
    );
    assert_eq!(created.status, "201 Created", "{}", created.body);

    let accepted = handle(
        &auth,
        &request(
            "POST",
            op_editor_core::auth_routes::INVITE_ACCEPT,
            &serde_json::json!({
                "token": issued_token(&created),
                "username": "newcomer",
                "password": PASSWORD,
            })
            .to_string(),
        ),
    );
    assert_eq!(accepted.status, "200 OK", "{}", accepted.body);
    let created_user = auth
        .db()
        .find_user_by_username("newcomer")
        .expect("look up")
        .expect("the account exists");
    assert_eq!(created_user.status, UserStatus::Active);
    assert_eq!(
        created_user.roles,
        vec!["ux_ui".to_string(), "qa".to_string()]
    );
    // Which is what makes the roles worth carrying: they are the identity the
    // routes read.
    assert!(op_editor_core::access::RoleSet::from_wire(
        created_user.roles.iter().map(String::as_str)
    )
    .rights()
    .can_edit());
}

#[test]
fn an_issued_invitation_cannot_be_accepted_twice() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let created = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    let token = issued_token(&created);
    let accept = |username: &str| {
        handle(
            &auth,
            &request(
                "POST",
                op_editor_core::auth_routes::INVITE_ACCEPT,
                &serde_json::json!({
                    "token": token,
                    "username": username,
                    "password": PASSWORD,
                })
                .to_string(),
            ),
        )
    };
    assert_eq!(accept("newcomer").status, "200 OK");
    let second = accept("someone-else");
    assert_eq!(second.status, "409 Conflict");
    assert_eq!(json(&second)["error"], "invite-already-accepted");
    // One link, one account.
    assert!(auth
        .db()
        .find_user_by_username("someone-else")
        .expect("look up")
        .is_none());
}

#[test]
fn an_invitation_an_administrator_withdrew_can_no_longer_be_accepted() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let created = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    let id = json(&created)["id"].as_str().expect("an id").to_string();

    let revoked = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            INVITE_REVOKE,
            &serde_json::json!({ "id": id }).to_string(),
        ),
    );
    assert_eq!(revoked.status, "200 OK", "{}", revoked.body);
    assert_eq!(json(&revoked)["revoked"], true);

    let accepted = handle(
        &auth,
        &request(
            "POST",
            op_editor_core::auth_routes::INVITE_ACCEPT,
            &serde_json::json!({
                "token": issued_token(&created),
                "username": "newcomer",
                "password": PASSWORD,
            })
            .to_string(),
        ),
    );
    assert_eq!(accepted.status, "404 Not Found");
    assert_eq!(json(&accepted)["error"], "invite-not-found");
    assert_eq!(auth.db().count_users().expect("count"), 1);
}

#[test]
fn an_expired_invitation_can_no_longer_be_accepted() {
    // Issued already expired, through the store: the acceptance path is the
    // only place the person holding a stale link finds out, and this proves
    // the link an administrator issued is subject to the same predicate.
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let expired = auth
        .db()
        .create_invite(
            &NewInvite::new(&[], Some(&operator.id), -1),
            crate::accounts::now_secs(),
        )
        .expect("issue an invitation that is already past its date");

    let accepted = handle(
        &auth,
        &request(
            "POST",
            op_editor_core::auth_routes::INVITE_ACCEPT,
            &serde_json::json!({
                "token": expired.token,
                "username": "newcomer",
                "password": PASSWORD,
            })
            .to_string(),
        ),
    );
    assert_eq!(accepted.status, "410 Gone");
    assert_eq!(json(&accepted)["error"], "invite-expired");
}
