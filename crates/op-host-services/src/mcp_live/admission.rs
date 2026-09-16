//! Admission control for the live-GUI MCP endpoint (`127.0.0.1:<port>/mcp`).
//!
//! The live endpoint drives the on-screen document, and during a
//! collaboration session that document is the SHARED one. The gate here is
//! browser screening — it closes the DNS-rebinding path that would let any
//! web page on the machine reach this endpoint.
//!
//! [`check_boundary`] — a `Host` that is not a numeric loopback literal on
//! the bound port, or ANY `Origin` other than this instance's own loopback
//! origin, is refused. Both headers are browser-controlled but not
//! page-forgeable, which is what actually closes DNS rebinding: a rebound
//! `evil.com` page still sends `Host: evil.com:<port>` and
//! `Origin: http://evil.com`. Requests with no `Origin` at all are normal
//! non-browser clients (the `op` CLI, the VS Code MCP proxy, a local agent
//! runner) and pass.
//!
//! No per-instance `X-OpenPencil-Token` is demanded: the local desktop and
//! a self-hosted serve-web daemon trust every local process that clears the
//! boundary — the token it published in `~/.openpencil/.op-mcp-port` and the
//! `ping` reply was readable by any such process anyway, so it only added
//! friction for a caller that had only the URL (a bare MCP client). The
//! online multi-tenant daemon is a SEPARATE request loop that authenticates
//! per account (`web_canvas_server::RequestAuth`, a `Bearer` token the hub
//! introspects), so relaxing this endpoint does not touch online auth.
//! `CollabGatePolicy` still runs on the UI thread for each apply and decides
//! what a session permits. `openpencil/shutdown` keeps its own body-carried
//! token check (`mcp_serve::shutdown_request_id`), unchanged.
//!
//! Deliberately still tokenless too: `OPTIONS` preflight, and the stateless
//! `initialize` / `notifications/initialized` / `ping` probes, which carry
//! no document data and are how a client discovers this instance.
//!
//! # No route here is admitted by origin alone
//!
//! Two routes used to widen the `Origin` rule to `chrome-extension://<id>`
//! callers: `POST /api/import/web-snapshot` and the paid
//! `/api/generate/design-md` job. Both existed for the OpenPencil Chrome
//! extension, which is gone from the tree and is not planned (#69, #81), and
//! both were removed with it rather than left as an extension-shaped door
//! with no holder. What that widening actually granted is worth recording,
//! because it is what the removal closes: the shape check on an extension id
//! is unforgeable to a *web page* but free to any non-browser client, and in
//! its default (unpinned) mode the snapshot route admitted EVERY installed
//! extension — not one known extension — to write into the live document
//! without a token. The paid design route was stricter: an unpinned extension
//! reached its handler only to read an `extensionNotPaired` refusal, and a
//! model turn needed an id pinned in `OPENPENCIL_EXTENSION_ALLOWED_IDS` (or,
//! for a non-browser caller, no `Origin` at all).
//!
//! The capabilities those routes fronted are untouched and still reachable
//! by the callers that actually exist: `import_web_snapshot` is a registered
//! MCP tool (so `/mcp` and the `op` CLI reach it), and the `design.md`
//! pipeline runs in-app against the selected chat model.

use std::fmt;

/// JSON-RPC error code for a refused request. Server-defined range
/// (-32000..=-32099), one step away from the -32000 this endpoint already
/// uses for "server busy" / "Invalid or missing session ID".
const DENIED_CODE: i32 = -32001;

/// Per-instance admission material for the live endpoint: the token the
/// server published and the port it actually bound. Shared by every
/// connection thread (`Arc`), immutable for the life of the server.
pub(super) struct LiveAdmission {
    token: String,
    port: u16,
}

impl LiveAdmission {
    pub(super) fn new(token: String, port: u16) -> Self {
        Self { token, port }
    }

    /// The per-instance identity token — also what the `ping` reply and
    /// the `openpencil/shutdown` check use, so the wire contract the CLI
    /// already knows is preserved verbatim.
    pub(super) fn token(&self) -> &str {
        &self.token
    }

    pub(super) fn port(&self) -> u16 {
        self.port
    }
}

/// Why a request was refused. A typed enum rather than a `String` (the
/// workspace rule) — and deliberately NOT an `McpLiveError` variant: every
/// value here is a *client* fault answered on the wire with a 401/403 and
/// a JSON-RPC error body, never a server fault the accept loop logs and
/// turns into a 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AdmissionDenial {
    /// `Host` absent, not a numeric loopback literal, or carrying a port
    /// other than the one this server bound.
    ForeignHost,
    /// An `Origin` header that is not this instance's own loopback origin.
    ForeignOrigin,
}

impl AdmissionDenial {
    /// HTTP status line. Boundary failures are 403 (the caller is not
    /// allowed to talk to this endpoint at all); token failures are 401
    /// (the caller may retry with the credential it was given).
    pub(super) fn http_status(self) -> &'static str {
        match self {
            AdmissionDenial::ForeignHost | AdmissionDenial::ForeignOrigin => "403 Forbidden",
        }
    }

    /// Client-facing reason. Intentionally coarse: it names the gate, not
    /// which byte of the token differed.
    pub(super) fn message(self) -> &'static str {
        match self {
            AdmissionDenial::ForeignHost => {
                "live MCP endpoint accepts loopback requests only (bad Host header)"
            }
            AdmissionDenial::ForeignOrigin => {
                "live MCP endpoint refuses cross-origin requests (bad Origin header)"
            }
        }
    }
}

