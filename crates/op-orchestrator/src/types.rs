//! 公共 trait 与类型 —— 编排器与外界的全部接口。
//!
//! 副作用只从 [`DocSink`](文档变更)与 [`LlmClient`](LLM 调用)
//! 两个 trait 进出;其余全是数据类型。

use crate::plan::Subtask;
use futures::stream::BoxStream;
use jian_ops_schema::node::PenNode;
use op_editor_core::{EditorCommand, EditorState, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

// The run's progress vocabulary is a family of its own in a sibling; the
// re-export keeps `crate::types::Progress` and the crate root's glob working.
#[path = "types_progress.rs"]
mod progress;
pub use progress::*;

/// 文档出口。写经 [`apply`](DocSink::apply);读经
/// [`state`](DocSink::state)。host 实现把 apply 与存盘 / 重绘 /
/// undo 批界绑在一处(对齐 `op-mcp` 的 applier 模式)。
pub trait DocSink: Send {
    /// 只读访问当前文档 —— cleanup 判定空 scaffold、未来 S3c
    /// 校验都要读。
    fn state(&self) -> &EditorState;
    /// 应用一条编辑命令;返回 `false` 表示命令被拒(文档未变)。
    fn apply(&mut self, cmd: EditorCommand) -> bool;
    /// Apply an `InsertSubtree` and return the post-remap root ids.
    /// `None` = rejected (document unchanged).
    ///
    /// The default implementation routes through `apply` and returns
    /// `Some(vec![])` on success — post-remap ids are unavailable on
    /// buffered / remote sinks where the real `EditorState` is not local.
    /// Override on immediate-apply sinks (e.g. `VecDocSink`) to surface
    /// the real remapped ids from `EditorState::insert_subtree_returning_root_ids`.
    fn insert_subtree_returning_root_ids(
        &mut self,
        nodes: Vec<PenNode>,
        parent_id: &NodeId,
    ) -> Option<Vec<String>> {
        let applied = self.apply(EditorCommand::InsertSubtree {
            nodes,
            parent_id: parent_id.clone(),
            page_id: None,
        });
        if applied {
            Some(vec![])
        } else {
            None
        }
    }
    /// 开启一个 undo 批 —— 批内的所有 apply 合并为一次 undo。
    fn begin_undo_batch(&mut self);
    /// 关闭当前 undo 批。
    fn end_undo_batch(&mut self);
}

/// LLM 调用出口。每次 [`call`](LlmClient::call) 是一次独立、无累积
/// 上下文的 LLM turn —— host 实现应为每次调用新建引擎,使规划与
/// 各 sub-agent 拿到隔离上下文。
pub trait LlmClient: Send + Sync {
    fn call(&self, req: CallRequest) -> BoxStream<'static, Result<LlmChunk, LlmError>>;
}

/// 一次 LLM 调用的完整输入。字段一次定全,S3b 并发 / 流式不必
/// 改 trait 签名。
#[derive(Debug, Clone)]
pub struct CallRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub timeout: Duration,
    pub abort: AbortFlag,
    /// 从请求开始到收到第一个文本 chunk 的超时;`None` 表示不设。
    /// Port of `noTextTimeoutMs` in the TS timeout profiles.
    pub no_text_timeout: Option<Duration>,
    /// 从第一个文本 chunk 到"真正内容"出现的超时;`None` 表示不设。
    /// Port of `firstTextTimeoutMs` in the TS timeout profiles.
    pub first_text_timeout: Option<Duration>,
}

/// 流元素 —— 区分文本与思考,与 TS 的 text/thinking/error 三分
/// 一致。错误走 `Result` 的 `Err(LlmError)`,不混入 chunk。
#[derive(Debug, Clone)]
pub enum LlmChunk {
    Text(String),
    Thinking(String),
}

/// 一次 LLM 调用的失败。`aborted` 区分用户中止与真实错误。
#[derive(Debug, Clone)]
pub struct LlmError {
    pub message: String,
    pub aborted: bool,
}

// ── S3c: Vision-validation traits + supporting types ──────────────────────────

/// 预校验(pre-validation)出口。host 实现把 TS `runPreValidationFixes`
/// 的七个 pass 注入;stub 实现直接返回零修复计数。
///
/// Port of `design-pre-validation.ts:38-45` shape + spec §4.1.
pub trait PreValidator: Send + Sync {
    /// 在文档 sink 上原地运行所有预校验修复 pass,并返回修复统计。
    fn run_pre_validation_fixes(&self, sink: &mut dyn DocSink) -> PreValidationResult;
}

