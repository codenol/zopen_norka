//! Tests for the zero-model program mode.
//!
//! The program files are real files in the system temp directory, so these
//! exercise the same `std::fs` + `op_mcp` path `main` uses. Nothing here
//! touches a network, a provider, a model or a credential — the whole point of
//! this mode is that it has none.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static TMP_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Writes `text` to a uniquely named file under the system temp directory.
fn write_temp(name: &str, text: &str) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "op-smoke-program-{}-{seq}-{name}",
        std::process::id()
    ));
    std::fs::write(&path, text).expect("temp file");
    path
}

/// A one-root program in the DSL this mode exists to run.
const ONE_FRAME_PROGRAM: &str =
    r#"root=I(null,{type:"frame",name:"P",x:0,y:0,width:320,height:200});"#;

#[test]
fn a_program_builds_a_document_with_no_model_environment() {
    // The regression: the branch sat below the provider/credential validation,
    // so a run that would never call a model demanded an API key.
    let mut state = op_editor_core::EditorState::new();
    assert!(
        apply_program(ONE_FRAME_PROGRAM, &mut state),
        "a valid program applies to a fresh document"
    );
    use op_editor_core::PenNodeExt as _;
    assert_eq!(state.active_children().len(), 1);
    assert_eq!(state.active_children()[0].base().name.as_deref(), Some("P"));
}

#[test]
fn a_program_that_builds_nothing_does_not_claim_success() {
    let mut state = op_editor_core::EditorState::new();
    assert!(
        !apply_program("// nothing to do\n", &mut state),
        "an empty program is not an applied program"
    );
}

#[test]
fn a_missing_program_file_fails_about_the_file_not_about_a_model() {
    let missing = std::env::temp_dir().join("op-smoke-program-does-not-exist.txt");
    let _ = std::fs::remove_file(&missing);
    let code = run_program(missing.to_str().expect("utf-8 path"), None);
    assert_eq!(code, ExitCode::from(3), "unreadable program is exit 3");
}

#[test]
fn the_result_is_written_where_asked() {
    let program = write_temp("one-frame.txt", ONE_FRAME_PROGRAM);
    let out = std::env::temp_dir().join(format!("op-smoke-program-out-{}.op", std::process::id()));
    let _ = std::fs::remove_file(&out);

    let code = run_program(
        program.to_str().expect("utf-8 path"),
        Some(out.to_str().expect("utf-8 path")),
    );

    assert_eq!(code, ExitCode::SUCCESS);
    let written = std::fs::read_to_string(&out).expect("the run wrote the document");
    assert!(written.contains("\"P\""), "{written}");
    let _ = std::fs::remove_file(&program);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn program_dispatch_precedes_the_credential_gate_in_main() {
    // Source order is the only thing that keeps a zero-model mode reachable
    // without a credential, so assert on it directly.
    let main_src = include_str!("main.rs");
    let dispatch = main_src
        .find("program_mode::run_if_requested")
        .expect("main dispatches the program mode");
    let gate = main_src
        .find("SmokeProviderKind::Anthropic =>")
        .expect("main still matches the anthropic provider arm");
    assert!(
        dispatch < gate,
        "program mode must be dispatched before the provider credential gate"
    );
}
