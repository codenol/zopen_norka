//! Tests for the share administration routes and their admission rules.

use super::*;

/// The document every share in this file is about.
const DOCUMENT: &str = "docA";
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant::{TenantError, TenantLimits};
use crate::web_canvas_server::tenant_auth::IdentityVia;

fn identity(user_id: &str) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        roles: op_editor_core::access::RoleSet::empty(),
        via: IdentityVia::ApiToken,
        scopes: McpScopes::FULL,
    }
}

fn registry() -> TenantRegistry {
    TenantRegistry::with_store(
        3102,
        TenantLimits::default(),
        Vec::new(),
        super::super::tenant_store::TenantStore::new(None),
    )
}

fn body_of(reply: &WebReply) -> serde_json::Value {
    serde_json::from_str(&reply.body).expect("json body")
}

fn grant(registry: &TenantRegistry, owner: &str, target: &str) -> WebReply {
    let identity = identity(owner);
    let lease = registry.lease_for(&identity).expect("lease");
    handle(
        "POST",
        share_routes::GRANT,
        &serde_json::json!({ "userId": target }).to_string(),
        &identity,
        &lease,
        registry,
        None,
        Some(DOCUMENT),
    )
}

/// A grant that names an account the deployment does not have.
///
/// The invite field of the Share dialog is a text box, and a text box accepts a
/// NAME. Before this check the route recorded the name as a grant and answered
/// `200 changed:true`: the row appeared in "Who has access" as if it were a
/// person, and the person it named was refused with `tenant-not-shared`. Found
/// by running the share scenario against a real deployment.
#[test]
fn a_grant_for_an_account_that_does_not_exist_is_refused() {
    use crate::accounts::AccountsDb;
    use crate::document_test_dir::TempDir;
    use crate::web_canvas_server::account_routes::AccountAuth;
    use std::sync::Arc;

    let dir = TempDir::new("share-unknown-account");
    let accounts = AccountAuth::new(Arc::new(
        AccountsDb::open(dir.path()).expect("open the account store"),
    ));
    let registry = registry();
    let identity = identity("userA");
    let lease = registry.lease_for(&identity).expect("lease");

    let reply = handle(
        "POST",
        share_routes::GRANT,
        &serde_json::json!({ "userId": "fourth" }).to_string(),
        &identity,
        &lease,
        &registry,
        Some(&accounts),
        Some(DOCUMENT),
    );

    assert_eq!(reply.status, "400 Bad Request", "{}", reply.body);
    assert_eq!(body_of(&reply)["error"], "unknown-account");
}

/// The invite field offers "account name", so a name has to work.
///
/// It used to be accepted verbatim and grant nothing — a row that looked like a
/// person and refused them at the door (issue #117). What the field holds is now
/// resolved against the deployment's account list: an id, a handle, or an
/// address, and what gets RECORDED is always the id (issue #130).
#[test]
fn a_grant_by_account_name_grants_that_account() {
    use crate::accounts::{AccountsDb, NewUser};
    use crate::document_test_dir::TempDir;
    use crate::web_canvas_server::account_routes::AccountAuth;
    use std::sync::Arc;

    let dir = TempDir::new("share-by-name");
    let accounts = AccountAuth::new(Arc::new(
        AccountsDb::open(dir.path()).expect("open the account store"),
    ));
    let colleague = accounts
        .db()
        .create_user(
            &NewUser {
                id: None,
                username: "colleague",
                display_name: "Ada Colleague",
                email: None,
                password: Some("basket-lantern-quiet-41"),
                roles: &["contributor"],
            },
            crate::accounts::now_secs(),
        )
        .expect("create an account");

    let registry = registry();
    let identity = identity("userA");
    let lease = registry.lease_for(&identity).expect("lease");
    for named in ["colleague", "Ada Colleague", &colleague.id] {
        let reply = handle(
            "POST",
            share_routes::GRANT,
            &serde_json::json!({ "userId": named }).to_string(),
            &identity,
            &lease,
            &registry,
            Some(&accounts),
            Some(DOCUMENT),
        );
        // A display name is not a handle and is not an id: only the two the
        // account list is keyed by resolve.
        let expected = if named == "Ada Colleague" {
            "400 Bad Request"
        } else {
            "200 OK"
        };
        assert_eq!(reply.status, expected, "{named}: {}", reply.body);
        if named == "colleague" {
            // The row records the ID, whatever was typed.
            assert_eq!(
                body_of(&reply)["sharedWith"][0]["account"],
                colleague.id.as_str()
            );
            assert_eq!(
                body_of(&reply)["sharedWith"][0]["displayName"],
                "Ada Colleague"
            );
            assert_eq!(body_of(&reply)["sharedWith"][0]["username"], "colleague");
        }
    }
    assert_eq!(lease.tenant().shared_with(DOCUMENT).len(), 1);
}

