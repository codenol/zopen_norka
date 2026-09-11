//! Canvas agent indicators for the design-agent tool-loop (Phase 2.3) AND
//! the single-shot design orchestrator (`current_design: DesignSession`).
//!
//! Both drivers share the same `DesignLoopIndicator` epoch/color/name/
//! initial-frame-set shape and the same `register_new_frames` /
//! `collect_top_level_frame_ids` core — only the "is a turn running" signal
//! and the epoch-ownership semantics differ:
//!
//! ## Chat-loop driver — [`pump_indicator`]
//!
//! When `OPENPENCIL_DESIGN_AGENT_LOOP` is active and a design turn is
//! running, this module:
//! 1. Calls `op_editor_core::agent_indicators::begin()` to start an epoch.
//! 2. Assigns the single-agent identity via
//!    `op_orchestrator::agent_identity::assign_agent_identities`.
//! 3. Each pump, registers any *new* top-level Frame nodes (not present when
//!    the turn started) with `agent_indicators::add_frame` so the canvas
//!    painter draws a colour glow + name badge around them.
//! 4. When the chat session ends (turn done), calls
//!    `agent_indicators::finish_if_epoch` — queued reveals drain
//!    gracefully, then the overlay clears itself — and clears
//!    `state.chat.agents_running`. (A user stop elsewhere calls
//!    `end_if_epoch` for an immediate teardown.)
//!
//! Additive only — CRUD chat and orchestrator paths never set
//! `agents_running > 0`, so this driver is a no-op for them.
//!
//! ## Design-session (orchestrator) driver — [`pump_design_session_indicator`]
//!
//! `chat_session_launch.rs::launch_cli_standard_turn` and
//! `op_host_services::design_session::start` (the builtin-provider design
//! path) both already call `agent_indicators::begin()` themselves before
//! constructing the `DesignSession`, and `DesignSession::drop` already
//! retires that epoch (`finish_if_epoch` on natural completion,
//! `end_if_epoch` on abort). So this driver only *reads* the active epoch
//! via `active_epoch()` and registers new top-level frames while
//! `current_design.is_some()` — it never begins or ends an epoch itself,
//! which keeps epoch ownership single-writer even though both drivers can
//! run against the same process (never against the same turn — see the
//! function doc for why).

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use jian_ops_schema::node::PenNode;
use op_editor_core::agent_indicators;
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::EditorState;
use op_orchestrator::agent_identity::assign_agent_identities_seeded;

/// Per-run randomness for the identity pool — nanos are plenty to meet a
/// different face each run without an RNG dependency.
pub(crate) fn identity_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}

/// Runtime state of one active design-loop indicator epoch.
pub(crate) struct DesignLoopIndicator {
    /// Epoch handle returned by `agent_indicators::begin()`.
    pub epoch: u64,
    /// Hex colour assigned to the single agent, e.g. `"#FF6B6B"`.
    pub color: String,
    /// Display name assigned to the single agent, e.g. `"Norka"`.
    pub name: String,
    /// Top-level Frame ids that existed BEFORE the turn started.
    /// Frames added during the turn are the ones we tag.
    pub initial_frame_ids: HashSet<String>,
}

