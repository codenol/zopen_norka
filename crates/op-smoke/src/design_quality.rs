//! Design-quality mode (`op-smoke quality`) — the 8-prompt generation corpus,
//! measured in one command instead of by hand.
//!
//! ## What it does
//!
//! For every prompt in the corpus it makes the four calls the two hand
//! measurements of 2026-09-16 made (`.openpencil-tmp/gq/`, `.openpencil-tmp/gq2/`):
//! `POST /api/file/new` → `GET /api/mcp/document` → `POST /api/ai/standard` with
//! the browser's own body (drained to EOF) → `GET /api/mcp/document`. It then
//! scores what landed with `op-smoke`'s own audit ([`crate::audit_mode`] —
//! the real-layout geometry diagnostics plus [`crate::audit_rubric`]), prints a
//! scorecard, writes the same data as JSON, and exits non-zero when a prompt
//! came in below the expectation recorded in the corpus.
//!
//! ```sh
//! cargo run -p op-smoke -- quality --url http://127.0.0.1:3199
//! cargo run -p op-smoke -- quality --url http://127.0.0.1:3199 --only 04,05 --out /tmp/dq
//! ```
//!
//! Flags (every one of them optional except `--url`):
//!
//! - `--url <URL>` — **required**, the running daemon. There is no default port
//!   on purpose: a hard-coded port measures whatever happens to be listening on
//!   it, which is exactly how a "measurement" turns into a rumour.
//! - `--corpus <FILE>` — default `crates/op-smoke/corpus/design-quality.json`.
//! - `--out <DIR>` — default `.openpencil-tmp/design-quality`; per-prompt raw
//!   SSE, delta, thinking, the read-back document and its audit report go here,
//!   plus `design-quality.json` (the scorecard itself).
//! - `--model`, `--provider`, `--builtin-provider-id` — override the turn body.
//!   Env: `OPENPENCIL_SMOKE_QUALITY_MODEL` / `OPENPENCIL_ORCHESTRATOR_MODEL`,
//!   `OPENPENCIL_SMOKE_QUALITY_PROVIDER`. The corpus carries the browser's own
//!   values as the default.
//! - `--timeout <SECS>` — per HTTP call, default 300 (a design turn has been
//!   measured at 82s; the hand passes used `curl -m 200`).
//! - `--only <ID[,ID]>` — run a subset (`--only 04`, `--only 04,08`).
//! - `--dry-run` — print the plan and the recorded expectations, make no call.
//!
//! ## What it does NOT do
//!
//! - **It never guesses a daemon.** `--url` is mandatory and nothing here has a
//!   hard-coded port.
//! - **It makes no model call of its own**: the model runs inside the daemon you
//!   point it at, which is why, like `audit` and `program`, this mode is
//!   dispatched above the credential gate — it needs no
//!   `OPENPENCIL_LLM_PROVIDER`, no API key and no `agy` binary.
//! - **It cannot see rendering.** It reads documents, not pixels: whether the
//!   dark theme of prompt 5 is actually in force, and whether prompt 6's result
//!   is *beautiful*, are not measurable from `/api/mcp/document` and are printed
//!   as `n/a` with the corpus's reason rather than guessed. That is the
//!   measurement hole both hand passes hit.
//! - **It cannot prove the model's answer was complete.** It reports truncation
//!   signals (`{` against `}` balance, an unterminated `I(…)`, an unclosed code
//!   fence) from the reply text, and says `n/a` for replies that carry no
//!   document payload at all.
//! - **It cannot tell a regression from a bad sample.** The model is not
//!   deterministic: two identical runs of this corpus on 2026-09-16 measured
//!   2 regressions and then 0, with the same binary and the same daemon. One run
//!   is one sample. A `REGRESSION` line therefore means "this run came in below
//!   the recorded baseline", not "a commit changed the code" — re-run the prompt
//!   (`--only <id>`) before believing it.
//! - **It cannot attribute an orchestrator-route screen by name.** Only the DSL
//!   route names the frames it emits, so `landed` is 0 on the routes that apply
//!   server-side; on those rows the page-0 count is the measurement, and a
//!   stream error makes it a ceiling rather than proof (each such row says so
//!   under `caution`).
//!
//! ## WARNING: it resets the daemon's document
//!
//! Every prompt begins with `POST /api/file/new`. Point this at a daemon you
//! own, not at the one a browser tab is editing.
//!
//! ## Exit codes
//!
//! `0` clean · `1` a prompt regressed against the recorded baseline · `2` usage
//! error · `3` a prompt could not be measured, or the corpus/daemon is unusable
//! · `4` the scorecard could not be written.
//!
//! The gate itself is defined in [`report`]: a `required` prompt that falls below
//! `screenMinNodes` (or reports `done` without moving the document version) is a
//! regression; a `known_broken` prompt — one the last recorded pass already
//! failed — is reported as `KNOWN-BROKEN` and does not fail a run, because a gate
//! that is red forever is a gate nobody reads.

