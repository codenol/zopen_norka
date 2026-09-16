//! `plan_repair` — JSON-repair value coercers + plan repair/finalize.
//!
//! Task B1: helper functions ported from
//! `apps/web/src/services/ai/orchestrator-planning.ts:272-352`.
//! Task B2: `repair_plan_object`, `finalize_plan`, `extract_subtask_candidates`,
//! `coerce_subtask`, `build_fallback_heights` — port of
//! `apps/web/src/services/ai/orchestrator-planning.ts:76-254`.
//! Task B3 (parse_orchestrator_response) will be appended in the next task.

use crate::dashboard_columns::is_strong_sidebar_subtask;
use crate::design_type::{detect_design_type, DesignType};
use crate::plan::build_fallback_plan;
use crate::plan::{OrchestratorPlan, PlanFill, Region, RootFrameSpec, Subtask};
use crate::request_dimensions::requested_root_dimensions;
use crate::types::DesignRequest;
use op_editor_core::session_kit;
use serde_json::Value;

/// The catalogue style-guide name the retired `design.md` brief used to force
/// onto the plan. Port of `DESIGN_MD_STYLE_GUIDE_NAME` from
/// `orchestrator-prompt-optimizer.ts`.
///
/// Test-only, and `cfg(test)` rather than `dead_code`: a session kit is the
/// design system, so `finalize_plan` sets `style_guide_name = None` and no
/// production path can name this value. What still needs the name is the pair
/// of tests that assert it is ABSENT from a plan and from the planning context
/// — they had it as a bare `"design-md-custom"` literal, which is how a
/// retired contract ends up living in two test files instead of one constant.
#[cfg(test)]
pub(crate) const DESIGN_MD_STYLE_GUIDE_NAME: &str = "design-md-custom";

// ── public(crate) helpers ─────────────────────────────────────────────────────

/// Non-empty trimmed string, or `None`.
///
/// Port of TS `asString`.
pub(crate) fn as_string(value: &Value) -> Option<String> {
    let s = value.as_str()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Finite number strictly > 0, or `None`.
///
/// Port of TS `asPositiveNumber`.
pub(crate) fn as_positive_number(value: &Value) -> Option<f64> {
    let n = value.as_f64()?;
    if n.is_finite() && n > 0.0 {
        Some(n)
    } else {
        None
    }
}

/// Finite number ≥ 0, or `None`.
///
/// Port of TS `asNonNegativeNumber`.
pub(crate) fn as_non_negative_number(value: &Value) -> Option<f64> {
    let n = value.as_f64()?;
    if n.is_finite() && n >= 0.0 {
        Some(n)
    } else {
        None
    }
}

/// Validates that the value is one of `"none"`, `"vertical"`, or
/// `"horizontal"`, matching the layout field type on `RootFrameSpec`.
///
/// Port of TS `asLayout`.
pub(crate) fn as_layout(value: &Value) -> Option<String> {
    match value.as_str()? {
        "none" | "vertical" | "horizontal" => Some(value.as_str().unwrap().to_owned()),
        _ => None,
    }
}

/// Coerces a fill value:
/// - an array of `{type, color}` objects → `Vec<PlanFill>` (entries without
///   a valid `color` are dropped; `type` defaults to `"solid"`).
/// - a bare color string → single-entry `Vec<PlanFill>` with `type="solid"`.
/// - anything else → `None`.
///
/// Port of TS `coerceFill`.
pub(crate) fn coerce_fill(value: &Value) -> Option<Vec<PlanFill>> {
    if let Some(arr) = value.as_array() {
        let solids: Vec<PlanFill> = arr
            .iter()
            .filter(|entry| is_record(entry))
            .filter_map(|entry| {
                let color = as_string(&entry["color"])?;
                let kind = as_string(&entry["type"]).unwrap_or_else(|| "solid".to_owned());
                Some(PlanFill { kind, color })
            })
            .collect();
        if solids.is_empty() {
            None
        } else {
            Some(solids)
        }
    } else {
        let color = as_string(value)?;
        Some(vec![PlanFill {
            kind: "solid".to_owned(),
            color,
        }])
    }
}

/// Returns `true` when `value` is a JSON object (not null, not an array).
///
/// Port of TS `isRecord`.
pub(crate) fn is_record(value: &Value) -> bool {
    value.is_object()
}

/// Converts `label` to a safe section id:
/// - lowercase
/// - runs of `[^a-z0-9]` → `-`
/// - leading/trailing `-` stripped
/// - empty result → `"section-{index + 1}"` (1-based, matching TS)
///
/// Port of TS `makeSafeSectionId`.
pub(crate) fn make_safe_section_id(label: &str, index: usize) -> String {
    // Build lowercase, replacing any non-alphanumeric run with '-'
    let mut result = String::new();
    let lower = label.to_lowercase();
    let mut in_sep = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            in_sep = false;
            result.push(ch);
        } else if !in_sep {
            in_sep = true;
            result.push('-');
        }
    }
    // Trim leading/trailing '-'
    let trimmed = result.trim_matches('-');
    if trimmed.is_empty() {
        format!("section-{}", index + 1)
    } else {
        trimmed.to_owned()
    }
}

