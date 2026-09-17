//! Tests for the zero-model audit mode.
//!
//! The load/parse legs run against real files in the system temp directory, so
//! they exercise the same `std::fs` + compat-loader path `main` uses. Nothing
//! here touches the network, a provider or a credential — the whole point of
//! this mode is that it has none.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static TMP_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Writes `text` to a uniquely named file under the system temp directory.
/// Test-local path maths only; the suite never writes inside the repository.
fn write_temp(name: &str, text: &str) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "op-smoke-audit-{}-{seq}-{name}",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("temp file");
    path
}

/// A minimal but genuinely valid canonical `.op`: one named empty frame root.
fn minimal_op_json() -> String {
    serde_json::json!({
        "version": "1.0",
        "children": [
            { "type": "frame", "id": "root", "name": "Login", "width": 390, "height": 844,
              "children": [] }
        ],
    })
    .to_string()
}

#[test]
fn a_missing_file_fails_about_the_file_not_about_a_model() {
    let path = std::env::temp_dir().join("op-smoke-audit-does-not-exist.op");
    let error = audit_file(path.to_str().expect("utf-8 path")).expect_err("must fail");
    assert!(error.starts_with("[AUDIT] read "), "{error}");
    assert!(error.contains("does-not-exist.op"), "{error}");
    // The failure a user sees must be about the input document. An audit makes
    // no model call, so naming a key or a provider here would be a lie.
    for forbidden in ["API_KEY", "ANTHROPIC", "provider", "OPENPENCIL_LLM"] {
        assert!(!error.contains(forbidden), "{forbidden} in: {error}");
    }
}

#[test]
fn an_unparseable_file_reports_the_parse_failure_with_its_path() {
    let file = write_temp("broken.op", "{ not json at all");
    let error = audit_file(file.to_str().expect("utf-8 path")).expect_err("must fail");
    assert!(error.starts_with("[AUDIT] parse "), "{error}");
    assert!(error.contains("broken.op"), "{error}");
    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_clean_document_audits_clean_and_reports_its_own_path() {
    let file = write_temp("clean.op", &minimal_op_json());
    let path = file.to_str().expect("utf-8 path").to_string();
    let report = audit_file(&path).expect("audit must succeed");
    assert!(report.clean, "{}", report.json);

    let parsed: serde_json::Value = serde_json::from_str(&report.json).expect("report is JSON");
    assert_eq!(parsed["file"], serde_json::json!(path));
    assert_eq!(parsed["issueCount"], serde_json::json!(0));
    assert_eq!(parsed["roots"], serde_json::json!(["Login (root)"]));
    // The rubric is the machine-checkable half of the report; the completeness
    // section stays absent because this path never drove an orchestrator.
    assert!(parsed["rubric"].is_object(), "{}", report.json);
    assert!(
        parsed["rubric"].get("completeness").is_none(),
        "{}",
        report.json
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn an_empty_document_is_clean_rather_than_an_error() {
    // A `.op` with no page content is a legitimate (if useless) file: it must
    // audit to zero issues with no roots, not fail.
    let file = write_temp("empty.op", r#"{"version":"1.0","children":[]}"#);
    let report = audit_file(file.to_str().expect("utf-8 path")).expect("audit must succeed");
    assert!(report.clean, "{}", report.json);
    let parsed: serde_json::Value = serde_json::from_str(&report.json).expect("report is JSON");
    assert_eq!(parsed["roots"], serde_json::json!([]));
    let _ = std::fs::remove_file(&file);
}

#[test]
fn audit_dispatch_precedes_the_credential_gate_in_main() {
    // The regression this mode was fixed for: the audit branch used to sit
    // BELOW the provider/credential validation in `main`, so a zero-LLM audit
    // was unreachable without an API key. Source order is the only thing that
    // keeps it reachable, so assert on it directly.
    let main_src = include_str!("main.rs");
    let dispatch = main_src
        .find("audit_mode::run_if_requested")
        .expect("main dispatches the audit mode");
    let gate = main_src
        .find("SmokeProviderKind::Anthropic =>")
        .expect("main still matches the anthropic provider arm");
    assert!(
        dispatch < gate,
        "audit mode must be dispatched before the provider credential gate"
    );
}