/// 截图出口。host 实现调用 `SkiaEngine.captureRegion`(Rust 侧走
/// `op_pen_loader::editor_state_to_layout_scene` + raster 导出);stub
/// 返回 `None` 表示"跳过视觉轮次"。
///
/// `state` 是**当前已变更**的文档 —— 校验循环在每一轮 apply 修复**之后**
/// 调用本方法,所以 host 必须渲染 `sink.state()` 此刻持有的实时状态。
/// 这与 `PreValidator::run_pre_validation_fixes` 收 sink 同理(后者读
/// `sink.state().doc`),trait 给 provider 一个看到实时状态的入口而不必
/// 在带外共享一份会过期的快照。
///
/// Port of `design-screenshot.ts` shape + spec §4.1.
pub trait ScreenshotProvider: Send + Sync {
    /// 捕捉画布根帧截图,返回 base64 PNG;`None` 表示不可用 / 跳过。
    fn capture_root_frame(&self, state: &EditorState) -> Option<String>;
}

/// 视觉 LLM 调用出口。host 实现使用多模态 LLM;stub 返回
/// `VisionResponse::Skipped`。
///
/// Port of `validateDesignScreenshot` shape in `design-validation.ts:137-228`
/// + spec §4.1.
pub trait VisionLlmClient: Send + Sync {
    /// 执行一次同步视觉校验调用并返回结果。
    ///
    /// CONTRACT — `Text` means "the model answered about **every** picture in
    /// `req.images`". An implementation MUST return `Skipped` rather than send
    /// a call missing one of them (the prompt names each picture by its
    /// position, so a dropped image turns the comparison the prompt asks for
    /// into a question about nothing) and MUST return `Skipped` rather than
    /// issue a text-only call when the image cannot reach the model: a model
    /// handed a file name
    /// writes a confident description of a picture it never saw, and
    /// `reference_brief` feeds that text to the planner as an inventory of the
    /// user's screen (issue #61). Enforced host-side by asking
    /// `ChatProvider::attachment_transport` first.
    fn validate(&self, req: VisionCallRequest) -> VisionResponse;
}

/// `PreValidator::run_pre_validation_fixes` 的返回值 —— 修复统计。
///
/// Port of the TS `{ total, byCategory }` shape.
#[derive(Debug, Clone, Default)]
pub struct PreValidationResult {
    /// 本次运行所有 pass 合计修复的节点数。
    pub total: usize,
    /// 按 pass 类别细分的修复计数(key = pass 名称)。
    pub by_category: BTreeMap<String, usize>,
}

/// What a picture inside a [`VisionCallRequest`] is — the part it plays in the
/// sentence the prompt uses to ask for a comparison.
///
/// The role exists because the prompt tells the model which picture is which by
/// *position* ("image 1 is …"), so the payload and the prompt have to agree on
/// the order. Issue #62 was exactly that disagreement: the prompt promised a
/// reference screenshot the request never carried, and the model answered about
/// the only picture it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionRole {
    /// The screenshot of the design under review.
    Design,
    /// The user's reference design the [`Self::Design`] screenshot should match.
    Reference,
}

impl VisionRole {
    /// The words a prompt uses when it names this picture to the model.
    pub fn prompt_label(self) -> &'static str {
        match self {
            Self::Design => "the CURRENT design under review",
            Self::Reference => "the user's REFERENCE design",
        }
    }

    /// 1-based position of this role in `images` — the number a prompt must
    /// use when it names that picture. `None` when no picture carries it.
    pub fn position_in(self, images: &[VisionImage]) -> Option<usize> {
        images.iter().position(|i| i.role == self).map(|i| i + 1)
    }
}

/// One picture a [`VisionCallRequest`] carries: its base64 payload plus the
/// role that decides how the prompt must name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisionImage {
    /// Which picture this is — see [`VisionRole`].
    pub role: VisionRole,
    /// base64 payload (PNG / JPEG / GIF / WebP). The host decides the media
    /// type from the bytes, never from this struct.
    pub base64: String,
}

impl VisionImage {
    pub fn new(role: VisionRole, base64: impl Into<String>) -> Self {
        Self {
            role,
            base64: base64.into(),
        }
    }
}

