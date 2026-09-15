//! Tenant-sharing HTTP route paths, shared by the web shell (client) and the
//! serve-web daemon (server).
//!
//! Only the multi-account online deployment serves these; the local and
//! managed daemons have exactly one document and nobody to share it with.
//! Keeping the paths in one wasm-clean crate both sides already depend on is
//! the same arrangement `auth_routes` and `collab_routes` use.

/// Prefix shared by every route below (used by the server's sensitive-POST /
/// CORS gating).
pub const API_PREFIX: &str = "/api/share/";

/// `POST` — add an account to the caller's own access list.
///
/// Body: `{"userId":"<account id>"}`. The grantor is the request's verified
/// identity and is never taken from the body.
pub const GRANT: &str = "/api/share/grant";

/// `POST` — remove an account from the caller's own access list.
pub const REVOKE: &str = "/api/share/revoke";

/// `GET` — who the caller shares with, and who shares with the caller.
pub const LIST: &str = "/api/share/list";

/// `POST` — turn "anybody with the link" on or off, and at what level.
///
/// Body: `{"enabled":bool,"level":"viewer"|"commenter"|"editor"|"admin"}`.
/// Its own route rather than a field of [`GRANT`], because it is not about an
/// account: it widens the document to every signed-in caller at once, and a
/// client that had to express that as a grant would have to invent an account
/// to grant to.
pub const LINK_ACCESS: &str = "/api/share/link";

/// Query parameter naming the tenant a request is addressed to.
///
/// A header would be the more usual choice, but `EventSource` cannot set
/// request headers, and `/api/mcp/events` is exactly the route a visitor
/// needs most — a shared document that does not push updates is not shared in
/// any useful sense. Rather than split the mechanism (header for XHR, query
/// for SSE) and have two places to get wrong, everything uses the query.
///
/// The value is an account id. It is a REQUEST for access, never a grant of
/// it: the server still resolves the caller's own identity and checks it
/// against the owner's access list.
pub const TENANT_QUERY: &str = "tenant";

/// Read the tenant parameter out of a raw query string (no leading `?`).
///
/// Deliberately tiny and dependency-free so both the wasm shell and the
/// daemon parse it identically. Percent-decoding is not attempted: an account
/// id is an opaque token, and a value needing escapes is not one.
pub fn tenant_from_query(query: &str) -> Option<&str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == TENANT_QUERY)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tenant_parameter_is_read_out_of_a_query_string() {
        assert_eq!(tenant_from_query("tenant=userA"), Some("userA"));
        assert_eq!(tenant_from_query("x=1&tenant=userA&y=2"), Some("userA"));
        assert_eq!(tenant_from_query("y=2&tenant=userA"), Some("userA"));
    }

    #[test]
    fn an_absent_or_empty_tenant_parameter_is_none() {
        for query in ["", "x=1", "tenant=", "tenants=userA", "atenant=userA"] {
            assert_eq!(tenant_from_query(query), None, "{query:?}");
        }
    }

    #[test]
    fn only_the_exact_parameter_name_matches() {
        assert_eq!(tenant_from_query("Tenant=userA"), None);
    }

    #[test]
    fn every_route_sits_under_the_declared_prefix() {
        for route in [GRANT, REVOKE, LIST] {
            assert!(route.starts_with(API_PREFIX), "{route}");
        }
    }
}

/// Query parameter naming the DOCUMENT a request is about.
///
/// The sibling of [`TENANT_QUERY`], and the answer to a measured defect: a
/// share used to name only an owner, so a grant made in one document's dialog
/// opened every document that account owned (issue #127), and a
/// tenant-addressed read answered whichever document that tenant had opened
/// last (issue #128). A document has exactly one name in this system — the
/// store key that `/api/files/<key>` already uses — so that is what travels.
pub const FILE_QUERY: &str = "file";

/// Read the document parameter out of a raw query string (no leading `?`).
pub fn file_from_query(query: &str) -> Option<&str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == FILE_QUERY)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

/// The document a request is about, from wherever the request says it.
///
/// Three places, in order of authority:
///
/// 1. the path, for the file routes (`/api/files/<key>/open` and friends) —
///    the key is part of the address;
/// 2. `?file=<key>`, for the routes that address a document without being one
///    of the file routes (`GET /api/mcp/document`, the share routes);
/// 3. the JSON body's `file` field, for the share routes' POSTs, which are
///    already carrying a body.
///
/// `None` means the request did NOT name a document, and every caller decides
/// what that means for it: the share routes refuse it (a share is about a
/// document), while a read falls back to the tenant's own open document for as
/// long as a client older than the field exists.
pub fn document_from_request(path: &str, query: Option<&str>, body: &str) -> Option<String> {
    if let Some(key) = document_key_from_path(path) {
        return Some(key);
    }
    if let Some(key) = query.and_then(file_from_query) {
        return Some(key.to_string());
    }
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get(FILE_QUERY)
                .and_then(|key| key.as_str())
                .map(str::to_string)
        })
        .filter(|key| !key.trim().is_empty())
}

/// The document key inside a file-route path, if the path is one.
///
/// `/api/files/<key>/open` → `<key>`; `/api/files/<key>` → `<key>`; anything
/// else → `None`. Empty segments are not keys.
fn document_key_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/api/files/")?;
    let key = rest.split('/').next()?;
    (!key.is_empty()).then(|| key.to_string())
}
