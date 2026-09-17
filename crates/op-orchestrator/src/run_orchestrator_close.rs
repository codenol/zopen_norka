//! The closing stages of `Orchestrator::run` — phase 4's cleanup call and the
//! tally it produces, phase 5's post-generation validation, the
//! unfilled-screen invariant and the `RunSummary` the run answers with.
//!
//! Carved off `run_orchestrator.rs` to keep that driver under the 800-line cap.
//! The seam is the run's END: both functions take a document the sub-agents
//! have already written and answer with what the run left behind, so the
//! planning / scaffold / sub-agent phases stay in the spine where their order
//! is the behaviour.

use super::*;
use crate::repair_summary::RepairSummary;

/// The channels a closing stage talks through: where the document is written,
/// where progress is reported, and the flag that says the run was cancelled.
///
/// One value rather than three arguments because every stage in this module
/// needs all three and none of them means anything alone — a sink with no
/// progress channel reports nothing, and an abort flag with no sink has nothing
/// to abort. `finalize_run_cleanup` takes the two it uses directly, because it
/// never has to ask whether the run was cancelled.
pub(super) struct RunChannels<'a> {
    pub sink: &'a mut dyn DocSink,
    pub on_progress: &'a mut dyn FnMut(Progress),
    pub abort: &'a AbortFlag,
}

/// Phase 4's cleanup call, scoped either to the roots this run appended into
/// or to every screen-group root it inserted, plus the tally the passes left.
pub(super) fn finalize_run_cleanup(
    sink: &mut dyn DocSink,
    plan: &OrchestratorPlan,
    root_ids: &[String],
    outcomes: &[SubtaskOutcome],
    skip_root_insertion: bool,
    norm: &NormInfo,
    on_progress: &mut dyn FnMut(Progress),
) -> RepairSummary {
    let mut quality = crate::repair_summary::RepairSummary::default();
    if skip_root_insertion {
        let new_roots: Vec<&str> = outcomes
            .iter()
            .flat_map(|o| o.inserted_root_ids.iter().map(String::as_str))
            .collect();
        let target_roots: Vec<&str> = root_ids.iter().map(String::as_str).collect();
        crate::repair_scope::finalize_appended_design(
            sink,
            plan,
            &new_roots,
            &target_roots,
            &mut quality,
        );
    } else {
        // Every screen-group root (not just the first) goes in — this is
        // the co-op point with `wire_screen_navigation` (Track A of the
        // interactive-preview plan): `run_cleanup_passes` runs it LAST,
        // over `sink.state()` as a whole, so as long as every new root is
        // actually IN the document by now (it is — they're inserted
        // above, before any subtask runs) it links all of them into App
        // Mode navigation regardless of which root_ids are passed here.
        // Passing them all is still correct scoping for the OTHER
        // whole-root cleanup passes (dedup / avatar-repair / etc.).
        let root_id_refs: Vec<&str> = root_ids.iter().map(String::as_str).collect();
        finalize_design_with_summary_and_policy(
            sink,
            plan,
            &root_id_refs,
            &mut quality,
            CleanupPolicy {
                preserve_requested_root_height: norm.preserve_requested_root_height,
                is_deck: norm.is_deck,
                // `root_ids` here are the screen-group roots this run just
                // inserted — every node under them is this run's own.
                roots_are_run_output: true,
            },
        );
    }
    on_progress(Progress::CleanupDone);
    quality
}

/// Phase 5 / 6 and the answer: report the cleanup tally, run the vision
/// validation, name the screens that came out empty, and build the summary.
pub(super) fn close_run(
    channels: RunChannels<'_>,
    quality: &RepairSummary,
    request: &DesignRequest,
    providers: &ValidationProviders<'_>,
    outcomes: Vec<SubtaskOutcome>,
    root_ids: &[String],
) -> RunSummary {
    let RunChannels {
        sink,
        on_progress,
        abort,
    } = channels;
    // Turn the cleanup stage's tally into a user-visible credential. Only
    // fires when the passes actually ran (an empty summary means cleanup
    // was skipped entirely), so nothing is ever vouched for that nobody
    // checked.
    if !quality.is_empty() {
        on_progress(Progress::QualityChecked {
            checks: quality
                .checked()
                .into_iter()
                .map(|c| c.key().to_string())
                .collect(),
            repairs: quality
                .repaired()
                .into_iter()
                .map(|(check, count)| (check.key().to_string(), count))
                .collect(),
            records: quality
                .records()
                .iter()
                .map(crate::RepairRecord::line)
                .collect(),
            notes: quality.notes().to_vec(),
        });
    }
    sink.end_undo_batch();

    // -- 阶段 5:视觉校验 (S3c D1) — 在 cleanup 后、返回 RunSummary 前 --
    // Port of `orchestrator.ts:1247-1292`.
    // 守卫: request.validation_enabled && !abort.is_set().
    if request.validation_enabled && !abort.is_set() {
        let _ = run_post_generation_validation(
            sink,
            providers.pre_validator,
            providers.screenshot,
            providers.vision,
            &providers.system_prompt,
            request,
            on_progress,
            abort,
        );
    }

    // -- 阶段 6:承诺-交付不变量(classic 路径的诚实上报,与 loop 路径共享检测器)--
    // Runs AFTER cleanup + validation so a screen a structural pass or
    // the vision loop still touched is judged on its FINAL state, not
    // a stale mid-run snapshot. `mark_unfilled_screens` labels the
    // canvas itself (not just this summary) — a user scrolling the
    // layer panel sees it even without reading `RunSummary`.
    let unfilled_hits = crate::unfilled_screens::detect_unfilled_screens(sink.state());
    let unfilled_screens: Vec<String> = unfilled_hits.iter().map(|h| h.name.clone()).collect();
    if !unfilled_hits.is_empty() {
        crate::unfilled_screens::mark_unfilled_screens(sink, &unfilled_hits);
        on_progress(Progress::UnfilledScreens {
            names: unfilled_screens.clone(),
        });
    }

    let total_nodes = outcomes.iter().map(|o| o.node_count).sum();
    let paintable_nodes = outcomes.iter().map(|o| o.paintable_nodes).sum();
    RunSummary {
        // First surviving root is the "primary" root_frame_id — mirrors
        // the deleted concurrent path's identical convention so this
        // field's meaning never changed shape for existing callers.
        //
        // "Surviving" is the word that had gone stale: the ids captured
        // above are taken BEFORE `finalize_design`, which swaps the real
        // root in through `ReplaceSubtree` and allocates a fresh id, so the
        // field could name a node the document does not contain (issue
        // #29). A field that names the root is worth only what it resolves
        // to, so a stale id gives way to the root that is actually there.
        root_frame_id: live_root_id(sink.state(), root_ids),
        subtasks: outcomes,
        total_nodes,
        paintable_nodes,
        unfilled_screens,
    }
}
