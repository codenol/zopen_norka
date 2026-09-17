//! End-to-end tests for the multi-account accept loop.
//!
//! These drive real requests through `serve_one_online`, so they cover the
//! whole path a public deployment exposes: header parse → identity → tenant →
//! route table. The isolation cases are the hard gate — a failure there means
//! one account can read another account's document and the AI credentials
//! sitting in its in-memory editor.

use std::net::{IpAddr, Ipv4Addr};

use super::*;

struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Read for MockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for MockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One request, as the wire sees it.
/// The public origin this test deployment answers for.
const PUBLIC_ORIGIN: &str = "https://canvas.example";

/// The document these tests share. A share is about ONE document now
/// (issue #127), so a request addressed at another account's tenant must say
/// which one — exactly as a browser does with `?file=<key>`.
pub(super) const SHARED_DOC: &str = "shared-doc";

struct Request {
    method: &'static str,
    path: &'static str,
    body: String,
    token: Option<&'static str>,
    content_type: Option<&'static str>,
    /// The session cookie's value, when the request carries one.
    cookie: Option<String>,
    origin: Option<&'static str>,
    /// The `User-Agent` header, when the request carries one. A browser always
    /// does; the tests that do not care about it leave it off, which is also
    /// what a client that identifies itself as nothing looks like.
    user_agent: Option<&'static str>,
    /// Addresses the request at another account's tenant, as the browser does
    /// with `?tenant=` on the page URL.
    tenant: Option<&'static str>,
    /// Names the DOCUMENT the request is about, as the browser does with
    /// `?file=<key>`.
    file: Option<&'static str>,
}

impl Request {
    fn new(method: &'static str, path: &'static str) -> Self {
        Self {
            method,
            path,
            body: String::new(),
            token: None,
            content_type: None,
            cookie: None,
            origin: None,
            user_agent: None,
            tenant: None,
            file: None,
        }
    }

    fn json(method: &'static str, path: &'static str, body: &str) -> Self {
        Self {
            body: body.to_string(),
            content_type: Some("application/json"),
            ..Self::new(method, path)
        }
    }

    fn with_bearer(mut self, token: &'static str) -> Self {
        self.token = Some(token);
        self
    }

    /// Present the deployment's session cookie, as a browser would. Takes the
    /// value, so a token a sign-in just handed back is presented the same way.
    fn with_session(mut self, session: &str) -> Self {
        self.cookie = Some(session.to_string());
        self
    }

    fn with_origin(mut self, origin: &'static str) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Present the browser's own description of itself, as every real client
    /// does — and as the session row is expected to remember (issue #76).
    fn with_user_agent(mut self, user_agent: &'static str) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Name the document the request is about, beside the tenant it addresses.
    fn with_file(mut self, file: &'static str) -> Self {
        self.file = Some(file);
        self
    }

    fn wire(&self) -> String {
        let auth = self
            .token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let content_type = self
            .content_type
            .map(|t| format!("Content-Type: {t}\r\n"))
            .unwrap_or_default();
        let cookie = self
            .cookie
            .as_deref()
            .map(|c| {
                format!(
                    "Cookie: {}={c}\r\n",
                    super::super::tenant_auth::SESSION_COOKIE_NAME
                )
            })
            .unwrap_or_default();
        let origin = self
            .origin
            .map(|o| format!("Origin: {o}\r\n"))
            .unwrap_or_default();
        let user_agent = self
            .user_agent
            .map(|agent| format!("User-Agent: {agent}\r\n"))
            .unwrap_or_default();
        let target = match (self.tenant, self.file) {
            (Some(tenant), Some(file)) => format!("{}?tenant={tenant}&file={file}", self.path),
            (Some(tenant), None) => format!("{}?tenant={tenant}", self.path),
            // A document without a tenant: the caller's own, named explicitly.
            (None, Some(file)) => format!("{}?file={file}", self.path),
            (None, None) => self.path.to_string(),
        };
        format!(
            "{} {target} HTTP/1.1\r\nHost: canvas.example\r\n{auth}{cookie}{origin}{user_agent}\
             {content_type}Content-Length: {}\r\n\r\n{}",
            self.method,
            self.body.len(),
            self.body
        )
    }
}

fn verifier() -> StaticVerifier {
    StaticVerifier::parse("tokA=userA,tokB=userB")
}

fn registry() -> TenantRegistry {
    TenantRegistry::new(
        3102,
        TenantLimits::default(),
        vec![PUBLIC_ORIGIN.to_string()],
    )
}

/// The address every request in these tests arrives from, in the shape the
/// accept loop gets it (`TcpStream::peer_addr`, `.ip()`): a documentation
/// address (`TEST-NET-3`, RFC 5737), so a test that asserts on it cannot be
/// reading a real one by accident.
///
/// Passed to `serve_one_online` because that is where the real loop reads it —
/// off the socket, before the stream is moved into the connection thread — and
/// a test that omitted it would leave the per-address sign-in budget and the
/// address on the session row (issue #76) out of force rather than proving
/// anything about them.
pub(super) const TEST_PEER: Option<IpAddr> = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));

