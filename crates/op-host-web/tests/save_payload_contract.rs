//! Source-level guardrails for the browser-only Save routing.
//!
//! DOM/XHR behavior still needs browser smoke. These checks keep the large
//! payload lifetime contract visible in native CI.

fn source(name: &str) -> String {
    std::fs::read_to_string(format!("{}/src/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

#[test]
fn each_save_destination_builds_only_its_own_payload() {
    let io = source("dom_io/document_io.rs");
    // The decision moved one function out: `save_document` now asks
    // `save_destination(daemon_first, local_document)` and matches on the
    // answer. The contract is the same — the dispatcher decides by
    // `daemon_first`, and the dispatcher builds no payload — so it is checked
    // where the decision lives now, and the hand-off is checked too. Pointing
    // at a slice of the file is what let this test keep passing while the
    // function under it changed shape (issues #224, #225).
    let dispatcher = io
        .split("pub(super) fn save_document")
        .nth(1)
        .and_then(|tail| tail.split("fn save_to_daemon").next())
        .expect("save dispatcher");
    assert!(
        dispatcher.contains("save_destination(daemon_first, local)"),
        "the dispatcher asks the decision function"
    );
    assert!(!dispatcher.contains("serialize_save_payload"));

    let destination = io
        .split("fn save_destination")
        .nth(1)
        .and_then(|tail| tail.split("pub(super) fn save_document").next())
        .expect("save destination decision");
    assert!(
        destination.contains("daemon_first"),
        "the decision is still `daemon_first`'s to make"
    );
    assert!(!destination.contains("serialize_save_payload"));

    let daemon = io
        .split("fn save_to_daemon")
        .nth(1)
        .and_then(|tail| tail.split("fn enqueue_daemon_save").next())
        .expect("daemon save enqueue");
    assert!(!daemon.contains("serialize_save_payload"));
    assert!(daemon.contains("requested_epoch"));
    assert!(daemon.contains("requested_generation"));
    assert!(daemon.contains("enqueue_daemon_save"));

    let active_daemon = io
        .split("fn start_daemon_save")
        .nth(1)
        .and_then(|tail| tail.split("fn save_to_browser_if_snapshot_current").next())
        .expect("active daemon save body");
    assert_eq!(active_daemon.matches("serialize_save_payload").count(), 1);
    assert!(active_daemon.contains("SavePayloadTarget::Daemon"));
    assert!(!active_daemon.contains("SavePayloadTarget::BrowserDownload"));
    assert!(!active_daemon.contains("json.clone()"));
    assert!(active_daemon.contains("save_to_browser_if_snapshot_current"));
    assert!(active_daemon.contains("finish_daemon_save()"));
    let release = active_daemon
        .find("drop(body)")
        .expect("release rejected daemon body");
    let sync_fallback = active_daemon
        .rfind("save_to_browser_if_snapshot_current(inner")
        .expect("synchronous browser fallback");
    assert!(release < sync_fallback);

    let browser = io
        .split("fn save_to_browser<C:")
        .nth(1)
        .and_then(|tail| tail.split("fn download_saved_document").next())
        .expect("browser save body");
    assert_eq!(browser.matches("serialize_save_payload").count(), 1);
    assert!(browser.contains("SavePayloadTarget::BrowserDownload"));
    assert!(!browser.contains("SavePayloadTarget::Daemon"));
}

#[test]
fn daemon_saves_are_single_flight_latest_wins_and_fallback_is_snapshot_scoped() {
    let io = source("dom_io/document_io.rs");
    let queue = source("file_actions/save_queue.rs");

    assert!(io.contains("LatestSaveQueue<DaemonSaveLaunch>"));
    assert!(io.contains("queue.borrow_mut().enqueue(launch)"));
    assert!(io.contains("queue.borrow_mut().finish()"));
    assert!(queue.contains("self.pending = Some(launch)"));
    assert!(queue.contains("match self.pending.take()"));

    let fallback = io
        .split("fn save_to_browser_if_snapshot_current")
        .nth(1)
        .and_then(|tail| tail.split("fn save_to_browser").next())
        .expect("snapshot-scoped browser fallback");
    assert!(fallback.contains("save_snapshot_matches_document"));
    assert!(fallback.contains("state.document_revision()"));
    assert!(fallback.contains("snapshot_epoch"));
    assert!(fallback.contains("snapshot_generation"));
    assert!(fallback.contains("snapshot_revision"));
    assert!(fallback.contains("if current"));
    assert!(fallback.contains("save_to_browser(inner)"));

    let payload = source("file_actions/save_payload.rs");
    let exact_match = payload
        .split("pub fn save_snapshot_matches_document")
        .nth(1)
        .and_then(|tail| tail.split("/// Serialize exactly").next())
        .expect("exact fallback identity helper");
    assert!(exact_match.contains("save_ack_matches_document"));
    assert!(exact_match.contains("live_document_revision == snapshot_document_revision"));
}

#[test]
fn both_payloads_stream_the_borrowed_document_without_an_intermediate_value() {
    let payload = source("file_actions/save_payload.rs");
    let daemon = payload
        .split("fn serialize_daemon_request")
        .nth(1)
        .and_then(|tail| tail.split("#[derive(Debug)]").next())
        .expect("daemon serializer");
    let download = payload
        .split("fn serialize_download_document")
        .nth(1)
        .and_then(|tail| tail.split("fn serialize_daemon_request").next())
        .expect("download serializer");

    assert!(payload.contains("image_table::write_document_with_extension"));
    assert!(daemon.contains("write_canonical_document(&mut out, state)"));
    assert!(download.contains("write_canonical_document(&mut out, state)"));
    assert!(!daemon.contains("serde_json::json!({"));
    assert!(!daemon.contains("serde_json::to_value"));
    assert!(!download.contains("serde_json::to_value"));
}
