//! Tests for the share administration routes and their admission rules.

use super::*;

/// The document every share in this file is about.
const DOCUMENT: &str = "docA";
use crate::accounts::{AccountsDb, NewUser};
use crate::document_test_dir::TempDir;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::account_routes::AccountAuth;
use crate::web_canvas_server::tenant::{TenantError, TenantLimits};
use crate::web_canvas_server::tenant_auth::IdentityVia;
use std::sync::Arc;

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

/// One mutation dispatched the way the route table dispatches it, against a
/// deployment that may or may not have an account list.
fn mutate_as(
    registry: &TenantRegistry,
    accounts: Option<&AccountAuth>,
    caller: &str,
    route: &'static str,
    target: &str,
) -> WebReply {
    let owner = identity(caller);
    let lease = registry.lease_for(&owner).expect("lease");
    handle(
        "POST",
        route,
        &serde_json::json!({ "userId": target }).to_string(),
        &owner,
        &lease,
        registry,
        accounts,
        Some(DOCUMENT),
    )
}

/// One account in a deployment's list, with an address when the case needs one
/// — an address is a spelling of an account like any other (issue #130).
fn create_account(
    accounts: &AccountAuth,
    username: &str,
    email: Option<&str>,
) -> crate::accounts::User {
    let mut new = NewUser::active(username, "Person", "basket-lantern-quiet-41");
    if let Some(email) = email {
        new = new.with_email(email);
    }
    accounts
        .db()
        .create_user(&new, crate::accounts::now_secs())
        .expect("create an account")
}

/// A deployment's account list, with the accounts named in `usernames`.
fn accounts_with(label: &str, usernames: &[&str]) -> (TempDir, AccountAuth) {
    let dir = TempDir::new(label);
    let accounts = AccountAuth::new(Arc::new(
        AccountsDb::open(dir.path()).expect("open the account store"),
    ));
    for username in usernames {
        create_account(&accounts, username, None);
    }
    (dir, accounts)
}

/// A revoke resolves the same string a grant does (issue #130) — the invite
/// field is one field, and a person revoking what they just invited types the
/// same thing back — and removes only the account it named.
///
/// Two handles that share a prefix are the interesting pair: resolving happens
/// in the directory, once, so `colleague` cannot take `colleague2` with it.
#[test]
fn a_revoke_by_account_name_removes_that_account_and_no_other() {
    let (_dir, accounts) = accounts_with("share-revoke-by-name", &["colleague", "colleague2"]);
    let registry = registry();
    let by_name = |target: &str, route: &'static str| {
        mutate_as(&registry, Some(&accounts), "userA", route, target)
    };
    let accounts_of = |username: &str| {
        accounts
            .db()
            .find_user_by_username(username)
            .expect("lookup")
            .unwrap_or_else(|| panic!("{username} exists"))
    };
    for target in ["colleague", "colleague2"] {
        assert_eq!(
            by_name(target, share_routes::GRANT).status,
            "200 OK",
            "{target}"
        );
    }
    let listed = registry
        .lease_for(&identity("userA"))
        .expect("lease")
        .tenant()
        .shared_with(DOCUMENT);
    assert_eq!(listed.len(), 2, "both accounts are on the list: {listed:?}");
    assert!(listed.contains(&accounts_of("colleague").id));

    // By handle…
    let revoked = by_name("colleague", share_routes::REVOKE);
    assert_eq!(revoked.status, "200 OK", "{}", revoked.body);
    assert_eq!(body_of(&revoked)["changed"], true);

    // …and by id, which is the other half of the same resolution.
    let by_id = by_name(&accounts_of("colleague2").id, share_routes::REVOKE);
    assert_eq!(by_id.status, "200 OK", "{}", by_id.body);
    assert_eq!(body_of(&by_id)["changed"], true);

    let lease = registry.lease_for(&identity("userA")).expect("lease");
    assert!(
        lease.tenant().shared_with(DOCUMENT).is_empty(),
        "one revoke removed one account, and the name took nobody else with it"
    );

    // A second revoke of a row that is gone is still fine, and still says so.
    let again = by_name("colleague", share_routes::REVOKE);
    assert_eq!(again.status, "200 OK", "{}", again.body);
    assert_eq!(body_of(&again)["changed"], false);
}

