//! Who may ask, and the gates that stand in front of every one of these routes.
//!
//! Fixtures are in the parent module; `use super::*` brings them in.

use super::*;

// ---------------------------------------------------------------------------
// Who may ask
// ---------------------------------------------------------------------------

#[test]
fn a_signed_in_account_without_the_account_list_is_refused_on_every_route() {
    let (_dir, auth) = deployment();
    sign_in(&auth, "operator", &["admin"]);
    let contributor = sign_in(&auth, "contributor", &["qa"]);

    // Every role that is not an administrator, including the one that may
    // edit everything: UX/UI reaches every document and not the account list.
    for (index, roles) in [
        &["ux_ui"][..],
        &["qa"][..],
        &["analyst"][..],
        &["software", "frontend"][..],
        &[][..],
    ]
    .into_iter()
    .enumerate()
    {
        let person = sign_in(&auth, &format!("person-{index}"), roles);
        for (method, path, body) in admin_requests() {
            let reply = handle(&auth, &as_account(&person, method, path, &body));
            assert_eq!(reply.status, "403 Forbidden", "{roles:?} {method} {path}");
            assert_eq!(
                json(&reply)["error"],
                "admin-role-required",
                "{roles:?} {method} {path}"
            );
        }
    }
    // And the same refusal for the account that may not manage users, with its
    // own cookie, asserted separately so the loop above cannot be read as
    // proving something about an anonymous caller.
    let reply = handle(&auth, &as_account(&contributor, "GET", USERS, ""));
    assert_eq!(reply.status, "403 Forbidden");
}

/// Every administration route, with a body that would otherwise be accepted.
fn admin_requests() -> Vec<(&'static str, &'static str, String)> {
    vec![
        ("POST", INVITES, issue_body(&[])),
        ("GET", INVITES, String::new()),
        ("POST", INVITE_REVOKE, r#"{"id":"x"}"#.to_string()),
        ("GET", USERS, String::new()),
        ("POST", USER_ROLES, r#"{"id":"u_x","roles":[]}"#.to_string()),
        (
            "POST",
            USER_STATUS,
            r#"{"id":"u_x","status":"disabled"}"#.to_string(),
        ),
    ]
}

#[test]
fn a_caller_with_no_session_is_told_to_sign_in_and_not_that_it_lacks_a_role() {
    let (_dir, auth) = deployment();
    sign_in(&auth, "operator", &["admin"]);
    for (method, path, body) in admin_requests() {
        let reply = handle(&auth, &request(method, path, &body));
        assert_eq!(reply.status, "401 Unauthorized", "{method} {path}");
        assert_eq!(json(&reply)["error"], "unauthorized", "{method} {path}");
    }
}

#[test]
fn a_credential_that_never_resolved_is_not_an_authorization_decision() {
    // A stale cookie is 401 even for an account that IS an administrator: the
    // answer to "I do not know who this is" must not be confused with "I do
    // and you may not".
    let (_dir, auth) = deployment();
    let operator = sign_in(&auth, "operator", &["admin"]);
    auth.db()
        .revoke_session(&operator.token)
        .expect("end the session");
    let reply = handle(&auth, &as_account(&operator, "GET", USERS, ""));
    assert_eq!(reply.status, "401 Unauthorized");
    assert_eq!(json(&reply)["error"], "unauthorized");
}

#[test]
fn a_disabled_administrators_session_stops_working_here_too() {
    let (_dir, auth) = deployment();
    let first = sign_in(&auth, "operator", &["admin"]);
    let second = sign_in(&auth, "second", &["admin"]);
    handle(
        &auth,
        &as_account(
            &first,
            "POST",
            USER_STATUS,
            &serde_json::json!({ "id": second.id, "status": "disabled" }).to_string(),
        ),
    );
    let reply = handle(&auth, &as_account(&second, "GET", USERS, ""));
    assert_eq!(
        reply.status, "401 Unauthorized",
        "disabling the account is what made its session stop resolving"
    );
}
