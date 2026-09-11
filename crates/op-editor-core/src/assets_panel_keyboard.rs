//! Left-rail Assets search — live filter, same append/pop discipline as
//! the variables-panel search box.

use crate::editor_ui_state::LeftPanelTab;
use crate::state::EditorState;

impl crate::editor_ui_state::EditorUiState {
    /// Whether the left-rail Assets search box owns the keyboard.
    pub fn assets_search_input_active(&self) -> bool {
        self.slides_panel.tab == LeftPanelTab::Assets
            && self.assets_panel.search_focused
            && !self.preview.mode
    }

    /// Blur the Assets search box, keeping the typed filter.
    pub fn blur_assets_search(&mut self) -> bool {
        if !self.assets_search_input_active() {
            return false;
        }
        self.assets_panel.search_focused = false;
        true
    }
}

/// Append `c` to the Assets search filter.
pub fn assets_search_text(state: &mut EditorState, c: char, now_ms: u64) -> bool {
    if !state.editor_ui.assets_search_input_active() || c.is_control() {
        return false;
    }
    state
        .editor_ui
        .assets_panel
        .search_input
        .insert_str(&c.to_string(), now_ms);
    state.editor_ui.assets_panel.search =
        state.editor_ui.assets_panel.search_input.text().to_string();
    state.editor_ui.assets_panel.scroll.offset = 0.0;
    true
}

/// Pop the last char off the Assets search filter.
/// `Some(false)` on an empty filter: the box still owns the key.
pub fn assets_search_backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    if !state.editor_ui.assets_search_input_active() {
        return None;
    }
    let before = state.editor_ui.assets_panel.search_input.text().to_string();
    state.editor_ui.assets_panel.search_input.backspace(now_ms);
    let after = state.editor_ui.assets_panel.search_input.text().to_string();
    state.editor_ui.assets_panel.search = after.clone();
    if before == after {
        return Some(false);
    }
    state.editor_ui.assets_panel.scroll.offset = 0.0;
    Some(true)
}
