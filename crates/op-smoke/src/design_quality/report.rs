//! Turning measurements into a scorecard, a headline and a gate verdict.
//!
//! Everything here is pure: the rows come in as [`Measurement`] values that a
//! test can build by hand, so the two things that decide whether a run passes
//! (the truncation line and the regression line) are testable without a model,
//! a daemon or a network.
//!
//! ## What the gate is, and what it deliberately is not
//!
//! Per prompt the corpus records an expectation, and the gate compares the run
//! against exactly that:
//!
//! - `required` — the last pass delivered. Falling below `screenMinNodes`, or
//!   reporting `done` without moving the document version, is a **regression**.
//! - `known_broken` — the last pass already failed this prompt. Reproducing the
//!   recorded failure is printed as `KNOWN-BROKEN`, **not** as a regression;
//!   otherwise the gate would be red on every run for three prompts and nobody
//!   would read it. Reaching `screenMinNodes` is printed as `RECOVERED`.
//!
//! A turn that could not be measured at all (the daemon refused the read-back,
//! the turn POST failed) is neither: it is `NOT MEASURED` and forces exit 3,
//! because a partial run must never report success.

use serde::Serialize;

use super::corpus::{Corpus, GateState, Prompt, VerdictRule};
use super::truncation::TruncationReport;

/// Everything one prompt's measurement produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Measurement {
    pub(crate) id: String,
    /// Seconds the `/api/ai/standard` call took, wall clock.
    pub(crate) seconds: f64,
    pub(crate) thinking_chars: usize,
    pub(crate) delta_chars: usize,
    pub(crate) events: usize,
    /// `done` / `done-marker` / `error` / `None` when the stream ended without
    /// one (issue #203).
    pub(crate) terminal: Option<String>,
    /// SSE-level errors the daemon reported.
    pub(crate) errors: Vec<String>,
    /// Nodes under `pages[0]` after the turn.
    pub(crate) page0_nodes: u32,
    /// Top-level frame names after the turn.
    pub(crate) frames: Vec<String>,
    /// Nodes in the subtrees of the frames this turn's own DSL emitted (name
    /// attribution — a shared daemon makes a bare node diff untrustworthy).
    pub(crate) landed_nodes: u32,
    pub(crate) landed_frames: Vec<String>,
    pub(crate) root_statements: u32,
    pub(crate) version_before: Option<u64>,
    pub(crate) version_after: Option<u64>,
    /// `op-smoke`'s own audit of the resulting document (zero model calls).
    pub(crate) audit_issues: u32,
    pub(crate) audit_clean: bool,
    pub(crate) audit_first_issue: Option<String>,
    /// Set when the read-back document could not be audited — `issues` then
    /// reads `n/a` on the scorecard instead of a fabricated 0.
    pub(crate) audit_error: Option<String>,
    pub(crate) truncation: TruncationReport,
    /// Per-prompt artifact writes that failed (the measurement itself is still
    /// valid: it does not depend on disk).
    pub(crate) artifact_errors: Vec<String>,
    /// Set when the prompt could not be measured; the row is `NOT MEASURED`.
    pub(crate) probe_error: Option<String>,
}

impl Default for Measurement {
    fn default() -> Self {
        Self {
            id: String::new(),
            seconds: 0.0,
            thinking_chars: 0,
            delta_chars: 0,
            events: 0,
            terminal: None,
            errors: Vec::new(),
            page0_nodes: 0,
            frames: Vec::new(),
            landed_nodes: 0,
            landed_frames: Vec::new(),
            root_statements: 0,
            version_before: None,
            version_after: None,
            audit_issues: 0,
            audit_clean: true,
            audit_first_issue: None,
            audit_error: None,
            truncation: TruncationReport {
                shape: super::truncation::ReplyShape::Empty,
                signals: Vec::new(),
                truncated: false,
                not_applicable: None,
                applied_marker: false,
            },
            artifact_errors: Vec::new(),
            probe_error: None,
        }
    }
}

impl Measurement {
    /// Whether the turn wrote to the document at all.
    pub(crate) fn version_moved(&self) -> bool {
        match (self.version_before, self.version_after) {
            (Some(before), Some(after)) => after > before,
            // Without both samples the claim is not established; the row's
            // version column shows the missing half.
            _ => false,
        }
    }