/// A revoke of a string that names nobody is a no-op, not a refusal
/// (issue #129).
///
/// The two halves answer the same unknown string differently, and this is where
/// that is held: a grant refuses it, because the row it would write is a row
/// the product draws as a person and enforces at the door (#117); a revoke
/// removes the row that spelling keys, if there is one, and reports
/// `changed:false` when there is not. Refusing the revoke instead would make a
/// row unremovable in the two cases where a row outlives its account — an
/// account deleted from the deployment, and a row written before the lookup
/// existed, keyed by the name somebody typed (#130).
///
/// The issue left one thing open: whether a well-formed but unknown ID answers
/// like a name did. It does.
#[test]
fn a_revoke_of_a_string_that_names_nobody_is_reported_rather_than_refused() {
    let (_dir, accounts) = accounts_with("share-revoke-unknown", &["colleague"]);
    let registry = registry();
    let colleague = accounts
        .db()
        .find_user_by_username("colleague")
        .expect("lookup")
        .expect("the account exists");

    // The row the route wrote BEFORE it resolved anything: the spelling itself,
    // which is what a person typed (#117). A route without an account list
    // still writes rows that way.
    assert_eq!(
        mutate_as(&registry, None, "userA", share_routes::GRANT, "ghost").status,
        "200 OK"
    );
    // …beside a real account's row, which no revoke of a stranger may touch.
    assert_eq!(
        mutate_as(
            &registry,
            Some(&accounts),
            "userA",
            share_routes::GRANT,
            "colleague"
        )
        .status,
        "200 OK"
    );

    // Grant refuses a string that names nobody…
    for unknown in ["ghost", "u_00000000000000000000000000000000"] {
        let refused = mutate_as(
            &registry,
            Some(&accounts),
            "userA",
            share_routes::GRANT,
            unknown,
        );
        assert_eq!(
            refused.status, "400 Bad Request",
            "{unknown}: {}",
            refused.body
        );
        assert_eq!(body_of(&refused)["error"], "unknown-account");
        assert_eq!(
            body_of(&refused)["message"],
            "this deployment has no account with that id",
            "{unknown}"
        );
    }

    // …and a revoke does not: it removes the row that spelling keys, if there
    // is one.
    let removed = mutate_as(
        &registry,
        Some(&accounts),
        "userA",
        share_routes::REVOKE,
        "ghost",
    );
    assert_eq!(removed.status, "200 OK", "{}", removed.body);
    assert_eq!(body_of(&removed)["changed"], true);

    // A well-formed id that holds nobody removes nothing, and says exactly
    // that.
    let nothing = mutate_as(
        &registry,
        Some(&accounts),
        "userA",
        share_routes::REVOKE,
        "u_00000000000000000000000000000000",
    );
    assert_eq!(nothing.status, "200 OK", "{}", nothing.body);
    assert_eq!(body_of(&nothing)["changed"], false);

    // Through all of it the account that IS there kept its access.
    let remaining = registry
        .lease_for(&identity("userA"))
        .expect("lease")
        .tenant()
        .shared_with(DOCUMENT);
    assert_eq!(
        remaining.len(),
        1,
        "a revoke that named nobody removed somebody: {remaining:?}"
    );
    assert!(remaining.contains(&colleague.id));
}

/// A share is between the caller and SOMEBODY ELSE, and naming the caller is
/// refused however the caller is spelled (issue #141).
///
/// The check used to compare the string the body carried against the caller's
/// id, and it ran before the field was resolved (#130): an id was refused and a
/// handle was not — the handle resolved to the caller's own id and the grant
/// was RECORDED, a row in "Who has access" drawn as a person which is the
/// person reading it. The address branch had the same hole, which the issue
/// left open; one comparison against what the lookup answered closes both.
#[test]
fn a_grant_that_names_the_caller_itself_is_refused_however_it_is_spelled() {
    let (_dir, accounts) = accounts_with("share-self-by-name", &[]);
    let me = create_account(&accounts, "designer", Some("designer@example.com"));
    let other = create_account(&accounts, "colleague", Some("colleague@example.com"));
    let registry = registry();

    // All three spellings of the caller's OWN account: the id it is keyed by,
    // the handle it signs in with, and the address the directory holds for it.
    for spelling in [me.id.as_str(), "designer", "designer@example.com"] {
        let refused = mutate_as(
            &registry,
            Some(&accounts),
            &me.id,
            share_routes::GRANT,
            spelling,
        );
        assert_eq!(
            refused.status, "400 Bad Request",
            "{spelling}: {}",
            refused.body
        );
        assert_eq!(
            body_of(&refused)["error"],
            "cannot-share-with-self",
            "{spelling}: {}",
            refused.body
        );
    }
    // Nothing was recorded by any of them: the row the bug wrote is the point
    // of the fix, not just the status.
    assert!(registry
        .lease_for(&identity(&me.id))
        .expect("lease")
        .tenant()
        .shared_with(DOCUMENT)
        .is_empty());

    // The case that must keep working — somebody else, by every spelling the
    // field accepts. The address is one of them: the refusal above must not
    // have been bought by refusing addresses.
    for spelling in ["colleague", "colleague@example.com", other.id.as_str()] {
        let granted = mutate_as(
            &registry,
            Some(&accounts),
            &me.id,
            share_routes::GRANT,
            spelling,
        );
        assert_eq!(granted.status, "200 OK", "{spelling}: {}", granted.body);
        assert_eq!(
            body_of(&granted)["sharedWith"][0]["account"],
            other.id.as_str(),
            "{spelling}: {}",
            granted.body
        );
    }
    let shared = registry
        .lease_for(&identity(&me.id))
        .expect("lease")
        .tenant()
        .shared_with(DOCUMENT);
    assert_eq!(shared.len(), 1, "one account, not three rows: {shared:?}");
    assert!(shared.contains(&other.id));

    // And a deployment with NO account list keeps the guard it always had: the
    // typed string is the row key there, so the comparison is the same one.
    let no_list = mutate_as(&registry, None, &me.id, share_routes::GRANT, &me.id);
    assert_eq!(no_list.status, "400 Bad Request", "{}", no_list.body);
    assert_eq!(body_of(&no_list)["error"], "cannot-share-with-self");
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

/// A wrong method on a share route answers a CODE, like every other refusal
/// here (issue #147).
#[test]
fn a_wrong_method_on_a_share_route_answers_a_code_not_prose() {
    let registry = registry();
    let identity = identity("userA");
    let lease = registry.lease_for(&identity).expect("lease");
    let reply = handle(
        "DELETE",
        share_routes::GRANT,
        "",
        &identity,
        &lease,
        &registry,
        None,
        Some(DOCUMENT),
    );

    assert_eq!(reply.status, "405 Method Not Allowed", "{}", reply.body);
    assert_eq!(
        body_of(&reply)["error"], "method-not-allowed",
        "a client switches on `error`; prose there is a value it cannot match"
    );
    assert_eq!(
        body_of(&reply)["message"], "method not allowed for this share route",
        "and the sentence still travels, in the field the other refusals use"
    );
}