mod corpus;
mod document;
mod probe;
mod report;
mod truncation;

#[cfg(test)]
#[path = "design_quality/corpus_tests.rs"]
mod corpus_tests;
#[cfg(test)]
#[path = "design_quality/document_tests.rs"]
mod document_tests;
#[cfg(test)]
#[path = "design_quality/scorecard_tests.rs"]
mod scorecard_tests;
#[cfg(test)]
#[path = "design_quality/truncation_tests.rs"]
mod truncation_tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use corpus::{Corpus, GateState, Prompt, TurnBody};
use probe::DaemonProbe;
use report::{Measurement, Scorecard};

/// The subcommand that selects this mode.
pub(crate) const MODE_ARG: &str = "quality";

/// The corpus committed with the crate.
const DEFAULT_CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/corpus/design-quality.json");

/// Where per-prompt artifacts and the scorecard JSON land by default.
const DEFAULT_OUT_DIR: &str = ".openpencil-tmp/design-quality";

/// Default per-call HTTP timeout, in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

pub(crate) const USAGE: &str = "\
usage: op-smoke quality --url <DAEMON> [--corpus <FILE>] [--out <DIR>]
                        [--model <ID>] [--provider <NAME>] [--builtin-provider-id <ID>]
                        [--timeout <SECS>] [--only <ID[,ID]>] [--dry-run]

  Runs the design-quality corpus against a RUNNING daemon and prints a
  scorecard: one row per prompt with seconds, thinking/delta characters, the
  terminal event, nodes that landed, the top-level frames, truncation signals,
  the audit's issue count and a verdict.

  --url is required: this mode never guesses a port. It creates a FRESH
  DOCUMENT for every prompt (POST /api/file/new), so point it at a daemon you
  own.

  Exits 0 clean, 1 when a prompt regressed against the corpus's recorded
  baseline, 2 on usage, 3 when a prompt could not be measured, 4 when the
  scorecard could not be written.";

/// One parsed command line.
struct Options {
    url: String,
    corpus_path: String,
    out_dir: PathBuf,
    model: Option<String>,
    provider: Option<String>,
    builtin_provider_id: Option<String>,
    timeout: Duration,
    only: Vec<String>,
    dry_run: bool,
}

/// Runs the mode when `argv[1]` is `quality`.
///
/// `None` ⇒ this is not a quality run, so `main` carries on to the generation
/// modes. Called from `main` BEFORE any credential validation: the model runs
/// inside the daemon, so this mode needs no model environment of its own.
pub(crate) async fn run_if_requested() -> Option<ExitCode> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(MODE_ARG) {
        return None;
    }
    let rest: Vec<String> = args.collect();
    Some(run(&rest).await)
}

