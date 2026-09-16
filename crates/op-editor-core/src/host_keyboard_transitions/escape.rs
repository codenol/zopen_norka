//! Keyboard transitions that close chrome rather than edit it: the
//! import-modal gate, the commit-everything-on-modal pass, and opening
//! the import modal itself.
//!
//! Split out of the `host_keyboard_transitions` spine at the 800-line cap;
//! pure code motion, and the spine re-exports every name below.

use crate::editor_ui_state::EditorUiState;
use crate::figma_import_state::ImportSource;
use crate::state::EditorState;

/// Overlay/modal state that must keep ownership of the import chord.
/// The Git-panel and image-popover halves of the guard stay host-side
/// (host-resolved visibility predicates).
pub fn import_modal_blocked_by_overlay(ui: &EditorUiState) -> bool {
    ui.figma_import_in_progress
        || !ui.figma_import_pages.is_empty()
        || ui.export_dialog_open
        || ((ui.account_ui_available || ui.touch_chrome()) && ui.login_modal_open)
        || ui.agent_settings_open
        // A stale settings focus still routes keys to the port / API-key
        // field; the chord must not steal them.
        || ui.agent_settings.focus.is_some()
        || (ui.missing_fonts_modal_open
            && ui
                .missing_fonts_prompt
                .as_ref()
                .is_some_and(|prompt| !prompt.entries.is_empty()))
}

/// Commit canvas / layer editing before a full-screen modal covers it.
pub fn commit_editing_for_modal(state: &mut EditorState) {
    let _ = state.rename_commit();
    let _ = state.text_edit_commit();
    state.color_picker_blur_hex();
    state.color_picker_blur_rgb();
    let _ = state.close_color_picker();
}

/// Close every chrome overlay the import modal covers, then open it.
pub fn open_import_modal(ui: &mut EditorUiState, source: ImportSource) {
    ui.close_font_picker();
    ui.close_icon_picker();
    ui.component_browser_open = false;
    ui.component_browser_kit_picker_open = false;
    ui.component_browser_confirm_delete_kit = None;
    ui.component_browser_hover = None;
    ui.ime_preedit = None;
    ui.import_source = source;
    ui.figma_import_open = true;
    ui.figma_import_hover = None;
}
