//! The run's progress vocabulary — `Progress`, the screen-group wrapper it
//! carries ([`WorkerEvent`]), and the `SkillBrief` payload its `SubtaskSkills`
//! arm reports.
//!
//! Carved off `types.rs` to keep that module under the 800-line cap. The seam
//! is the event family: everything here is what a run TELLS its host, while
//! `types.rs` keeps the traits, the call/vision contracts and the run's
//! request + result types. Re-exported from `types`, so `crate::types::Progress`
//! and the crate root's glob are unchanged.

use std::collections::BTreeMap;

/// `run()` 的进度回调载荷。字段从 S3a 定全,S3b/S3c 只填新分支。
#[derive(Debug, Clone)]
pub enum Progress {
    Planning,
    /// Planning produced the FULL subtask list — emitted ONCE right after
    /// planning so the UI can show the complete task checklist upfront (TS
    /// parity), instead of revealing tasks one-by-one as each starts. Pairs
    /// `id` with `label` so the UI can mark each row done on `SubtaskDone`.
    Planned {
        subtasks: Vec<(String, String)>,
    },
    ScaffoldDone,
    SubtaskStarted {
        id: String,
        label: String,
    },
    SubtaskDone {
        id: String,
        node_count: usize,
    },
    SubtaskFailed {
        id: String,
        error: String,
    },
    /// Per-subtask skill-load report — emitted right after the sub-agent
    /// prompt is built (from the merged `SkillLoadReport`). `dropped` carries
    /// `(name, reason_display)` pairs for diagnostics; user-facing activity
    /// deliberately omits skill, token-budget, and context-drop internals.
    SubtaskSkills {
        id: String,
        included: Vec<SkillBrief>,
        dropped: Vec<(String, String)>,
        budget_used: u32,
        budget_max: u32,
    },
    /// Emitted on each subtask retry with the reason (e.g. "zero nodes
    /// generated"). `attempt` is the 1-based attempt number being retried into.
    SubtaskRetry {
        id: String,
        attempt: u8,
        reason: String,
    },
    /// Emitted once, right before a `geometry_echo` in-loop self-correction
    /// retry starts (`concurrent::run_subtask_retry_ladder`'s tail) — a
    /// fact line so the progress panel shows this step actually working,
    /// not a silent extra LLM call. `issue_count` is how many diagnostic
    /// lines `geometry_validation::geometry_diagnostics_for_roots` found.
    GeometryEcho {
        id: String,
        issue_count: usize,
    },
    /// Emitted ONCE, right before `RunSummary` is returned, whenever the
    /// "promise-delivery" invariant check (`unfilled_screens::detect_unfilled_screens`)
    /// finds a scaffolded screen that never received real content — the
    /// classic-path honest report (postmortem 0718-1-glm-1: a silently
    /// delivered blank screen). `names` are already marked on the canvas
    /// (`" (unfilled)"` suffix) by the time this fires. Never emitted when
    /// nothing is unfilled.
    UnfilledScreens {
        names: Vec<String>,
    },
    /// Emitted after each sub-agent LLM reply is applied — lets the UI
    /// show a live node count while the subtask is still running.
    SubtaskNodes {
        id: String,
        nodes_so_far: usize,
    },
    /// Emitted ONCE, right before the screen-group concurrent phase starts
    /// (D-lite "three-piece" visibility fix, 2026-07-17) — a user-facing
    /// confirmation that `group_count` screens are about to run against
    /// `workers` overlapping workers, so ⚡Nx's effect is legible instead of
    /// silent. Never emitted on the sequential path (`workers == 1` never
    /// fires this — see `run.rs`'s `effective_concurrency` branch).
    ConcurrentGroupsStarted {
        group_count: usize,
        workers: u32,
    },
    /// The dual of [`Progress::ConcurrentGroupsStarted`] — emitted ONCE when
    /// the plan has ≥2 screen groups but the SEQUENTIAL phase-3 loop ran
    /// anyway (`effective_concurrency == 1`). Self-diagnostic
    /// (sequential-execution root-cause hunt, 2026-07-17): `requested_workers`
    /// is the raw, unclamped `DesignRequest.concurrency` this turn carried —
    /// `1` proves the ⚡Nx picker's value never reached the orchestrator for
    /// this turn; any value `> 1` here would mean `effective_concurrency`
    /// itself has a bug (its contract guarantees `> 1` whenever
    /// `group_count > 1 && clamp_concurrency(requested_workers) > 1`).
    ScreenGroupsSequential {
        group_count: usize,
        requested_workers: u32,
    },
    /// Progress emitted by one screen-group agent in the concurrent classic
    /// orchestrator path. The boxed inner event keeps the recursive enum
    /// finite-sized while preserving the ordinary [`Progress`] vocabulary for
    /// subtask lifecycle updates.
    ///
    /// `group_idx` identifies the screen group, not a semaphore worker slot:
    /// three screen groups running with a concurrency limit of two still have
    /// three stable identities and three independent progress streams.
    WorkerScoped(WorkerEvent),
    CleanupDone,
    /// The user-visible quality credential for the classic path — emitted
    /// once, right after [`Progress::CleanupDone`], carrying what the cleanup
    /// passes actually checked and repaired
    /// (`crate::repair_summary::RepairSummary`, flattened to wire strings so
    /// hosts render it without depending on this crate's enum).
    ///
    /// A separate variant rather than a payload on `CleanupDone` so existing
    /// `CleanupDone` matchers (ordering assertions, the "polishing" narration)
    /// keep working untouched.
    ///
    /// Deliberately carries NO leftover-issue count: the promise-delivery
    /// check (`Progress::UnfilledScreens`) runs later in the pipeline, so
    /// anything claimed here about remaining work would be a guess. Renderers
    /// must omit that clause rather than assume "none".
    QualityChecked {
        /// Check-family keys that ran, in display order.
        checks: Vec<String>,
        /// `(check family, document edits applied)` for families that
        /// repaired something.
        repairs: Vec<(String, usize)>,
        /// One already-rendered line per applied edit, in application order
        /// (`crate::repair_record::RepairRecord::line`). The itemized half of
        /// the same tally `repairs` counts — see `RepairSummary`, where both
        /// are derived from one list so they cannot disagree.
        records: Vec<String>,
        /// Statements about the run that are not edits — today, the
        /// deliberate skip of a whole pass tier for authored template input
        /// (`crate::repair_tier`). Never counted as repairs; rendered ahead
        /// of them, because "this was not run" outranks "this was".
        notes: Vec<String>,
    },
    // ── S3c: Vision-validation progress variants ─────────────────────────────
    /// 视觉校验阶段开始(pre-validation 将在此之后立即运行)。
    ValidationStarted,
    /// Pre-validation 完成;`applied` = 总修复数,`by_category` = 分类明细。
    ValidationPreCheckDone {
        applied: usize,
        by_category: BTreeMap<String, usize>,
    },
    /// 某轮视觉 LLM 调用开始。`round` ∈ [1, MAX_VALIDATION_ROUNDS]。
    ValidationRoundStarted {
        round: u8,
    },
    /// 某轮视觉 LLM 调用完成并已应用修复。
    ValidationRoundDone {
        round: u8,
        applied: usize,
        quality_score: u8,
    },
    /// 整个视觉校验阶段完成。`total_applied` = pre + 所有轮次之和。
    ValidationDone {
        total_applied: usize,
    },
    // ── S4: Visual-Ref pipeline progress variants ─────────────────────────────
    /// Visual-ref pipeline started (before design-system generation).
    ///
    /// Port of the entry point in `visual-ref-orchestrator.ts:62`.
    VisualRefStarted,
    /// Design system generated and variables seeded.
    ///
    /// Port of stage 1 in `visual-ref-orchestrator.ts:74-90`.
    VisualRefDesignSystem {
        /// Number of `SetVariable*` commands emitted (one per palette/spacing/radius token).
        var_count: usize,
    },
    /// HTML code generated from the design system.
    ///
    /// Port of stage 2 in `visual-ref-orchestrator.ts:92-106`.
    VisualRefHtmlGenerated {
        /// Byte length of the generated HTML string.
        byte_len: usize,
    },
    /// Screenshot step complete (or skipped when `VisualRefProvider` returns `None`).
    ///
    /// Port of stage 3 in `visual-ref-orchestrator.ts:108-122`.
    VisualRefScreenshotReady {
        /// `true` when the screenshot was skipped (provider returned `None`).
        skipped: bool,
    },
    /// Visual-ref pipeline fell back to plain orchestration.
    ///
    /// Emitted when any stage fails or the provider skips. Port of the
    /// fallback path in `visual-ref-orchestrator.ts:124-140`.
    VisualRefFallback {
        /// Human-readable reason for the fallback.
        reason: String,
    },
}

