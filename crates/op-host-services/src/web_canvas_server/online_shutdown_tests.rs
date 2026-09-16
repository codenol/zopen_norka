//! The shutdown write barrier and its start-up probes.
//!
//! Split out of `online_share_tests.rs` at the 800-line cap; nested under it
//! so `use super::*` still reaches the request builder and helpers.

use super::*;

// ---------------------------------------------------------------------------
// B3: the shutdown write barrier.
// ---------------------------------------------------------------------------

#[test]
fn the_write_barrier_admits_writes_until_shutdown_closes_it() {
    use crate::web_canvas_server::tenant::WriteBarrier;
    let barrier = WriteBarrier::default();
    assert!(!barrier.is_closed());

    let pass = barrier.enter().expect("open");
    assert_eq!(barrier.active(), 1);
    drop(pass);
    assert_eq!(barrier.active(), 0);

    barrier.close();
    assert!(barrier.is_closed());
    assert!(
        barrier.enter().is_none(),
        "a stopping daemon must refuse writes rather than ack one it will not persist"
    );
}

#[test]
fn the_drain_returns_only_when_no_write_is_in_flight() {
    // The process must never flush over a live writer: there is no deadline at
    // which losing acked work becomes acceptable, so the wait is unbounded and
    // the orchestrator's grace period is the outer bound.
    use crate::web_canvas_server::tenant::WriteBarrier;
    let barrier = std::sync::Arc::new(WriteBarrier::default());
    barrier.close();

    let held = barrier.enter_for_test();
    let waiting = std::sync::Arc::clone(&barrier);
    let handle = std::thread::spawn(move || {
        super::super::drain_write_barrier_for_test(&waiting);
    });
    // Still blocked while the pass is held.
    std::thread::sleep(std::time::Duration::from_millis(120));
    assert!(!handle.is_finished(), "the drain must wait for the writer");

    drop(held);
    handle
        .join()
        .expect("the drain returns once the writer finishes");
    assert_eq!(barrier.active(), 0);
}

#[test]
fn a_held_pass_keeps_the_barrier_busy_so_the_flush_waits() {
    // The window this closes: a worker past the connection drain, about to
    // take the state lock, commits after the flush snapshotted the document —
    // having already answered 200.
    use crate::web_canvas_server::tenant::WriteBarrier;
    let barrier = WriteBarrier::default();
    let held = barrier.enter().expect("open");
    barrier.close();
    assert_eq!(
        barrier.active(),
        1,
        "closing must not abandon a write already inside"
    );
    drop(held);
    assert_eq!(barrier.active(), 0, "the drain can now proceed");
}

#[test]
fn closing_between_the_check_and_the_increment_does_not_admit_a_writer() {
    // `enter` re-checks after incrementing precisely so a close landing in
    // that window cannot admit a writer the drain has stopped waiting for.
    use crate::web_canvas_server::tenant::WriteBarrier;
    let barrier = std::sync::Arc::new(WriteBarrier::default());
    barrier.close();
    for _ in 0..100 {
        assert!(barrier.enter().is_none());
    }
    assert_eq!(barrier.active(), 0, "a refused entry must not leak a count");
}

#[test]
fn a_write_during_shutdown_is_refused_rather_than_acked() {
    use crate::web_canvas_server::tenant::WriteBarrier;
    let registry = registry();
    let verifier = verifier();
    let barrier = WriteBarrier::default();
    barrier.close();

    let request = Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("tokA");
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.wire().into_bytes()),
        output: Vec::new(),
    };
    serve_one_online(&mut stream, &registry, &verifier, None, &barrier, TEST_PEER).expect("serve");
    let response = String::from_utf8_lossy(&stream.output).into_owned();
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 503 Service Unavailable",
        "{response}"
    );
    assert_eq!(body(&response)["error"], "shutting-down");
}

#[test]
fn an_mcp_write_tool_is_refused_once_the_barrier_closes() {
    // `/mcp` used to apply straight to the editor with no barrier, so a write
    // tool could commit after the flush snapshot — the exact window the REST
    // path was already protected from.
    use crate::web_canvas_server::tenant::WriteBarrier;
    let registry = registry();
    let verifier = verifier();
    let barrier = WriteBarrier::default();
    barrier.close();

    let call = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"add_page","arguments":{"name":"x"}}}"#;
    let request = Request::json("POST", "/mcp", call).with_bearer("tokA");
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.wire().into_bytes()),
        output: Vec::new(),
    };
    serve_one_online(&mut stream, &registry, &verifier, None, &barrier, TEST_PEER).expect("serve");
    let response = String::from_utf8_lossy(&stream.output).into_owned();
    // A tools/call error envelope, not a transport failure: the client keeps
    // its session and can read why.
    assert_eq!(status_line(&response), "HTTP/1.1 200 OK", "{response}");
    let payload = body(&response);
    assert_eq!(payload["result"]["isError"], true, "{response}");
    assert!(
        payload["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("shutting-down"),
        "{response}"
    );
}

#[test]
fn an_mcp_read_tool_still_works_while_shutting_down() {
    // Only writes are refused: a read cannot be lost by the flush.
    use crate::web_canvas_server::tenant::WriteBarrier;
    let barrier = WriteBarrier::default();
    barrier.close();

    let call = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"get_document_info","arguments":{}}}"#;
    let request = Request::json("POST", "/mcp", call).with_bearer("tokA");
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.wire().into_bytes()),
        output: Vec::new(),
    };
    serve_one_online(
        &mut stream,
        &registry(),
        &verifier(),
        None,
        &barrier,
        TEST_PEER,
    )
    .expect("serve");
    let response = String::from_utf8_lossy(&stream.output).into_owned();
    assert_ne!(body(&response)["result"]["isError"], true, "{response}");
}

#[test]
fn the_write_classification_matches_the_tool_catalog() {
    // The barrier decision is made before dispatch, from this metadata.
    use crate::mcp_serve::tool_profile::tool_writes;
    for write in [
        "add_page",
        "insert_node",
        "delete_node",
        "undo",
        "batch_design",
    ] {
        assert!(tool_writes(write), "{write}");
    }
    for read in [
        "get_node",
        "list_pages",
        "get_document_info",
        "snapshot_layout",
    ] {
        assert!(!tool_writes(read), "{read}");
    }
    // Unclassified tools are admitted through the barrier rather than past it.
    assert!(tool_writes("add_some_dynamic_kit_component"));
}
