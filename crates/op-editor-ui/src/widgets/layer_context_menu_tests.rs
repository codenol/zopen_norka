//! Widget-level tests for the layer / page context menu's row set.
//!
//! The host half of "Copy link" (origin + clipboard) is browser-bound and
//! cannot run on the native test target, so what is pinned here is the half
//! that can be: the row exists on layer rows, is reachable by a press at the
//! point the menu paints it, and is absent where it means nothing.

use crate::widgets::layer_context_menu::{LayerContextAction, LayerContextMenu};
use crate::Point2D;
use op_editor_core::editor_ui_state::{LayerContextMenuState, LayerContextTarget};
use op_editor_core::node_id::NodeId;
use op_editor_core::EditorState;

fn menu_for(state: &EditorState, target: LayerContextTarget) -> LayerContextMenu {
    LayerContextMenu::for_state(
        state,
        LayerContextMenuState {
            target,
            anchor_x: 0.0,
            anchor_y: 0.0,
            menu: Default::default(),
        },
    )
}

/// Every action a press can reach, in paint order — the same walk the hosts'
/// press helpers do, so "the row is in the menu" means "the row is clickable".
fn reachable_actions(menu: &LayerContextMenu) -> Vec<LayerContextAction> {
    let rect = menu.rect();
    let mut found: Vec<LayerContextAction> = Vec::new();
    let mut y = rect.origin.y;
    while y < rect.origin.y + rect.size.y {
        if let Some(action) = menu.hit_test(Point2D::new(rect.origin.x + 24.0, y)) {
            if found.last() != Some(&action) {
                found.push(action);
            }
        }
        y += 2.0;
    }
    found
}

#[test]
fn a_layer_row_offers_copy_link() {
    let mut state = EditorState::default();
    state.selection.set = vec![NodeId::new("n1")];

    let menu = menu_for(&state, LayerContextTarget::Layer(NodeId::new("n1")));
    let actions = reachable_actions(&menu);

    assert!(
        actions.contains(&LayerContextAction::CopyLink),
        "the layer menu must offer Copy link, got {actions:?}"
    );
    // Next to Duplicate (the menu's other clipboard-ish row), above the
    // destructive tail.
    let copy_link = actions
        .iter()
        .position(|action| *action == LayerContextAction::CopyLink)
        .expect("checked above");
    let duplicate = actions
        .iter()
        .position(|action| *action == LayerContextAction::Duplicate)
        .expect("Duplicate is a layer row");
    assert!(copy_link > duplicate, "Copy link follows Duplicate");
}

#[test]
fn a_page_row_has_no_copy_link() {
    // A page tab names no node, and the command is "link to the selection".
    let state = EditorState::default();
    let menu = menu_for(&state, LayerContextTarget::Page(0));

    assert!(!reachable_actions(&menu).contains(&LayerContextAction::CopyLink));
}
