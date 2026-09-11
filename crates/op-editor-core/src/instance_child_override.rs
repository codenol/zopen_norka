//! Virtual component-instance child lookup for inspector reads/writes.
//!
//! Canvas expansion gives every authored component descendant a
//! render-only `refId__childId` anchor. This module validates that id
//! against the real Ref + component tree and resolves the effective
//! child without teaching generic document walkers about virtual nodes.

use crate::instance_override::resolve_instance_display_node;
use crate::node_id::NodeId;
use crate::pen_node_ext::PenNodeExt;
use crate::walkers::find_node;
use jian_ops_schema::node::PenNode;
use jian_ops_schema::PenDocument;

fn find_authored_node<'a>(doc: &'a PenDocument, id: &str) -> Option<&'a PenNode> {
    fn walk<'a>(nodes: &'a [PenNode], id: &str) -> Option<&'a PenNode> {
        for node in nodes {
            if node.id_str() == id {
                return Some(node);
            }
            if let Some(children) = node.children() {
                if let Some(hit) = walk(children, id) {
                    return Some(hit);
                }
            }
        }
        None
    }

    if let Some(pages) = doc.pages.as_ref() {
        for page in pages {
            if let Some(hit) = walk(&page.children, id) {
                return Some(hit);
            }
        }
    }
    walk(&doc.children, id)
}

fn find_descendant<'a>(children: &'a [PenNode], id: &NodeId) -> Option<&'a PenNode> {
    for child in children {
        if child.id_str() == id.as_str() {
            return Some(child);
        }
        if let Some(grandchildren) = child.children() {
            if let Some(hit) = find_descendant(grandchildren, id) {
                return Some(hit);
            }
        }
    }
    None
}

fn component_child_source<'a>(
    doc: &'a PenDocument,
    ref_node: &'a PenNode,
) -> Option<&'a [PenNode]> {
    let PenNode::Ref(reference) = ref_node else {
        return None;
    };
    let component = find_authored_node(doc, &reference.target)?;
    component
        .children()
        .filter(|children| !children.is_empty())
        .or_else(|| ref_node.children())
        .map(|children| children.as_slice())
}

fn underscore_separators(raw: &str) -> impl Iterator<Item = usize> + '_ {
    raw.as_bytes()
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| (pair == b"__").then_some(index))
}

fn reconstruct_virtual_id(ref_id: &str, path: &[NodeId]) -> String {
    path.iter().fold(ref_id.to_string(), |acc, id| {
        crate::ref_resolve::instance_child_virtual_id(&acc, id.as_str())
    })
}

/// Walk `remainder` against a component (or nested-ref) child tree.
/// One-level ids consume the whole remainder; nested expanded refs
/// split at `__` and continue into the inner Ref's target.
fn match_override_path(
    doc: &PenDocument,
    children: &[PenNode],
    remainder: &str,
) -> Option<Vec<NodeId>> {
    if remainder.is_empty() {
        return None;
    }
    let mut found = None;
    let mut ambiguous = false;
    let mut consider = |candidate: Vec<NodeId>| {
        if found
            .as_ref()
            .is_some_and(|existing: &Vec<NodeId>| existing != &candidate)
        {
            ambiguous = true;
        } else {
            found = Some(candidate);
        }
    };
    if let Some(id) = NodeId::new_opt(remainder) {
        if find_descendant(children, &id).is_some() {
            consider(vec![id]);
        }
    }
    for separator in underscore_separators(remainder) {
        let (first_raw, rest_with_separator) = remainder.split_at(separator);
        let rest = &rest_with_separator[2..];
        let Some(first_id) = NodeId::new_opt(first_raw) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let Some(node) = find_descendant(children, &first_id) else {
            continue;
        };
        let Some(inner_children) = component_child_source(doc, node) else {
            continue;
        };
        let Some(mut tail) = match_override_path(doc, inner_children, rest) else {
            continue;
        };
        let mut path = vec![first_id];
        path.append(&mut tail);
        consider(path);
    }
    (!ambiguous).then_some(found).flatten()
}

