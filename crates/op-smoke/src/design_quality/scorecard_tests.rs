//! The scorecard's two decisions, tested without a model or a daemon:
//! the verdict per prompt, and the gate's regression lines.

use super::corpus::Corpus;
use super::report::{evaluate, gate_summary, regression_lines, Measurement, Verdict};

/// The corpus committed with the crate — the same file a run reads.
fn corpus() -> Corpus {
    Corpus::load(super::DEFAULT_CORPUS).expect("the committed corpus must load")
}

/// A measurement with the given page-0 node count and a document version that
/// moved by one.
fn drew(id: &str, page0_nodes: u32) -> Measurement {
    Measurement {
        id: id.to_string(),
        page0_nodes,
        version_before: Some(10),
        version_after: Some(11),
        terminal: Some("done".to_string()),
        ..Measurement::default()
    }
}

fn only(corpus: &Corpus, id: &str) -> Vec<super::corpus::Prompt> {
    corpus
        .selected(&[id.to_string()])
        .unwrap_or_else(|e| panic!("select {id}: {e}"))
}

#[test]
fn required_prompt_below_the_floor_regresses_and_the_line_names_it() {
    let corpus = corpus();
    let prompts = only(&corpus, "01");
    let card = evaluate(&corpus, &prompts, &[drew("01-login-ru", 0)]);

    assert_eq!(card.exit_code(), 1);
    assert_eq!(card.rows[0].verdict, Verdict::No);
    let lines = regression_lines(&card);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("REGRESSION 01-login-ru (prompt 1): "),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains("expected at least 5 node(s) under pages[0] (a screen), got 0"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("corpus note: "), "{}", lines[0]);
    assert!(
        gate_summary(&card).contains("1 of 1 prompt(s) regressed"),
        "{}",
        gate_summary(&card)
    );
}

#[test]
fn required_prompt_that_drew_a_screen_passes_quietly() {
    let corpus = corpus();
    let prompts = only(&corpus, "01");
    let card = evaluate(&corpus, &prompts, &[drew("01-login-ru", 29)]);

    assert_eq!(card.exit_code(), 0);
    assert_eq!(card.rows[0].verdict, Verdict::Yes);
    assert!(regression_lines(&card).is_empty());
    assert!(card.rows[0].verdict_reason.contains("screen drawn"));
    assert!(
        gate_summary(&card)
            .starts_with("no prompt regressed against the recorded baseline (0 of 1)"),
        "{}",
        gate_summary(&card)
    );
}

#[test]
fn required_prompt_that_reported_done_without_moving_the_document_regresses() {
    let corpus = corpus();
    let prompts = only(&corpus, "01");
    let stalled = Measurement {
        version_before: Some(80),
        version_after: Some(80),
        ..drew("01-login-ru", 29)
    };
    let card = evaluate(&corpus, &prompts, &[stalled]);

    assert_eq!(card.exit_code(), 1);
    let lines = regression_lines(&card);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0]
            .contains("the turn reported done but the document version did not move (80 -> 80)"),
        "{}",
        lines[0]
    );
}

#[test]
fn known_broken_prompt_reproducing_the_recorded_failure_does_not_fail_the_gate() {
    // Prompt 4 is the recorded #202 regression: gq2 left pages[0] with 0 nodes.
    // Reproducing it must be reported, not gated — a gate that is red on every
    // run is a gate nobody reads.
    let corpus = corpus();
    let prompts = only(&corpus, "04");
    let card = evaluate(&corpus, &prompts, &[drew("04-settings-en", 0)]);

    assert_eq!(card.exit_code(), 0);
    assert_eq!(card.rows[0].verdict, Verdict::No);
    assert!(card.rows[0].known_broken);
    assert!(!card.rows[0].recovered);
    assert!(card.rows[0].gate_failures.is_empty());
    assert_eq!(card.gate.known_broken, vec!["04-settings-en".to_string()]);
    assert!(regression_lines(&card).is_empty());
    assert!(
        gate_summary(&card).contains("known-broken, recorded not regressed: 04-settings-en"),
        "{}",
        gate_summary(&card)
    );
}

#[test]
fn known_broken_prompt_that_draws_a_screen_is_reported_recovered() {
    let corpus = corpus();
    let prompts = only(&corpus, "04");
    let card = evaluate(&corpus, &prompts, &[drew("04-settings-en", 40)]);

    assert_eq!(card.exit_code(), 0);
    assert_eq!(card.rows[0].verdict, Verdict::Yes);
    assert!(card.rows[0].recovered);
    assert!(!card.rows[0].known_broken);
    assert_eq!(card.gate.recovered, vec!["04-settings-en".to_string()]);
    assert!(gate_summary(&card).contains("RECOVERED: 04-settings-en"));
}