/// Drive one request through the online loop and return the raw response.
fn serve(registry: &TenantRegistry, verifier: &StaticVerifier, request: Request) -> String {
    serve_as(registry, verifier, None, request)
}

/// The same, with this deployment's own accounts behind it.
///
/// The account tier is passed in the shape the accept loop passes it, so a
/// request that reaches it is dispatched by exactly the code a deployment runs
/// — including the fact that it is reached BEFORE the identity check.
fn serve_as(
    registry: &TenantRegistry,
    verifier: &dyn IdentityVerifier,
    accounts: Option<&super::super::account_routes::AccountAuth>,
    request: Request,
) -> String {
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.wire().into_bytes()),
        output: Vec::new(),
    };
    // An open barrier: the shutdown path is exercised by its own tests.
    let barrier = crate::web_canvas_server::tenant::WriteBarrier::default();
    serve_one_online(
        &mut stream,
        registry,
        verifier,
        accounts,
        &barrier,
        TEST_PEER,
    )
    .expect("serve_one_online");
    String::from_utf8_lossy(&stream.output).into_owned()
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or_default()
}

fn body(response: &str) -> serde_json::Value {
    let payload = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has a body");
    serde_json::from_str(payload).unwrap_or(serde_json::Value::Null)
}

/// A minimal canonical document, in the shape `POST /api/mcp/document` takes.
const SYNC_BODY: &str = r##"{"document":{"version":"1.0.0","children":[{"id":"n9","type":"rectangle","name":"Tenant Rect","x":1,"y":2,"width":80,"height":40,"fill":[{"type":"solid","color":"#123456"}]}]},"sourceClientId":"web"}"##;

// ---------------------------------------------------------------------------
// Isolation — the hard gate.
// ---------------------------------------------------------------------------

#[test]
fn one_account_document_write_is_invisible_to_another_account() {
    let registry = registry();
    let verifier = verifier();

    let pushed = serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    assert_eq!(status_line(&pushed), "HTTP/1.1 200 OK", "{pushed}");

    let a = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    let b = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/document").with_bearer("tokB"),
    );

    assert_eq!(body(&a)["version"], 1);
    assert!(
        a.contains("Tenant Rect"),
        "the writer sees its own document: {a}"
    );
    assert_eq!(body(&b)["version"], 0, "{b}");
    assert!(
        !b.contains("Tenant Rect"),
        "one account's document must never appear in another's: {b}"
    );
}

#[test]
fn one_account_editor_credentials_are_invisible_to_another_account() {
    let registry = registry();
    let verifier = verifier();
    // Reach into A's tenant the way a credential write would, then ask B for
    // the models its own editor can serve. A shared editor would leak the
    // provider list — and the keys behind it — across accounts.
    {
        let lease = registry
            .lease_for(
                &verifier
                    .resolve(&PresentedCredentials {
                        bearer: Some("tokA".into()),
                        session_cookie: None,
                    })
                    .expect("userA"),
            )
            .expect("lease A");
        let mut guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
        guard.editor.editor_ui.agent_settings.mcp_server.port = 4242;
    }

    let lease_b = registry
        .lease_for(
            &verifier
                .resolve(&PresentedCredentials {
                    bearer: Some("tokB".into()),
                    session_cookie: None,
                })
                .expect("userB"),
        )
        .expect("lease B");
    let guard = lease_b.state().lock().unwrap_or_else(|p| p.into_inner());
    assert_ne!(
        guard.editor.editor_ui.agent_settings.mcp_server.port, 4242,
        "account settings, and the credentials beside them, must not be shared"
    );
}

