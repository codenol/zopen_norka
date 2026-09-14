//! Browser-origin screening for the daemon's sensitive POST routes
//! (credentials / AI / auth / figma): same-origin by default, widened only
//! by the explicit `OPENPENCIL_WEB_ALLOWED_ORIGINS` allowlist. Split out of
//! `web_canvas_server.rs` to keep the spine under the 800-line cap.

pub(crate) const WEB_ALLOWED_ORIGINS_ENV: &str = "OPENPENCIL_WEB_ALLOWED_ORIGINS";

pub(super) fn is_sensitive_browser_post(request: &crate::mcp_serve::HttpRequest) -> bool {
    request.method == "POST"
        && (request.path == "/api/settings/credentials"
            || request.path.starts_with("/api/ai/")
            || request
                .path
                .starts_with(op_editor_core::auth_routes::API_PREFIX)
            || request
                .path
                .starts_with(op_editor_core::collab_routes::API_PREFIX)
            || request.path.starts_with("/api/figma/"))
}

/// The refusal a sensitive browser POST earns before any route sees it, if it
/// earns one.
///
/// One function because there are two callers: the route tier
/// ([`super::connection::dispatch`]) and the account tier
/// ([`super::account_routes`]), which is dispatched AHEAD of it — signing in is
/// exactly the request that has no credential yet, so it cannot wait for a
/// verified identity. Two copies of this check is how one of them ends up
/// missing the content-type half and a drive-by page gets to post a "simple
/// request" at the sign-in route.
///
/// The two halves, in order:
///
/// * **Origin.** A cross-site page can post to us; the `Origin` header is what
///   says whether the request came from this deployment's own page. A missing
///   `Origin` is allowed — native clients send none — and the loopback case is
///   trusted without configuration, exactly as the credentials route already
///   trusts it. `allowed_origins` is the deployment's own list, the same one
///   the online accept loop reads out of its tenant registry, so "an origin
///   this deployment answers for" is decided once.
/// * **Content type.** `text/plain`, form encoding, and no declared type are
///   all "simple requests" a browser sends without a preflight, so a page that
///   cannot pass the Origin check would otherwise still get its bytes in.
pub(super) fn sensitive_post_refusal(
    request: &crate::mcp_serve::HttpRequest,
    allowed_origins: &[String],
) -> Option<(&'static str, &'static str)> {
    if !is_sensitive_browser_post(request) {
        return None;
    }
    if !sensitive_origin_allowed(request, allowed_origins) {
        return Some((
            "403 Forbidden",
            "cross-origin sensitive request is forbidden",
        ));
    }
    if !content_type_is_json(request.content_type.as_deref()) {
        return Some((
            "415 Unsupported Media Type",
            "this route requires Content-Type: application/json",
        ));
    }
    None
}

/// `application/json` (optionally with parameters, e.g. `; charset=utf-8`).
pub(super) fn content_type_is_json(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json")
    })
}

/// The env spelling of an allowlist, split into entries.
pub(super) fn allowed_origins_from_config(allowed_origins: Option<&str>) -> Vec<String> {
    allowed_origins
        .into_iter()
        .flat_map(|origins| origins.split(','))
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(str::to_string)
        .collect()
}

/// The same check, for a caller that holds the allowlist as the environment
/// spelling of it. The route tiers pass the parsed list instead: it is the one
/// the deployment was started with, so a request is admitted by the same list
/// it is then judged against.
pub(super) fn credential_request_origin_allowed_with_config(
    request: &crate::mcp_serve::HttpRequest,
    allowed_origins: Option<&str>,
) -> bool {
    sensitive_origin_allowed(request, &allowed_origins_from_config(allowed_origins))
}

/// Whether this request's browser `Origin` may reach a route that acts on the
/// caller's account.
///
/// The one implementation of the comparison; the two wrappers above and below
/// differ only in where the allowlist came from. That is deliberate — the
/// daemon reads the same `OPENPENCIL_WEB_ALLOWED_ORIGINS` variable once at
/// start-up into the tenant registry and again per request here, and two
/// comparison implementations would be two answers to "is this origin ours".
pub(super) fn sensitive_origin_allowed(
    request: &crate::mcp_serve::HttpRequest,
    allowed_origins: &[String],
) -> bool {
    let Some(origin) = request.origin.as_deref() else {
        // Non-browser clients do not normally send Origin. Server persistence
        // is an opt-in private-deployment feature, so those clients remain
        // usable while browser requests are constrained by the unforgeable
        // Origin header.
        return true;
    };
    let Some(host) = request.host.as_deref() else {
        return false;
    };
    let Some(origin) = parse_http_origin(origin) else {
        return false;
    };
    let Ok(host) = reqwest::Url::parse(&format!("http://{host}/")) else {
        return false;
    };
    let same_request_authority =
        origin
            .host_str()
            .zip(host.host_str())
            .is_some_and(|(origin_host, request_host)| {
                let request_port = host.port().or_else(|| match origin.scheme() {
                    "http" => Some(80),
                    "https" => Some(443),
                    _ => None,
                });
                origin_host.eq_ignore_ascii_case(request_host)
                    && origin.port_or_known_default() == request_port
            });
    if !same_request_authority {
        return false;
    }
    if origin.host_str().is_some_and(is_loopback_web_host) {
        return true;
    }
    allowed_origins
        .iter()
        .filter_map(|configured| parse_http_origin(configured))
        .any(|configured| same_url_origin(&origin, &configured))
}

pub(crate) fn parse_http_origin(value: &str) -> Option<reqwest::Url> {
    let origin = reqwest::Url::parse(value).ok()?;
    (matches!(origin.scheme(), "http" | "https")
        && origin.username().is_empty()
        && origin.password().is_none()
        && origin.host_str().is_some()
        && origin.path() == "/"
        && origin.query().is_none()
        && origin.fragment().is_none())
    .then_some(origin)
}

pub(super) fn same_url_origin(left: &reqwest::Url, right: &reqwest::Url) -> bool {
    left.scheme() == right.scheme()
        && left
            .host_str()
            .zip(right.host_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        && left.port_or_known_default() == right.port_or_known_default()
}

pub(super) fn is_loopback_web_host(host: &str) -> bool {
    let ip_literal = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || host
            .to_ascii_lowercase()
            .strip_suffix(".localhost")
            .is_some_and(|prefix| !prefix.is_empty())
        || ip_literal
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}