#[test]
fn a_grant_admits_the_named_account_and_nobody_else() {
    let registry = registry();
    let reply = grant(&registry, "userA", "userB");
    assert_eq!(reply.status, "200 OK");
    assert_eq!(body_of(&reply)["changed"], true);

    let visitor = identity("userB");
    assert!(registry
        .lease_for_shared("userA", &visitor, DOCUMENT)
        .is_ok());

    let stranger = identity("userC");
    assert_eq!(
        registry
            .lease_for_shared("userA", &stranger, DOCUMENT)
            .unwrap_err(),
        TenantError::NotShared
    );
}

#[test]
fn a_revoke_takes_effect_on_the_next_request() {
    let registry = registry();
    grant(&registry, "userA", "userB");
    let visitor = identity("userB");
    assert!(registry
        .lease_for_shared("userA", &visitor, DOCUMENT)
        .is_ok());

    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    let reply = handle(
        "POST",
        share_routes::REVOKE,
        r#"{"userId":"userB"}"#,
        &owner,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    assert_eq!(reply.status, "200 OK");
    assert_eq!(body_of(&reply)["changed"], true);

    // The access list is consulted per request, so this is immediate — no
    // session to expire first.
    assert_eq!(
        registry
            .lease_for_shared("userA", &visitor, DOCUMENT)
            .unwrap_err(),
        TenantError::NotShared
    );
}

#[test]
fn an_owner_always_reaches_their_own_document() {
    let registry = registry();
    let owner = identity("userA");
    assert!(registry.lease_for_shared("userA", &owner, DOCUMENT).is_ok());
}

#[test]
fn a_repeated_grant_is_acknowledged_without_changing_anything() {
    let registry = registry();
    assert_eq!(
        body_of(&grant(&registry, "userA", "userB"))["changed"],
        true
    );
    assert_eq!(
        body_of(&grant(&registry, "userA", "userB"))["changed"],
        false
    );
}

#[test]
fn revoking_an_account_that_was_never_granted_is_not_an_error() {
    let registry = registry();
    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    let reply = handle(
        "POST",
        share_routes::REVOKE,
        r#"{"userId":"nobody"}"#,
        &owner,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    assert_eq!(reply.status, "200 OK");
    assert_eq!(body_of(&reply)["changed"], false);
}

#[test]
fn the_list_reports_both_directions() {
    let registry = registry();
    grant(&registry, "userA", "userB");
    grant(&registry, "userC", "userB");

    let visitor = identity("userB");
    let lease = registry.lease_for(&visitor).expect("lease");
    let reply = handle(
        "GET",
        share_routes::LIST,
        "",
        &visitor,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    let body = body_of(&reply);
    assert_eq!(reply.status, "200 OK");
    // userB has shared with nobody…
    assert_eq!(body["sharedWith"].as_array().map(Vec::len), Some(0));
    // …and two accounts have shared with userB, each with the level it gives
    // them: a guest who is not told what they hold cannot act on it.
    let mine: Vec<&str> = body["sharedWithMe"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|value| value.get("owner").and_then(|owner| owner.as_str()))
        .collect();
    assert_eq!(mine, vec!["userA", "userC"]);
    assert_eq!(
        body["sharedWithMe"][0]["level"], "viewer",
        "the level the owner's list recorded, fail-closed for a list written before levels"
    );
}

#[test]
fn a_visitor_cannot_reshare_the_document_they_were_given() {
    // The route always edits the CALLER's own tenant, so a grant issued by a
    // visitor lands on the visitor's own document, never the owner's.
    let registry = registry();
    grant(&registry, "userA", "userB");
    grant(&registry, "userB", "userC");

    let stranger = identity("userC");
    assert_eq!(
        registry
            .lease_for_shared("userA", &stranger, DOCUMENT)
            .unwrap_err(),
        TenantError::NotShared,
        "userB must not be able to widen userA's access list"
    );
    assert!(
        registry
            .lease_for_shared("userB", &stranger, DOCUMENT)
            .is_ok(),
        "userB may of course share their own document"
    );
}

#[test]
fn an_account_cannot_share_with_itself() {
    let registry = registry();
    let reply = grant(&registry, "userA", "userA");
    assert_eq!(reply.status, "400 Bad Request");
    assert_eq!(body_of(&reply)["error"], "cannot-share-with-self");
}

#[test]
fn a_malformed_share_body_is_refused() {
    let registry = registry();
    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    for body in [
        "",
        "{}",
        "not json",
        r#"{"userId":""}"#,
        r#"{"userId":123}"#,
        r#"{"user":"x"}"#,
    ] {
        let reply = handle(
            "POST",
            share_routes::GRANT,
            body,
            &owner,
            &lease,
            &registry,
            None,
            Some(DOCUMENT),
        );
        assert_eq!(reply.status, "400 Bad Request", "{body:?}");
    }
}

#[test]
fn an_oversized_share_body_is_refused_before_it_is_parsed() {
    let registry = registry();
    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    let body = format!(r#"{{"userId":"{}"}}"#, "x".repeat(MAX_SHARE_BODY_BYTES));
    let reply = handle(
        "POST",
        share_routes::GRANT,
        &body,
        &owner,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    assert_eq!(reply.status, "413 Payload Too Large");
}

#[test]
fn a_wrong_method_on_a_share_route_is_405() {
    let registry = registry();
    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    let reply = handle(
        "GET",
        share_routes::GRANT,
        "",
        &owner,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    assert_eq!(reply.status, "405 Method Not Allowed");
}

#[test]
fn the_share_routes_are_recognised_and_nothing_else_is() {
    for route in [
        share_routes::GRANT,
        share_routes::REVOKE,
        share_routes::LIST,
    ] {
        assert!(is_share_route(route), "{route}");
    }
    for other in ["/api/mcp/document", "/api/share", "/api/share/", "/mcp"] {
        assert!(!is_share_route(other), "{other}");
    }
}

#[test]
fn a_forbidden_share_and_an_unknown_one_answer_identically() {
    // Otherwise the difference tells a caller which accounts exist.
    assert_eq!(TenantError::NotShared.http_status(), "403 Forbidden");
    let registry = registry();
    let stranger = identity("userC");
    let unknown = registry
        .lease_for_shared("nobody-at-all", &stranger, DOCUMENT)
        .unwrap_err();
    grant(&registry, "userA", "userB");
    let forbidden = registry
        .lease_for_shared("userA", &stranger, DOCUMENT)
        .unwrap_err();
    assert_eq!(unknown, forbidden);
}

#[test]
fn the_grant_past_the_ceiling_is_refused_rather_than_silently_dropped() {
    // The store writes a bounded list, so accepting the 257th grant would
    // report a success that vanishes on the next save.
    let registry = registry();
    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    for index in 0..super::super::tenant_store::MAX_SHARED_ACCOUNTS {
        let reply = handle(
            "POST",
            share_routes::GRANT,
            &serde_json::json!({ "userId": format!("guest-{index}") }).to_string(),
            &owner,
            &lease,
            &registry,
            None,
            Some(DOCUMENT),
        );
        assert_eq!(reply.status, "200 OK", "grant {index}");
    }
    let overflow = handle(
        "POST",
        share_routes::GRANT,
        r#"{"userId":"one-too-many"}"#,
        &owner,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    assert_eq!(overflow.status, "400 Bad Request", "{}", overflow.body);
    assert_eq!(body_of(&overflow)["error"], "share-limit-reached");
    // And the refused account really is absent, not quietly present in memory.
    assert!(!lease
        .tenant()
        .shared_with(DOCUMENT)
        .contains("one-too-many"));
}

#[test]
fn a_repeat_grant_at_the_ceiling_still_succeeds() {
    // The ceiling bounds NEW accounts; re-granting one already on the list
    // changes nothing and must not be refused.
    let registry = registry();
    let owner = identity("userA");
    let lease = registry.lease_for(&owner).expect("lease");
    for index in 0..super::super::tenant_store::MAX_SHARED_ACCOUNTS {
        handle(
            "POST",
            share_routes::GRANT,
            &serde_json::json!({ "userId": format!("guest-{index}") }).to_string(),
            &owner,
            &lease,
            &registry,
            None,
            Some(DOCUMENT),
        );
    }
    let repeat = handle(
        "POST",
        share_routes::GRANT,
        r#"{"userId":"guest-0"}"#,
        &owner,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );
    assert_eq!(repeat.status, "200 OK", "{}", repeat.body);
}
