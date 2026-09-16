//! The selection and clipboard arms of [`EditorState::apply`].
//!
//! One family because every command here acts on the CURRENT selection (or on
//! the selection the command itself installs) instead of addressing a node by
//! id — the node-addressed and table arms stay in the spine.

use super::*;
use crate::walkers::ReorderDirection;

impl EditorState {
    pub(super) fn cmd_clear_selection(&mut self) -> bool {
        self.clear_selection();
        true
    }

    pub(super) fn cmd_set_selection(&mut self, node_id: NodeId) -> bool {
        // Scoped to the active page — parity with shell-core,
        // which rejected off-page ids so later reads stay
        // consistent.
        if !node_id.is_real() || find_node(self.active_children(), &node_id).is_none() {
            return false;
        }
        self.set_single_selection(node_id);
        true
    }

    pub(super) fn cmd_set_selection_set(&mut self, node_ids: Vec<NodeId>) -> bool {
        // Resolve every id against the active page; unknown /
        // off-page ids are dropped silently.
        let resolved: Vec<NodeId> = node_ids
            .into_iter()
            .filter(|id| id.is_real() && find_node(self.active_children(), id).is_some())
            .collect();
        if resolved.is_empty() {
            self.clear_selection();
        } else {
            let anchor = resolved.last().cloned().unwrap();
            if self.selection.anchor != anchor || self.selection.set != resolved {
                self.editor_ui.image_panel.close_popovers();
            }
            self.selection.anchor = anchor;
            self.selection.set = resolved;
        }
        true
    }

    pub(super) fn cmd_toggle_node_selection(&mut self, node_id: NodeId) -> bool {
        if !node_id.is_real() || find_node(self.active_children(), &node_id).is_none() {
            return false;
        }
        self.toggle_selection(node_id);
        true
    }

    pub(super) fn cmd_duplicate_selected(
        &mut self,
        allocator: &mut dyn IdAllocator,
        offset_px: i32,
    ) -> Result<bool, IdAllocError> {
        Ok(self
            .duplicate_selected_with_allocator(allocator, offset_px as f64)?
            .is_some())
    }

    pub(super) fn cmd_delete_selected(&mut self) -> bool {
        if self.selection.set.is_empty() {
            return false;
        }
        let snap = self.snapshot_for_history();
        if self.delete_selected() {
            self.history_push_past(snap);
            true
        } else {
            false
        }
    }

    pub(super) fn cmd_nudge_selected(&mut self, dx: i32, dy: i32) -> bool {
        if self.selection.set.is_empty() || (dx == 0 && dy == 0) {
            return false;
        }
        let snap = self.snapshot_for_history();
        if self.translate_selected(dx as f64, dy as f64) {
            self.history_push_past(snap);
            true
        } else {
            false
        }
    }

    pub(super) fn cmd_group_selected(
        &mut self,
        allocator: &mut dyn IdAllocator,
    ) -> Result<bool, IdAllocError> {
        let snap = self.snapshot_for_history();
        Ok(
            if self.group_selected_with_allocator(allocator)?.is_some() {
                self.history_push_past(snap);
                true
            } else {
                false
            },
        )
    }

    pub(super) fn cmd_ungroup_selected(&mut self) -> bool {
        let snap = self.snapshot_for_history();
        if self.ungroup_selected() {
            self.history_push_past(snap);
            true
        } else {
            false
        }
    }

    pub(super) fn cmd_reorder_selected(&mut self, direction: ReorderDirection) -> bool {
        if !self.selection.anchor.is_real() {
            return false;
        }
        let snap = self.snapshot_for_history();
        if self.reorder_selected(direction) {
            self.history_push_past(snap);
            true
        } else {
            false
        }
    }

    pub(super) fn cmd_align_selected(&mut self, action: &str) -> bool {
        let Some(parsed) = parse_align_action(action) else {
            return false;
        };
        // `align_selected` pushes its own history on real
        // motion.
        self.align_selected(parsed)
    }

    pub(super) fn cmd_cut_selected(&mut self) -> bool {
        let snap = self.snapshot_for_history();
        if self.cut_selected() {
            self.history_push_past(snap);
            true
        } else {
            false
        }
    }

    pub(super) fn cmd_paste_clipboard(
        &mut self,
        allocator: &mut dyn IdAllocator,
        offset_px: i32,
    ) -> Result<bool, IdAllocError> {
        let snap = self.snapshot_for_history();
        let new_ids = self.paste_clipboard_with_allocator(allocator, offset_px as f64)?;
        if new_ids.is_empty() {
            return Ok(false);
        }
        self.history_push_past(snap);
        Ok(true)
    }
}
