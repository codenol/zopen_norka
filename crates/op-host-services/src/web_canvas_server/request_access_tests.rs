//! Tests for the one place that decides what a caller may do.
//!
//! These are the whole coverage of the roles decision: the function is pure,
//! so every rule the routes obey is asserted here rather than through a route.
//! The route tests prove the wiring (that a route asks this function and
//! renders its answer); these prove the answer itself.

use super::*;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::tenant_auth::IdentityVia;
use op_editor_core::access::RoleSet;

/// A verified account holding `roles`, spelled the way the hub spells them.
fn identity(user_id: &str, roles: &[&str]) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        roles: RoleSet::from_wire(roles),
        via: IdentityVia::ApiToken,
        scopes: McpScopes::FULL,
    }
}

/// The owner of the document, with `roles`.
fn owner(roles: &[&str]) -> ResolvedIdentity {
    identity("userA", roles)
}

fn assert_all_allowed(access: &RequestAccess<'_>) {
    for action in DocumentAction::ALL {
        assert_eq!(access.decide(action), Ok(()), "{action:?}");
    }
}

fn assert_read_only(access: &RequestAccess<'_>) {
    assert_eq!(access.decide(DocumentAction::View), Ok(()));
    for action in DocumentAction::ALL.into_iter().filter(|a| a.is_write()) {
        assert_eq!(access.decide(action), Err(AccessRefusal::ReadOnly), "{action:?}");
    }
}

fn assert_refused_whole(access: &RequestAccess<'_>) {
    for action in DocumentAction::ALL {
        assert_eq!(access.decide(action), Err(AccessRefusal::NotShared), "{action:?}");
    }
}

#[test]
fn local_and_managed_deployments_refuse_nothing() {
    // The condition that lets the roles model land at all: local work does not
    // break and does not need a login.
    for mode in [ServeMode::Local, ServeMode::Managed] {
        let access = RequestAccess::local_operator(mode);
        assert_all_allowed(&access);
        assert_eq!(access.mode(), mode);
        assert_eq!(access.caller_id(), None);
        assert_eq!(access.owner_id(), None);
    }
}

#[test]
fn an_owner_with_an_editing_role_may_do_everything() {
    for role in ["ux_ui", "admin", "UX/UI", "Админ"] {
        let caller = owner(&[role]);
        let access = RequestAccess::online("userA", &caller, false);
        assert_all_allowed(&access);
    }
}

#[test]
fn an_owner_without_a_role_still_works_on_their_own_document() {
    // The operator's decision: the document is theirs to work on, whatever
    // roles the hub sends — otherwise a deployment whose hub sends none would
    // be read-only for the people the documents belong to.
    let caller = owner(&[]);
    assert_all_allowed(&RequestAccess::online("userA", &caller, false));
}

#[test]
fn a_shared_visitor_with_an_editing_role_may_do_everything() {
    let visitor = identity("userB", &["ux_ui"]);
    assert_all_allowed(&RequestAccess::online("userA", &visitor, true));
}

#[test]
fn a_shared_visitor_without_a_role_reads_but_never_writes() {
    // The case the whole "empty means no roles, not no access" rule exists
    // for: someone handed a link, with nothing in their account yet.
    let visitor = identity("userB", &[]);
    let access = RequestAccess::online("userA", &visitor, true);
    assert_read_only(&access);
    assert_eq!(access.caller_id(), Some("userB"));
}

#[test]
fn a_shared_contributor_role_reads_but_never_writes_someone_elses_document() {
    // Five of the seven roles land in the contributor bucket: none of them may
    // change a document they were merely given. On their *own* document they
    // may, like any owner — that is the point of ownership, not a privilege
    // these roles carry.
    for role in ["software", "analyst", "frontend", "backend", "qa"] {
        let visitor = identity("userB", &[role]);
        assert_read_only(&RequestAccess::online("userA", &visitor, true));
        let proprietor = owner(&[role]);
        assert_all_allowed(&RequestAccess::online("userA", &proprietor, false));
    }
}

#[test]
fn a_stranger_is_refused_the_document_before_the_action_is_considered() {
    // Not on the access list, no matter what roles: the document itself is
    // out of reach, and every action answers the same way.
    let stranger = identity("userC", &["admin"]);
    assert_refused_whole(&RequestAccess::online("userA", &stranger, false));
}

#[test]
fn an_unknown_role_grants_nothing() {
    // A hub that renames a role, or sends one this build has never heard of,
    // must never land in a privileged bucket — and a blank entry is not
    // "no roles at all" either; both answer as a caller with no role.
    for raw in ["wizard", "", "  "] {
        // A *visitor* with such a role: the owner's own document is theirs
        // regardless of roles, so the rule has to be proved on someone else's.
        let caller = identity("userB", &[raw]);
        assert!(caller.roles.is_empty(), "{raw:?}");
        assert!(!caller.roles.unrecognized().is_empty(), "{raw:?}");
        assert_read_only(&RequestAccess::online("userA", &caller, true));
    }
}

#[test]
fn a_partly_unknown_role_list_keeps_the_half_it_understood() {
    // Union, not intersection: a mistyped extra role must not take away the
    // edit that a recognised one grants.
    let caller = owner(&["wizard", "ux_ui"]);
    assert_eq!(caller.roles.unrecognized(), vec!["wizard".to_string()]);
    assert_all_allowed(&RequestAccess::online("userA", &caller, false));
}