#[test]
fn known_broken_prompt_that_gets_worse_regresses() {
    // Prompt 6's recorded failure left the untouched 1-node starter frame; an
    // empty page is worse than that and must be caught.
    let corpus = corpus();
    let prompts = only(&corpus, "06");
    let card = evaluate(
        &corpus,
        &prompts,
        &[Measurement {
            version_before: Some(80),
            version_after: Some(81),
            ..drew("06-vague-ru", 0)
        }],
    );

    assert_eq!(card.exit_code(), 1);
    let lines = regression_lines(&card);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains(
            "expected at least 1 node(s) under pages[0] (the recorded gq2 failure left 1), got 0"
        ),
        "{}",
        lines[0]
    );
    // One row must never claim both: coming in below the RECORDED failure is a
    // regression, not a reproduced known-broken prompt.
    assert!(!card.rows[0].known_broken);
    assert!(card.gate.known_broken.is_empty());
    assert!(
        !gate_summary(&card).contains("known-broken"),
        "{}",
        gate_summary(&card)
    );
}

#[test]
fn a_stream_error_is_reported_as_a_ceiling_not_as_proof() {
    // Prompt 2 in the live run: the provider timed out, yet the kit's 470-node
    // placed recipe was left on the page. The row still counts a screen — and
    // says so, so the number is not read as the turn's own output.
    let corpus = corpus();
    let prompts = only(&corpus, "02");
    let card = evaluate(
        &corpus,
        &prompts,
        &[Measurement {
            errors: vec!["openai-compatible POST … timed out".to_string()],
            terminal: Some("error".to_string()),
            ..drew("02-servers-ru", 470)
        }],
    );

    assert_eq!(card.rows[0].verdict, Verdict::Yes);
    assert_eq!(
        card.headline.stream_errors,
        vec!["02-servers-ru".to_string()]
    );
    // …and it is not a gate failure on its own: the screen did land.
    assert_eq!(card.exit_code(), 0);
}

#[test]
fn document_moved_verdict_reports_the_no_op() {
    // Prompt 6 asks for nothing concrete, so the verdict is about movement.
    let corpus = corpus();
    let prompts = only(&corpus, "06");
    let card = evaluate(
        &corpus,
        &prompts,
        &[Measurement {
            page0_nodes: 1,
            version_before: Some(80),
            version_after: Some(80),
            terminal: Some("done".to_string()),
            ..Measurement::default()
        }],
    );

    assert_eq!(card.rows[0].verdict, Verdict::No);
    assert!(
        card.rows[0]
            .verdict_reason
            .contains("did not move the document (80 -> 80)"),
        "{}",
        card.rows[0].verdict_reason
    );
    // …and the taste half is never guessed.
    assert!(card.rows[0]
        .unmeasurable
        .iter()
        .any(|r| r.contains("BEAUTIFUL")));
}

#[test]
fn an_unmeasurable_prompt_is_not_a_pass_and_not_a_verdict() {
    let corpus = corpus();
    let prompts = only(&corpus, "01");
    let card = evaluate(
        &corpus,
        &prompts,
        &[Measurement {
            probe_error: Some("[QUALITY] GET /api/mcp/document failed".to_string()),
            ..drew("01-login-ru", 0)
        }],
    );

    assert_eq!(card.exit_code(), 3);
    assert_eq!(card.rows[0].verdict, Verdict::NotMeasured);
    assert!(regression_lines(&card)[0].starts_with("NOT MEASURED 01-login-ru"));
    assert!(!card.gate.passed);
}

#[test]
fn headline_counts_match_the_last_recorded_pass() {
    // Replaying gq2's own numbers through the scorecard: it is the calibration
    // of this tool against the hand measurement it replaces.
    let corpus = corpus();
    let prompts = corpus.selected(&[]).expect("all prompts");
    let recorded: [(u32, u32, Option<&str>); 8] = [
        (29, 0, Some("done")),  // 1
        (214, 8, Some("done")), // 2
        (75, 0, None),          // 3 — ended without a terminal event (#203)
        (0, 0, Some("done")),   // 4
        (82, 0, None),          // 5 — (#203)
        (1, 0, Some("done")),   // 6
        (126, 1, None),         // 7 — (#203)
        (0, 0, Some("done")),   // 8
    ];
    let measurements: Vec<Measurement> = prompts
        .iter()
        .zip(recorded.iter())
        .map(|(prompt, (nodes, issues, terminal))| Measurement {
            id: prompt.id.clone(),
            page0_nodes: *nodes,
            audit_issues: *issues,
            terminal: terminal.map(str::to_string),
            version_before: Some(1),
            version_after: Some(2),
            ..Measurement::default()
        })
        .collect();
    let card = evaluate(&corpus, &prompts, &measurements);

    // 4 of the 7 prompts that asked for a screen drew one (1, 2, 3, 5, 7);
    // prompts 4 and 8 are the recorded #202 regressions.
    assert_eq!(card.headline.screen_requested, 7);
    assert_eq!(card.headline.screen_drawn, 5);
    // Geometry issues in 2 of 8 documents — and 2 of the 5 that hold a screen,
    // which is the reading RESULTS.md insists on next to the raw count.
    assert_eq!(card.headline.geometry_issues, 2);
    assert_eq!(card.headline.documents_with_screen, 5);
    assert_eq!(card.headline.geometry_issues_with_screen, 2);
    assert_eq!(card.headline.terminal_missing, 3);
    // Nothing regressed against the recorded numbers, and 4, 6, 8 stay
    // known-broken.
    assert_eq!(card.exit_code(), 0);
    assert_eq!(card.gate.known_broken.len(), 3);
}