/// `VisionLlmClient::validate` 的输入 —— 单轮视觉校验请求。
///
/// Port of the call-site params in `validateDesignScreenshot`
/// (`design-validation.ts:137`).
#[derive(Debug, Clone)]
pub struct VisionCallRequest {
    /// 视觉 LLM system prompt。
    pub system: String,
    /// 携带节点树 dump 与修复指令的 user message。
    pub message: String,
    /// The pictures this request asks the model to look at, **in the order the
    /// transport delivers them** — which is the order the `message` counts as
    /// "image 1", "image 2", … A transport that cannot deliver every one of
    /// them must answer [`VisionResponse::Skipped`] rather than send a call
    /// whose prompt names pictures the model does not have.
    pub images: Vec<VisionImage>,
    /// 覆盖模型名称;`None` 表示由 host 决定。
    pub model: Option<String>,
    /// 覆盖 provider;`None` 表示由 host 决定。
    pub provider: Option<String>,
    /// 本轮调用的超时时间。
    pub timeout: Duration,
}

impl VisionCallRequest {
    /// 1-based position of the first image with `role` — the number a prompt
    /// must use when it names that picture.
    ///
    /// Positions are counted over [`Self::images`] and are therefore only
    /// truthful while the client delivers every image (its contract: all or
    /// [`VisionResponse::Skipped`]).
    pub fn position_of(&self, role: VisionRole) -> Option<usize> {
        role.position_in(&self.images)
    }
}

/// `VisionLlmClient::validate` 的返回值。
///
/// `Text` = 模型返回了 JSON 文本;`Skipped` = stub / 截图不可用 / host 选择跳过。
/// `Skipped` 的 `reason` 是**诊断**文本("图像没送到"与"模型没答"需要不同的修法),
/// 不是可展示给用户的文案。
#[derive(Debug, Clone)]
pub enum VisionResponse {
    /// 模型返回了完整文本(JSON 格式,待 `parse_validation_response` 解析)。
    Text(String),
    /// 本轮跳过;可选地附带跳过原因(用于日志)。
    Skipped { reason: Option<String> },
}

/// Bundle of the three vision-validation trait objects.
///
/// Passed once to `Orchestrator::run` and threaded to the post-cleanup hook
/// in each of the 3 execution paths (sequential / dashboard / concurrent).
/// All fields are trait-object references — the concrete types live host-side.
///
/// Approach (c) from the D1 spec: a bundle struct, passed as a single param,
/// avoids adding 3 separate fields to the `Orchestrator` struct and keeps the
/// `run` signature manageable.
pub struct ValidationProviders<'a> {
    /// Pre-validation (pure code checks, no LLM).
    pub pre_validator: &'a dyn PreValidator,
    /// Screenshot capture (provides the vision loop's input image).
    pub screenshot: &'a dyn ScreenshotProvider,
    /// Vision LLM caller (multimodal round evaluation).
    pub vision: &'a dyn VisionLlmClient,
    /// System prompt for the vision LLM — resolved by the host from
    /// `op-ai-skills::resolveSkills('validation', '')` (or passed as a
    /// constant from the Rust skill registry).
    pub system_prompt: String,
}

// ── S4: VisualRefProvider ─────────────────────────────────────────────────────

/// Visual-reference rendering outlet.
///
/// host 实现把 HTML 字符串渲染成 base64 PNG 截图;stub 返回 `None`
/// 表示"跳过视觉参考阶段"。
///
/// Port of the `renderHtmlToScreenshot` call-site shape in
/// `visual-ref-orchestrator.ts:108-122` + spec §4.5.
pub trait VisualRefProvider: Send + Sync {
    /// 将 HTML 字符串渲染为给定像素尺寸的截图,返回 base64 PNG;
    /// `None` 表示不可用 / 跳过。
    fn render_html_to_screenshot(&self, html: &str, width: f64, height: f64) -> Option<String>;
}

// ── S4 end ────────────────────────────────────────────────────────────────────

// ── S3c end ───────────────────────────────────────────────────────────────────

/// 廉价可克隆的中止句柄(`Arc<AtomicBool>` 语义)。
#[derive(Debug, Clone, Default)]
pub struct AbortFlag(Arc<AtomicBool>);