/// Distributes `total_height` across `count` sections using a weighted
/// allocation and a remainder fix-up loop identical to the TS implementation.
///
/// Weights: first section 1.4×, last section (when count ≥ 3) 0.6×, others 1.0×.
/// Minimum section height: 80 px.
///
/// Port of TS `allocateSectionHeights`.
pub(crate) fn allocate_section_heights(total_height: i64, count: usize) -> Vec<i64> {
    if count == 0 {
        return vec![];
    }
    if count == 1 {
        return vec![total_height];
    }

    let min_height: i64 = 80;

    // Build weight array
    let weights: Vec<f64> = (0..count)
        .map(|i| {
            if i == 0 {
                1.4_f64
            } else if i == count - 1 && count >= 3 {
                0.6_f64
            } else {
                1.0_f64
            }
        })
        .collect();

    let total_weight: f64 = weights.iter().sum();

    let mut heights: Vec<i64> = weights
        .iter()
        .map(|&w| {
            let raw = ((total_height as f64) * w / total_weight).round() as i64;
            raw.max(min_height)
        })
        .collect();

    // Add-up fix-up: distribute surplus by round-robin from the middle
    let mut allocated: i64 = heights.iter().sum();
    let mut idx = count / 2; // floor(count / 2) — matches TS Math.floor
    while allocated < total_height {
        heights[idx] += 1;
        allocated += 1;
        idx = (idx + 1) % count;
    }

    // Subtract fix-up: trim from the end, respecting min_height.
    // If every section is already at min_height but the sum still exceeds
    // total_height (e.g. 5×80 > 360), stop — otherwise this loops forever.
    let mut idx = count - 1;
    let mut scanned = 0usize;
    while allocated > total_height {
        if heights[idx] > min_height {
            heights[idx] -= 1;
            allocated -= 1;
            scanned = 0;
        } else {
            scanned += 1;
            if scanned >= count {
                break;
            }
        }
        if idx == 0 {
            idx = count - 1;
        } else {
            idx -= 1;
        }
    }

    heights
}

// ── Task B2: repair_plan_object + finalize_plan ───────────────────────────────

