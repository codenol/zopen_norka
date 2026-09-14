//! Changing an account: roles, status, and what must not be changed by its own holder.
//!
//! Fixtures are in the parent module; `use super::*` brings them in.

use super::*;

// ---------------------------------------------------------------------------
// Changing an account
// ---------------------------------------------------------------------------

#[test]
fn an_administrator_reroles_an_account() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let contributor = account(&auth, "contributor", &["qa"]);

    let reply = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            USER_ROLES,
            &serde_json::json!({ "id": contributor.id, "roles": ["ux_ui", "admin"] }).to_string(),
        ),
    );
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    assert_eq!(
        json(&reply)["user"]["roles"],
        serde_json::json!(["ux_ui", "admin"]),
        "the answer is the row as the store now holds it"
    );
    assert_eq!(
        auth.db()
            .find_user_by_id(&contributor.id)
            .expect("look up")
            .expect("the account")
            .roles,
        vec!["ux_ui".to_string(), "admin".to_string()]
    );
}

#[test]
fn an_administrator_takes_every_role_away_with_an_empty_list() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let contributor = account(&auth, "contributor", &["qa"]);
    let reply = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            USER_ROLES,
            &serde_json::json!({ "id": contributor.id, "roles": [] }).to_string(),
        ),
    );
    assert_eq!(reply.status, "200 OK", "{}", reply.body);
    assert_eq!(json(&reply)["user"]["roles"], serde_json::json!([]));
}

#[test]
fn an_administrator_disables_and_reenables_an_account() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let contributor = sign_in(&auth, "contributor", &["qa"]);
    let change = |status: &str| {
        handle(
            &auth,
            &as_account(
                &operator,
                "POST",
                USER_STATUS,
                &serde_json::json!({ "id": contributor.id, "status": status }).to_string(),
            ),
        )
    };

    let disabled = change("disabled");
    assert_eq!(disabled.status, "200 OK", "{}", disabled.body);
    assert_eq!(json(&disabled)["user"]["status"], "disabled");
    // Disabling is a decision and not a label: the credential that account
    // already holds stops opening anything. The session ROW survives — the
    // store records state and does not interpret it, see `crate::accounts` —
    // and the answer is one layer up, where `UserStatus::may_sign_in` is
    // asked. That is the layer a request actually passes through.
    assert!(auth
        .verifier()
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some(contributor.token.clone()),
        })
        .is_err());

    assert_eq!(json(&change("active"))["user"]["status"], "active");
    // And re-enabling gives the same session back: nothing was thrown away.
    assert!(auth
        .verifier()
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some(contributor.token.clone()),
        })
        .is_ok());
}

#[test]
fn a_status_an_operator_does_not_set_is_refused_and_named() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let contributor = account(&auth, "contributor", &["qa"]);
    for status in ["invited", "orphan", "nonsense"] {
        let reply = handle(
            &auth,
            &as_account(
                &operator,
                "POST",
                USER_STATUS,
                &serde_json::json!({ "id": contributor.id, "status": status }).to_string(),
            ),
        );
        assert_eq!(reply.status, "400 Bad Request", "{status}");
        assert_eq!(json(&reply)["error"], "unsupported-status", "{status}");
        let message = json(&reply)["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(message.contains("active"), "{status}: {message}");
        assert!(message.contains("disabled"), "{status}: {message}");
    }
    assert_eq!(
        auth.db()
            .find_user_by_id(&contributor.id)
            .expect("look up")
            .expect("the account")
            .status,
        UserStatus::Active
    );
}

