//! Regression tests for node-shape normalizers.

#[test]
fn normalize_text_content_number_to_string() {
    let mut node = serde_json::json!({
        "type": "text",
        "content": 2024,
        "id": "test1"
    });
    super::normalize_node_shape(&mut node);
    assert_eq!(node["content"], "2024");
}

#[test]
fn normalize_text_content_float_to_string() {
    let mut node = serde_json::json!({
        "type": "text",
        "content": 8.5,
        "id": "test2"
    });
    super::normalize_node_shape(&mut node);
    assert_eq!(node["content"], "8.5");
}

#[test]
fn normalize_text_content_string_unchanged() {
    let mut node = serde_json::json!({
        "type": "text",
        "content": "Hello World",
        "id": "test3"
    });
    super::normalize_node_shape(&mut node);
    assert_eq!(node["content"], "Hello World");
}

#[test]
fn normalize_text_content_boolean_untouched() {
    let mut node = serde_json::json!({
        "type": "text",
        "content": true,
        "id": "test4"
    });
    super::normalize_node_shape(&mut node);
    assert_eq!(
        node["content"], true,
        "boolean content is left for schema to reject"
    );
}

#[test]
fn normalize_text_content_null_untouched() {
    let mut node = serde_json::json!({
        "type": "text",
        "content": serde_json::Value::Null,
        "id": "test5"
    });
    super::normalize_node_shape(&mut node);
    assert!(
        node["content"].is_null(),
        "null content is left for schema to reject"
    );
}

#[test]
fn normalize_text_content_array_unchanged() {
    let mut node = serde_json::json!({
        "type": "text",
        "content": [
            { "text": "styled", "fontSize": 16 }
        ],
        "id": "test6"
    });
    super::normalize_node_shape(&mut node);
    assert!(node["content"].is_array());
}

#[test]
fn normalize_non_text_node_content_untouched() {
    let mut node = serde_json::json!({
        "type": "frame",
        "content": 2024,
        "id": "test7"
    });
    super::normalize_node_shape(&mut node);
    assert_eq!(
        node["content"], 2024,
        "non-text nodes skip content normalization"
    );
}

// ── role-name node types (issue #206) ─────────────────────────────────────

/// `divider` is a ROLE in the skills catalogue, not a `PenNode` variant. A
/// model that writes it as a type must still get its element: the alias
/// becomes the rectangle the role rides on, and the authored props survive.
#[test]
fn divider_type_becomes_a_real_node_carrying_the_role() {
    let mut node = serde_json::json!({
        "type": "divider",
        "name": "card-divider",
        "width": "fill_container",
        "height": 1,
        "fill": [{ "type": "solid", "color": "#E6EBF3" }]
    });
    super::normalize_node_shape(&mut node);
    assert_eq!(node["type"], "rectangle");
    assert_eq!(node["role"], "divider");
    assert_eq!(node["width"], "fill_container");
    assert_eq!(node["height"], 1);
    assert_eq!(node["fill"][0]["color"], "#E6EBF3");
}

/// A role-shaped divider with no fill would render as nothing (a rectangle
/// with neither fill nor stroke draws no pixel) — the same silent loss in
/// another costume. The alias supplies the hairline.
#[test]
fn divider_type_without_a_fill_still_gets_a_hairline() {
    let mut node = serde_json::json!({ "type": "divider", "name": "Divider" });
    super::normalize_node_shape(&mut node);
    assert_eq!(node["width"], "fill_container");
    assert_eq!(node["height"], 1);
    assert_eq!(node["layout"], "none");
    assert_eq!(node["fill"][0]["type"], "solid");
    assert!(
        node["fill"][0]["color"].is_string(),
        "a divider needs a colour to be visible: {node}"
    );
}

/// A named vertical divider flips the axis, exactly as the catalogue says
/// (`width=1, height=fill_container`).
#[test]
fn vertical_divider_type_flips_the_axis() {
    let mut node = serde_json::json!({ "type": "divider", "name": "Vertical Divider" });
    super::normalize_node_shape(&mut node);
    assert_eq!(node["width"], 1);
    assert_eq!(node["height"], "fill_container");
}

/// Every other node type is left exactly as authored — the alias table covers
/// documented role names only, not typos and not real types.
#[test]
fn unknown_node_types_are_left_for_the_schema_to_reject() {
    let mut node = serde_json::json!({ "type": "sprocket", "name": "Not A Node" });
    super::normalize_node_shape(&mut node);
    assert_eq!(
        node["type"], "sprocket",
        "an unknown type must still fail loudly rather than become a rectangle"
    );
}