#[test]
fn ownership_is_decided_by_identity_and_not_by_the_access_list() {
    // A missing (or stale) access-list entry must not lock an owner out of
    // their own document.
    let caller = owner(&["ux_ui"]);
    assert_all_allowed(&RequestAccess::online("userA", &caller, false));
    // …and the flag cannot admit someone the ids do not match.
    let impostor = identity("userB", &["ux_ui"]);
    assert_all_allowed(&RequestAccess::online("userB", &impostor, true));
    assert_refused_whole(&RequestAccess::online("userA", &impostor, false));
}

#[test]
fn an_online_carrier_without_an_account_fails_closed() {
    // Unconstructible through `online`, and still refused rather than opened:
    // a carrier that names no caller has proved nothing.
    let access = RequestAccess::local_operator(ServeMode::Online);
    assert_refused_whole(&access);
}

// ---------------------------------------------------------------------------
// The second question: whose stored document is this.
// ---------------------------------------------------------------------------

#[test]
fn a_local_deployment_reaches_every_row_in_its_own_directory() {
    // One operator, one directory: every file in it is theirs, including the
    // ownerless rows the legacy index brought over.
    for mode in [ServeMode::Local, ServeMode::Managed] {
        let access = RequestAccess::local_operator(mode);
        assert!(access.reaches_stored_document(Some("userA")));
        assert!(
            access.reaches_stored_document(None),
            "{mode:?}: the operator's own rows have no account to name"
        );
    }
}

#[test]
fn a_caller_reaches_its_own_documents_whatever_its_roles() {
    // Seeing is not editing: the documents a caller owns are theirs to address
    // with no roles at all, and this check is about addressing.
    for roles in [&[][..], &["wizard"][..]] {
        let caller = owner(roles);
        let access = RequestAccess::online("userA", &caller, false);
        assert!(access.reaches_stored_document(Some("userA")));
    }
}

#[test]
fn a_shared_visitor_reaches_the_owners_documents_and_only_those() {
    // The grant is on the workspace, so every document of the owner's that the
    // visitor names is reachable — and a third account's is not, however the
    // visitor's roles read.
    let visitor = identity("userB", &["admin"]);
    let access = RequestAccess::online("userA", &visitor, true);
    assert!(access.reaches_stored_document(Some("userA")));
    assert!(access.reaches_stored_document(Some("userB")));
    assert!(!access.reaches_stored_document(Some("userC")));
    assert!(!access.reaches_stored_document(None));
}

#[test]
fn an_account_that_owns_nothing_reaches_nobody_elses_documents() {
    // The hole the owner column closes: a key names a row in a directory every
    // account shares, so a caller whose own lease says nothing about that row
    // must not reach it — the lease is on THEIR workspace, not on the document
    // the key names.
    let stranger = identity("userB", &["admin"]);
    let access = RequestAccess::online("userB", &stranger, false);
    assert!(access.reaches_stored_document(Some("userB")));
    assert!(!access.reaches_stored_document(Some("userA")));
    assert!(!access.reaches_stored_document(None));
}

#[test]
fn an_ownerless_row_is_reached_by_no_account() {
    // NULL owner: the legacy import's rows, and everything a daemon without
    // accounts made. There is no account to match them against, so the answer
    // is no — for an owner, for a granted visitor, for everyone.
    let proprietor = owner(&["admin"]);
    assert!(!RequestAccess::online("userA", &proprietor, false).reaches_stored_document(None));
    let visitor = identity("userB", &["admin"]);
    assert!(!RequestAccess::online("userA", &visitor, true).reaches_stored_document(None));
}

#[test]
fn the_share_flag_is_what_admits_a_visitor_and_nothing_else_is() {
    // A caller naming an owner it holds no lease on: the flag is false, and the
    // answer must be refused even though the owner id is the one it named. This
    // is the fail-closed direction of a check that would otherwise be "the
    // request mentioned the right account".
    let visitor = identity("userB", &["admin"]);
    assert!(!RequestAccess::online("userA", &visitor, false).reaches_stored_document(Some("userA")));
    // And a carrier with no verified caller reaches nothing, owner or not.
    let bare = RequestAccess::local_operator(ServeMode::Online);
    assert!(!bare.reaches_stored_document(Some("userA")));
    assert!(!bare.reaches_stored_document(None));
}

#[test]
fn the_actions_name_themselves_and_split_into_reads_and_writes() {
    let names: Vec<&str> = DocumentAction::ALL.iter().map(|a| a.as_str()).collect();
    assert_eq!(names, ["view", "edit", "delete", "restore"]);
    assert!(!DocumentAction::View.is_write());
    for action in [DocumentAction::Edit, DocumentAction::Delete, DocumentAction::Restore] {
        assert!(action.is_write(), "{action:?}");
    }
}

#[test]
fn the_two_refusals_read_differently_and_carry_a_code() {
    assert_eq!(AccessRefusal::NotShared.http_status(), "403 Forbidden");
    assert_eq!(AccessRefusal::ReadOnly.http_status(), "403 Forbidden");
    assert_eq!(AccessRefusal::NotShared.code(), "tenant-not-shared");
    assert_eq!(AccessRefusal::ReadOnly.code(), "read-only-role");
    assert_ne!(
        AccessRefusal::NotShared.to_string(),
        AccessRefusal::ReadOnly.to_string()
    );
    assert!(!AccessRefusal::ReadOnly.to_string().is_empty());
}

#[test]
fn a_refusal_renders_the_coded_body_every_route_already_uses() {
    for refusal in [AccessRefusal::NotShared, AccessRefusal::ReadOnly] {
        let reply = refusal_reply(refusal);
        assert_eq!(reply.status, refusal.http_status());
        let body: serde_json::Value = serde_json::from_str(&reply.body).expect("json");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], refusal.code());
        assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
    }
}