impl AbortFlag {
    pub fn new() -> Self {
        Self::default()
    }
    /// 置位 —— 之后所有 `is_set` 返回 `true`。
    pub fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// 是否已被中止。
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Clone the underlying cancellation signal for adapters whose public
    /// API accepts `Arc<AtomicBool>` (for example `ChatProvider`). The clone
    /// observes and updates the same run-scoped flag as [`Self::set`].
    pub fn shared_atomic(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

/// Run-scoped budget for the `geometry_echo` in-loop self-correction step
/// (`concurrent::run_subtask_retry_ladder`) — caps the TOTAL number of
/// subtasks across one `Orchestrator::run()` call that may spend an extra
/// LLM call self-correcting a geometry violation, so a slow model or a
/// systematically-violating design can't compound retry cost across the
/// whole run. Cheap, cloneable `Arc<AtomicUsize>` semantics — same shape as
/// [`AbortFlag`], threaded the same way (by reference) down to every place
/// that runs a subtask.
#[derive(Debug, Clone)]
pub struct GeometryEchoBudget(Arc<AtomicUsize>);

impl GeometryEchoBudget {
    pub fn new(cap: usize) -> Self {
        Self(Arc::new(AtomicUsize::new(cap)))
    }

    /// Consume one unit of budget iff any remains. Returns `true` when the
    /// caller may proceed with an echo retry — `false` means the run-wide
    /// cap is already exhausted (or was `0` to begin with, e.g. the
    /// `OPENPENCIL_GEOMETRY_ECHO=0` rollback valve — see
    /// [`geometry_echo_cap`]).
    pub(crate) fn try_consume(&self) -> bool {
        self.0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }
}

/// `min(subtask_count, 6)` unless `env_value == Some("0")` (the
/// `OPENPENCIL_GEOMETRY_ECHO=0` rollback safety valve — geometry echo
/// defaults ON), in which case `0`. A zero-budget [`GeometryEchoBudget`]
/// degrades to a no-op exactly like "no budget left", so disabling the env
/// var needs no separate branch anywhere the budget is consumed.
///
/// Takes the env value as a plain `Option<&str>` (rather than reading
/// `std::env::var` itself) so this — the actual cap LOGIC — stays a pure,
/// directly-testable function; only the one-line call site in `run.rs`
/// touches the real process environment (untested glue, matching this
/// crate's `is_self_check_rejection` / thin-adapter convention elsewhere).
pub(crate) fn geometry_echo_cap(subtask_count: usize, env_value: Option<&str>) -> usize {
    if env_value == Some("0") {
        0
    } else {
        subtask_count.min(6)
    }
}

/// 规划 prompt 的构造模式 —— TS rich/minimal/compact 三档。
/// Plan B 的格式化器与 Plan C 的 `build_orchestrator_prompt` 共用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningMode {
    Rich,
    Minimal,
    Compact,
}

/// `build_orchestrator_prompt` 的产物 —— 比裸 `CallRequest` 多带
/// compact 模式的 `forced_style_guide_name`(S3b-1b 回填 plan 用)。
#[derive(Debug, Clone)]
pub struct PlanningPrompt {
    pub call_request: CallRequest,
    /// compact 模式预选的 styleGuideName;rich/minimal 为 None。
    pub forced_style_guide_name: Option<String>,
    pub mode: PlanningMode,
}

/// 用户消息的意图分类 —— 决定走编排器还是普通聊天。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Design,
    Chat,
}

/// 单个 subtask 的执行结果。`error` 带值但 `node_count > 0` 表示
/// "部分产出"(软错误);`node_count == 0` 表示零节点失败。
#[derive(Debug, Clone)]
pub struct SubtaskOutcome {
    pub id: String,
    /// Forest-root count (insert bookkeeping / continue-stop).
    pub node_count: usize,
    /// Resolved/paintable descendant count for honest Done reporting.
    pub paintable_nodes: usize,
    pub error: Option<String>,
    /// Post-remap ids of the roots this subtask inserted (append-mode
    /// cleanup scopes to exactly these — Component 11). Empty on failure
    /// or when the sink is buffered (ids unavailable until replay).
    pub inserted_root_ids: Vec<String>,
    /// The persisted subtask spec, present ONLY on a zero-node failure —
    /// carries `region`/`elements`/`screen`/`parent_frame_id` through to the
    /// host so a failed row's manual "Retry" button (progress-panel remedy,
    /// see `crate::retry_subtask`) can re-run this EXACT subtask later,
    /// instead of a re-derived approximation. `None` on success (nothing to
    /// retry) — avoids cloning the spec on the common path.
    pub subtask: Option<Subtask>,
}

