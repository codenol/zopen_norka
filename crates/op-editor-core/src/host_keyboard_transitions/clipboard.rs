//! Keyboard transitions that move content between the selection and a
//! clipboard-like buffer: select-all, duplicate and paste.
//!
//! Split out of the `host_keyboard_transitions` spine at the 800-line cap;
//! pure code motion, and the spine re-exports every name below.

use super::edit::{
    mirror_property_input_legacy, mirror_variable_row_input_legacy,
    mirror_variables_header_input_legacy,
};
use super::prompt_center_select_all;

use crate::state::EditorState;

/// Cmd/Ctrl+A over the focused chrome text input, in host priority
/// order. Callers resolve the surfaces whose predicates are host-side
/// (image popovers, settings modal, Git panel) BEFORE calling this.
pub fn select_all_focused_input(state: &mut EditorState, now_ms: u64) -> bool {
    if prompt_center_select_all(state, now_ms) {
        return true;
    }
    if let Some(rename) = state.ui.layer_rename.as_mut() {
        rename.input.select_all();
        rename.input.touch(now_ms);
        return true;
    }
    if state.ui.text_editing.is_some() {
        let _ = state.text_edit_select_all_now(now_ms);
        return true;
    }
    if state.ui.property_focus.is_some() || state.editor_ui.effect_param_focus.is_some() {
        state.ui.property_input.select_all();
        state.ui.property_input.touch(now_ms);
        mirror_property_input_legacy(state, true, now_ms);
        return true;
    }
    if state.editor_ui.variables_header_rename_active() {
        state.editor_ui.variables_header_input.select_all();
        state.editor_ui.variables_header_input.touch(now_ms);
        mirror_variables_header_input_legacy(state, true, now_ms);
        return true;
    }
    if state.editor_ui.variable_row_focus.is_some() {
        state.editor_ui.variable_row_input.select_all();
        state.editor_ui.variable_row_input.touch(now_ms);
        mirror_variable_row_input_legacy(state, true, now_ms);
        return true;
    }
    if state.editor_ui.icon_picker.open {
        state.editor_ui.icon_picker_select_all = true;
        return true;
    }
    if state.editor_ui.chat_model_picker.open {
        state.editor_ui.chat_model_picker_input.select_all();
        state.editor_ui.chat_model_picker_input.touch(now_ms);
        return true;
    }
    if state.editor_ui.component_browser_open {
        state.editor_ui.component_browser_select_all = true;
        return true;
    }
    if state.chat.focused {
        state.chat.select_all_input(now_ms);
        return true;
    }
    false
}

// ─── Selection edit ops ────────────────────────────────────────────────
//
// The "does a text input own the keyboard?" guard stays host-side —
// each host resolves a different set of surfaces.

/// Cmd/Ctrl+D — duplicate the selection as a sibling at +10 doc px.
pub fn duplicate_selection(state: &mut EditorState, next_node_id: &mut u64) -> bool {
    if state.selection.is_empty() {
        return false;
    }
    state.commit_history();
    state.duplicate_selected(next_node_id, 10.0).is_some()
}

/// Cmd/Ctrl+V — paste the clipboard at +10 doc px; selection follows
/// the clones.
pub fn paste_clipboard_at_default_offset(state: &mut EditorState, next_node_id: &mut u64) -> bool {
    if state.clipboard.is_empty() {
        return false;
    }
    state.commit_history();
    !state.paste_clipboard(next_node_id, 10.0).is_empty()
}
