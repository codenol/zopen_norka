//! Section and analytics route paths, shared by the web shell and the daemon.
//!
//! The section routes live INSIDE the document family (`/api/files/<key>/...`)
//! because a section is reached through the document it lives in, and the
//! analytics routes are a family of their own because an asset is reached by its
//! own key. Both spellings are here rather than in the browser host so the
//! client and the server cannot drift — the same arrangement `share_routes` and
//! `auth_routes` use.

/// The segment under a document key that carries its sections.
pub const SECTIONS_SEGMENT: &str = "sections";

/// `GET` — every section of a document that has properties.
/// `POST /api/files/<key>/sections/<node>` writes one.
pub fn sections(key: &str) -> String {
    format!("/api/files/{key}/{SECTIONS_SEGMENT}")
}

/// One section of a document, addressed by the frame that marks it.
pub fn section(key: &str, node: &str) -> String {
    format!("/api/files/{key}/{SECTIONS_SEGMENT}/{node}")
}

/// The prefix every analytics route sits under.
pub const ANALYTICS_PREFIX: &str = "/api/analytics";

/// The caller's own analytics assets.
pub const ANALYTICS: &str = ANALYTICS_PREFIX;

/// One analytics asset, by its own short key.
pub fn analytics(key: &str) -> String {
    format!("{ANALYTICS_PREFIX}/{key}")
}

/// One asset's name — its markdown keeps the address it was written at.
pub fn analytics_rename(key: &str) -> String {
    format!("{ANALYTICS_PREFIX}/{key}/rename")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_is_reached_through_its_document() {
        assert_eq!(sections("abc"), "/api/files/abc/sections");
        assert_eq!(section("abc", "n1"), "/api/files/abc/sections/n1");
    }

    #[test]
    fn an_asset_is_reached_by_its_own_key() {
        assert_eq!(analytics("k1"), "/api/analytics/k1");
        assert_eq!(analytics_rename("k1"), "/api/analytics/k1/rename");
    }
}
