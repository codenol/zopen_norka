//! Toolbar action dispatch for web press/click paths — panel toggles
//! delegate to the shared `EditorUiState` transitions.

use super::WidgetHost;

impl WidgetHost {
    pub(in crate::widget_host) fn dispatch_toolbar_action(
        &mut self,
        action: op_editor_ui::widgets::ToolbarAction,
    ) -> bool {
        use op_editor_ui::widgets::ToolbarAction;
        match action {
            ToolbarAction::Undo => {
                let acted = self.editor_state.undo();
                if acted {
                    self.mark_dirty();
                    self.refresh_missing_fonts_after_history_change();
                }
                acted
            }
            ToolbarAction::Redo => {
                let acted = self.editor_state.redo();
                if acted {
                    self.mark_dirty();
                    self.refresh_missing_fonts_after_history_change();
                }
                acted
            }
            ToolbarAction::ToggleVariablesPanel => {
                self.editor_state.editor_ui.toggle_variables_panel();
                self.mark_dirty();
                true
            }
            ToolbarAction::ToggleDesignPanel => {
                self.editor_state.editor_ui.toggle_design_md_panel();
                self.mark_dirty();
                true
            }
            ToolbarAction::ToggleComments => {
                // The mode switch is the state's: the rail's occupant, the
                // button's active fill and the canvas click that drops a pin are
                // all read from `pin_mode`, so nothing here decides anything.
                // Turning it on also asks for the conversation (see
                // `CommentsUiState::begin_mode`).
                self.editor_state.editor_ui.comments.toggle_pin_mode();
                self.mark_dirty();
                true
            }
        }
    }
}
