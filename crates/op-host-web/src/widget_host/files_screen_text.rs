//! Text input for the file screen's search field.
//!
//! Small enough to live on the host rather than in a shared flow: the field is
//! a plain string with a caret at the end, and the only interesting rule is
//! that it must swallow keystrokes while it has focus — otherwise a typed
//! letter reaches the canvas shortcuts behind the screen.

use op_editor_core::AppScreen;

/// Longest search a card list can meaningfully use.
const MAX_QUERY_CHARS: usize = 64;

impl super::WidgetHost {
    /// Whether the search field owns the keyboard right now.
    fn file_search_active(&self) -> bool {
        self.editor_state.editor_ui.screen == AppScreen::Files
            && self.editor_state.editor_ui.server_files_search_focused
    }

    /// Push a character into the search field. Returns whether it was consumed.
    pub(in crate::widget_host) fn file_search_takes_text(&mut self, c: char) -> bool {
        if !self.file_search_active() {
            return false;
        }
        // Control characters arrive here too (tab, escape sequences); the
        // field is a name filter, so only printable input belongs in it.
        if !c.is_control() && self.editor_state.editor_ui.server_files_query.chars().count() < MAX_QUERY_CHARS
        {
            self.editor_state.editor_ui.server_files_query.push(c);
            self.mark_dirty();
        }
        true
    }

    /// Remove the last character. Returns whether it was consumed.
    pub(in crate::widget_host) fn file_search_takes_backspace(&mut self) -> bool {
        if !self.file_search_active() {
            return false;
        }
        if self.editor_state.editor_ui.server_files_query.pop().is_some() {
            self.mark_dirty();
        }
        true
    }

    /// Focus or unfocus the search field.
    pub(in crate::widget_host) fn set_file_search_focused(&mut self, focused: bool) -> bool {
        if self.editor_state.editor_ui.server_files_search_focused == focused {
            return false;
        }
        self.editor_state.editor_ui.server_files_search_focused = focused;
        self.mark_dirty();
        true
    }
}