/// Entry point for `op-smoke quality …`.
async fn run(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let corpus = match Corpus::load(&options.corpus_path) {
        Ok(corpus) => corpus,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(3);
        }
    };
    let prompts = match corpus.selected(&options.only) {
        Ok(prompts) => prompts,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let body = corpus.body_for(&prompts[0], &options.overrides());
    println!(
        "op-smoke design-quality — {} prompt(s) vs {}\ncorpus: {}\nmodel:  {}  provider: {}  \
         effort: {}  thinking: {}  max_output_tokens: {}\nout:    {}",
        prompts.len(),
        options.url,
        options.corpus_path,
        body.model,
        body.provider,
        body.effort,
        body.thinking,
        body.max_output_tokens,
        options.out_dir.display()
    );

    if options.dry_run {
        print_plan(&corpus, &prompts);
        return ExitCode::SUCCESS;
    }

    if let Err(message) = std::fs::create_dir_all(&options.out_dir) {
        eprintln!(
            "[QUALITY] create out dir {}: {message}",
            options.out_dir.display()
        );
        return ExitCode::from(4);
    }
    let probe = match DaemonProbe::new(&options.url, corpus.routes.clone(), options.timeout) {
        Ok(probe) => probe,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    // Reachability preflight: an unreachable daemon must fail before the run
    // spends five to ten minutes of model time on eight prompts.
    if let Err(message) = probe.read_document().await {
        eprintln!(
            "{message}\n[QUALITY] is the daemon at {} running?",
            options.url
        );
        return ExitCode::from(3);
    }

    let started = std::time::Instant::now();
    let mut measurements = Vec::with_capacity(prompts.len());
    for (position, prompt) in prompts.iter().enumerate() {
        let measurement = measure_one(&probe, &corpus, prompt, &options).await;
        println!(
            "[{}/{}] {} — {:.1}s, pages[0] {} node(s), audit {} issue(s){}{}",
            position + 1,
            prompts.len(),
            prompt.id,
            measurement.seconds,
            measurement.page0_nodes,
            measurement.audit_issues,
            measurement
                .probe_error
                .as_ref()
                .map(|e| format!(" — NOT MEASURED: {e}"))
                .unwrap_or_default(),
            measurement
                .errors
                .first()
                .map(|e| format!(" — stream error: {e}"))
                .unwrap_or_default(),
        );
        measurements.push(measurement);
    }

    let scorecard = report::evaluate(&corpus, &prompts, &measurements);
    let json_path = options.out_dir.join("design-quality.json");
    if let Err(message) = write_scorecard(&json_path, &scorecard) {
        eprintln!("{message}");
        return ExitCode::from(4);
    }

    print_scorecard(&scorecard, &options, started.elapsed());
    println!("\nscorecard written: {}", json_path.display());
    ExitCode::from(scorecard.exit_code())
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            url: String::new(),
            corpus_path: std::env::var("OPENPENCIL_SMOKE_QUALITY_CORPUS")
                .unwrap_or_else(|_| DEFAULT_CORPUS.to_string()),
            out_dir: PathBuf::from(
                std::env::var("OPENPENCIL_SMOKE_QUALITY_OUT")
                    .unwrap_or_else(|_| DEFAULT_OUT_DIR.to_string()),
            ),
            model: std::env::var("OPENPENCIL_SMOKE_QUALITY_MODEL")
                .ok()
                .or_else(|| std::env::var("OPENPENCIL_ORCHESTRATOR_MODEL").ok())
                .filter(|m| !m.is_empty()),
            provider: std::env::var("OPENPENCIL_SMOKE_QUALITY_PROVIDER")
                .ok()
                .filter(|p| !p.is_empty()),
            builtin_provider_id: None,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            only: Vec::new(),
            dry_run: false,
        };
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
                _ => (arg, None),
            };
            let mut take_value = |what: &str| -> Result<String, String> {
                if let Some(value) = inline.clone() {
                    return Ok(value);
                }
                index += 1;
                args.get(index)
                    .cloned()
                    .ok_or_else(|| format!("error: {what} needs a value"))
            };
            match flag {
                "--url" => options.url = take_value("--url")?,
                "--corpus" => options.corpus_path = take_value("--corpus")?,
                "--out" => options.out_dir = PathBuf::from(take_value("--out")?),
                "--model" => options.model = Some(take_value("--model")?),
                "--provider" => options.provider = Some(take_value("--provider")?),
                "--builtin-provider-id" => {
                    options.builtin_provider_id = Some(take_value("--builtin-provider-id")?)
                }
                "--timeout" => {
                    let raw = take_value("--timeout")?;
                    options.timeout = Duration::from_secs(
                        raw.parse()
                            .map_err(|_| format!("error: --timeout {raw:?} is not a number"))?,
                    );
                }
                "--only" => {
                    options.only = take_value("--only")?
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                "--dry-run" => options.dry_run = true,
                other => return Err(format!("error: unknown argument {other:?}")),
            }
            index += 1;
        }
        if options.url.trim().is_empty() {
            return Err(
                "error: --url <DAEMON> is required (e.g. --url http://127.0.0.1:3199): this mode \
                 drives a running daemon and never guesses a port"
                    .to_string(),
            );
        }
        Ok(options)
    }

    fn overrides(&self) -> corpus::BodyOverrides {
        corpus::BodyOverrides {
            provider: self.provider.clone(),
            builtin_provider_id: self.builtin_provider_id.clone(),
            model: self.model.clone(),
        }
    }
}