#[test]
fn each_account_polls_its_own_version_counter() {
    let registry = registry();
    let verifier = verifier();
    serve(
        &registry,
        &verifier,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA"),
    );
    let a = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/version").with_bearer("tokA"),
    );
    let b = serve(
        &registry,
        &verifier,
        Request::new("GET", "/api/mcp/version").with_bearer("tokB"),
    );
    assert_eq!(body(&a)["version"], 1);
    assert_eq!(body(&b)["version"], 0);
}

// ---------------------------------------------------------------------------
// Authentication.
// ---------------------------------------------------------------------------

#[test]
fn a_request_with_no_credential_is_refused_before_any_tenant_exists() {
    let registry = registry();
    let response = serve(
        &registry,
        &verifier(),
        Request::new("GET", "/api/mcp/document"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 401 Unauthorized");
    assert_eq!(
        registry.tenant_count(),
        0,
        "an unauthenticated request must not be able to make the daemon allocate a tenant"
    );
}

#[test]
fn an_unknown_token_is_refused_with_the_same_answer_as_a_missing_one() {
    let registry = registry();
    let missing = serve(
        &registry,
        &verifier(),
        Request::new("GET", "/api/mcp/version"),
    );
    let unknown = serve(
        &registry,
        &verifier(),
        Request::new("GET", "/api/mcp/version").with_bearer("tokZ"),
    );
    assert_eq!(status_line(&missing), status_line(&unknown));
    assert_eq!(body(&missing)["error"], body(&unknown)["error"]);
}

#[test]
fn the_static_bundle_is_reachable_without_any_credential() {
    // The host page has to load before the browser can present a session, so
    // the static tier answers anonymously. Which page it serves depends on
    // whether a wasm bundle is present in this checkout — what matters here
    // is that the static layer, not the 401 gate, is what answered.
    let response = serve(&registry(), &verifier(), Request::new("GET", "/"));
    assert!(
        response.contains("text/html"),
        "the static tier must answer `/` without a credential: {response}"
    );
    assert!(
        !response.contains("unauthorized"),
        "`/` must not be behind the identity gate: {response}"
    );
}

#[test]
fn a_cors_preflight_is_answered_without_a_credential() {
    let response = serve(
        &registry(),
        &verifier(),
        Request::new("OPTIONS", "/api/mcp/document"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 204 No Content");
    assert!(response.contains("Authorization"), "{response}");
}

#[test]
fn a_deployment_with_no_verifier_answers_503_rather_than_serving_anyone() {
    let response = serve(
        &registry(),
        &StaticVerifier::parse(""),
        Request::new("GET", "/api/mcp/document").with_bearer("tokA"),
    );
    assert_eq!(status_line(&response), "HTTP/1.1 503 Service Unavailable");
    assert_eq!(body(&response)["error"], "verifier-unavailable");
}

// The route-table and origin-hardening scenarios are `include!`d rather
// than moved into a `mod`: a `mod` renames every test after it (the
// harness names a test by the module path it is defined in), and the
// before/after `--list` diff has to stay empty. The fragment is the
// original lines 412-864, byte-for-byte.
include!("online_run_loop_tests_route_table.rs");

#[cfg(test)]
#[path = "online_mcp_tests.rs"]
mod mcp_profile;

#[cfg(test)]
#[path = "online_share_tests.rs"]
mod share;

#[cfg(test)]
#[path = "online_files_tests.rs"]
mod files;

/// The account tier driven through the loop: the tier's whole point is WHERE it
/// runs, and only an end-to-end request can show that. See the file's docs.
#[path = "online_account_tests.rs"]
mod account_tests;