/// Tries to repair a valid-JSON-but-schema-invalid value into an
/// `OrchestratorPlan`.  Returns `None` when the object has no recognisable
/// subtask candidates at all (empty after coercion).
///
/// Port of `repairPlanObject` from
/// `apps/web/src/services/ai/orchestrator-planning.ts:89-138`.
pub(crate) fn repair_plan_object(obj: &Value, request: &DesignRequest) -> Option<OrchestratorPlan> {
    let fallback = build_fallback_plan(request);
    let raw_subtasks = extract_subtask_candidates(obj);
    if raw_subtasks.is_empty() {
        return None;
    }

    // rootFrame source: prefer `rootFrame` object, fall back to the top-level
    // object itself (faithfully mirrors TS `isRecord(obj.rootFrame) ? …`).
    let root_source = if is_record(&obj["rootFrame"]) {
        &obj["rootFrame"]
    } else {
        obj
    };

    let fallback_heights = build_fallback_heights(&fallback, raw_subtasks.len());

    let root_frame = RootFrameSpec {
        id: as_string(&root_source["id"]).unwrap_or_else(|| fallback.root_frame.id.clone()),
        name: as_string(&root_source["name"]).unwrap_or_else(|| fallback.root_frame.name.clone()),
        width: as_positive_number(&root_source["width"]).unwrap_or(fallback.root_frame.width),
        height: as_non_negative_number(&root_source["height"])
            .unwrap_or(fallback.root_frame.height),
        layout: Some(
            as_layout(&root_source["layout"])
                .or_else(|| fallback.root_frame.layout.clone())
                .unwrap_or_else(|| "vertical".to_owned()),
        ),
        gap: Some(
            as_non_negative_number(&root_source["gap"])
                .or(fallback.root_frame.gap)
                .unwrap_or(0.0),
        ),
        padding: fallback.root_frame.padding,
        fill: coerce_fill(&root_source["fill"]).or_else(|| fallback.root_frame.fill.clone()),
    };

    let subtasks: Vec<Subtask> = raw_subtasks
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            coerce_subtask(
                candidate,
                index,
                root_frame.width,
                fallback_heights.get(index).copied().unwrap_or(160),
            )
        })
        .collect();

    if subtasks.is_empty() {
        return None;
    }

    // styleGuideName aliasing: prefer camelCase `styleGuideName`, then
    // snake_case `style_guide`, then the fallback. Session rules are not a
    // catalog style guide, so they force no name of their own.
    let style_guide_name = as_string(&obj["styleGuideName"])
        .or_else(|| as_string(&obj["style_guide"]))
        .or_else(|| fallback.style_guide_name.clone());

    let mut repaired = OrchestratorPlan {
        root_frame,
        subtasks,
        style_guide_name,
    };

    Some(finalize_plan(&mut repaired, Some(obj), request))
}

/// Post-processes a plan after strict-parse or repair.
///
/// - A session kit owns style: drop any catalog `styleGuideName` and apply
///   the kit canvas fill when present.
/// - desktop kit chassis: drop subtasks that duplicate sentinel chrome
///   (sidebar / topbar / breadcrumbs) so generation only fills the slot.
///
/// The old `design.md` branch that forced `style_guide_name =
/// "design-md-custom"` and painted the page with the brief's palette is
/// gone: rules carry no colours, and the kit is the design system.
pub(crate) fn finalize_plan(
    plan: &mut OrchestratorPlan,
    _raw_obj: Option<&Value>,
    request: &DesignRequest,
) -> OrchestratorPlan {
    if let Some(canvas) = session_kit().canvas.as_ref() {
        plan.style_guide_name = None;
        plan.root_frame.fill = Some(vec![PlanFill {
            kind: "solid".to_owned(),
            color: canvas.fill.clone(),
        }]);
    }
    if detect_design_type(&request.prompt).type_ == DesignType::DesktopScreen {
        strip_kit_owned_chrome_subtasks(plan);
        strip_non_kit_invented_modules(plan, request);
        normalize_body_subtask_ids(plan, request);
        apply_kit_canvas_size(plan, &request.prompt);
    }
    plan.clone()
}

fn is_kit_owned_chrome(st: &Subtask) -> bool {
    if is_strong_sidebar_subtask(st) {
        return true;
    }
    let t = format!("{} {}", st.id, st.label).to_lowercase();
    let compact = t.replace([' ', '-', '_'], "");
    compact.contains("topbar")
        || compact.contains("breadcrumb")
        || compact.contains("navbar")
        || compact.contains("appbar")
        || compact.contains("header")
        || compact == "shell"
        || compact.contains("appshell")
}

fn kit_has_type_matching(needle: &str) -> bool {
    let needle = needle.to_lowercase();
    op_editor_core::session_kit().types.iter().any(|ty| {
        ty.id.to_lowercase().contains(&needle) || ty.name.to_lowercase().contains(&needle)
    })
}