/// Prints the corpus plan without touching the daemon.
fn print_plan(corpus: &Corpus, prompts: &[Prompt]) {
    println!("\nplan (--dry-run: no call was made)");
    for prompt in prompts {
        println!(
            "\n{:>2}  {}  [{}]  asks for a screen: {}  verdict rule: {:?}",
            prompt.index, prompt.id, prompt.language, prompt.screen_requested, prompt.verdict_rule
        );
        println!("    intent : {}", prompt.intent);
        println!("    prompt : {}", prompt.prompt);
        println!(
            "    recorded: {} pages[0]={}n screen={} audit={} → {}  |  {} pages[0]={}n screen={} \
             audit={} → {}",
            prompt.baseline.pass,
            prompt.baseline.page0_nodes,
            prompt.baseline.screen,
            prompt.baseline.audit_issues,
            prompt.baseline.verdict,
            prompt.last.pass,
            prompt.last.page0_nodes,
            prompt.last.screen,
            prompt.last.audit_issues,
            prompt.last.verdict
        );
        println!(
            "    gate   : {} — {}",
            match prompt.expectation.state {
                GateState::Required => format!(
                    "required: pages[0] >= {} node(s), document version must move: {}",
                    corpus.screen_min_nodes, prompt.expectation.require_version_move
                ),
                GateState::KnownBroken => format!(
                    "known-broken: fail only below pages[0] = {} node(s) (the recorded {} failure); \
                     >= {} reports RECOVERED",
                    prompt.expectation.floor_page0_nodes,
                    prompt.last.pass,
                    corpus.screen_min_nodes
                ),
            },
            prompt.expectation.note
        );
        for reason in &prompt.unmeasurable {
            println!("    n/a    : {reason}");
        }
    }
}

/// Measures one prompt: the four calls, the artifacts, the audit.
async fn measure_one(
    probe: &DaemonProbe,
    corpus: &Corpus,
    prompt: &Prompt,
    options: &Options,
) -> Measurement {
    let mut measurement = Measurement {
        id: prompt.id.clone(),
        ..Measurement::default()
    };
    let body: TurnBody = corpus.body_for(prompt, &options.overrides());

    if let Err(error) = probe.new_document().await {
        measurement.probe_error = Some(error);
        return measurement;
    }
    let before = match probe.read_document().await {
        Ok(before) => before,
        Err(error) => {
            measurement.probe_error = Some(error);
            return measurement;
        }
    };
    measurement.version_before = before.version;

    let stream = match probe.stream_turn(&body).await {
        Ok(stream) => stream,
        Err(error) => {
            measurement.probe_error = Some(error);
            return measurement;
        }
    };
    measurement.seconds = stream.seconds;
    measurement.thinking_chars = stream.thinking.chars().count();
    measurement.delta_chars = stream.delta.chars().count();
    measurement.events = stream.events;
    measurement.terminal = stream.terminal.clone();
    measurement.errors = stream.errors.clone();
    measurement.truncation = truncation::detect(&stream.delta);
    artifact(
        &mut measurement,
        &options.out_dir,
        prompt,
        "sse",
        &stream.raw,
    );
    artifact(
        &mut measurement,
        &options.out_dir,
        prompt,
        "delta.txt",
        &stream.delta,
    );
    artifact(
        &mut measurement,
        &options.out_dir,
        prompt,
        "thinking.txt",
        &stream.thinking,
    );

    probe.settle().await;
    let after = match probe.read_document().await {
        Ok(after) => after,
        Err(error) => {
            measurement.probe_error = Some(error);
            return measurement;
        }
    };
    measurement.version_after = after.version;

    let inventory = document::page0(&after.payload, 0);
    measurement.page0_nodes = inventory.nodes;
    measurement.frames = inventory.frame_names();
    let statements = document::scan_dsl_statements(&stream.delta);
    let (landed, landed_nodes) = document::attribution(&statements, &inventory);
    measurement.landed_frames = landed.iter().map(|f| f.name.clone()).collect();
    measurement.landed_nodes = landed_nodes;
    measurement.root_statements = document::root_statement_count(&statements);

    audit(&mut measurement, prompt, &after.payload, options);
    measurement
}

