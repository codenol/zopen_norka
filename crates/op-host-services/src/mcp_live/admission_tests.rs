//! Admission-gate tests for the live-MCP endpoint. Driven through the real
//! `serve_connection` router (generic over the stream, like the web-canvas
//! daemon's connection tests) so they cover the wiring, not just the
//! predicates: a refused request must never reach the UI-request channel.

use super::super::*;
use super::*;

const PORT: u16 = 51234;
const TOKEN: &str = "d34db33f-cafe";
/// A read-only tool call. Reads were entirely unauthenticated before this
/// gate existed, so the read path is exactly what these tests must pin.
const LIST_PAGES_CALL: &str = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"list_pages","arguments":{}}}"#;

struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl std::io::Read for MockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.input, buf)
    }
}

impl std::io::Write for MockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Run one raw HTTP request through the live router and return the raw
/// response.
fn drive(request: &str, req_tx: &Sender<UiRequest>) -> String {
    let admission = LiveAdmission::new(TOKEN.to_string(), PORT);
    let stateful_lock = Mutex::new(());
    let quit_flag = AtomicBool::new(false);
    let wake_ui: UiWake = Arc::new(|| {});
    let client_identity = Mutex::new(None);
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.as_bytes().to_vec()),
        output: Vec::new(),
    };
    serve_connection(
        &mut stream,
        req_tx,
        &admission,
        &stateful_lock,
        &quit_flag,
        &wake_ui,
        &client_identity,
    )
    .expect("a refused request is answered on the wire, never a server error");
    String::from_utf8_lossy(&stream.output).into_owned()
}

fn request(path: &str, headers: &str, body: &str) -> String {
    request_with_method("POST", path, headers, body)
}