    /// `v19 -> v38`, or `? -> 38` when a sample is missing.
    pub(crate) fn version_line(&self) -> String {
        let show = |v: Option<u64>| v.map(|v| v.to_string()).unwrap_or_else(|| "?".into());
        format!(
            "{} -> {}",
            show(self.version_before),
            show(self.version_after)
        )
    }
}

/// The scorecard's verdict for one prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    Yes,
    No,
    /// The prompt could not be measured — never folded into Yes/No.
    NotMeasured,
}

impl Verdict {
    /// The column's three characters.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Verdict::Yes => "YES",
            Verdict::No => "NO",
            Verdict::NotMeasured => "n/a",
        }
    }
}

/// One prompt's row on the scorecard.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    pub(crate) index: u32,
    pub(crate) id: String,
    pub(crate) language: String,
    pub(crate) intent: String,
    pub(crate) prompt: String,
    pub(crate) screen_requested: bool,
    pub(crate) verdict_rule: VerdictRule,
    pub(crate) verdict: Verdict,
    pub(crate) verdict_reason: String,
    /// Judgement calls this measurement cannot make, with the corpus's reason.
    pub(crate) unmeasurable: Vec<String>,
    pub(crate) measurement: Measurement,
    /// `required` / `known_broken` and what the run did against it.
    pub(crate) expectation_state: GateState,
    pub(crate) expectation_note: String,
    pub(crate) gate_failures: Vec<String>,
    pub(crate) known_broken: bool,
    pub(crate) recovered: bool,
}

/// The run-level counts printed under the table.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Headline {
    pub(crate) prompts: usize,
    /// Prompts that asked for a screen.
    pub(crate) screen_requested: usize,
    /// …of which one was drawn.
    pub(crate) screen_drawn: usize,
    /// Documents that hold a screen at all.
    pub(crate) documents_with_screen: usize,
    /// Documents the audit found geometry issues in.
    pub(crate) geometry_issues: usize,
    /// …restricted to the documents that hold a screen: the honest reading,
    /// because a document with no content has nothing to audit.
    pub(crate) geometry_issues_with_screen: usize,
    /// Turns that ended without a terminal SSE event.
    pub(crate) terminal_missing: usize,
    /// Replies that could be judged and were truncated.
    pub(crate) truncated: usize,
    /// Replies whose truncation could be judged at all.
    pub(crate) truncation_judged: usize,
    /// Documents the audit could not score — excluded from the geometry count
    /// rather than counted as clean.
    pub(crate) audit_skipped: usize,
    pub(crate) not_measured: usize,
    /// Ids whose stream reported an error. On those rows the content that
    /// landed may be the route's placed base rather than this turn's own
    /// output (`gq/RESULTS.md` measured exactly that on a transport failure),
    /// so the node count there is a ceiling, not a proof.
    pub(crate) stream_errors: Vec<String>,
}

/// The gate's verdict for the whole run.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Gate {
    pub(crate) regressions: Vec<Regression>,
    pub(crate) known_broken: Vec<String>,
    pub(crate) recovered: Vec<String>,
    pub(crate) not_measured: Vec<String>,
    pub(crate) passed: bool,
}

/// One prompt that came in below its recorded expectation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Regression {
    pub(crate) index: u32,
    pub(crate) id: String,
    pub(crate) reasons: Vec<String>,
    pub(crate) note: String,
}

/// The printed and saved scorecard.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Scorecard {
    pub(crate) screen_min_nodes: u32,
    pub(crate) rows: Vec<Row>,
    pub(crate) headline: Headline,
    pub(crate) gate: Gate,
}

impl Scorecard {
    /// `0` = nothing regressed and every prompt was measured, `1` = a
    /// regression, `3` = a prompt could not be measured at all.
    pub(crate) fn exit_code(&self) -> u8 {
        if !self.gate.not_measured.is_empty() {
            3
        } else if self.gate.passed {
            0
        } else {
            1
        }
    }
}

/// Builds the scorecard from the corpus and one measurement per prompt.
///
/// `measurements` must be in the same order as `prompts`; the spine builds both
/// from the same iteration so they cannot drift.
pub(crate) fn evaluate(
    corpus: &Corpus,
    prompts: &[Prompt],
    measurements: &[Measurement],
) -> Scorecard {
    let screen_min = corpus.screen_min_nodes;
    let rows: Vec<Row> = prompts
        .iter()
        .zip(measurements.iter())
        .map(|(prompt, measurement)| row_for(prompt, measurement, screen_min))
        .collect();
    let headline = headline_of(&rows, screen_min);
    let gate = gate_of(&rows);
    Scorecard {
        screen_min_nodes: screen_min,
        rows,
        headline,
        gate,
    }
}