/// Split a canvas instance-child anchor into the authored outer Ref and
/// the chain of original descendant ids. Nested expanded refs yield
/// `outer__innerRefId__leafId` and a path of length ≥ 2 so two copies of
/// the same master do not share one override slot.
pub fn split_instance_override_path(
    anchor: &NodeId,
    doc: &PenDocument,
) -> Option<(NodeId, Vec<NodeId>)> {
    let raw = anchor.as_str();
    if !anchor.is_real() || !raw.contains("__") || find_authored_node(doc, raw).is_some() {
        return None;
    }
    let mut found = None;
    let mut ambiguous = false;
    // Check every adjacent underscore pair. `str::match_indices` advances
    // past a match and therefore misses the real boundary in an id such as
    // `inst___icon` (`ref_id = "inst_"`, child id = "icon").
    for separator in underscore_separators(raw) {
        let (ref_raw, child_with_separator) = raw.split_at(separator);
        let remainder = &child_with_separator[2..];
        let Some(ref_id) = NodeId::new_opt(ref_raw) else {
            continue;
        };
        if remainder.is_empty() {
            continue;
        }
        let Some(ref_node) = find_authored_node(doc, ref_id.as_str()) else {
            continue;
        };
        if !matches!(ref_node, PenNode::Ref(_)) {
            continue;
        }
        let Some(children) = component_child_source(doc, ref_node) else {
            continue;
        };
        let Some(path) = match_override_path(doc, children, remainder) else {
            continue;
        };
        if reconstruct_virtual_id(ref_id.as_str(), &path) != raw {
            continue;
        }
        let candidate = (ref_id, path);
        if found
            .as_ref()
            .is_some_and(|existing| existing != &candidate)
        {
            ambiguous = true;
        } else {
            found = Some(candidate);
        }
    }
    (!ambiguous).then_some(found).flatten()
}

/// One-level wrapper: authored Ref + original child id. Nested
/// `outer__inner__leaf` anchors return `None` here; use
/// [`split_instance_override_path`] for the full chain.
pub fn split_instance_child_anchor(anchor: &NodeId, doc: &PenDocument) -> Option<(NodeId, NodeId)> {
    let (ref_id, path) = split_instance_override_path(anchor, doc)?;
    (path.len() == 1).then(|| (ref_id, path.into_iter().next().unwrap()))
}

/// Merge leaf override keys along `path` into an instance `descendants`
/// map. Nested paths become `descendants[inner].descendants[leaf]`.
pub(crate) fn merge_instance_override_path(
    descendants: &mut serde_json::Map<String, serde_json::Value>,
    path: &[String],
    overrides: serde_json::Map<String, serde_json::Value>,
) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        let entry = descendants
            .entry(path[0].clone())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let serde_json::Value::Object(existing) = entry {
            for (key, value) in overrides {
                existing.insert(key, value);
            }
        }
        return;
    }
    let entry = descendants
        .entry(path[0].clone())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let serde_json::Value::Object(existing) = entry else {
        return;
    };
    let nested = existing
        .entry("descendants".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let serde_json::Value::Object(nested_map) = nested {
        merge_instance_override_path(nested_map, &path[1..], overrides);
    }
}

fn resolve_leaf_along_path(
    doc: &PenDocument,
    instance: &PenNode,
    path: &[NodeId],
    anchor: &NodeId,
) -> Option<PenNode> {
    let mut current = instance.clone();
    for (index, child_id) in path.iter().enumerate() {
        let display = resolve_instance_display_node(doc, &current)?;
        let mut child = find_node(display.children()?, child_id)?.clone();
        if index + 1 == path.len() {
            child.base_mut().id = anchor.as_str().to_string();
            return Some(child);
        }
        if !matches!(child, PenNode::Ref(_)) {
            return None;
        }
        current = child;
    }
    None
}

