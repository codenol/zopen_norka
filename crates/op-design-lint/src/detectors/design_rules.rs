//! The deterministic half of the generation canon
//! (`design/design-rules-canon.md`).
//!
//! Every finding's `reason` STARTS with its rule id — `C-02: …`, `L-02: …` — so
//! a note can be traced back to the canon in the transcript, in a correction
//! round, and by a person reading a report. Only rules decidable from the
//! document itself live here; the judgement rules belong to the verifier.
//!
//! Why this exists at all: the canon was 8.8 KB of prose, and the prompt took
//! two `do` and one `don't` per kit type. A rule nobody can check is a
//! preference; a rule with a detector is a guardrail.

use jian_ops_schema::node::PenNode;

use crate::issue::{FixProperty, Issue, IssueCategory, IssueSeverity};
use crate::node_util::{
    children, first_fill_color, node_id, node_kind, node_name, numeric_height, numeric_width,
    NodeKind,
};

/// Bondi — the action colour (`C-01`).
const BONDI: &str = "#2d98b4";
/// Java — identity only, never a button fill (`C-02`).
const JAVA: &str = "#00bec8";
/// The sidebar's own width (`L-03`).
const SIDEBAR_WIDTH: f64 = 251.0;
/// Kit control height (`K-02`).
const CONTROL_HEIGHT: f64 = 32.0;

/// Names that mean "this root is the app shell" (`L-02`).
fn is_shell_root(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("layout/default")
        || name.contains("sidebar/default")
        || name.contains("topbar")
        || name.contains("app shell")
}

/// Master-name patterns a hand-drawn control imitates (`G-01`).
const CONTROL_PATTERNS: [&str; 6] = [
    "button/",
    "input/",
    "checkbox/",
    "chip/",
    "badge/",
    "pagination/",
];

fn name_of(node: &PenNode) -> String {
    node_name(node).unwrap_or_default().to_string()
}

fn hex_lower(hex: &str) -> String {
    let trimmed = hex.trim().to_lowercase();
    if trimmed.starts_with('#') {
        trimmed
    } else {
        format!("#{trimmed}")
    }
}

fn violation(
    node_id: String,
    property: FixProperty,
    current: serde_json::Value,
    reason: String,
) -> Issue {
    Issue {
        node_id,
        category: IssueCategory::DesignRule,
        severity: IssueSeverity::Warning,
        property,
        current_value: current,
        suggested_value: serde_json::Value::Null,
        reason,
    }
}

/// Walk the active page's top-level nodes and report canon violations.
///
/// Page-level rules (`L-02`, `G-03`) are decided from the roots; per-node rules
/// (`C-01`, `C-02`, `K-02`, `L-03`, `G-01`) from the walk.
pub fn detect_design_rule_violations(roots: &[PenNode]) -> Vec<Issue> {
    let mut out = Vec::new();

    // L-02 — one shell per page. Every extra is a screen nobody asked for.
    let shells: Vec<&PenNode> = roots
        .iter()
        .filter(|root| is_shell_root(&name_of(root)))
        .collect();
    for extra in shells.iter().skip(1) {
        out.push(violation(
            node_id(extra).to_string(),
            FixProperty::Remove,
            serde_json::json!(name_of(extra)),
            format!(
                "L-02: {} is a second app shell on this page — a screen holds ONE \
                 Layout/Default (plus its own Sidebar/topbar), never a second one",
                name_of(extra)
            ),
        ));
    }

    // G-03 — a root with no children is leftover scaffolding.
    if roots.len() > 1 {
        for root in roots.iter().filter(|root| {
            matches!(node_kind(root), NodeKind::Frame | NodeKind::Group)
                && children(root).is_empty()
        }) {
            out.push(violation(
                node_id(root).to_string(),
                FixProperty::Remove,
                serde_json::json!(name_of(root)),
                format!(
                    "G-03: {} is an empty top-level frame — leftover scaffolding, not a screen",
                    name_of(root)
                ),
            ));
        }
    }

    for root in roots {
        walk(root, &mut out);
    }
    out
}

fn walk(node: &PenNode, out: &mut Vec<Issue>) {
    let name = name_of(node);
    let lower = name.to_lowercase();
    let is_ref = matches!(node_kind(node), NodeKind::Ref);
    let fill = first_fill_color(node).map(hex_lower);

    // C-02 / C-01 — the action colour.
    if lower.contains("button") {
        if let Some(hex) = fill.as_deref() {
            if hex == JAVA {
                out.push(violation(
                    node_id(node).to_string(),
                    FixProperty::Fill,
                    serde_json::json!(hex),
                    format!(
                        "C-02: {name} is filled Java {JAVA} — Java is the identity mark \
                         (logo, switch-on), never a button fill. Use Bondi {BONDI}"
                    ),
                ));
            } else if lower.contains("accent") && hex != BONDI {
                out.push(violation(
                    node_id(node).to_string(),
                    FixProperty::Fill,
                    serde_json::json!(hex),
                    format!(
                        "C-01: {name} is an accent button filled {hex} — the primary \
                         action is Bondi {BONDI} ($button/filled/accent/…)"
                    ),
                ));
            }
        }
    }

    // K-02 — the input control's own size. Checked by name, whatever the node
    // kind is: a 38px field is wrong whether it was instantiated or drawn.
    if lower.contains("input") && !lower.contains("icon") {
        if let Some(height) = numeric_height(node) {
            if (height - CONTROL_HEIGHT).abs() > 0.5 {
                out.push(violation(
                    node_id(node).to_string(),
                    FixProperty::Height,
                    serde_json::json!(height),
                    format!(
                        "K-02: {name} is {height:.0}px tall — kit inputs are \
                         {CONTROL_HEIGHT:.0}px (236×32, radius 8)"
                    ),
                ));
            }
        }
    }

    // L-03 — the sidebar's width.
    if lower.contains("sidebar") && !children(node).is_empty() {
        if let Some(width) = numeric_width(node) {
            if (width - SIDEBAR_WIDTH).abs() > 0.5 {
                out.push(violation(
                    node_id(node).to_string(),
                    FixProperty::Remove,
                    serde_json::json!(width),
                    format!(
                        "L-03: {name} is {width:.0}px wide — the kit sidebar is \
                         {SIDEBAR_WIDTH:.0}px (not 240–280)"
                    ),
                ));
            }
        }
    }

    // G-01 — a control imitated by hand instead of instantiated.
    if !is_ref
        && matches!(node_kind(node), NodeKind::Frame | NodeKind::Group)
        && CONTROL_PATTERNS
            .iter()
            .any(|pattern| lower.starts_with(pattern))
    {
        out.push(violation(
            node_id(node).to_string(),
            FixProperty::Remove,
            serde_json::json!(name),
            format!(
                "G-01: {name} is a kit control drawn as a {} — controls are instantiated \
                 as type:\"ref\" with descendants, not rebuilt as frames",
                crate::node_util::node_kind_str(node)
            ),
        ));
    }

    for child in children(node) {
        walk(child, out);
    }
}

#[cfg(test)]
#[path = "design_rules_tests.rs"]
mod tests;
