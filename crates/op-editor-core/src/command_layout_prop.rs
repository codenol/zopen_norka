//! `SetNodeLayoutProp` command application — sets layout / text
//! properties that do not have their own typed `EditorCommand` variants.
//!
//! Covers: `layout`, `gap`, `padding`, `letterSpacing`, `lineHeight`,
//! `opacity`, `x`, `y`, `fontFamily`, `textAlign`, `textAlignVertical`,
//! `textGrowth`, `alignItems`, `justifyContent`, `clipContent`, and
//! sizing keywords for `width` / `height` (`"fit_content"` /
//! `"fill_container"`).
//!
//! Each property dispatches to the canonical node field via a
//! per-variant match, using the validate-then-mutate discipline.

use crate::command::LayoutPropValue;
use crate::node_id::NodeId;
use crate::pen_node_ext::PenNodeExt;
use crate::state::EditorState;
use crate::walkers::find_node_mut;
use jian_ops_schema::node::base::NumberOrExpression;
use jian_ops_schema::node::container::{AlignItems, JustifyContent, LayoutMode, Padding};
use jian_ops_schema::node::text::{TextAlign, TextAlignVertical, TextGrowth};
use jian_ops_schema::node::PenNode;
use jian_ops_schema::sizing::{SizingBehavior, SizingKeyword};

// ── helper: parse justify_content keyword ────────────────────────────────────

fn parse_justify_content(s: &str) -> Option<JustifyContent> {
    match s {
        "start" => Some(JustifyContent::Start),
        "center" => Some(JustifyContent::Center),
        "end" => Some(JustifyContent::End),
        "space_between" => Some(JustifyContent::SpaceBetween),
        "space_around" => Some(JustifyContent::SpaceAround),
        _ => None,
    }
}

// ── helper: parse align_items keyword ────────────────────────────────────────

// Accepts the full CSS-ish alias set TS tolerates (incl. `stretch`,
// `flex-start`, `baseline`, …) via the canonical schema's `from_css`, so a
// set-layout command carrying any value a TS builder/file emits is honored
// rather than silently rejected.
fn parse_align_items(s: &str) -> Option<AlignItems> {
    AlignItems::from_css(s)
}

// ── helper: parse layout keyword ─────────────────────────────────────────────

fn parse_layout_mode(s: &str) -> Option<LayoutMode> {
    match s {
        "none" => Some(LayoutMode::None),
        "vertical" => Some(LayoutMode::Vertical),
        "horizontal" => Some(LayoutMode::Horizontal),
        _ => None,
    }
}

// ── helper: parse text_align keyword ─────────────────────────────────────────

fn parse_text_align(s: &str) -> Option<TextAlign> {
    match s {
        "left" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        "justify" => Some(TextAlign::Justify),
        _ => None,
    }
}

// ── helper: parse text_align_vertical keyword ────────────────────────────────

fn parse_text_align_vertical(s: &str) -> Option<TextAlignVertical> {
    match s {
        "top" => Some(TextAlignVertical::Top),
        "middle" => Some(TextAlignVertical::Middle),
        "bottom" => Some(TextAlignVertical::Bottom),
        _ => None,
    }
}

// ── helper: parse text_growth keyword ────────────────────────────────────────

fn parse_text_growth(s: &str) -> Option<TextGrowth> {
    match s {
        "auto" => Some(TextGrowth::Auto),
        "fixed-width" => Some(TextGrowth::FixedWidth),
        "fixed-width-height" => Some(TextGrowth::FixedWidthHeight),
        _ => None,
    }
}

// ── helper: parse sizing keyword ──────────────────────────────────────────────

fn parse_sizing_keyword(s: &str) -> Option<SizingBehavior> {
    match s {
        "fit_content" => Some(SizingBehavior::Keyword(SizingKeyword::FitContent)),
        "fill_container" => Some(SizingBehavior::Keyword(SizingKeyword::FillContainer)),
        _ => None,
    }
}

// ── helper: build Padding from LayoutPropValue ────────────────────────────────

fn build_padding(value: &LayoutPropValue) -> Option<Padding> {
    match value {
        LayoutPropValue::Number(n) => {
            if n.is_finite() {
                Some(Padding::Uniform(*n))
            } else {
                None
            }
        }
        LayoutPropValue::NumberArray(arr) => match arr.len() {
            1 => Some(Padding::Uniform(arr[0])),
            2 => Some(Padding::XY([arr[0], arr[1]])),
            4 => Some(Padding::LtrB([arr[0], arr[1], arr[2], arr[3]])),
            _ => None,
        },
        LayoutPropValue::Keyword(_) | LayoutPropValue::Bool(_) => None,
    }
}

// ── impl EditorState ──────────────────────────────────────────────────────────