/// Drop planner-invented analytics modules that the session kit does not own
/// (Skala has table/pagination/input — not KPI cards or charts).
fn is_non_kit_invented_module(st: &Subtask) -> bool {
    let t = format!("{} {}", st.id, st.label).to_lowercase();
    let compact = t.replace([' ', '-', '_'], "");
    // Keep table / toolbar / pagination body even if the planner named them
    // "signals-table" etc.
    if compact.contains("table")
        || compact.contains("toolbar")
        || compact.contains("pagination")
        || compact.contains("paginat")
        || t.contains("таблица")
        || t.contains("поиск")
    {
        return false;
    }
    const PATTERNS: &[&str] = &[
        "kpi",
        "metric",
        "chart",
        "signal",
        "signals",
        "credits",
        "exposure",
        "drawdown",
        "winrate",
        "win-rate",
        "analytics",
    ];
    for p in PATTERNS {
        let compact_p = p.replace('-', "");
        if (t.contains(p) || compact.contains(compact_p.as_str())) && !kit_has_type_matching(p) {
            return true;
        }
    }
    false
}

fn strip_non_kit_invented_modules(plan: &mut OrchestratorPlan, _request: &DesignRequest) {
    // The kit is always the style source now; the old design.md escape
    // hatch is gone with the brief.
    plan.subtasks.retain(|st| !is_non_kit_invented_module(st));
    ensure_content_subtask(plan);
}

fn brief_or_prompt_mentions_table(request: &DesignRequest) -> bool {
    let mut hay = request.prompt.to_lowercase();
    if let Some(brief) = request.reference_brief.as_ref() {
        hay.push(' ');
        hay.push_str(&brief.to_lowercase());
    }
    hay.contains("table")
        || hay.contains("таблица")
        || hay.contains("toolbar")
        || hay.contains("pagination")
        || hay.contains("пагинац")
        || hay.contains("search")
}

fn normalize_body_subtask_ids(plan: &mut OrchestratorPlan, request: &DesignRequest) {
    if !brief_or_prompt_mentions_table(request) {
        return;
    }
    for st in &mut plan.subtasks {
        let t = format!("{} {}", st.id, st.label).to_lowercase();
        if t.contains("table") || t.contains("таблица") || t.contains("grid") || t.contains("row")
        {
            if st.id != "table" {
                st.id = "table".into();
            }
            if st.label.to_lowercase().contains("kpi") {
                st.label = "Table".into();
            }
        } else if t.contains("toolbar")
            || t.contains("search")
            || t.contains("filter")
            || t.contains("поиск")
        {
            if st.id != "toolbar" {
                st.id = "toolbar".into();
            }
        } else if (t.contains("paginat") || t.contains("пагинац")) && st.id != "pagination" {
            st.id = "pagination".into();
        }
    }
}

fn ensure_content_subtask(plan: &mut OrchestratorPlan) {
    if plan.subtasks.is_empty() {
        let w = plan.root_frame.width;
        let h = if plan.root_frame.height > 0.0 {
            plan.root_frame.height
        } else {
            400.0
        };
        plan.subtasks.push(Subtask {
            id: "main".into(),
            label: "Main".into(),
            region: Region {
                width: w,
                height: h,
            },
            id_prefix: "main".into(),
            parent_frame_id: None,
            elements: Some(
                "product body in the Layout/Default content area (Main container)".into(),
            ),
            screen: None,
            generated_root_id: None,
            existing_section_labels: None,
            retry_feedback: None,
        });
    }
}

fn strip_kit_owned_chrome_subtasks(plan: &mut OrchestratorPlan) {
    plan.subtasks.retain(|st| !is_kit_owned_chrome(st));
    ensure_content_subtask(plan);
}

fn apply_kit_canvas_size(plan: &mut OrchestratorPlan, prompt: &str) {
    if requested_root_dimensions(prompt).is_some() {
        return;
    }
    let Some(canvas) = session_kit().canvas.as_ref() else {
        return;
    };
    plan.root_frame.width = canvas.width;
    plan.root_frame.height = canvas.height;
}

/// Returns the first non-empty candidate array from `subtasks` / `sections` /
/// `tasks` in the object — or an empty vec when none are found.
///
/// Port of `extractSubtaskCandidates` from
/// `apps/web/src/services/ai/orchestrator-planning.ts:186-191`.
fn extract_subtask_candidates(obj: &Value) -> Vec<Value> {
    for key in &["subtasks", "sections", "tasks"] {
        if let Some(arr) = obj[key].as_array() {
            if !arr.is_empty() {
                return arr.clone();
            }
        }
    }
    vec![]
}

