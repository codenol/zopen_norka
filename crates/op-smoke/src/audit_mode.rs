//! Zero-model audit mode (`OPENPENCIL_SMOKE_AUDIT=<path.op>`).
//!
//! Loads an EXISTING `.op` off disk, scores it with the REAL-layout geometry
//! diagnostics (the same detector family the per-batch feedback uses) plus the
//! `audit_rubric` metrics, prints one JSON report on stdout and exits. This is
//! the machine-checkable leg of the develop → generate → render → audit loop
//! and the quality gate a CI job runs.
//!
//! **It makes no LLM call, so it must not need a provider, a credential or a
//! model.** That is why `main` dispatches here before it parses
//! `OPENPENCIL_LLM_PROVIDER` and before it builds any client: an audit has to
//! work on a machine that has no model at all (a CI runner, a fresh checkout,
//! a QA gate with no key in the environment). Every failure this mode prints
//! names the FILE it was given — never a missing key.
//!
//! Exit code: `0` = structurally clean, `1` = geometry issues found,
//! `3` = the file could not be read or parsed.

use std::process::ExitCode;

/// Names the `.op` to audit. Unset ⇒ this is not an audit run.
pub(crate) const AUDIT_PATH_ENV: &str = "OPENPENCIL_SMOKE_AUDIT";

/// One audit run's result: the report to print and whether the document is
/// structurally clean (the value the exit code is keyed on).
#[derive(Debug)]
pub(crate) struct AuditReport {
    /// Pretty-printed JSON report — the machine-readable contract.
    pub(crate) json: String,
    /// `true` when the geometry diagnostics found nothing.
    pub(crate) clean: bool,
}

/// Runs the audit when `OPENPENCIL_SMOKE_AUDIT` names a file.
///
/// `None` ⇒ the variable is unset, so the caller carries on to the generation
/// modes. Called from `main` BEFORE any credential validation, so an audit is
/// reachable with no model environment whatsoever.
pub(crate) fn run_if_requested() -> Option<ExitCode> {
    let path = std::env::var(AUDIT_PATH_ENV).ok()?;
    Some(match audit_file(&path) {
        Ok(report) => {
            println!("{}", report.json);
            if report.clean {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(3)
        }
    })
}

/// Scores the `.op` at `path`. `Err` carries the exact message to print.
pub(crate) fn audit_file(path: &str) -> Result<AuditReport, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("[AUDIT] read {path}: {e}"))?;
    // Load through the compat loader (not raw serde) so a `.op` saved with the
    // deduplicated `images` table audits with its refs resolved instead of
    // dangling `op-image:` strings.
    let doc: jian_ops_schema::PenDocument = jian_ops_schema::load_str(&text)
        .map_err(|e| format!("[AUDIT] parse {path}: {e}"))?
        .value;
    Ok(audit_state(
        &op_editor_core::EditorState::from_document(doc),
        path,
    ))
}

/// Builds the report for an already-loaded document.
///
/// Split out of [`audit_file`] so a test can score an in-memory state without
/// a filesystem round-trip.
pub(crate) fn audit_state(state: &op_editor_core::EditorState, path: &str) -> AuditReport {
    let issues = op_orchestrator::geometry_validation::geometry_diagnostics(state);
    let roots: Vec<String> = state
        .active_children()
        .iter()
        .map(|n| {
            use op_editor_core::PenNodeExt;
            format!(
                "{} ({})",
                n.base().name.as_deref().unwrap_or("?"),
                n.id_str()
            )
        })
        .collect();
    let report = serde_json::json!({
        "file": path,
        "roots": roots,
        "issueCount": issues.len(),
        "issues": issues,
        // Chrome completeness / node-vocabulary / density metrics — the
        // dimensions raw geometry issues miss (see ab-g3 07-04 lesson).
        // Informational: exit code stays keyed on geometry issues alone.
        // `None`: this standalone-audit branch loads an existing `.op`
        // straight off disk and never drives the orchestrator, so there
        // is no `RunSummary` to report subtask-completeness against —
        // the rubric's `completeness` section is omitted here (never a
        // fabricated 0/0), same as `rubric_report`'s own doc contract.
        "rubric": crate::audit_rubric::rubric_report(state, None),
    });
    AuditReport {
        json: serde_json::to_string_pretty(&report).unwrap_or_default(),
        clean: issues.is_empty(),
    }
}

#[cfg(test)]
#[path = "audit_mode_tests.rs"]
mod tests;