impl EditorState {
    /// `SetNodeLayoutProp` — set a layout / text property on a node by
    /// name + typed value. Returns `true` when the field was written,
    /// `false` on an unknown (property, value shape) combination or a
    /// missing node.
    pub(crate) fn cmd_set_node_layout_prop(
        &mut self,
        node_id: &NodeId,
        property: &str,
        value: &LayoutPropValue,
    ) -> bool {
        if !node_id.is_real() {
            return false;
        }

        // Read-validate before taking the mutable borrow.
        match property {
            "gap" | "letterSpacing" | "lineHeight" | "opacity" | "x" | "y" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                if !n.is_finite() {
                    return false;
                }
            }
            "padding" => {
                if build_padding(value).is_none() {
                    return false;
                }
            }
            "layout" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_layout_mode(s).is_none() {
                    return false;
                }
            }
            "justifyContent" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_justify_content(s).is_none() {
                    return false;
                }
            }
            "alignItems" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_align_items(s).is_none() {
                    return false;
                }
            }
            "textAlign" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_text_align(s).is_none() {
                    return false;
                }
            }
            "textAlignVertical" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_text_align_vertical(s).is_none() {
                    return false;
                }
            }
            "fontFamily" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if s.trim().is_empty() {
                    return false;
                }
            }
            "textGrowth" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_text_growth(s).is_none() {
                    return false;
                }
            }
            "width" | "height" => {
                // Sizing keyword variant; numeric pixel writes go through
                // the existing UpdateNode command.
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                if parse_sizing_keyword(s).is_none() {
                    return false;
                }
            }
            "clipContent" => {
                if !matches!(value, LayoutPropValue::Bool(_)) {
                    return false;
                }
            }
            _ => return false,
        }

        // Mutable phase.
        let Some(node) = find_node_mut(self.active_children_mut(), node_id) else {
            return false;
        };

        let changed = match property {
            "gap" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                set_container_gap(node, *n)
            }
            "padding" => {
                let pad = build_padding(value).expect("validated above");
                set_container_padding(node, pad)
            }
            "layout" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let mode = parse_layout_mode(s).expect("validated above");
                set_container_layout(node, mode)
            }
            "justifyContent" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let jc = parse_justify_content(s).expect("validated above");
                set_container_justify(node, jc)
            }
            "alignItems" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let ai = parse_align_items(s).expect("validated above");
                set_container_align(node, ai)
            }
            "letterSpacing" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                set_text_letter_spacing(node, *n)
            }
            "lineHeight" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                set_text_line_height(node, *n)
            }
            "textAlign" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let ta = parse_text_align(s).expect("validated above");
                set_text_align(node, ta)
            }
            "textAlignVertical" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let ta = parse_text_align_vertical(s).expect("validated above");
                set_text_align_vertical(node, ta)
            }
            "fontFamily" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                set_text_font_family(node, s.trim().to_string())
            }
            "textGrowth" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let tg = parse_text_growth(s).expect("validated above");
                set_text_growth(node, tg)
            }
            "opacity" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                set_slot(&mut node.base_mut().opacity, NumberOrExpression::Number(*n))
            }
            "x" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                set_slot(&mut node.base_mut().x, *n)
            }
            "y" => {
                let LayoutPropValue::Number(n) = value else {
                    return false;
                };
                set_slot(&mut node.base_mut().y, *n)
            }
            "width" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let sb = parse_sizing_keyword(s).expect("validated above");
                set_node_width(node, sb)
            }
            "height" => {
                let LayoutPropValue::Keyword(s) = value else {
                    return false;
                };
                let sb = parse_sizing_keyword(s).expect("validated above");
                set_node_height(node, sb)
            }
            "clipContent" => {
                let LayoutPropValue::Bool(v) = value else {
                    return false;
                };
                set_container_clip_content(node, *v)
            }
            _ => false,
        };
        if changed && property_invalidates_preserved_geometry(property) {
            self.editor_ui.preserve_authored_geometry = false;
        }
        changed
    }
}

// ── field writers ─────────────────────────────────────────────────────────────

fn property_invalidates_preserved_geometry(property: &str) -> bool {
    matches!(
        property,
        "gap"
            | "padding"
            | "layout"
            | "justifyContent"
            | "alignItems"
            | "width"
            | "height"
            | "clipContent"
            | "textGrowth"
            | "x"
            | "y"
    )
}

fn set_container_gap(node: &mut PenNode, gap: f64) -> bool {
    let noe = NumberOrExpression::Number(gap);
    match node {
        PenNode::Frame(n) => set_slot(&mut n.container.gap, noe),
        PenNode::Group(n) => set_slot(&mut n.container.gap, noe),
        PenNode::Rectangle(n) => set_slot(&mut n.container.gap, noe),
        _ => false,
    }
}

