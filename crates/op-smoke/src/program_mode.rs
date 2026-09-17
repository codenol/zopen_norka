//! Zero-model program mode (`OPENPENCIL_SMOKE_PROGRAM=<path>`).
//!
//! Runs a Pencil-style `batch_design` DSL PROGRAM (a `binding=I(parent,{...})`
//! tree-builder) against a fresh document and saves the result, bypassing the
//! orchestrator entirely. It is the harness for "can a weak model emit a
//! structurally stable PROGRAM (parent-by-reference) instead of fragile flat
//! JSONL?", so the saved document is the RAW structure the program builds:
//! `postProcess` stays off, and the point of the experiment is that the program
//! needs no repair pass.
//!
//! **The program is the input, so this mode makes no LLM call** — no provider,
//! no model, no credential, no network. `main` therefore dispatches here before
//! it parses `OPENPENCIL_LLM_PROVIDER` and before it builds any client, exactly
//! as it does for the audit mode: a mode that would never have used a
//! credential must not be gated behind one (issue #192, the same defect #188
//! fixed for the audit).
//!
//! Exit code: `0` = the program applied, `1` = it produced no command or did
//! not apply, `3` = the program file could not be read, `4` = the result could
//! not be serialized or written.

use std::collections::BTreeMap;
use std::process::ExitCode;

/// Names the DSL program to run. Unset ⇒ this is not a program run.
pub(crate) const PROGRAM_PATH_ENV: &str = "OPENPENCIL_SMOKE_PROGRAM";

/// Where the resulting document goes; unset ⇒ applied-or-not is the whole
/// result and the document is only reported on stderr.
pub(crate) const PROGRAM_OUT_ENV: &str = "OPENPENCIL_SMOKE_OUT";

/// Runs the program when [`PROGRAM_PATH_ENV`] names a file.
///
/// `None` ⇒ the variable is unset, so the caller carries on to the generation
/// modes. Called from `main` BEFORE any credential validation.
pub(crate) fn run_if_requested() -> Option<ExitCode> {
    let path = std::env::var(PROGRAM_PATH_ENV).ok()?;
    let out = std::env::var(PROGRAM_OUT_ENV).ok();
    Some(run_program(&path, out.as_deref()))
}

/// Applies the program at `program_path` to a fresh document and writes the
/// result to `out` when one is named.
pub(crate) fn run_program(program_path: &str, out: Option<&str>) -> ExitCode {
    let program = match std::fs::read_to_string(program_path) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("[PROGRAM] read {program_path}: {error}");
            return ExitCode::from(3);
        }
    };
    let mut state = op_editor_core::EditorState::new();
    let applied = apply_program(&program, &mut state);

    let Some(out) = out.filter(|out| !out.is_empty()) else {
        return if applied {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    };
    let json = match serde_json::to_string_pretty(&state.doc) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("[PROGRAM] serialize failed: {error}");
            return ExitCode::from(4);
        }
    };
    match std::fs::write(out, json) {
        Ok(()) => {
            eprintln!("[PROGRAM] saved doc -> {out}");
            if applied {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("[PROGRAM] save failed ({out}): {error}");
            ExitCode::from(4)
        }
    }
}

/// Builds the `batch_design` tool over `state`, calls it with the program and
/// applies whatever `EditorCommand` comes back. `true` = it applied.
pub(crate) fn apply_program(program: &str, state: &mut op_editor_core::EditorState) -> bool {
    let tool = op_mcp::batch_design_snapshot(state);
    let mut args: BTreeMap<String, String> = BTreeMap::new();
    args.insert("operations".into(), program.to_string());
    match op_mcp::McpTool::call(&tool, &args) {
        op_mcp::ToolOutcome::OkJsonWithCommand(json, cmd) => {
            eprintln!("[PROGRAM] result envelope: {json}");
            let applied = state.apply(cmd);
            eprintln!("[PROGRAM] apply -> {applied}");
            applied
        }
        op_mcp::ToolOutcome::OkJson(json) => {
            eprintln!("[PROGRAM] no command produced: {json}");
            false
        }
        other => {
            eprintln!("[PROGRAM] unexpected outcome: {other:?}");
            false
        }
    }
}

#[cfg(test)]
#[path = "program_mode_tests.rs"]
mod tests;