/// Resolve an authored Ref root or one of its canvas-only virtual
/// child anchors into the effective node shown by the inspector.
/// Child results keep the virtual anchor id while all other fields
/// come from the component child plus `descendants` along the path.
pub fn resolve_instance_display_node_for_anchor(
    doc: &PenDocument,
    anchor: &NodeId,
) -> Option<PenNode> {
    if let Some(node) = find_authored_node(doc, anchor.as_str()) {
        return matches!(node, PenNode::Ref(_))
            .then(|| resolve_instance_display_node(doc, node))
            .flatten();
    }
    let (ref_id, path) = split_instance_override_path(anchor, doc)?;
    let ref_node = find_authored_node(doc, ref_id.as_str())?;
    resolve_leaf_along_path(doc, ref_node, &path, anchor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walkers::{find_node, find_node_mut};
    use crate::EditorState;

    const DOC: &str = r##"{
      "version":"1.0.0",
      "children":[
        {"type":"frame","id":"button","name":"Button","reusable":true,
         "width":120,"height":40,"children":[
           {"type":"icon_font","id":"icon","name":"home","iconFontName":"home",
            "width":24,"height":24,"fill":[{"type":"solid","color":"#111111"}]}
         ]},
        {"type":"ref","id":"inst","ref":"button","x":200,"y":80}
      ]
    }"##;

    fn state() -> EditorState {
        let doc = jian_ops_schema::load_str(DOC).expect("fixture").value;
        EditorState::from_document(doc)
    }

    fn rich_color_state() -> EditorState {
        let doc = jian_ops_schema::load_str(
            r##"{
              "version":"1.0.0",
              "children":[
                {"type":"frame","id":"card","name":"Card","reusable":true,
                 "width":120,"height":80,"children":[
                   {"type":"rectangle","id":"paint","name":"Paint",
                    "width":80,"height":40,
                    "fill":[
                      {"type":"linear_gradient","angle":0,"stops":[
                        {"offset":0,"color":"#ffffff"},
                        {"offset":1,"color":"#00000080"}
                      ]},
                      {"type":"solid","color":"#123456"}
                    ],
                    "effects":[{"type":"shadow","offsetX":0,"offsetY":4,
                      "blur":8,"spread":0,"color":"#00000040"}]}
                 ]},
                {"type":"ref","id":"inst","ref":"card","x":200,"y":80}
              ]
            }"##,
        )
        .expect("rich color fixture")
        .value;
        EditorState::from_document(doc)
    }

    fn ref_node(state: &EditorState) -> &jian_ops_schema::node::RefNode {
        match find_node(state.active_children(), &NodeId::new("inst")) {
            Some(PenNode::Ref(reference)) => reference,
            other => panic!("inst must remain a Ref, got {other:?}"),
        }
    }

    #[test]
    fn picker_opens_edits_and_records_history_for_virtual_child() {
        let mut state = state();
        state.set_single_selection(NodeId::new("inst__icon"));
        assert!(state.open_color_picker(crate::ui_draft::ColorTarget::Fill, 0.0));
        assert!(state.color_picker_set_hsv(0.0, 1.0, 1.0));
        assert!(state.close_color_picker());
        assert_eq!(state.history.past.len(), 1);
        assert_eq!(
            ref_node(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("icon"))
                .and_then(|v| v.pointer("/fill/0/color"))
                .and_then(serde_json::Value::as_str),
            Some("#ff0000")
        );
    }

    #[test]
    fn picker_routes_every_node_color_target_to_virtual_child_override() {
        let mut indexed = rich_color_state();
        indexed.set_single_selection(NodeId::new("inst__paint"));
        assert!(indexed.open_color_picker_for_fill(crate::ui_draft::ColorTarget::Fill, 1, 0.0));
        assert!(indexed.color_picker_set_hsv(0.0, 1.0, 1.0));
        assert_eq!(
            ref_node(&indexed)
                .descendants
                .as_ref()
                .and_then(|d| d.get("paint"))
                .and_then(|v| v.pointer("/fill/1/color"))
                .and_then(serde_json::Value::as_str),
            Some("#ff0000")
        );

        let mut gradient = rich_color_state();
        gradient.set_single_selection(NodeId::new("inst__paint"));
        assert!(gradient.open_color_picker(crate::ui_draft::ColorTarget::GradientStop(1), 0.0));
        assert!(gradient.color_picker_set_hsv(120.0, 1.0, 1.0));
        assert_eq!(
            ref_node(&gradient)
                .descendants
                .as_ref()
                .and_then(|d| d.get("paint"))
                .and_then(|v| v.pointer("/fill/0/stops/1/color"))
                .and_then(serde_json::Value::as_str),
            Some("#00ff0080")
        );

        let mut effect = rich_color_state();
        effect.set_single_selection(NodeId::new("inst__paint"));
        assert!(effect.open_color_picker(crate::ui_draft::ColorTarget::EffectColor(0), 0.0));
        effect.color_picker_focus_hex();
        effect.color_picker_hex_backspace(0);
        effect.color_picker_hex_char('f', 1);
        assert_eq!(
            ref_node(&effect)
                .descendants
                .as_ref()
                .and_then(|d| d.get("paint"))
                .and_then(|v| v.pointer("/effects/0/color"))
                .and_then(serde_json::Value::as_str),
            Some("#00000f40")
        );
    }

    #[test]
    fn split_accepts_ref_id_ending_in_underscore() {
        let mut state = state();
        let PenNode::Ref(reference) = find_node(state.active_children(), &NodeId::new("inst"))
            .expect("ref")
            .clone()
        else {
            panic!("expected ref");
        };
        let mut underscored = reference;
        underscored.base.id = "inst_".into();
        state.doc.children.push(PenNode::Ref(underscored));

        assert_eq!(
            split_instance_child_anchor(&NodeId::new("inst___icon"), &state.doc),
            Some((NodeId::new("inst_"), NodeId::new("icon")))
        );
    }

    #[test]
    fn direct_fill_opacity_write_routes_to_virtual_child_override() {
        let mut state = state();
        state.set_single_selection(NodeId::new("inst__icon"));
        assert!(state.set_selected_fill_opacity(0.25));
        assert_eq!(
            ref_node(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("icon"))
                .and_then(|v| v.pointer("/fill/0/opacity"))
                .and_then(serde_json::Value::as_f64),
            Some(0.25)
        );
    }

    #[test]
    fn history_repair_handles_a_full_past_deque() {
        let mut state = state();
        for _ in 0..crate::HISTORY_CAP {
            state.commit_history();
        }
        let child = NodeId::new("inst__icon");
        state.set_single_selection(child.clone());
        let scope = state
            .begin_instance_write_for_anchor()
            .expect("virtual child scope");
        state.commit_history();
        assert!(state.set_selected_color(true, "#ff0000"));
        state.finish_instance_write(scope);

        assert_eq!(state.history.past.len(), crate::HISTORY_CAP);
        let newest = state.history.past.back().expect("newest history snapshot");
        assert!(matches!(
            newest.doc.snapshot_find_node(0, &NodeId::new("inst")),
            Some(PenNode::Ref(_))
        ));
        assert!(newest.doc.snapshot_find_node(0, &child).is_none());
    }

    #[test]
    fn instance_text_child_edit_writes_a_content_override() {
        let mut state = EditorState::from_document(
            jian_ops_schema::load_str(
                r##"{
              "version":"1.0.0",
              "children":[
                {"type":"frame","id":"chip","name":"Chip","reusable":true,
                 "width":80,"height":24,"children":[
                   {"type":"text","id":"label","name":"Label",
                    "content":"Label","fontSize":12,"width":40,"height":16}
                 ]},
                {"type":"ref","id":"inst","ref":"chip","x":20,"y":20}
              ]
            }"##,
            )
            .expect("fixture")
            .value,
        );
        let child = NodeId::new("inst__label");
        assert!(
            state.start_text_edit(child.clone()),
            "virtual text must enter edit"
        );
        assert_eq!(state.text_edit_content(), Some("Label"));
        assert!(state.text_edit_select_all_now(0));
        assert!(state.text_edit_insert("Home", 100));
        assert!(state.text_edit_commit());
        assert_eq!(
            ref_node(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("label"))
                .and_then(|v| v.get("content"))
                .and_then(serde_json::Value::as_str),
            Some("Home")
        );
        assert!(
            matches!(
                find_node(state.active_children(), &NodeId::new("inst")),
                Some(PenNode::Ref(_))
            ),
            "the instance must stay a Ref after the text override"
        );
        assert!(
            find_node(state.active_children(), &child).is_none(),
            "virtual child ids are not authored"
        );
    }

    fn nested_nav_state() -> EditorState {
        EditorState::from_document(
            jian_ops_schema::load_str(
                r##"{
              "version":"1.0.0",
              "children":[
                {"type":"frame","id":"item","name":"Item","reusable":true,
                 "width":80,"height":24,"children":[
                   {"type":"icon_font","id":"icon","name":"icon","iconFontName":"eye",
                    "iconFontFamily":"lucide","width":16,"height":16},
                   {"type":"text","id":"label","name":"Label",
                    "content":"Label","fontSize":12,"width":40,"height":16}
                 ]},
                {"type":"frame","id":"nav","name":"Nav","reusable":true,
                 "width":100,"height":52,"children":[
                   {"type":"ref","id":"row-a","ref":"item"},
                   {"type":"ref","id":"row-b","ref":"item"}
                 ]},
                {"type":"ref","id":"shell","ref":"nav","x":0,"y":0}
              ]
            }"##,
            )
            .expect("nested fixture")
            .value,
        )
    }

    fn shell_ref(state: &EditorState) -> &jian_ops_schema::node::RefNode {
        match find_node(state.active_children(), &NodeId::new("shell")) {
            Some(PenNode::Ref(reference)) => reference,
            other => panic!("shell must remain a Ref, got {other:?}"),
        }
    }

    #[test]
    fn nested_instance_path_does_not_collapse_to_one_level() {
        let state = nested_nav_state();
        assert_eq!(
            split_instance_child_anchor(&NodeId::new("shell__row-a__label"), &state.doc),
            None,
            "one-level split must not steal the inner ref id"
        );
        assert_eq!(
            split_instance_override_path(&NodeId::new("shell__row-a__label"), &state.doc),
            Some((
                NodeId::new("shell"),
                vec![NodeId::new("row-a"), NodeId::new("label")]
            ))
        );
    }

    #[test]
    fn nested_instance_text_edit_writes_under_the_inner_ref() {
        let mut state = nested_nav_state();
        let child = NodeId::new("shell__row-a__label");
        assert!(state.start_text_edit(child.clone()));
        assert!(state.text_edit_select_all_now(0));
        assert!(state.text_edit_insert("Home", 100));
        assert!(state.text_edit_commit());
        assert_eq!(
            shell_ref(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("row-a"))
                .and_then(|v| v.pointer("/descendants/label/content"))
                .and_then(serde_json::Value::as_str),
            Some("Home")
        );
        assert!(
            shell_ref(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("row-b"))
                .is_none(),
            "the sibling copy of the same master must stay untouched"
        );
    }

    #[test]
    fn instance_icon_child_replace_writes_an_icon_override() {
        let mut state = state();
        state.set_single_selection(NodeId::new("inst__icon"));
        assert!(state.replace_selected_icon("star", "lucide", None));
        assert_eq!(
            ref_node(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("icon"))
                .and_then(|v| v.get("iconFontName"))
                .and_then(serde_json::Value::as_str),
            Some("star")
        );
        assert!(matches!(
            find_node(state.active_children(), &NodeId::new("inst")),
            Some(PenNode::Ref(_))
        ));
        let master = find_node(state.active_children(), &NodeId::new("icon")).expect("master");
        let PenNode::IconFont(icon) = master else {
            panic!("expected master icon_font");
        };
        assert_eq!(icon.icon_font_name, "home");
    }

    #[test]
    fn nested_instance_icon_replace_writes_under_the_inner_ref() {
        let mut state = nested_nav_state();
        state.set_single_selection(NodeId::new("shell__row-b__icon"));
        assert!(state.replace_selected_icon("star", "lucide", None));
        assert_eq!(
            shell_ref(&state)
                .descendants
                .as_ref()
                .and_then(|d| d.get("row-b"))
                .and_then(|v| v.pointer("/descendants/icon/iconFontName"))
                .and_then(serde_json::Value::as_str),
            Some("star")
        );
        assert!(shell_ref(&state)
            .descendants
            .as_ref()
            .and_then(|d| d.get("row-a"))
            .is_none());
    }

    #[test]
    fn authored_node_wins_over_a_colliding_virtual_id() {
        let mut state = state();
        let ordinary: PenNode = serde_json::from_value(serde_json::json!({
            "type":"rectangle", "id":"inst__icon", "name":"Authored",
            "width":40, "height":40,
            "fill":[{"type":"solid","color":"#222222"}]
        }))
        .expect("ordinary authored node");
        state.doc.children.push(ordinary);
        state.set_single_selection(NodeId::new("inst__icon"));

        assert!(
            resolve_instance_display_node_for_anchor(&state.doc, &NodeId::new("inst__icon"))
                .is_none()
        );
        assert!(state.set_selected_color(true, "#00ff00"));
        let ordinary = find_node(state.active_children(), &NodeId::new("inst__icon"))
            .expect("authored node remains selected");
        assert_eq!(
            crate::fills::first_solid_fill_hex(ordinary),
            Some("#00ff00")
        );
        assert!(ref_node(&state).descendants.is_none());

        let master_icon = find_node_mut(state.active_children_mut(), &NodeId::new("icon"))
            .expect("master icon remains untouched");
        assert_eq!(
            crate::fills::first_solid_fill_hex(master_icon),
            Some("#111111")
        );
    }
}