impl fmt::Display for AdmissionDenial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Gate 1 — browser screening, applied to EVERY request (including the
/// stateless probes and the REST document-sync route) before any routing.
///
/// One rule for every path: `Host` must be this instance's numeric loopback
/// authority and `Origin`, when present, must be this instance's own loopback
/// origin. No route widens it — see the module doc for the extension-shaped
/// widening this replaced and why it was removed rather than kept.
pub(super) fn check_boundary(
    req: &crate::mcp_serve::HttpRequest,
    admission: &LiveAdmission,
) -> Result<(), AdmissionDenial> {
    if !host_allowed(req.host.as_deref(), admission.port()) {
        return Err(AdmissionDenial::ForeignHost);
    }
    if !origin_allowed(req.origin.as_deref(), admission.port()) {
        return Err(AdmissionDenial::ForeignOrigin);
    }
    Ok(())
}

/// JSON-RPC error body for a refused `/mcp` request, echoing the caller's
/// request id so a client correlates the refusal with its call instead of
/// hanging (same discipline as `op_mcp::parser`'s parse-failure path).
pub(super) fn denial_json_rpc(request_body: &str, denial: AdmissionDenial) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","error":{{"code":{DENIED_CODE},"message":"{}"}},"id":{}}}"#,
        crate::mcp_serve::json_escape(denial.message()),
        request_id_raw(request_body)
    )
}

/// The caller's top-level JSON-RPC `id`, verbatim (so a string id stays a
/// string id), or `null` when the body is not a JSON object / has no id.
fn request_id_raw(request_body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(request_body)
        .ok()
        .and_then(|value| value.get("id").map(|id| id.to_string()))
        .unwrap_or_else(|| "null".to_string())
}

/// `Host` must name a numeric loopback address. A DNS name — including
/// `localhost` — is refused: rebinding attacks work precisely by pointing
/// a name at 127.0.0.1, and a browser writes the name it was given into
/// `Host`, so accepting names would leave the hole open.
///
/// The port is checked when present. It may be absent: `op`'s own
/// transport sends a bare `Host: 127.0.0.1`
/// (`op_rpc_transport::TcpJsonRpc::http_post_request`), and a browser can
/// never produce that against a non-80 port — it always writes the target
/// port it dialled. So "no port" identifies a non-browser client rather
/// than widening the browser surface.
fn host_allowed(host: Option<&str>, expected_port: u16) -> bool {
    // HTTP/1.1 requires `Host`; every browser and every client in this
    // repo sends it. Absent ⇒ refuse rather than guess.
    let Some(raw) = host else {
        return false;
    };
    let Some((host, port)) = split_authority(raw.trim()) else {
        return false;
    };
    is_numeric_loopback(host) && port.is_none_or(|port| port == expected_port)
}

/// Any `Origin` other than this instance's own loopback origin is refused.
/// `None` is the normal non-browser case (CLI / proxy) and passes — a page
/// cannot suppress the header on a cross-origin request, so "no Origin"
/// is not something an attacker page can claim.
fn origin_allowed(origin: Option<&str>, expected_port: u16) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let origin = origin.trim();
    // The live endpoint is plain HTTP on loopback, so only an `http://`
    // origin can possibly be it; `null` (sandboxed iframe / `file://`)
    // and any `https://` page fall through to a refusal.
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    // A real serialized origin is scheme + authority and nothing else.
    if authority.contains(['/', '@', '?', '#']) {
        return false;
    }
    let Some((host, port)) = split_authority(authority) else {
        return false;
    };
    is_numeric_loopback(host) && port.unwrap_or(80) == expected_port
}

/// The `Access-Control-Allow-Origin` value this endpoint may echo back for
/// `req` — the ONE origin the boundary accepts, or `None` (emit no header at
/// all).
///
/// Never `*`. A permissive wildcard on a loopback endpoint lets ANY browser
/// context that can reach the socket read the reply. `None` covers the
/// non-browser callers (`op`, the MCP proxy), which send no `Origin` and never
/// look at CORS headers.
pub(super) fn cors_origin_for<'a>(
    req: &'a crate::mcp_serve::HttpRequest,
    admission: &LiveAdmission,
) -> Option<&'a str> {
    let origin = req.origin.as_deref()?.trim();
    // Exactly the boundary's own answer: an origin that `check_boundary`
    // refused must never be echoed back, on any path.
    origin_allowed(Some(origin), admission.port()).then_some(origin)
}

/// Split an HTTP authority (`127.0.0.1:3100`, `127.0.0.1`, `[::1]:3100`)
/// into host and optional port. A malformed port refuses the whole value.
fn split_authority(value: &str) -> Option<(&str, Option<u16>)> {
    if let Some(rest) = value.strip_prefix('[') {
        let (inside, tail) = rest.split_once(']')?;
        return match tail {
            "" => Some((inside, None)),
            tail => {
                let port = tail.strip_prefix(':')?.parse::<u16>().ok()?;
                Some((inside, Some(port)))
            }
        };
    }
    match value.rsplit_once(':') {
        // An unbracketed IPv6 literal lands here with a nonsense split;
        // `is_numeric_loopback` then rejects the truncated host, which is
        // correct — unbracketed IPv6 in an authority is malformed anyway.
        Some((host, port)) => Some((host, Some(port.parse::<u16>().ok()?))),
        None => Some((value, None)),
    }
}

/// A numeric IP literal in a loopback range (127.0.0.0/8 or `::1`).
/// Names never qualify.
fn is_numeric_loopback(host: &str) -> bool {
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