/// `run()` 成功返回的汇总。
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub root_frame_id: String,
    pub subtasks: Vec<SubtaskOutcome>,
    /// Sum of forest-root counts (insert bookkeeping).
    pub total_nodes: usize,
    /// Sum of resolved/paintable descendants — prefer this for Done copy.
    pub paintable_nodes: usize,
    /// Names of top-level "screen" roots (`unfilled_screens::detect_unfilled_screens`)
    /// that never received real content by the time the run finished — the
    /// "promise-delivery" invariant's classic-path honest report. Empty on
    /// the common path. Each name is also already marked on the canvas
    /// itself (`unfilled_screens::mark_unfilled_screens`'s " (unfilled)"
    /// suffix) before this summary is built, so a caller that only reads
    /// this field and one that only looks at the canvas see the same story.
    pub unfilled_screens: Vec<String>,
}

/// `run()` 的失败。
#[derive(Debug, Clone)]
pub enum OrchestratorError {
    /// 用户中途取消。
    Aborted,
    /// 跑完但未产出任何真实内容。
    NoContent,
    /// 所有已运行 subtask 全部失败(零节点)。
    /// 内含第一个非空错误字符串,方便调用方记录或展示。
    AllFailed(String),
    /// 内部错误(意外情况)。
    Internal(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::Aborted => write!(f, "orchestration aborted by user"),
            OrchestratorError::NoContent => write!(f, "orchestration produced no content"),
            OrchestratorError::AllFailed(m) => write!(f, "orchestration failed: {m}"),
            OrchestratorError::Internal(m) => write!(f, "orchestration internal error: {m}"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

/// 追加上下文 —— 当用户要求扩展已有页面时由 host 填入。
///
/// Port of `AppendContext` in `apps/web/src/services/ai/ai-types.ts:28-37`.
/// 当存在时,编排器跳过创建新根 frame,将生成的区块插入现有目标 frame。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendContext {
    /// 新 sub-agent 区块应插入的 frame id。
    pub target_parent_id: String,
    /// 目标 frame 的宽度(用于给 sub-agent 确定区域尺寸)。
    pub target_width: f64,
    /// 现有顶层区块的标签列表 —— sub-agent 被告知不要重复这些。
    pub existing_section_labels: Vec<String>,
    /// 目标 frame 属于移动页面(宽度 ≤ 480)时为 true。
    pub is_mobile: bool,
}

/// Existing-canvas facts for a request that creates sibling screens.
///
/// Unlike [`AppendContext`], this does not target an existing parent. It
/// carries the artboard contract and the exact screens the user promised so
/// planning failure can still fan out into correctly-sized top-level roots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationContext {
    pub screen_width: f64,
    pub screen_height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    pub screen_names: Vec<String>,
}

/// User-attached reference image for design grounding (screenshot to match).
/// Not part of the JSON wire for `DesignRequest` — carried in-process only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceAttachment {
    /// Original file name, e.g. `screenshot.png`.
    pub name: String,
    /// MIME type, e.g. `image/png`.
    pub media_type: String,
    /// Raw file bytes (not base64).
    pub data: Vec<u8>,
}

impl ReferenceAttachment {
    pub fn is_image(&self) -> bool {
        self.media_type.starts_with("image/")
    }
}

/// A recipe the product already placed in the document.
///
/// The product selects it (see `op_editor_core::select_recipe`) and inserts
/// its master before the model runs, so "start from the recipe" is a fact the
/// model can see rather than advice it may skip. The prompt then frames the
/// turn as adaptation of `node_id`, not composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeBase {
    pub recipe_id: String,
    pub master_id: String,
    /// The placed root's node id in the live document.
    pub node_id: String,
    pub name: String,
    /// What the recipe already provides, from the kit's own notes.
    pub notes: String,
}

