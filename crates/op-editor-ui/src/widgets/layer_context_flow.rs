//! Layer / page context-menu row dispatch, split out of `press_flow.rs`
//! to honor the 800-line cap. Re-exported from `press_flow` so both
//! hosts keep their existing `press_flow::…` import paths.

use op_editor_core::host_press_transitions as core_press;
use op_editor_core::ui_draft::LayerContextTarget;
use op_editor_core::{BooleanOp, EditorState, IdAllocError, IdAllocator};

use crate::widgets::layer_context_menu::LayerContextAction;

/// Residual host work after [`apply_layer_context_action`]. Both hosts
/// mark dirty afterwards regardless of the variant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerContextStep {
    /// Fully applied by the shared dispatch.
    Done,
    /// Host runs its `apply_group()`.
    Group,
    /// Host runs its `apply_boolean_op(op)`.
    Boolean(BooleanOp),
    /// Host re-fits the viewport on the active page.
    Refit,
}

/// Id-minting policy for the context-menu dispatch. Duplicate is the only
/// row that mints ids, so both hosts share one body instead of forking the
/// whole match per policy.
enum LayerContextIds<'a> {
    /// Standalone `n{counter}` allocation off the host counter.
    Sequential(&'a mut u64),
    /// Collaboration session allocation from an owner-assigned namespace.
    Allocator(&'a mut dyn IdAllocator),
}

/// Layer / page context-menu row dispatch.
pub fn apply_layer_context_action(
    state: &mut EditorState,
    next_node_id: &mut u64,
    action: LayerContextAction,
    target: LayerContextTarget,
    now_ms: u64,
) -> LayerContextStep {
    // The sequential policy is infallible: `duplicate_selected` already
    // swallows an exhausted id space as a no-op, so the error arm below
    // is unreachable from here.
    apply_layer_context_ids(
        state,
        &mut LayerContextIds::Sequential(next_node_id),
        action,
        target,
        now_ms,
    )
    .unwrap_or(LayerContextStep::Done)
}

/// Allocator-aware form of [`apply_layer_context_action`] — every fresh id
/// comes from the collaboration session's namespace. Only the Duplicate row
/// allocates, so it is the only source of an error.
pub fn apply_layer_context_action_with_allocator(
    state: &mut EditorState,
    allocator: &mut dyn IdAllocator,
    action: LayerContextAction,
    target: LayerContextTarget,
    now_ms: u64,
) -> Result<LayerContextStep, IdAllocError> {
    apply_layer_context_ids(
        state,
        &mut LayerContextIds::Allocator(allocator),
        action,
        target,
        now_ms,
    )
}

fn apply_layer_context_ids(
    state: &mut EditorState,
    ids: &mut LayerContextIds<'_>,
    action: LayerContextAction,
    target: LayerContextTarget,
    now_ms: u64,
) -> Result<LayerContextStep, IdAllocError> {
    use LayerContextAction as A;
    use LayerContextTarget as T;
    let step = match (action, target) {
        (A::Duplicate, T::Layer(id)) => {
            // Act on the whole multi-selection when the right-clicked
            // row is part of it; otherwise retarget to just this row.
            if !state.is_selected(&id) {
                state.set_single_selection(id);
            }
            match ids {
                LayerContextIds::Sequential(next_node_id) => {
                    state.commit_history();
                    let _ = state.duplicate_selected(next_node_id, 10.0);
                }
                LayerContextIds::Allocator(allocator) => {
                    // Snapshot before the mutation but push it only once the
                    // clone lands, so an exhausted namespace cannot leave a
                    // no-op entry on the undo stack.
                    let snapshot = state.snapshot_for_history();
                    if state
                        .duplicate_selected_with_allocator(*allocator, 10.0)?
                        .is_some()
                    {
                        state.history_push_past(snapshot);
                    }
                }
            }
            LayerContextStep::Done
        }
        (A::Delete, T::Layer(id)) => {
            // Keep the multi-selection so Delete removes every selected
            // layer, not just the right-clicked one.
            if !state.is_selected(&id) {
                state.set_single_selection(id);
            }
            state.commit_history();
            let _ = state.delete_selected();
            LayerContextStep::Done
        }
        (A::GroupSelection, T::Layer(_)) => LayerContextStep::Group,
        // TS boolean rows act on the current selection and push history
        // explicitly (`layer-panel.tsx:389-407`); the host's
        // `apply_boolean_op` does both.
        (
            A::BooleanUnion | A::BooleanSubtract | A::BooleanIntersect | A::BooleanExclude,
            T::Layer(_),
        ) => {
            let op = match action {
                A::BooleanSubtract => BooleanOp::Subtract,
                A::BooleanIntersect => BooleanOp::Intersect,
                A::BooleanExclude => BooleanOp::Exclude,
                _ => BooleanOp::Union,
            };
            LayerContextStep::Boolean(op)
        }
        (A::ToggleLock, T::Layer(id)) => {
            // TS toggleLock runs through mutateWithHistory
            // (document-store-node-actions.ts:176-188).
            core_press::with_doc_history(state, |s| s.toggle_node_locked(&id));
            LayerContextStep::Done
        }
        (A::ToggleVisibility, T::Layer(id)) => {
            // TS toggleVisibility runs through mutateWithHistory
            // (document-store-node-actions.ts:162-174).
            core_press::with_doc_history(state, |s| s.toggle_node_hidden(&id));
            LayerContextStep::Done
        }
        (A::CreateComponent, T::Layer(id)) => {
            let _ = state.create_component_from_node_name(&id);
            LayerContextStep::Done
        }
        (A::DetachComponent | A::DetachInstance, T::Layer(id)) => {
            // Reusable component sheds its flag; a Ref instance
            // materializes into an independent subtree (#22).
            let _ = state.detach_component(&id);
            LayerContextStep::Done
        }
        // Page CRUD pushes history in TS (document-store-pages.ts:19-121)
        // — snapshot-before-mutate, skipped when the guard rejects the op.
        (A::DuplicatePage, T::Page(idx)) => {
            if core_press::with_doc_history(state, |s| s.duplicate_page(idx).is_some()) {
                LayerContextStep::Refit
            } else {
                LayerContextStep::Done
            }
        }
        (A::MovePageUp, T::Page(idx)) => {
            core_press::with_doc_history(state, |s| s.move_page_up(idx));
            LayerContextStep::Done
        }
        (A::MovePageDown, T::Page(idx)) => {
            core_press::with_doc_history(state, |s| s.move_page_down(idx));
            LayerContextStep::Done
        }
        (A::DeletePage, T::Page(idx)) => {
            let deleting_active = idx == state.ui.active_page_index;
            if core_press::with_doc_history(state, |s| s.remove_page(idx)) && deleting_active {
                LayerContextStep::Refit
            } else {
                LayerContextStep::Done
            }
        }
        (A::RenamePage, T::Page(idx)) => {
            if state.start_rename_page(idx) {
                if let Some(rename) = state.ui.layer_rename.as_mut() {
                    rename.input.touch(now_ms);
                }
            }
            LayerContextStep::Done
        }
        (A::RenameLayer, T::Layer(id)) => {
            if state.start_rename_layer(id) {
                if let Some(rename) = state.ui.layer_rename.as_mut() {
                    rename.input.touch(now_ms);
                }
            }
            LayerContextStep::Done
        }
        // Copy link is performed by the host: the link needs the platform's
        // origin and clipboard, and this flow has neither. Reaching this arm
        // means a host that has not wired the row yet, so it stays a no-op —
        // the alternative, a dedicated `LayerContextStep` variant, would force
        // every host to answer for a command only some platforms can perform.
        (A::CopyLink, _) => LayerContextStep::Done,
        // Mismatched action/target — no-op.
        _ => LayerContextStep::Done,
    };
    Ok(step)
}
