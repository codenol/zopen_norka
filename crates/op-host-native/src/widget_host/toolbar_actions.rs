//! Toolbar action dispatch for native press/click paths — panel toggles
//! delegate to the shared `EditorUiState` transitions.

use super::WidgetHostNative;

impl WidgetHostNative {
    pub(in crate::widget_host) fn dispatch_toolbar_action(
        &mut self,
        action: op_editor_ui::widgets::ToolbarAction,
    ) -> bool {
        use op_editor_ui::widgets::ToolbarAction;
        match action {
            ToolbarAction::Undo => self.apply_undo(),
            ToolbarAction::Redo => self.apply_redo(),
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
            // Added only to keep the shared toolbar-action enum exhaustive. The
            // comment button is offered exclusively where a comment client
            // exists (`CommentsUiState::transport`, set by the web host) and the
            // state refuses the mode without one, so no native path can reach
            // this arm and no native surface is affected.
            ToolbarAction::ToggleComments => {
                self.editor_state.editor_ui.comments.toggle_pin_mode();
                self.mark_dirty();
                true
            }
        }
    }
}
