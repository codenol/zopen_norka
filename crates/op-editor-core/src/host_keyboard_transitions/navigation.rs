//! Keyboard transitions that move something rather than write something:
//! caret movement, selection nudging, z-order reorder and the tool switch
//! (`keyboard_caret.rs` plus the arrow / bracket arms of `shortcuts.rs`).
//!
//! Split out of the `host_keyboard_transitions` spine at the 800-line cap;
//! pure code motion, and the spine re-exports every name below.

use super::edit::{
    mirror_property_input_legacy, mirror_variable_row_input_legacy,
    mirror_variables_header_input_legacy,
};

use crate::state::EditorState;
use crate::walkers::ReorderDirection;
use crate::Tool;

/// Move the caret one character inside the rule editor.
pub fn design_rule_caret_move(
    state: &mut EditorState,
    forward: bool,
    select: bool,
    now_ms: u64,
) -> Option<bool> {
    let draft = state.editor_ui.design_md_panel.rule_draft.as_mut()?;
    let input = draft.focused_input();
    let before = (input.text().to_owned(), input.caret());
    if forward {
        input.move_right(select, now_ms);
    } else {
        input.move_left(select, now_ms);
    }
    Some((before.0.as_str(), before.1) != (input.text(), input.caret()))
}

/// Route an arrow `(dx, dy)` into the rule editor's caret.
///
/// `None` while the editor is closed — the caller then falls through to
/// canvas nudging. `Some` means the editor owns the key either way.
pub fn design_rule_caret_step(
    state: &mut EditorState,
    dx: f32,
    dy: f32,
    select: bool,
    now_ms: u64,
) -> Option<bool> {
    if dx != 0.0 {
        return design_rule_caret_move(state, dx > 0.0, select, now_ms);
    }
    if dy != 0.0 {
        return design_rule_vertical_caret(state, dy > 0.0, now_ms);
    }
    None
}

/// Move the caret one *logical* line up / down inside the rule editor.
///
/// Deliberately line-based rather than wrap-aware: the rule body is short
/// prose, and keeping this backend-free means Up / Down stay a pure state
/// transition instead of needing the wrapped layout at press time.
pub fn design_rule_vertical_caret(
    state: &mut EditorState,
    down: bool,
    now_ms: u64,
) -> Option<bool> {
    let draft = state.editor_ui.design_md_panel.rule_draft.as_mut()?;
    // The title is a single line: nothing to move between.
    if draft.focus == crate::design_rules_ui::DesignRuleFocus::Title {
        return Some(false);
    }
    let input = &mut draft.body;
    let text = input.text().to_owned();
    let caret = input.caret().min(text.len());
    let line_start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[caret..].find('\n').map_or(text.len(), |i| caret + i);
    let column = caret - line_start;
    let target = if down {
        if line_end >= text.len() {
            text.len()
        } else {
            let next_start = line_end + 1;
            let next_end = text[next_start..]
                .find('\n')
                .map_or(text.len(), |i| next_start + i);
            (next_start + column).min(next_end)
        }
    } else if line_start == 0 {
        0
    } else {
        let prev_end = line_start - 1;
        let prev_start = text[..prev_end].rfind('\n').map_or(0, |i| i + 1);
        (prev_start + column).min(prev_end)
    };
    if target == caret {
        return Some(false);
    }
    input.set_caret(target, now_ms);
    Some(true)
}