/// Runs `op-smoke`'s own audit (`audit_mode`, zero model calls) over the
/// document this turn produced and keeps its report beside the raw stream.
fn audit(
    measurement: &mut Measurement,
    prompt: &Prompt,
    payload: &serde_json::Value,
    options: &Options,
) {
    let document = payload.get("document").unwrap_or(payload);
    let text = document.to_string();
    let loaded = match jian_ops_schema::load_str(&text) {
        Ok(loaded) => loaded.value,
        Err(error) => {
            measurement.audit_error = Some(format!("load read-back document: {error}"));
            return;
        }
    };
    let label = format!("{} (read back by op-smoke quality)", prompt.id);
    let state = op_editor_core::EditorState::from_document(loaded);
    let report = crate::audit_mode::audit_state(&state, &label);
    artifact(
        measurement,
        &options.out_dir,
        prompt,
        "audit.json",
        &report.json,
    );
    artifact(measurement, &options.out_dir, prompt, "op", &text);
    let parsed: serde_json::Value = match serde_json::from_str(&report.json) {
        Ok(parsed) => parsed,
        Err(error) => {
            measurement.audit_error = Some(format!("parse audit report: {error}"));
            return;
        }
    };
    measurement.audit_clean = report.clean;
    measurement.audit_issues = parsed
        .get("issueCount")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    measurement.audit_first_issue = parsed
        .get("issues")
        .and_then(serde_json::Value::as_array)
        .and_then(|issues| issues.first())
        .and_then(|issue| issue.as_str())
        .map(str::to_string);
}

/// Writes one per-prompt artifact. A failed write is reported on the row (the
/// measurement itself is still valid — it does not depend on disk).
fn artifact(
    measurement: &mut Measurement,
    out_dir: &Path,
    prompt: &Prompt,
    suffix: &str,
    body: &str,
) {
    let path = out_dir.join(format!("{}.{suffix}", prompt.id));
    if let Err(error) = std::fs::write(&path, body) {
        measurement
            .artifact_errors
            .push(format!("{}: {error}", path.display()));
    }
}