/// 编排器输入 —— 一次设计请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    /// Effective design rules for this session — the planning prompt's
    /// only design-system input. The rules are already resolved (library
    /// kit + document rules + overrides, disabled ones dropped) by the
    /// caller, which owns that state.
    #[serde(default)]
    pub rules: Vec<jian_ops_schema::DesignRule>,
    /// 并发度:允许同时运行的 screen-group worker 数。
    /// 调用方应传 store-clamped 值 [1,6];crate 内部防御性 clamp。
    /// 默认为 1(顺序执行)。Port of TS `request.concurrency ?? 1`.
    pub concurrency: u32,
    /// 追加模式上下文 —— 仅当 host 检测到 append intent 时填入。
    /// Port of `AIDesignRequest.context.appendContext` in `ai-types.ts:51`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_context: Option<AppendContext>,
    /// Sibling-screen continuation facts derived from the live canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_context: Option<ContinuationContext>,
    /// 是否启用后生成视觉校验循环(S3c)。
    /// 对应 TS `VALIDATION_ENABLED` flag(默认 `true`)。
    /// host 可将其设为 `false` 以跳过整个视觉校验阶段。
    /// Port of `VALIDATION_ENABLED` in `ai-runtime-config.ts:109`.
    #[serde(default = "default_validation_enabled")]
    pub validation_enabled: bool,
    /// Vestigial flag — reserved, never read. The visual-ref pipeline (S4)
    /// was removed as dead code (2026-06-06): it ported a TS pipeline upstream
    /// had already deleted (commit 0f12b6e9) and nothing dispatched on this
    /// flag. Kept (not removed) to avoid churning the ~40 `DesignRequest`
    /// construction sites; defaults `false`.
    #[serde(default = "default_visual_ref_enabled")]
    pub visual_ref_enabled: bool,
    /// Style guide the user pinned in the Asset Center, by `name`.
    ///
    /// When it names a guide the registry still carries, planning uses that
    /// one instead of ranking the catalog against the prompt — the pin is the
    /// user overriding the inference, so a prompt that reads "fintech" must
    /// not pull the fintech guide out from under a pinned brutalist one. A
    /// name the registry has dropped falls back to the ranking with a log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_style_guide: Option<String>,
    /// Reference screenshots the user attached for this design turn.
    /// In-process only (`serde(skip)`); web/desktop hosts copy chat
    /// attachments here before `Orchestrator::run`. Because it is skipped,
    /// a request that crossed a JSON hop carries an empty list — see
    /// [`Self::reference_evidence`], which refuses to read that as "no picture".
    #[serde(skip)]
    pub reference_attachments: Vec<ReferenceAttachment>,
    /// Structured layout inventory from a multimodal pass over
    /// [`Self::reference_attachments`]. Filled by
    /// [`crate::reference_brief::enrich_request_with_reference_brief`]
    /// before planning; in-process only.
    #[serde(skip)]
    pub reference_brief: Option<String>,
}

impl DesignRequest {
    /// What this request itself knows about the turn's reference image, for the
    /// recipe decisions that used to read the prompt's words instead
    /// (issue #65).
    ///
    /// Only the positive answer is knowledge. `reference_attachments` is
    /// `serde(skip)`, so an empty list means either "this turn had no picture"
    /// or "this request was decoded from JSON and the pictures were dropped" —
    /// and a reader of the request cannot tell those apart. Reporting
    /// [`ReferenceEvidence::NoImage`] for the empty case would assert something
    /// this type does not carry, and would then *place* a recipe on a reference
    /// turn whose attachment was lost in transit — the silent-loss family of
    /// issue #61/#64. So the empty case stays
    /// [`ReferenceEvidence::Unknown`]: the fact when there is one, the words
    /// otherwise, which is exactly the pre-existing behaviour and now says why.
    pub fn reference_evidence(&self) -> op_editor_core::ReferenceEvidence {
        if self
            .reference_attachments
            .iter()
            .any(ReferenceAttachment::is_image)
        {
            op_editor_core::ReferenceEvidence::Attached
        } else {
            op_editor_core::ReferenceEvidence::Unknown
        }
    }
}

/// Mirrors the serde defaults exactly, so a request built through `Default`
/// and one decoded from `{"prompt":"…"}` are the same request. Its purpose is
/// to let call sites name only the fields they care about — a request struct
/// with a dozen fields is otherwise re-listed in full at every site, and each
/// new field becomes a mechanical edit across all of them.
impl Default for DesignRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            model: None,
            provider: None,
            rules: Vec::new(),
            concurrency: 1,
            append_context: None,
            continuation_context: None,
            validation_enabled: default_validation_enabled(),
            visual_ref_enabled: default_visual_ref_enabled(),
            pinned_style_guide: None,
            reference_attachments: Vec::new(),
            reference_brief: None,
        }
    }
}

fn default_validation_enabled() -> bool {
    true
}

fn default_visual_ref_enabled() -> bool {
    false
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