/// Left / Right arrow across the three `TextInputState`-backed chrome
/// drafts, in host priority order: property / effect-param → variables
/// header rename → variables row. `false` when none of them is focused,
/// so the caller can fall through to its own tail (the native host's
/// preset-name draft) or to node-nudge.
pub fn property_caret_move(state: &mut EditorState, forward: bool, now_ms: u64) -> bool {
    if state.ui.property_focus.is_some() || state.editor_ui.effect_param_focus.is_some() {
        if forward {
            state.ui.property_input.move_right(false, now_ms);
        } else {
            state.ui.property_input.move_left(false, now_ms);
        }
        mirror_property_input_legacy(state, false, now_ms);
        return true;
    }
    if state.editor_ui.variables_header_rename_active() {
        if forward {
            state
                .editor_ui
                .variables_header_input
                .move_right(false, now_ms);
        } else {
            state
                .editor_ui
                .variables_header_input
                .move_left(false, now_ms);
        }
        mirror_variables_header_input_legacy(state, false, now_ms);
        return true;
    }
    if state.editor_ui.variable_row_focus.is_some() {
        if forward {
            state.editor_ui.variable_row_input.move_right(false, now_ms);
        } else {
            state.editor_ui.variable_row_input.move_left(false, now_ms);
        }
        mirror_variable_row_input_legacy(state, false, now_ms);
        return true;
    }
    false
}

/// Left / Right arrow during an inline layer rename.
pub fn rename_caret(state: &mut EditorState, forward: bool, now_ms: u64) -> bool {
    let moved = if forward {
        state.rename_caret_right()
    } else {
        state.rename_caret_left()
    };
    if moved {
        if let Some(rename) = state.ui.layer_rename.as_mut() {
            rename.input.touch(now_ms);
        }
    }
    moved
}

/// Left / Right arrow on the focused chat input. Consumes the key even
/// at text boundaries so it never falls through to canvas nudge.
///
/// `extend` grows the selection from its existing anchor (Shift+arrow)
/// instead of collapsing it.
///
/// While an IME composition is live the key is swallowed WITHOUT moving
/// the caret: the platform owns caret motion inside a preedit, and
/// splicing our own move under it would desync the composing region from
/// what the input method believes it is editing.
pub fn chat_input_caret(state: &mut EditorState, forward: bool, extend: bool, now_ms: u64) -> bool {
    if !state.chat.focused {
        return false;
    }
    if state.chat.input.composition().is_some() {
        return true;
    }
    if forward {
        state.chat.input.move_right(extend, now_ms);
    } else {
        state.chat.input.move_left(extend, now_ms);
    }
    true
}

// ─── Select-all across focused inputs ──────────────────────────────────

/// Arrow-key nudge — layout-aware move first, plain translate second.
pub fn nudge_selection(state: &mut EditorState, dx: f32, dy: f32) -> bool {
    if state.selection.is_empty() {
        return false;
    }
    let snap = state.snapshot_for_history();
    if state.move_selected_in_layout_direction(dx as f64, dy as f64) {
        state.history_push_past(snap);
        return true;
    }
    if state.translate_selected(dx as f64, dy as f64) {
        state.history_push_past(snap);
        return true;
    }
    false
}

/// `[` / `]` — bump the selection within its parent's children.
pub fn reorder_selection(state: &mut EditorState, direction: ReorderDirection) -> bool {
    if state.selection.is_empty() {
        return false;
    }
    state.commit_history();
    state.reorder_selected(direction)
}

/// Single-key tool switch tail: drop the canvas hover outline, install
/// the tool, and sync the toolbar shape slot for shape variants.
///
/// Hosts run their own prologue first — image-crop exit, variable-row
/// commit, and the pen-path discard (TS `onToolChange` resets the pen
/// preview without committing, `skia-pen-tool.ts:38-50`); the native
/// host routes that through its own `cancel_pen_on_tool_switch`
/// helper, which the pen module owns.
pub fn set_active_tool(state: &mut EditorState, tool: Tool) {
    // Leaving Select must drop the hover outline immediately — cursor
    // moves stop updating it for other tools.
    state.editor_ui.canvas_hover_node = None;
    // One tool at a time, comment tool included: every tool path (the toolbar,
    // the key shortcuts, the command router) funnels through here, so leaving
    // the comment mode belongs here too rather than in each host.
    state.editor_ui.comments.end_mode();
    state.tool = tool;
    if matches!(
        tool,
        Tool::Rect | Tool::Ellipse | Tool::Polygon | Tool::Line | Tool::Pen
    ) {
        state.editor_ui.shape_tool = tool;
    }
}

// ─── Import-modal chord (Cmd+Shift+F / Cmd+Shift+H) ────────────────────