/// Coerces a single subtask candidate (string or object) into a `Subtask`, or
/// returns `None` when it cannot be recovered.
///
/// Port of `coerceSubtask` + `asElements` from
/// `apps/web/src/services/ai/orchestrator-planning.ts:215-270`.
fn coerce_subtask(
    candidate: &Value,
    index: usize,
    root_width: f64,
    default_height: i64,
) -> Option<Subtask> {
    if let Some(s) = candidate.as_str() {
        let label = s.trim().to_owned();
        if label.is_empty() {
            return None;
        }
        return Some(Subtask {
            id: make_safe_section_id(&label, index),
            label,
            region: Region {
                width: root_width,
                height: default_height as f64,
            },
            id_prefix: String::new(),
            parent_frame_id: None,
            elements: None,
            screen: None,
            generated_root_id: None,
            existing_section_labels: None,
            retry_feedback: None,
        });
    }

    if !is_record(candidate) {
        return None;
    }

    // label aliasing: label ?? name ?? title ?? section ?? "Section N"
    let label = as_string(&candidate["label"])
        .or_else(|| as_string(&candidate["name"]))
        .or_else(|| as_string(&candidate["title"]))
        .or_else(|| as_string(&candidate["section"]))
        .unwrap_or_else(|| format!("Section {}", index + 1));

    // region source: prefer `region` object, else the candidate itself
    let region_source = if is_record(&candidate["region"]) {
        &candidate["region"]
    } else {
        candidate
    };
    let width = as_positive_number(&region_source["width"]).unwrap_or(root_width);
    let height = as_positive_number(&region_source["height"])
        .or_else(|| as_positive_number(&candidate["height"]))
        .map(|h| h as i64)
        .unwrap_or(default_height);

    // elements aliasing: elements ?? scope ?? description
    let elements_raw = if candidate["elements"] != Value::Null {
        &candidate["elements"]
    } else if candidate["scope"] != Value::Null {
        &candidate["scope"]
    } else {
        &candidate["description"]
    };
    let elements = coerce_elements(elements_raw);

    // screen aliasing: screen ?? page
    let screen = as_string(&candidate["screen"]).or_else(|| as_string(&candidate["page"]));

    Some(Subtask {
        id: as_string(&candidate["id"]).unwrap_or_else(|| make_safe_section_id(&label, index)),
        label,
        region: Region {
            width,
            height: height as f64,
        },
        id_prefix: String::new(),
        parent_frame_id: None,
        elements,
        screen,
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    })
}

/// Coerces a value into a comma-joined elements string, matching the TS
/// `asElements` helper.
fn coerce_elements(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let trimmed = s.trim().to_owned();
        return if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if let Some(arr) = value.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|item| item.as_str())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        return if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        };
    }
    None
}

/// Builds the fallback height array for `count` subtasks derived from the
/// fallback plan shape.
///
/// Port of `buildFallbackHeights` from
/// `apps/web/src/services/ai/orchestrator-planning.ts:193-213`.
pub(crate) fn build_fallback_heights(fallback: &OrchestratorPlan, count: usize) -> Vec<i64> {
    if count == 0 {
        return vec![];
    }

    let fw = fallback.root_frame.width;
    let fh = fallback.root_frame.height;

    // Component shape: narrow (≤480) + zero/undefined height
    let is_component_shape = fw <= 480.0 && (fh == 0.0);
    if is_component_shape {
        return vec![200_i64; count];
    }

    // Mobile: narrow (≤500), split 812 evenly
    if fw <= 500.0 {
        let per_section = ((if fh == 0.0 { 812.0 } else { fh }) / count as f64).floor() as i64;
        return vec![per_section; count];
    }

    // Desktop: weighted allocation
    let total_height = if fh == 0.0 {
        if count >= 4 {
            4000
        } else {
            800
        }
    } else {
        fh as i64
    };
    allocate_section_heights(total_height, count)
}

// ── Task B3: parse_orchestrator_response cascade ──────────────────────────────