fn row_for(prompt: &Prompt, measurement: &Measurement, screen_min: u32) -> Row {
    let (verdict, verdict_reason) = judge(prompt, measurement, screen_min);
    let mut gate_failures = Vec::new();
    if measurement.probe_error.is_none() {
        match prompt.expectation.state {
            GateState::Required => {
                if measurement.page0_nodes < screen_min {
                    gate_failures.push(format!(
                        "expected at least {screen_min} node(s) under pages[0] (a screen), got {}",
                        measurement.page0_nodes
                    ));
                }
                if prompt.expectation.require_version_move && !measurement.version_moved() {
                    gate_failures.push(format!(
                        "the turn reported {} but the document version did not move ({})",
                        measurement
                            .terminal
                            .clone()
                            .unwrap_or_else(|| "no terminal event".to_string()),
                        measurement.version_line()
                    ));
                }
            }
            GateState::KnownBroken => {
                let floor = prompt.expectation.floor_page0_nodes;
                if measurement.page0_nodes < floor {
                    gate_failures.push(format!(
                        "expected at least {floor} node(s) under pages[0] (the recorded {} failure \
                         left {floor}), got {}",
                        prompt.last.pass, measurement.page0_nodes
                    ));
                }
            }
        }
    }
    let recovered = prompt.expectation.state == GateState::KnownBroken
        && measurement.probe_error.is_none()
        && measurement.page0_nodes >= screen_min
        && (!prompt.expectation.require_version_move || measurement.version_moved());
    Row {
        index: prompt.index,
        id: prompt.id.clone(),
        language: prompt.language.clone(),
        intent: prompt.intent.clone(),
        prompt: prompt.prompt.clone(),
        screen_requested: prompt.screen_requested,
        verdict_rule: prompt.verdict_rule,
        verdict,
        verdict_reason,
        unmeasurable: prompt.unmeasurable.clone(),
        measurement: measurement.clone(),
        expectation_state: prompt.expectation.state,
        expectation_note: prompt.expectation.note.clone(),
        // A recorded failure counts as "known-broken" only when it is
        // REPRODUCED: a prompt that came in below even its recorded floor is a
        // regression, and one row must never claim both.
        known_broken: prompt.expectation.state == GateState::KnownBroken
            && !recovered
            && gate_failures.is_empty(),
        recovered,
        gate_failures,
    }
}

/// The verdict and the sentence that explains it.
fn judge(prompt: &Prompt, measurement: &Measurement, screen_min: u32) -> (Verdict, String) {
    if let Some(error) = &measurement.probe_error {
        return (Verdict::NotMeasured, format!("not measured: {error}"));
    }
    match prompt.verdict_rule {
        VerdictRule::Screen => {
            if measurement.page0_nodes >= screen_min {
                (
                    Verdict::Yes,
                    format!(
                        "screen drawn: pages[0] holds {} node(s) in {} top-level frame(s)",
                        measurement.page0_nodes,
                        measurement.frames.len()
                    ),
                )
            } else {
                (
                    Verdict::No,
                    format!(
                        "no screen: pages[0] holds {} node(s) after the turn (a screen is at \
                         least {screen_min})",
                        measurement.page0_nodes
                    ),
                )
            }
        }
        VerdictRule::DocumentMoved => {
            if !measurement.version_moved() {
                (
                    Verdict::No,
                    format!(
                        "the turn reported {} and did not move the document ({}); pages[0] holds \
                         {} node(s)",
                        measurement
                            .terminal
                            .clone()
                            .unwrap_or_else(|| "no terminal event".to_string()),
                        measurement.version_line(),
                        measurement.page0_nodes
                    ),
                )
            } else if measurement.page0_nodes >= screen_min {
                (
                    Verdict::Yes,
                    format!(
                        "the document moved ({}) and pages[0] holds {} node(s)",
                        measurement.version_line(),
                        measurement.page0_nodes
                    ),
                )
            } else {
                (
                    Verdict::No,
                    format!(
                        "the document moved ({}) but pages[0] still holds {} node(s) — the turn \
                         wrote nothing a reader can see",
                        measurement.version_line(),
                        measurement.page0_nodes
                    ),
                )
            }
        }
    }
}

