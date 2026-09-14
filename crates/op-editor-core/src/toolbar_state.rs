//! State-layer mirrors of the vertical toolbar's hit + action enums.
//!
//! Lives in its own module (not `editor_ui_state.rs`) because that
//! file already sits over the 800-line repo cap — every new field
//! that lands on `EditorUiState` should bring its supporting types
//! here so the spine stops growing.
//!
//! `ToolbarAction` mirrors `op_editor_ui::widgets::toolbar::ToolbarAction`
//! and `ToolbarHover` mirrors `ToolbarHit`. Both stay free of widget
//! dependencies so `op-editor-core` remains wasm32-clean.

use crate::tool::Tool;

/// One-shot action a toolbar button can dispatch.
///
/// `ToggleComments` is a mode switch rather than a command, and it sits in this
/// enum with `ToggleVariablesPanel` for the same reason: both are "show me this
/// instead", both are decided by state the widget layer already reads, and
/// neither needs a `Tool` variant — the tool enum is what a canvas click
/// *builds*, and a comment is not a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    Undo,
    Redo,
    ToggleVariablesPanel,
    ToggleDesignPanel,
    /// Activate or leave the comment tool.
    ToggleComments,
}

/// Which toolbar item the cursor is over. `None` on
/// `EditorUiState.toolbar_hover` = no hover wash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHover {
    Tool(Tool),
    Action(ToolbarAction),
    /// The shape slot (compound rect/ellipse/polygon/line/pen + chevron).
    ShapeSlot,
}