#[test]
fn an_administrator_cannot_lock_the_deployment_out_through_their_own_account() {
    // The one edit with no way back. `op admin create` only ever makes the
    // FIRST administrator and refuses a store that already has an account, and
    // the `NORKA_ADMIN_*` variables are ignored for the same reason — so a
    // deployment whose only administrator disables themselves, or drops their
    // own account list, has no route at all to a working one.
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);

    let disabled = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            USER_STATUS,
            &serde_json::json!({ "id": operator.id, "status": "disabled" }).to_string(),
        ),
    );
    assert_eq!(disabled.status, "409 Conflict", "{}", disabled.body);
    assert_eq!(json(&disabled)["error"], "self-lockout-refused");
    assert_eq!(
        auth.db()
            .find_user_by_id(&operator.id)
            .expect("look up")
            .expect("the account")
            .status,
        UserStatus::Active
    );

    let demoted = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            USER_ROLES,
            &serde_json::json!({ "id": operator.id, "roles": ["qa"] }).to_string(),
        ),
    );
    assert_eq!(demoted.status, "409 Conflict", "{}", demoted.body);
    assert_eq!(json(&demoted)["error"], "self-lockout-refused");

    // What IS allowed is a change that keeps the account list: adding a role
    // beside `admin` takes nothing away.
    let kept = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            USER_ROLES,
            &serde_json::json!({ "id": operator.id, "roles": ["admin", "qa"] }).to_string(),
        ),
    );
    assert_eq!(kept.status, "200 OK", "{}", kept.body);

    // And another administrator can still make the change that removes the
    // right, so the rule narrows self-service rather than freezing the list.
    let second = sign_in(&auth, "second", &["admin"]);
    let demoted = handle(
        &auth,
        &as_account(
            &second,
            "POST",
            USER_ROLES,
            &serde_json::json!({ "id": operator.id, "roles": ["qa"] }).to_string(),
        ),
    );
    assert_eq!(demoted.status, "200 OK", "{}", demoted.body);
    assert_eq!(json(&demoted)["user"]["roles"], serde_json::json!(["qa"]));
}

#[test]
fn revoking_an_accepted_invitation_is_refused_with_the_reason() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    let created = handle(
        &auth,
        &as_account(&operator, "POST", INVITES, &issue_body(&[])),
    );
    let id = json(&created)["id"].as_str().expect("an id").to_string();
    handle(
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

    let reply = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            INVITE_REVOKE,
            &serde_json::json!({ "id": id }).to_string(),
        ),
    );
    assert_eq!(reply.status, "409 Conflict", "{}", reply.body);
    assert_eq!(json(&reply)["error"], "invite-already-accepted");
    // The record of how the account came to exist survives, and the link stays
    // spent.
    assert_eq!(auth.db().list_invites(10, 0).expect("list").len(), 1);
    assert_eq!(
        auth.db()
            .find_user_by_username("newcomer")
            .expect("look up")
            .expect("the account")
            .status,
        UserStatus::Active
    );
}

#[test]
fn withdrawing_an_unknown_or_malformed_id_is_answered_rather_than_guessed_at() {
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    for id in ["", "not-an-id", "ab", "zzzz"] {
        let reply = handle(
            &auth,
            &as_account(
                &operator,
                "POST",
                INVITE_REVOKE,
                &serde_json::json!({ "id": id }).to_string(),
            ),
        );
        // An empty `id` never gets this far — `required_text` refuses it as a
        // malformed body — and the rest are ids this deployment holds nothing
        // under.
        assert!(
            reply.status == "404 Not Found" || reply.status == "400 Bad Request",
            "{id:?} -> {} {}",
            reply.status,
            reply.body
        );
    }
    // A well-formed id for a row that does not exist, including the token
    // pasted by mistake instead of the id.
    let wrong = crate::accounts::hash_hex(&crate::accounts::hash_token("some-token"));
    let reply = handle(
        &auth,
        &as_account(
            &operator,
            "POST",
            INVITE_REVOKE,
            &serde_json::json!({ "id": wrong }).to_string(),
        ),
    );
    assert_eq!(reply.status, "404 Not Found", "{}", reply.body);
    assert_eq!(json(&reply)["error"], "invite-not-found");
}