/// Collect the ids of every top-level `Frame` node on the active page.
pub(crate) fn collect_top_level_frame_ids(state: &EditorState) -> HashSet<String> {
    state
        .active_children()
        .iter()
        .filter_map(|node| {
            if matches!(node, PenNode::Frame(_)) {
                Some(node.id_str().to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Register any Frame nodes that appeared since the turn started.
/// Called every pump while the chat session is still alive.
///
/// Uses `add_frame_if_absent`, not `add_frame`: the classic Orchestrator's
/// screen-group scaffold (D-lite's inter-group concurrency) may already have
/// tagged a fresh root with its OWN per-group identity — distinct
/// colour/name per screen — before this pump next observes it. Blindly
/// overwriting with this driver's single identity every frame would
/// collapse N concurrent agents' badges back down to one (the "single
/// cursor" visibility bug this pump was implicated in, 2026-07-17). For the
/// ordinary single-agent turn this is behavior-identical to `add_frame`:
/// the frame is untagged the first time this driver sees it either way.
pub(crate) fn register_new_frames(indicator: &DesignLoopIndicator, state: &EditorState) {
    for node in state.active_children() {
        if let PenNode::Frame(_) = node {
            let id = node.id_str();
            if !indicator.initial_frame_ids.contains(id) {
                agent_indicators::add_frame_if_absent(
                    indicator.epoch,
                    id,
                    &indicator.color,
                    &indicator.name,
                );
            }
        }
    }
}

/// Called every frame from `app_handler`'s `RedrawRequested` branch,
/// right after `chat_session::pump`.
///
/// Lifecycle:
/// - When `state.chat.agents_running.0 > 0` and no indicator exists yet
///   → creates one (begins the epoch, snapshots initial frames).
/// - While the indicator exists and `current_chat.is_some()` → registers
///   newly-added frames.
/// - When the indicator exists but `current_chat` has gone (turn done /
///   stopped) → tears down: retires the epoch, clears `agents_running`.
pub(super) fn pump_indicator(
    indicator: &mut Option<DesignLoopIndicator>,
    current_chat: &Option<op_editor_host_core::chat::ChatSession>,
    state: &mut EditorState,
) {
    // Lazy creation when the design loop starts a turn.
    if state.chat.agents_running.0 > 0 && indicator.is_none() {
        let epoch = agent_indicators::active_epoch().unwrap_or_else(agent_indicators::begin);
        let identities = assign_agent_identities_seeded(1, identity_seed());
        let id = identities
            .into_iter()
            .next()
            .expect("assign_agent_identities(1) always yields one");
        let initial = collect_top_level_frame_ids(state);
        // Persona parity with the canvas: the streaming assistant bubble
        // wears the run's agent identity (name + colour), not the backend
        // label — Pencil's transcript reads "• Cosmo", never "Codex CLI".
        if let Some(msg) = state
            .chat
            .messages
            .iter_mut()
            .rev()
            .find(|msg| msg.role == op_editor_core::ChatRole::Assistant && msg.streaming)
        {
            msg.agent_name = Some(id.name.clone());
            msg.agent_color = Some(id.color.clone());
        }
        agent_indicators::confirm_cursor_agent(epoch, &id.color, &id.name);
        *indicator = Some(DesignLoopIndicator {
            epoch,
            color: id.color,
            name: id.name,
            initial_frame_ids: initial,
        });
    }

    if let Some(ind) = indicator.as_ref() {
        if current_chat.is_some() {
            // Turn still in flight — tag any frames that appeared.
            register_new_frames(ind, state);
        } else {
            // Turn ended (session dropped) — finish gracefully so the
            // queued reveals play out before the overlay clears itself.
            let epoch = ind.epoch;
            agent_indicators::finish_if_epoch(epoch);
            state.chat.agents_running = (0, 0);
            *indicator = None;
        }
    }
}

/// Called every frame from `app_handler`'s `RedrawRequested` branch, right
/// after `design_session::pump_commands` / `pump_progress` have drained this
/// frame's deltas — mirrors [`pump_indicator`] running after
/// `chat_session::pump` so a same-frame session-finish is observed here
/// instead of lagging a full frame behind.
///
/// Lifecycle (deliberately asymmetric with [`pump_indicator`] — see the
/// module doc):
/// - Lazy creation after the routed design session publishes its first
///   user-visible activity: reads the epoch the launch site already began via
///   [`op_editor_core::agent_indicators::active_epoch`] (never calls
///   `begin()` — that would either steal a fresh epoch out from under the
///   in-flight `DesignSession`'s `indicator_epoch`, or silently no-op if
///   one is already active, neither of which this driver should decide).
///   Waiting for an activity keeps an async classifier's ordinary chat route
///   from being relabelled as a design agent. `None` also means no epoch is
///   active for this turn (e.g. a test harness
///   using `DesignSession::from_channels` without an epoch) — the driver
///   stays dormant rather than fabricating one.
/// - While the indicator exists and `current_design` stays `Some` →
///   registers newly-added top-level frames, same as the chat-loop driver.
/// - When the indicator exists but `current_design` has gone back to `None`
///   (turn finished or aborted) → drops ONLY the local indicator handle.
///   The epoch itself was already retired by `DesignSession::drop` at the
///   moment `current_design` was cleared (`design_session::pump_progress`
///   sets `*current = None` on `poll.finished`, which drops the session in
///   place) — ending it again here would race a fresh epoch a subsequent
///   turn may have already begun.
///
/// The chat-loop and design-session drivers never observe the same turn:
/// `launch_cli_standard_turn` parks both a `ChatSession` and a
/// `DesignSession` up front while an async classifier picks the route, but
/// it never sets `state.chat.agents_running`, so [`pump_indicator`]'s lazy
/// creation gate (`agents_running.0 > 0`) stays closed for that path —
/// only a `OPENPENCIL_DESIGN_AGENT_LOOP` turn sets `agents_running`, and
/// that turn never populates `current_design`.
pub(super) fn pump_design_session_indicator(
    indicator: &mut Option<DesignLoopIndicator>,
    current_design: &Option<crate::design_session::DesignSession>,
    state: &mut EditorState,
    running_tab: Option<usize>,
) {
    let identity = current_design
        .is_some()
        .then(|| ensure_design_session_transcript_identity(state, running_tab))
        .flatten();
    if let (Some((name, color)), None) = (identity, indicator.as_ref()) {
        if let Some(epoch) = agent_indicators::active_epoch() {
            let initial = collect_top_level_frame_ids(state);
            *indicator = Some(DesignLoopIndicator {
                epoch,
                color,
                name,
                initial_frame_ids: initial,
            });
        }
    }

    if let Some(ind) = indicator.as_ref() {
        if current_design.is_some() {
            register_new_frames(ind, state);
        } else {
            // The epoch was already retired by `DesignSession::drop` when
            // `current_design` was cleared — only the local handle is ours
            // to drop.
            *indicator = None;
        }
    }
}

/// Stamp a product persona only after typed design activity arrives. External
/// CLI turns park a `DesignSession` while intent classification is still in
/// flight, so assigning at `current_design.is_some()` would mislabel ordinary
/// chat turns. The provider remains visible in the model selector; the message
/// and canvas use the same user-facing design-agent identity. Cursor
/// confirmation happens here (rather than only in the later indicator pump)
/// so a turn that publishes activity and finishes in one UI frame still keeps
/// its identity for the queued reveal drain.
pub(super) fn ensure_design_session_transcript_identity(
    state: &mut EditorState,
    running_tab: Option<usize>,
) -> Option<(String, String)> {
    let chat = state.chat.run_tab_mut(running_tab);
    let message = chat.messages.iter_mut().rev().find(|message| {
        message.role == op_editor_core::ChatRole::Assistant
            && message.streaming
            && message.design_worker_group.is_none()
    })?;
    if message.activities.is_empty() {
        return None;
    }
    let (name, color) =
        if let (Some(name), Some(color)) = (&message.agent_name, &message.agent_color) {
            (name.clone(), color.clone())
        } else if let Some(tag) = agent_indicators::snapshot().cursor_agent {
            // Adopt whatever identity is ALREADY confirmed for this run
            // instead of minting an independent one (dual-cursor-identity
            // fix, 2026-07-17). The classic Orchestrator's D-lite concurrent
            // screen-group path confirms its primary group's identity
            // before this pump ever runs (see `run.rs`'s `group_identities`
            // doc) — adopting it here is what keeps the transcript speaker
            // matching one of the visible cursors instead of a third,
            // unrelated persona picked by this function's own random seed.
            message.agent_name = Some(tag.name.clone());
            message.agent_color = Some(tag.color.clone());
            (tag.name, tag.color)
        } else {
            let identity = assign_agent_identities_seeded(1, identity_seed())
                .into_iter()
                .next()
                .expect("assign_agent_identities_seeded(1) always yields one");
            message.agent_name = Some(identity.name.clone());
            message.agent_color = Some(identity.color.clone());
            (identity.name, identity.color)
        };
    if let Some(epoch) = agent_indicators::active_epoch() {
        agent_indicators::confirm_cursor_agent(epoch, &color, &name);
    }
    Some((name, color))
}

/// True when the active design-loop epoch still has scheduled reveals that
/// should finish before the loop-end structural finalizer rewrites the tree.
pub(crate) fn reveal_drain_pending_for_active_epoch() -> bool {
    let Some(epoch) = agent_indicators::active_epoch() else {
        return false;
    };
    let Some(end) = agent_indicators::latest_reveal_end_ms(epoch) else {
        return false;
    };
    reveal_now_millis() < end
}

fn reveal_now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "design_loop_indicator_tests.rs"]
mod tests;
