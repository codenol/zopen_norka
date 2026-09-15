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