fn headline_of(rows: &[Row], screen_min: u32) -> Headline {
    let measured: Vec<&Row> = rows
        .iter()
        .filter(|row| row.measurement.probe_error.is_none())
        .collect();
    let audited: Vec<&&Row> = measured
        .iter()
        .filter(|row| row.measurement.audit_error.is_none())
        .collect();
    let has_screen = |row: &Row| row.measurement.page0_nodes >= screen_min;
    Headline {
        prompts: rows.len(),
        screen_requested: measured.iter().filter(|r| r.screen_requested).count(),
        screen_drawn: measured
            .iter()
            .filter(|r| r.screen_requested && has_screen(r))
            .count(),
        documents_with_screen: measured.iter().filter(|r| has_screen(r)).count(),
        geometry_issues: audited
            .iter()
            .filter(|r| r.measurement.audit_issues > 0)
            .count(),
        geometry_issues_with_screen: audited
            .iter()
            .filter(|r| has_screen(r) && r.measurement.audit_issues > 0)
            .count(),
        terminal_missing: measured
            .iter()
            .filter(|r| r.measurement.terminal.is_none())
            .count(),
        truncated: measured
            .iter()
            .filter(|r| r.measurement.truncation.truncated)
            .count(),
        truncation_judged: measured
            .iter()
            .filter(|r| r.measurement.truncation.not_applicable.is_none())
            .count(),
        audit_skipped: measured.len() - audited.len(),
        not_measured: rows.len() - measured.len(),
        stream_errors: measured
            .iter()
            .filter(|r| {
                !r.measurement.errors.is_empty()
                    || r.measurement.terminal.as_deref() == Some("error")
            })
            .map(|r| r.id.clone())
            .collect(),
    }
}

fn gate_of(rows: &[Row]) -> Gate {
    let mut gate = Gate::default();
    for row in rows {
        if row.measurement.probe_error.is_some() {
            gate.not_measured.push(row.id.clone());
            continue;
        }
        if !row.gate_failures.is_empty() {
            gate.regressions.push(Regression {
                index: row.index,
                id: row.id.clone(),
                reasons: row.gate_failures.clone(),
                note: row.expectation_note.clone(),
            });
        }
        if row.known_broken {
            gate.known_broken.push(row.id.clone());
        }
        if row.recovered {
            gate.recovered.push(row.id.clone());
        }
    }
    gate.passed = gate.regressions.is_empty() && gate.not_measured.is_empty();
    gate
}

/// The gate's readable failure lines — the same text the scorecard prints and
/// the same text the regression test asserts on.
pub(crate) fn regression_lines(scorecard: &Scorecard) -> Vec<String> {
    let mut lines = Vec::new();
    for regression in &scorecard.gate.regressions {
        let reasons = regression.reasons.join("; ");
        lines.push(format!(
            "REGRESSION {} (prompt {}): {reasons} — corpus note: {}",
            regression.id, regression.index, regression.note
        ));
    }
    for id in &scorecard.gate.not_measured {
        lines.push(format!(
            "NOT MEASURED {id}: the prompt produced no measurement — a partial run is not a pass"
        ));
    }
    lines
}

/// The gate's one-line summary, including the exit code it implies.
pub(crate) fn gate_summary(scorecard: &Scorecard) -> String {
    let total = scorecard.headline.prompts;
    let regressed = scorecard.gate.regressions.len();
    let mut summary = if regressed == 0 {
        format!("no prompt regressed against the recorded baseline (0 of {total})")
    } else {
        format!("{regressed} of {total} prompt(s) regressed against the recorded baseline")
    };
    if !scorecard.gate.known_broken.is_empty() {
        summary.push_str(&format!(
            "; known-broken, recorded not regressed: {}",
            scorecard.gate.known_broken.join(", ")
        ));
    }
    if !scorecard.gate.recovered.is_empty() {
        summary.push_str(&format!(
            "; RECOVERED: {}",
            scorecard.gate.recovered.join(", ")
        ));
    }
    if !scorecard.gate.not_measured.is_empty() {
        summary.push_str(&format!(
            "; not measured: {}",
            scorecard.gate.not_measured.join(", ")
        ));
    }
    summary.push_str(&format!(" (exit {})", scorecard.exit_code()));
    summary
}