/// Writes the scorecard JSON beside the printed one.
fn write_scorecard(path: &Path, scorecard: &Scorecard) -> Result<(), String> {
    let json = serde_json::to_string_pretty(scorecard)
        .map_err(|e| format!("[QUALITY] serialize scorecard: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("[QUALITY] write {}: {e}", path.display()))
}

/// Prints the scorecard table, the headline counts and the gate's verdict.
fn print_scorecard(scorecard: &Scorecard, options: &Options, elapsed: Duration) {
    println!(
        "\n #  {:<18} {:>6} {:>7} {:>7}  {:<11} {:>4} {:>6}  {:<4} {:<9} {:>6}  verdict",
        "id", "sec", "think", "delta", "term", "p0", "landed", "asks", "trunc", "issues"
    );
    for row in &scorecard.rows {
        let measurement = &row.measurement;
        println!(
            "{:>2}  {:<18} {:>6.1} {:>7} {:>7}  {:<11} {:>4} {:>6}  {:<4} {:<9} {:>6}  {}",
            row.index,
            row.id,
            measurement.seconds,
            measurement.thinking_chars,
            measurement.delta_chars,
            measurement
                .terminal
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            measurement.page0_nodes,
            measurement.landed_nodes,
            if row.screen_requested { "yes" } else { "no" },
            measurement.truncation.label(),
            if measurement.audit_error.is_some() {
                "n/a".to_string()
            } else {
                measurement.audit_issues.to_string()
            },
            row.verdict.label()
        );
        println!(
            "    frames  : {}",
            if measurement.frames.is_empty() {
                "(none)".to_string()
            } else {
                measurement.frames.join(", ")
            }
        );
        let detail = |label: &str, value: &str| println!("    {label:<8}: {value}");
        detail("intent", &row.intent);
        if !measurement.landed_frames.is_empty() {
            detail(
                "landed",
                &format!(
                    "{} ({} node(s), {} root statement(s))",
                    measurement.landed_frames.join(", "),
                    measurement.landed_nodes,
                    measurement.root_statements
                ),
            );
        }
        if !measurement.truncation.signals.is_empty() {
            detail("trunc", &measurement.truncation.signal_line());
        }
        if let Some(reason) = &measurement.truncation.not_applicable {
            detail("n/a", &format!("truncation — {reason}"));
        }
        if let Some(issue) = &measurement.audit_first_issue {
            detail("issue", issue);
        }
        if let Some(error) = &measurement.audit_error {
            detail("n/a", &format!("audit — {error}"));
        }
        detail(
            "verdict",
            &format!("{} — {}", row.verdict.label(), row.verdict_reason),
        );
        for reason in &row.unmeasurable {
            detail("n/a", reason);
        }
        if row.known_broken {
            detail(
                "gate",
                &format!("KNOWN-BROKEN (recorded): {}", row.expectation_note),
            );
        }
        if row.recovered {
            detail(
                "gate",
                "RECOVERED — this prompt now meets the screen threshold the last pass missed",
            );
        }
        for failure in &row.gate_failures {
            detail("gate", &format!("REGRESSION — {failure}"));
        }
        for error in &measurement.errors {
            detail("stream", error);
        }
        if !measurement.errors.is_empty() || measurement.terminal.as_deref() == Some("error") {
            detail(
                "caution",
                "the stream reported an error, so what landed may be the route's placed base \
                 rather than this turn's own output — read this row's node count as a ceiling, \
                 not as proof (gq/RESULTS.md measured exactly this on a transport failure)",
            );
        }
        for error in &measurement.artifact_errors {
            detail("artifact", error);
        }
        if let Some(error) = &measurement.probe_error {
            detail("error", error);
        }
        println!();
    }

    let headline = &scorecard.headline;
    println!("headline");
    println!(
        "  {} of {} prompt(s) that asked for a screen drew one",
        headline.screen_drawn, headline.screen_requested
    );
    println!(
        "  geometry issues: {} of {} document(s)  (over the {} that hold a screen: {}){}",
        headline.geometry_issues,
        headline.prompts - headline.not_measured,
        headline.documents_with_screen,
        headline.geometry_issues_with_screen,
        if headline.audit_skipped > 0 {
            format!("  [audit skipped for {}]", headline.audit_skipped)
        } else {
            String::new()
        }
    );
    println!(
        "  terminal event: {} of {} turn(s) ended without one (issue #203 — reported, not gated)",
        headline.terminal_missing,
        headline.prompts - headline.not_measured
    );
    println!(
        "  truncation: {} of {} reply/replies that carried a document payload were cut off",
        headline.truncated, headline.truncation_judged
    );
    if !headline.stream_errors.is_empty() {
        println!(
            "  stream errors: {} of {} turn(s) — {} (their node counts are ceilings: see the \
             `caution` line on each row)",
            headline.stream_errors.len(),
            headline.prompts - headline.not_measured,
            headline.stream_errors.join(", ")
        );
    }
    println!(
        "  total: {:.1}s against {}",
        elapsed.as_secs_f64(),
        options.url
    );

    println!("\ngate");
    let lines = report::regression_lines(scorecard);
    if lines.is_empty() {
        println!("  (no failure line: every prompt was measured and none regressed)");
    }
    for line in lines {
        println!("  {line}");
    }
    println!("  {}", report::gate_summary(scorecard));
    println!(
        "  note: one run is ONE SAMPLE of a non-deterministic model. A REGRESSION line means \"this \
         run came in below the recorded baseline\", not \"a commit changed the code\" — run the \
         prompt again (`--only <id>`) before treating a line as a regression in the product."
    );
}