fn set_container_layout(node: &mut PenNode, mode: LayoutMode) -> bool {
    match node {
        PenNode::Frame(n) => set_slot(&mut n.container.layout, mode),
        PenNode::Group(n) => set_slot(&mut n.container.layout, mode),
        PenNode::Rectangle(n) => set_slot(&mut n.container.layout, mode),
        _ => false,
    }
}

fn set_container_padding(node: &mut PenNode, pad: Padding) -> bool {
    match node {
        PenNode::Frame(n) => set_slot(&mut n.container.padding, pad),
        PenNode::Group(n) => set_slot(&mut n.container.padding, pad),
        PenNode::Rectangle(n) => set_slot(&mut n.container.padding, pad),
        _ => false,
    }
}

fn set_container_clip_content(node: &mut PenNode, value: bool) -> bool {
    match node {
        PenNode::Frame(n) => set_slot(&mut n.container.clip_content, value),
        PenNode::Group(n) => set_slot(&mut n.container.clip_content, value),
        PenNode::Rectangle(n) => set_slot(&mut n.container.clip_content, value),
        _ => false,
    }
}

fn set_container_justify(node: &mut PenNode, jc: JustifyContent) -> bool {
    match node {
        PenNode::Frame(n) => set_slot(&mut n.container.justify_content, jc),
        PenNode::Group(n) => set_slot(&mut n.container.justify_content, jc),
        PenNode::Rectangle(n) => set_slot(&mut n.container.justify_content, jc),
        _ => false,
    }
}

fn set_container_align(node: &mut PenNode, ai: AlignItems) -> bool {
    match node {
        PenNode::Frame(n) => set_slot(&mut n.container.align_items, ai),
        PenNode::Group(n) => set_slot(&mut n.container.align_items, ai),
        PenNode::Rectangle(n) => set_slot(&mut n.container.align_items, ai),
        _ => false,
    }
}

fn set_text_letter_spacing(node: &mut PenNode, v: f64) -> bool {
    match node {
        PenNode::Text(t) => set_slot(&mut t.letter_spacing, v),
        _ => false,
    }
}

fn set_text_line_height(node: &mut PenNode, v: f64) -> bool {
    match node {
        PenNode::Text(t) => set_slot(&mut t.line_height, v),
        _ => false,
    }
}

fn set_text_align(node: &mut PenNode, ta: TextAlign) -> bool {
    match node {
        PenNode::Text(t) => set_slot(&mut t.text_align, ta),
        _ => false,
    }
}

fn set_text_align_vertical(node: &mut PenNode, ta: TextAlignVertical) -> bool {
    match node {
        PenNode::Text(t) => set_slot(&mut t.text_align_vertical, ta),
        _ => false,
    }
}

fn set_text_font_family(node: &mut PenNode, family: String) -> bool {
    match node {
        PenNode::Text(t) => set_slot(&mut t.font_family, family),
        _ => false,
    }
}

fn set_text_growth(node: &mut PenNode, tg: TextGrowth) -> bool {
    match node {
        PenNode::Text(t) => set_slot(&mut t.text_growth, tg),
        _ => false,
    }
}

/// Set a slot, answering whether anything CHANGED.
///
/// Re-applying the value the document already holds is not an edit. The
/// validation pass re-sends its fixes, and a fix that changed nothing still
/// counted as a change — a content-revision bump, a version bump, and a full
/// document refetch in every open tab (issue #230; measured: version
/// v167 -> v169 while the serialised document stayed byte-identical).
fn set_slot<T: PartialEq>(slot: &mut Option<T>, value: T) -> bool {
    if slot.as_ref() == Some(&value) {
        return false;
    }
    *slot = Some(value);
    true
}

/// [`set_slot`] for `width`/`height`: the container kinds keep the axis on
/// their container, every other kind keeps it on the node itself.
macro_rules! set_size_slot {
    ($node:expr, $value:expr, $field:ident) => {
        match $node {
            PenNode::Frame(n) => set_slot(&mut n.container.$field, $value),
            PenNode::Group(n) => set_slot(&mut n.container.$field, $value),
            PenNode::Rectangle(n) => set_slot(&mut n.container.$field, $value),
            PenNode::Ellipse(n) => set_slot(&mut n.$field, $value),
            PenNode::Polygon(n) => set_slot(&mut n.$field, $value),
            PenNode::Path(n) => set_slot(&mut n.$field, $value),
            PenNode::Text(n) => set_slot(&mut n.$field, $value),
            PenNode::TextInput(n) => set_slot(&mut n.$field, $value),
            PenNode::Image(n) => set_slot(&mut n.$field, $value),
            PenNode::IconFont(n) => set_slot(&mut n.$field, $value),
            _ => false,
        }
    };
}

fn set_node_width(node: &mut PenNode, sb: SizingBehavior) -> bool {
    set_size_slot!(node, sb, width)
}

fn set_node_height(node: &mut PenNode, sb: SizingBehavior) -> bool {
    set_size_slot!(node, sb, height)
}
