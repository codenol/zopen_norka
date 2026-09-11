//! State-layer mirror of the rules panel's buttons.
//!
//! Mirrors the hoverable subset of `DesignMdHit` (the widget crate's
//! click enum) for the hover wash stored on
//! `EditorUiState.design_md_panel.hover`. Drag-header / inside hits never
//! hover. Same wasm32-clean discipline as the other `*_state` mirrors.

/// Which design-md-panel button the cursor is over. `None` = no hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignMdButton {
    /// The header `✕` close button.
    Close,
    /// The rules-view "new rule" button.
    NewRule,
    /// The on/off switch of rules-view row `index`.
    RuleToggle(u16),
    /// The delete button of rules-view row `index`.
    RuleDelete(u16),
    /// The body of rules-view row `index`.
    RuleEdit(u16),

    /// The form's save button.
    RuleSave,
    /// The editor's undo button.
    RuleUndo,
    /// The editor's redo button.
    RuleRedo,
    /// The form's cancel button.
    RuleCancel,
}