/// Stable context attached to one screen group's progress events.
#[derive(Debug, Clone)]
pub struct WorkerEvent {
    pub group_idx: usize,
    pub screen: String,
    pub identity: crate::agent_identity::AgentIdentity,
    pub event: Box<Progress>,
}

impl Progress {
    /// Wrap an ordinary progress event with its screen-group identity.
    pub fn worker_scoped(
        group_idx: usize,
        screen: impl Into<String>,
        identity: crate::agent_identity::AgentIdentity,
        event: Progress,
    ) -> Self {
        Self::WorkerScoped(WorkerEvent {
            group_idx,
            screen: screen.into(),
            identity,
            event: Box::new(event),
        })
    }
}

/// A one-line summary of an included skill, surfaced to the chat UI via
/// `Progress::SubtaskSkills`. Mirrors `op_ai_skills::SkillLoadEntry` minus
/// the `category` field (the UI line doesn't display it).
#[derive(Debug, Clone)]
pub struct SkillBrief {
    pub name: String,
    pub token_count: u32,
    pub truncated: bool,
}

impl SkillBrief {
    /// Build a brief from an `op-ai-skills` report entry (name + token_count +
    /// truncated; category is dropped — the UI line doesn't show it).
    pub fn from_entry(e: &op_ai_skills::SkillLoadEntry) -> SkillBrief {
        SkillBrief {
            name: e.name.clone(),
            token_count: e.token_count,
            truncated: e.truncated,
        }
    }
}

/// Short, user-facing word for a `DropReason` (used in the `▸ dropped:` line).
fn drop_reason_display(reason: &op_ai_skills::DropReason) -> &'static str {
    use op_ai_skills::DropReason::*;
    match reason {
        IntentMiss => "intent",
        BudgetExhausted => "budget",
        TierFiltered => "tier",
        MinimalMode => "minimal",
        ReducedComplexity => "reduced",
        Deduped => "dedup",
        ContentMismatch => "mismatch",
        ModelFamilyMiss => "family",
    }
}

/// Decompose a merged `SkillLoadReport` into the four payload parts of
/// `Progress::SubtaskSkills` (included briefs, `(name, reason)` drops,
/// budget_used, budget_max).
pub fn report_to_progress_parts(
    report: &op_ai_skills::SkillLoadReport,
) -> (Vec<SkillBrief>, Vec<(String, String)>, u32, u32) {
    let included = report.included.iter().map(SkillBrief::from_entry).collect();
    let dropped = report
        .dropped
        .iter()
        .map(|d| (d.name.clone(), drop_reason_display(&d.reason).to_string()))
        .collect();
    (included, dropped, report.budget_used, report.budget_max)
}