/// Parse the raw LLM text output into an `OrchestratorPlan`, applying three
/// extraction strategies in order. Returns `None` when all six probes fail.
///
/// The `bool` in the return tuple is `repaired`: `true` when the successful
/// probe was the repair path (rather than the strict serde deserialization).
///
/// Strategies (each tried strict-then-repair):
/// 1. **Direct** — `raw.trim()` as-is.
/// 2. **Fenced** — strip a markdown code fence (` ```json ` or bare ` ``` `).
/// 3. **Brace-slice** — `raw[first '{' ..= last '}']`.
///
/// Port of `parseOrchestratorResponse` from
/// `apps/web/src/services/ai/orchestrator-planning.ts:20-55`.
pub(crate) fn parse_orchestrator_response(
    raw: &str,
    request: &DesignRequest,
) -> Option<(OrchestratorPlan, bool)> {
    let trimmed = raw.trim();

    // Strategy 1: direct
    if let Some(plan) = try_parse_plan_strict(trimmed, request) {
        return Some((plan, false));
    }
    if let Some(plan) = try_repair_plan_text(trimmed, request) {
        return Some((plan, true));
    }

    // Strategy 2: fenced code block
    if let Some(fenced_text) = extract_fence_content(trimmed) {
        let fenced_text = fenced_text.trim();
        if let Some(plan) = try_parse_plan_strict(fenced_text, request) {
            return Some((plan, false));
        }
        if let Some(plan) = try_repair_plan_text(fenced_text, request) {
            return Some((plan, true));
        }
    }

    // Strategy 3: brace-slice
    if let Some(braced_text) = extract_brace_slice(trimmed) {
        if let Some(plan) = try_parse_plan_strict(braced_text, request) {
            return Some((plan, false));
        }
        if let Some(plan) = try_repair_plan_text(braced_text, request) {
            return Some((plan, true));
        }
    }

    None
}

/// Strict probe: delegate to `parse_plan` (serde deserialization + non-empty
/// subtasks check).  Returns `None` on any parse or validation failure.
fn try_parse_plan_strict(text: &str, request: &DesignRequest) -> Option<OrchestratorPlan> {
    let mut plan = crate::plan::parse_plan(text).ok()?;
    Some(finalize_plan(&mut plan, None, request))
}

/// Repair probe: `serde_json::from_str::<Value>` → `repair_plan_object`.
/// Returns `None` on JSON parse failure or when `repair_plan_object` returns
/// `None`.
fn try_repair_plan_text(text: &str, request: &DesignRequest) -> Option<OrchestratorPlan> {
    let value: Value = serde_json::from_str(text).ok()?;
    repair_plan_object(&value, request)
}

/// Extract the content of a markdown code fence.
///
/// Matches ` ```(?:json)?\s*\n?([\s\S]*?)\n?``` ` — ported from the TS
/// regex without adding the `regex` crate.
///
/// Returns `None` when no fence is found.
fn extract_fence_content(text: &str) -> Option<&str> {
    // Find the opening fence marker "```"
    let fence_open = text.find("```")?;
    let after_open = &text[fence_open + 3..]; // skip the three backticks

    // Skip optional "json" tag
    let after_tag = after_open.strip_prefix("json").unwrap_or(after_open);

    // Skip optional whitespace / single newline (matches `\s*\n?`)
    let after_ws = after_tag.trim_start_matches([' ', '\t', '\r']);
    let content_start_in_tag = after_ws.strip_prefix('\n').unwrap_or(after_ws);

    // Compute absolute start offset for the content
    let content_abs_start = fence_open + 3 + (after_open.len() - content_start_in_tag.len());

    // Find the closing "```" from the start of remaining text
    let remaining = content_start_in_tag;
    let close_rel = remaining.find("```")?;

    // Strip optional trailing newline before the closing fence
    let raw_content = &remaining[..close_rel];
    let content = raw_content.strip_suffix('\n').unwrap_or(raw_content);

    // Return the slice from the original `text` buffer
    let end = content_abs_start + content.len();
    Some(&text[content_abs_start..end])
}

/// Extract `text[first_brace ..= last_brace]`.
///
/// Port of the brace-slice strategy in `parseOrchestratorResponse`.
fn extract_brace_slice(text: &str) -> Option<&str> {
    let first = text.find('{')?;
    let last = text.rfind('}')?;
    if last > first {
        Some(&text[first..=last])
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "plan_repair_tests.rs"]
mod tests;