fn request_with_method(method: &str, path: &str, headers: &str, body: &str) -> String {
    format!(
        "{method} {path} HTTP/1.1\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Headers a token-carrying non-browser client sends (VS Code MCP proxy
/// shape: explicit port, no `Origin`).
fn authed_headers() -> String {
    format!("Host: 127.0.0.1:{PORT}\r\nX-OpenPencil-Token: {TOKEN}\r\n")
}

#[test]
fn a_tokenless_tool_call_is_served() {
    // No X-OpenPencil-Token header: the local endpoint admits every caller
    // that clears the Host/Origin boundary, so a bare MCP client (only a
    // URL, no discovered token) reaches the UI thread and is served.
    let (req_tx, req_rx) = mpsc::channel();
    let responder = thread::spawn(move || match req_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(UiRequest::ListPages { ack }) => ack
            .send(op_mcp::ListPages {
                page_count: 3,
                active_page_index: 1,
                pages: vec![("p1".to_string(), "One".to_string())],
            })
            .is_ok(),
        _ => false,
    });
    let headers = format!("Host: 127.0.0.1:{PORT}\r\n");
    let response = drive(&request("/mcp", &headers, LIST_PAGES_CALL), &req_tx);

    assert!(
        responder.join().expect("responder thread"),
        "a tokenless tool call must reach the UI thread"
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(!response.contains("-32001"), "{response}");
}

#[test]
fn authenticated_tool_call_is_served() {
    let (req_tx, req_rx) = mpsc::channel();
    let responder = thread::spawn(move || match req_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(UiRequest::ListPages { ack }) => ack
            .send(op_mcp::ListPages {
                page_count: 3,
                active_page_index: 1,
                pages: vec![("p1".to_string(), "One".to_string())],
            })
            .is_ok(),
        _ => false,
    });
    let response = drive(
        &request("/mcp", &authed_headers(), LIST_PAGES_CALL),
        &req_tx,
    );

    assert!(
        responder.join().expect("responder thread"),
        "an authenticated tool call must reach the UI thread"
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("pageCount"), "{response}");
    assert!(!response.contains("-32001"), "{response}");
}

/// The `op` CLI's exact wire shape: no `Origin` (not a browser) and a bare
/// `Host: 127.0.0.1` with no port
/// (`op_rpc_transport::TcpJsonRpc::http_post_request`). It must keep
/// working with the token it is handed by the discovery file.
#[test]
fn cli_request_without_origin_or_host_port_is_served() {
    let (req_tx, req_rx) = mpsc::channel();
    let responder = thread::spawn(move || match req_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(UiRequest::ListPages { ack }) => ack
            .send(op_mcp::ListPages {
                page_count: 1,
                active_page_index: 0,
                pages: vec![("p1".to_string(), "One".to_string())],
            })
            .is_ok(),
        _ => false,
    });
    let headers = format!("Host: 127.0.0.1\r\nX-OpenPencil-Token: {TOKEN}\r\n");
    let response = drive(&request("/mcp", &headers, LIST_PAGES_CALL), &req_tx);

    assert!(
        responder.join().expect("responder thread"),
        "a portless-Host CLI request must still reach the UI thread"
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

#[test]
fn foreign_origin_tool_call_is_refused() {
    let (req_tx, req_rx) = mpsc::channel();
    // Even WITH the right token: a browser page that somehow learned the
    // token is still not an allowed caller.
    let headers = format!(
        "Host: 127.0.0.1:{PORT}\r\nOrigin: http://evil.example\r\nX-OpenPencil-Token: {TOKEN}\r\n"
    );
    let response = drive(&request("/mcp", &headers, LIST_PAGES_CALL), &req_tx);

    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    assert!(req_rx.try_recv().is_err(), "refused before the UI thread");

    // Its own loopback origin on its own port is the one allowed value.
    assert!(origin_allowed(
        Some(&format!("http://127.0.0.1:{PORT}")),
        PORT
    ));
    // Right host, wrong port — a different local server's page.
    assert!(!origin_allowed(Some("http://127.0.0.1:1"), PORT));
    // `localhost` is a NAME, and names are the rebinding vector.
    assert!(!origin_allowed(
        Some(&format!("http://localhost:{PORT}")),
        PORT
    ));
    assert!(!origin_allowed(Some("null"), PORT));
    assert!(origin_allowed(None, PORT), "non-browser clients pass");
}

#[test]
fn non_loopback_or_wrong_port_host_is_refused() {
    let (req_tx, req_rx) = mpsc::channel();
    // The DNS-rebinding shape: the browser resolved `evil.example` to
    // 127.0.0.1 but still writes the NAME into `Host`.
    let headers =
        format!("Host: evil.example:{PORT}\r\nX-OpenPencil-Token: {TOKEN}\r\nOrigin: http://evil.example\r\n");
    let response = drive(&request("/mcp", &headers, LIST_PAGES_CALL), &req_tx);
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    assert!(req_rx.try_recv().is_err(), "refused before the UI thread");

    // A loopback literal, but a port this server never bound.
    let headers = format!(
        "Host: 127.0.0.1:{}\r\nX-OpenPencil-Token: {TOKEN}\r\n",
        PORT + 1
    );
    let response = drive(&request("/mcp", &headers, LIST_PAGES_CALL), &req_tx);
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    assert!(req_rx.try_recv().is_err(), "refused before the UI thread");

    assert!(host_allowed(Some(&format!("127.0.0.1:{PORT}")), PORT));
    assert!(host_allowed(Some(&format!("[::1]:{PORT}")), PORT));
    assert!(!host_allowed(Some(&format!("localhost:{PORT}")), PORT));
    assert!(!host_allowed(Some(&format!("10.0.0.4:{PORT}")), PORT));
    assert!(!host_allowed(None, PORT), "a missing Host is refused");
}

/// The identity probes stay tokenless on purpose: `op` discovers this
/// instance by pinging it and matching the reply's token against the
/// discovery file. Gating `ping` would break discovery for every CLI.
#[test]
fn ping_probe_stays_tokenless_for_cli_discovery() {
    let (req_tx, req_rx) = mpsc::channel();
    let headers = "Host: 127.0.0.1\r\n".to_string();
    let ping = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
    let response = drive(&request("/mcp", &headers, ping), &req_tx);

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""mode":"live""#), "{response}");
    assert!(response.contains(TOKEN), "{response}");
    assert!(
        req_rx.try_recv().is_err(),
        "ping never touches the UI thread"
    );

    // …but a ping from a foreign origin is still refused, so a web page
    // cannot use the probe to harvest the token.
    let headers = format!("Host: 127.0.0.1:{PORT}\r\nOrigin: http://evil.example\r\n");
    let response = drive(&request("/mcp", &headers, ping), &req_tx);
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    assert!(!response.contains(TOKEN), "{response}");
}

// --- the removed extension routes (`POST /api/import/web-snapshot`,
// --- `/api/generate/design-md`) ---

/// A well-formed Chrome extension id: 32 characters from `a`–`p`.
const EXTENSION_ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

/// The two routes that existed only for the OpenPencil Chrome extension are
/// gone, and this is the test that says so (#81).
///
/// Why they are gone rather than guarded differently: they were the ONLY
/// paths that widened the boundary above to `chrome-extension://<id>`
/// callers, and their client — the extension — no longer exists in the tree
/// and is not planned (#69). The widening is what made them attack surface:
/// a `chrome-extension://` origin is unforgeable to a *web page*, but it is
/// free to any non-browser client that sets the header itself, and in the
/// default unpinned mode (`OPENPENCIL_EXTENSION_ALLOWED_IDS` unset) the
/// snapshot route admitted EVERY installed extension — not one known
/// extension — to POST a snapshot into the live document with no token at all
/// (the design route was stricter: an unpinned extension could only read its
/// own `extensionNotPaired` refusal, and model work needed a pinned id). An
/// allowlist nobody can populate is a setting that only confuses, so it went
/// with them: no environment variable re-opens these paths now.
///
/// What they fronted is not lost. `import_web_snapshot` is still a registered
/// MCP tool, reachable over `/mcp` below and through the `op` CLI, and the
/// `design.md` pipeline still runs in-app against the selected chat model.
#[test]
fn the_extension_only_routes_are_gone_for_every_caller() {
    const JOB: &str = "/api/generate/design-md/0123456789abcdef0123456789abcdef";
    for (method, path, body) in [
        ("POST", "/api/import/web-snapshot", "{}"),
        ("POST", "/api/generate/design-md", "{}"),
        ("GET", "/api/generate/design-md", ""),
        ("GET", JOB, ""),
        ("DELETE", JOB, ""),
    ] {
        // Only the header shapes the boundary still admits: a caller that
        // presents an extension origin never gets as far as the router (that
        // refusal is the test below), so the two must be asserted separately
        // or "not found" would paper over "not even routed".
        for headers in [
            // A local non-browser caller: no `Origin` at all.
            format!("Host: 127.0.0.1:{PORT}\r\nContent-Type: application/json\r\n"),
            // And a caller on this instance's own loopback origin.
            format!("Host: 127.0.0.1:{PORT}\r\nOrigin: http://127.0.0.1:{PORT}\r\n"),
        ] {
            let (req_tx, req_rx) = mpsc::channel();
            let response = drive(&request_with_method(method, path, &headers, body), &req_tx);
            assert!(
                response.starts_with("HTTP/1.1 404 Not Found"),
                "{method} {path} must not exist any more: {response}"
            );
            assert!(
                req_rx.try_recv().is_err(),
                "{method} {path} must never reach the UI thread"
            );
        }
    }
}

/// The fact the removal is premised on, pinned directly: a caller that
/// presents a forged extension `Origin` and no identity is REFUSED by the
/// boundary, on every path — including the two paths that used to be the
/// exception, where this exact request was admitted.
#[test]
fn a_forged_extension_origin_is_refused_without_any_identity() {
    for path in [
        "/api/import/web-snapshot",
        "/api/generate/design-md",
        "/mcp",
        "/api/mcp/document",
    ] {
        let (req_tx, req_rx) = mpsc::channel();
        let headers = format!(
            "Host: 127.0.0.1:{PORT}\r\nOrigin: {EXTENSION_ORIGIN}\r\n\
             Content-Type: application/json\r\n"
        );
        let response = drive(&request(path, &headers, LIST_PAGES_CALL), &req_tx);

        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden"),
            "{path}: {response}"
        );
        assert!(
            response.contains("bad Origin header"),
            "the Origin gate must be the one that refused: {response}"
        );
        assert!(
            !response.contains("Access-Control-Allow-Origin"),
            "a refused origin must never be echoed back: {response}"
        );
        assert!(
            req_rx.try_recv().is_err(),
            "{path}: a refused caller must never reach the UI thread"
        );
    }
}

/// `OPTIONS` is the one method that is still answered without a route match —
/// the preflight is a browser's, not a caller's — so it is worth pinning that
/// the answer stays scoped to this instance's own origin and never widens to
/// an extension's.
#[test]
fn the_preflight_refuses_a_forged_extension_origin_and_echoes_only_its_own() {
    let (req_tx, req_rx) = mpsc::channel();
    let headers = format!(
        "Host: 127.0.0.1:{PORT}\r\nOrigin: {EXTENSION_ORIGIN}\r\n\
         Access-Control-Request-Method: POST\r\n"
    );
    let response = drive(
        &request_with_method("OPTIONS", "/api/import/web-snapshot", &headers, ""),
        &req_tx,
    );
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    assert!(
        !response.contains("Access-Control-Allow-Origin"),
        "{response}"
    );
    assert!(req_rx.try_recv().is_err(), "a preflight touches no state");

    let own = format!("http://127.0.0.1:{PORT}");
    let headers = format!(
        "Host: 127.0.0.1:{PORT}\r\nOrigin: {own}\r\n\
         Access-Control-Request-Method: POST\r\n"
    );
    let response = drive(
        &request_with_method("OPTIONS", "/mcp", &headers, ""),
        &req_tx,
    );
    assert!(
        response.starts_with("HTTP/1.1 204 No Content"),
        "{response}"
    );
    assert!(
        response.contains(&format!("Access-Control-Allow-Origin: {own}\r\n")),
        "{response}"
    );
    assert!(
        !response.contains("Access-Control-Allow-Origin: *"),
        "{response}"
    );
}
