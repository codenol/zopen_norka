//! The canon's deterministic rules, at the decision they make.

use jian_ops_schema::node::PenNode;
use serde_json::json;

use super::detect_design_rule_violations;

fn node(value: serde_json::Value) -> PenNode {
    serde_json::from_value(value).expect("a node from JSON")
}

fn frame(id: &str, name: &str, children: Vec<PenNode>) -> PenNode {
    node(json!({
        "id": id,
        "type": "frame",
        "name": name,
        "x": 0, "y": 0, "width": 100, "height": 100,
        "children": children,
    }))
}

/// A control with its own box and fill — the shape the deterministic rules read.
///
/// Deliberately a frame and not a `ref`: a ref resolves its fill and size from
/// its master, so `C-01`/`C-02`/`K-02` cannot read them without a master
/// resolver, and the canon marks that as a separate job rather than pretending
/// the check exists.
fn control(id: &str, name: &str, width: f64, height: f64, fill: Option<&str>) -> PenNode {
    let mut value = json!({
        "id": id,
        "type": "frame",
        "name": name,
        "x": 0, "y": 0, "width": width, "height": height,
    });
    if let Some(hex) = fill {
        value["fill"] = json!([{ "type": "solid", "color": hex }]);
    }
    node(value)
}

fn reasons(roots: &[PenNode]) -> Vec<String> {
    detect_design_rule_violations(roots)
        .into_iter()
        .map(|issue| issue.reason)
        .collect()
}

#[test]
fn java_on_a_button_is_reported_with_its_rule_id() {
    // C-02, from the canon: Java is the identity mark, never a button fill.
    let roots = vec![control(
        "n1",
        "atom-button-filled-large-accent-default-text",
        120.0,
        32.0,
        Some("#00BEC8"),
    )];
    let found = reasons(&roots);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("C-02:"), "{}", found[0]);
    assert!(
        found[0].contains("#2d98b4"),
        "and it names the right colour: {}",
        found[0]
    );
}

#[test]
fn a_second_shell_root_is_reported() {
    // L-02: this is the shape the second generation round produced — a correct
    // screen plus another Layout/Default beside it.
    let roots = vec![
        frame(
            "n1",
            "Layout/Default",
            vec![frame("n2", "Main container", vec![])],
        ),
        frame(
            "n3",
            "Layout/Default",
            vec![frame("n4", "Sidebar/Default", vec![])],
        ),
    ];
    let found = reasons(&roots);
    assert!(
        found.iter().any(|reason| reason.starts_with("L-02:")),
        "a second shell is a rule violation: {found:?}"
    );
}

#[test]
fn an_empty_root_beside_a_screen_is_reported() {
    // G-03: leftover scaffolding.
    let roots = vec![
        frame(
            "n1",
            "Layout/Default",
            vec![frame("n2", "Main container", vec![])],
        ),
        frame("n3", "Frame", vec![]),
    ];
    let found = reasons(&roots);
    assert!(
        found.iter().any(|reason| reason.starts_with("G-03:")),
        "an empty top-level frame is reported: {found:?}"
    );
}

#[test]
fn a_38px_input_is_reported_and_a_32px_one_is_not() {
    // K-02.
    let tall = reasons(&[control("n1", "atom-input-default", 236.0, 38.0, None)]);
    assert!(
        tall.iter().any(|reason| reason.starts_with("K-02:")),
        "{tall:?}"
    );

    let right = reasons(&[control("n2", "atom-input-default", 236.0, 32.0, None)]);
    assert!(
        right.is_empty(),
        "the kit's own size is not a finding: {right:?}"
    );
}

#[test]
fn a_sidebar_that_is_not_251px_is_reported() {
    // L-03.
    let roots = vec![frame(
        "n1",
        "Sidebar/Default",
        vec![frame("n2", "menu-item", vec![])],
    )];
    let found = reasons(&roots);
    assert!(
        found.iter().any(|reason| reason.starts_with("L-03:")),
        "a 100px sidebar is not the kit's: {found:?}"
    );
}

#[test]
fn a_control_drawn_as_a_frame_is_reported() {
    // G-01: the pattern is the rule — a control that imitates a master's name
    // but is not a ref was rebuilt by hand.
    let roots = vec![frame("n1", "Button/Filled/Large", vec![])];
    let found = reasons(&roots);
    assert!(
        found.iter().any(|reason| reason.starts_with("G-01:")),
        "a hand-drawn control is reported: {found:?}"
    );
}

#[test]
fn a_clean_kit_screen_reports_nothing() {
    // The control that keeps the detector honest: a screen built the way the
    // canon asks for must come back empty.
    let roots = vec![frame(
        "n1",
        "Layout/Default",
        vec![
            frame("n2", "Sidebar/Default", vec![]),
            frame(
                "n3",
                "Main container",
                vec![control(
                    "n4",
                    "atom-button-filled-large-accent-default-text",
                    120.0,
                    32.0,
                    Some("#2D98B4"),
                )],
            ),
        ],
    )];
    let found = reasons(&roots);
    assert!(
        found.is_empty(),
        "a canon-following screen is clean: {found:?}"
    );
}
