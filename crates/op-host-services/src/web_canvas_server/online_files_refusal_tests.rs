//! The stored-document routes on a public deployment, driven end to end.
//!
//! Split out of `online_run_loop_tests.rs` (which supplies the request builder
//! and the registry) for the 800-line cap.
//!
//! These lock in the one decision the roles work deliberately did NOT change:
//! an online deployment still refuses `/api/files*` and `/api/recovery*`
//! wholesale, in front of the per-request access gate that
//! `files_routes::access_tests` proves. The reason is the document directory,
//! not the roles: `documents_dir()` has no tenant dimension, so a role check
//! would say who may edit without ever saying whose file it is. When a
//! per-owner store lands, this module is what has to be deleted — and the
//! failure it produces is the reminder that the gate underneath it is now the
//! only thing standing there.

use super::*;

/// Every shape the stored-document family answers, from both tiers.
const REFUSED_ROUTES: &[(&str, &str, &str)] = &[
    ("GET", "/api/files", ""),
    ("POST", "/api/files", "{}"),
    ("GET", "/api/files/abcd1234/thumb", ""),
    ("POST", "/api/files/abcd1234/save", "{}"),
    ("DELETE", "/api/files/abcd1234", ""),
    ("GET", "/api/recovery", ""),
    ("POST", "/api/recovery", "{}"),
    ("POST", "/api/recovery/restore", ""),
    ("DELETE", "/api/recovery", ""),
];

#[test]
fn an_account_is_refused_the_stored_document_routes_on_its_own_tenant() {
    // The owner of the tenant — the caller with the most authority this
    // deployment can hand out — still stops at the deployment refusal, because
    // what it protects is the shared directory rather than the caller.
    let registry = registry();
    let verifier = verifier();
    for (method, path, payload) in REFUSED_ROUTES {
        let response = serve(
            &registry,
            &verifier,
            Request::json(method, path, payload).with_bearer("tokA"),
        );
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 403 Forbidden",
            "{method} {path}: {response}"
        );
        assert_eq!(
            body(&response)["error"],
            "online-local-file-disabled",
            "{method} {path}"
        );
    }
}

#[test]
fn naming_another_accounts_tenant_does_not_reach_them_either() {
    // The refusal is not something a `?tenant=` parameter can step around: a
    // granted visitor is admitted to the TENANT and still refused the FILE
    // routes, which is the shape the per-owner store has to preserve.
    let registry = registry();
    let verifier = verifier();
    let granted = serve(
        &registry,
        &verifier,
        Request::json(
            "POST",
            op_editor_core::share_routes::GRANT,
            &serde_json::json!({ "userId": "userB" }).to_string(),
        )
        .with_bearer("tokA"),
    );
    assert_eq!(status_line(&granted), "HTTP/1.1 200 OK", "{granted}");

    for (method, path, payload) in REFUSED_ROUTES {
        let mut request = Request::json(method, path, payload).with_bearer("tokB");
        request.tenant = Some("userA");
        let response = serve(&registry, &verifier, request);
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 403 Forbidden",
            "{method} {path}: {response}"
        );
        assert_eq!(
            body(&response)["error"],
            "online-local-file-disabled",
            "{method} {path}"
        );
    }
}
