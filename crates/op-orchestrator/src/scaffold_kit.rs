//! Session-kit chassis: clone the sentinel onto the page instead of
//! delete-then-`InstantiateComponent` (that gap left the web canvas empty).

use jian_ops_schema::node::PenNode;
use op_editor_core::{
    document_has_kit_sentinel, session_kit, EditorCommand, EditorState, LayoutPropValue, NodeId,
    PenNodeExt,
};

use super::{SAFE_CANVAS_X, SAFE_CANVAS_Y};

/// True when the session kit's sentinel template should replace the generic
/// two-column / empty-frame scaffold.
pub fn kit_chassis_available(state: &EditorState, is_mobile: bool) -> bool {
    !is_mobile && document_has_kit_sentinel(state)
}

/// Detached clone of the session sentinel, placed at the safe canvas origin.
/// `InsertSubtree` remaps ids; the reusable flag is cleared so this is a
/// page instance, not a second master.
fn kit_sentinel_clone(state: &EditorState) -> Option<PenNode> {
    let id = NodeId::new(session_kit().sentinel_master_id.clone());
    let mut node = state.components.resolved_root(&state.doc, &id)?.clone();
    if let PenNode::Frame(frame) = &mut node {
        frame.reusable = None;
    }
    node.base_mut().x = Some(SAFE_CANVAS_X);
    node.base_mut().y = Some(SAFE_CANVAS_Y);
    Some(node)
}

/// Insert the session kit sentinel at the page root.
///
/// A lone empty starter is swapped in-place by `InsertSubtree` (same path as
/// a generated screen). Do not `DeleteNode` first: that empty page is what
/// live-sync pulled onto the web canvas, and the agent cursor never got a
/// reveal schedule for `InstantiateComponent`.
pub fn kit_chassis_commands(
    state: &EditorState,
    is_mobile: bool,
    _reuse_id: Option<&str>,
) -> Option<Vec<EditorCommand>> {
    if !kit_chassis_available(state, is_mobile) {
        return None;
    }
    let instance = kit_sentinel_clone(state)?;
    Some(vec![EditorCommand::InsertSubtree {
        nodes: vec![instance],
        parent_id: NodeId::NONE,
        page_id: None,
    }])
}

/// Turn the placeholder-centered `Main container` into a fillable content area
/// (vertical stack, start-aligned, padded) so generation is not fighting hug-center.
pub fn prepare_kit_content_area_commands(slot_id: &str) -> Vec<EditorCommand> {
    let id = NodeId::new(slot_id);
    let kw = |k: &str| LayoutPropValue::Keyword(k.into());
    vec![
        EditorCommand::SetNodeLayoutProp {
            node_id: id.clone(),
            property: "layout".into(),
            value: kw("vertical"),
        },
        EditorCommand::SetNodeLayoutProp {
            node_id: id.clone(),
            property: "alignItems".into(),
            value: kw("start"),
        },
        EditorCommand::SetNodeLayoutProp {
            node_id: id.clone(),
            property: "justifyContent".into(),
            value: kw("start"),
        },
        EditorCommand::SetNodeLayoutProp {
            node_id: id.clone(),
            property: "height".into(),
            value: kw("fill_container"),
        },
        EditorCommand::SetNodeLayoutProp {
            node_id: id.clone(),
            property: "padding".into(),
            value: LayoutPropValue::NumberArray(vec![24.0, 24.0]),
        },
        EditorCommand::SetNodeLayoutProp {
            node_id: id,
            property: "gap".into(),
            value: LayoutPropValue::Number(16.0),
        },
    ]
}
