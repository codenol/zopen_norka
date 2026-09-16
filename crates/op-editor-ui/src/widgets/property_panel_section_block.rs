//! The Section block: what a section was built from, and what it says (#59).
//!
//! Painted at the top of the property panel when the selection is a section,
//! above every ordinary section of the inspector — Position, Flex, Size — and
//! for a reason rather than by accident: what a section was built from is the
//! first question a reader of somebody else's design asks, and it is the one
//! thing the rest of the panel cannot answer.
//!
//! | Row | Answers |
//! | --- | --- |
//! | Built from | which document this section came from, and whether that document still hashes to what the link recorded |
//! | The summary | what this is, where to look, use cases, what to check |
//! | The flows | the paths drawn from the analytics, with their step counts |
//!
//! ## Why the empty states are three sentences and not one
//!
//! "Nobody has written anything about this section", "we have not read it yet"
//! and "the read failed" are three different facts, and a block that painted the
//! same nothing for all three would tell a designer their summary had been
//! erased when nobody had touched it. [`SectionPanelState`] carries the
//! difference; this file spends the sentences.
//!
//! ## Why the rows are planned before they are painted
//!
//! Paint and the content-height walker have to agree about how tall the block
//! is, or the inspector scrolls short and its last section can never be reached.
//! Every other section of this panel keeps a height function beside its paint
//! function and the two are held in step by hand. Here the block is a list of
//! rows with their heights ([`rows`]), so the height IS the sum of a list the
//! paint pass then walks — the two cannot drift.

use op_editor_core::editor_ui_state::section_panel::{SectionPanelState, SummaryField};
use op_editor_core::section::{LinkState, MovedSide};
use op_i18n::Locale;

use crate::theme::Theme;
use crate::widgets::property_panel_inputs::{
    paint_section_label, INPUT_HEIGHT, INPUT_RADIUS, PAD_X, SECTION_GAP, SECTION_HEADER_HEIGHT,
};
use crate::widgets::{text_metrics, PaintCx};
use crate::Rect;
use crate::{Color, Point2D, TextLayout};

/// Section-header size, matching the panel's own headers.
const LABEL_SIZE: f32 = 12.0;
/// Field names and secondary lines.
const CAPTION_SIZE: f32 = 10.5;
/// A line that is only a caption ("What this section is", "Flows").
const CAPTION_ROW: f32 = 18.0;
/// A name with a caption above it, or a value with a caption under it.
const NAMED_ROW: f32 = 28.0;
/// One answered question: its name, then what it says.
const FIELD_ROW: f32 = 30.0;
/// How many flows are listed before the rest are counted instead.
const MAX_FLOW_ROWS: usize = 3;

/// One line of the block. Owned strings: the rows outlive the borrow of the
/// state they were read from, and a plan that borrowed twice would be two
/// places to get the same row wrong.
enum Row {
    /// The block's own title.
    Title,
    /// A caption in the panel's muted tone.
    Caption(&'static str),
    /// A sentence in the muted tone, full width.
    Note(&'static str),
    /// The analytics document's name, and how its link stands.
    Analytics {
        name: String,
        state_key: Option<&'static str>,
    },
    /// The control that attaches one.
    Attach,
    /// One answered question, and which one it is.
    Field {
        field: SummaryField,
        key: &'static str,
        value: String,
    },
    /// One flow, with how many steps it has.
    Flow { name: String, steps: usize },
    /// "+N" for the flows the list did not show.
    Overflow { count: usize },
}

/// The block's rows and their heights, in the order they paint.
fn rows(state: &SectionPanelState) -> Vec<(Row, f32)> {
    let mut rows = vec![(Row::Title, SECTION_HEADER_HEIGHT)];

    // "Built from" is always here: "nothing is attached" is an answer a reader
    // needs, not an absence to hide.
    rows.push((Row::Caption("section.analytics"), CAPTION_ROW));
    match analytics_line(state) {
        Some((name, state_key)) => {
            let height = if state_key.is_none() {
                NAMED_ROW - CAPTION_ROW
            } else {
                NAMED_ROW
            };
            rows.push((Row::Analytics { name, state_key }, height));
        }
        // Nothing attached: the caption already says so, and a value line under
        // it would be the same sentence twice.
        None => rows.push((
            Row::Analytics {
                name: String::new(),
                state_key: None,
            },
            NAMED_ROW - CAPTION_ROW,
        )),
    }

    // The way to attach an analytics document, whether or not one is attached:
    // a section may be built from more than one, and replacing a link is the
    // same gesture as making it.
    rows.push((Row::Attach, CAPTION_ROW));

    // A section nobody has read says so and stops. Four empty fields under a
    // sentence that means "unknown" would be a second, quieter claim that they
    // are empty.
    if !state.read {
        let key = if state.failed {
            "section.readFailed"
        } else {
            "section.notRead"
        };
        rows.push((Row::Note(key), CAPTION_ROW));
        return rows;
    }

    rows.push((Row::Caption("section.summary"), CAPTION_ROW));
    let fields = summary_lines(state);
    if fields.is_empty() {
        rows.push((Row::Note("section.summary.empty"), CAPTION_ROW));
    } else {
        for (field, value) in fields {
            rows.push((
                Row::Field {
                    field,
                    key: field.i18n_key(),
                    value: value.to_string(),
                },
                FIELD_ROW,
            ));
        }
    }

    rows.push((Row::Caption("section.flows"), CAPTION_ROW));
    if state.properties.flows.is_empty() {
        rows.push((Row::Note("section.flows.none"), CAPTION_ROW));
    } else {
        for flow in state.properties.flows.iter().take(MAX_FLOW_ROWS) {
            rows.push((
                Row::Flow {
                    name: flow.name.clone(),
                    steps: flow.steps.len(),
                },
                NAMED_ROW,
            ));
        }
        let hidden = state.properties.flows.len().saturating_sub(MAX_FLOW_ROWS);
        if hidden > 0 {
            rows.push((Row::Overflow { count: hidden }, CAPTION_ROW));
        }
    }

    rows
}

/// The questions, with the rect each one is painted in.
///
/// Hit-test and paint walk the same `rows`, so a question that is painted is a
/// question that can be clicked — the rule the rest of this panel follows.
/// Rectangles are panel-absolute, ready to compare with a pointer.
pub fn section_field_rects(
    state: &SectionPanelState,
    x0: f32,
    y: f32,
    w: f32,
) -> Vec<(SummaryField, Rect)> {
    if !state.is_visible() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut top = y;
    for (row, height) in rows(state) {
        if let Row::Field { field, .. } = row {
            out.push((
                field,
                Rect::xywh(x0 + PAD_X, top + 2.0, w - PAD_X * 2.0, FIELD_ROW - 4.0),
            ));
        }
        top += height;
    }
    out
}

/// The rect the attach control occupies, when the block paints.
pub fn section_attach_rect(state: &SectionPanelState, x0: f32, y: f32, w: f32) -> Option<Rect> {
    if !state.is_visible() {
        return None;
    }
    let mut top = y;
    for (row, height) in rows(state) {
        if matches!(row, Row::Attach) {
            return Some(Rect::xywh(x0 + PAD_X, top, w - PAD_X * 2.0, height));
        }
        top += height;
    }
    None
}

/// How tall the block is, for the panel's content-height walker.
pub fn section_block_height(state: &SectionPanelState) -> f32 {
    if !state.is_visible() {
        return 0.0;
    }
    rows(state).iter().map(|(_, height)| height).sum::<f32>() + SECTION_GAP
}

/// Paint the block at `(x0, y)` inside a panel `w` wide.
///
/// Returns the y just below it, like every other section painter in this panel.
pub fn paint_section_block(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    locale: Locale,
    state: &SectionPanelState,
    x0: f32,
    y: f32,
    w: f32,
) -> f32 {
    if !state.is_visible() {
        return y;
    }
    let mut top = y;
    for (row, height) in rows(state) {
        match row {
            Row::Title => {
                paint_section_label(cx, theme, t(locale, "section.title"), x0, top, w);
            }
            Row::Caption(key) => {
                line(
                    cx,
                    t(locale, key),
                    CAPTION_SIZE,
                    theme.muted_foreground,
                    x0,
                    top,
                    w,
                    12.0,
                );
            }
            Row::Note(key) => {
                line(
                    cx,
                    t(locale, key),
                    LABEL_SIZE,
                    theme.muted_foreground,
                    x0,
                    top,
                    w,
                    12.0,
                );
            }
            Row::Analytics { name, state_key } => {
                if !name.is_empty() {
                    let color = match state_key {
                        // A link that no longer matches what it was made from is
                        // the one thing here that is a claim quietly becoming
                        // false, so it is worth the warning tone.
                        Some(key) if key != "section.state.inSync" => theme.destructive,
                        _ => theme.foreground,
                    };
                    line(cx, &name, LABEL_SIZE, color, x0, top, w, 12.0);
                    if let Some(key) = state_key {
                        line(
                            cx,
                            t(locale, key),
                            CAPTION_SIZE,
                            theme.muted_foreground,
                            x0,
                            top,
                            w,
                            26.0,
                        );
                    }
                }
            }
            Row::Attach => {
                // Muted while a load is in flight rather than hidden: a control
                // that disappears when pressed leaves the person wondering
                // whether anything happened at all.
                let color = if state.attaching {
                    theme.muted_foreground
                } else {
                    theme.primary
                };
                line(
                    cx,
                    t(locale, "section.analytics.attach"),
                    LABEL_SIZE,
                    color,
                    x0,
                    top,
                    w,
                    12.0,
                );
            }
            Row::Field { field, key, value } => {
                line(
                    cx,
                    t(locale, key),
                    CAPTION_SIZE,
                    theme.muted_foreground,
                    x0,
                    top,
                    w,
                    10.0,
                );
                // The focused question paints what is being typed rather than
                // what is stored: the two are the same until somebody edits,
                // and after that the draft is the truth the person can see.
                let focused = state.focus == Some(field);
                let shown = if focused {
                    state.draft.text()
                } else {
                    value.as_str()
                };
                if focused {
                    let rect = Rect::xywh(x0 + PAD_X, top + 2.0, w - PAD_X * 2.0, FIELD_ROW - 4.0);
                    cx.backend
                        .fill_round_rect(rect, INPUT_RADIUS, theme.background);
                    cx.backend
                        .stroke_round_rect(rect, INPUT_RADIUS, theme.primary, 1.0);
                    line_in(
                        cx,
                        shown,
                        LABEL_SIZE,
                        theme.foreground,
                        rect.origin.x + 8.0,
                        rect.origin.y,
                        rect.size.x - 16.0,
                        (INPUT_HEIGHT - 8.0) / 2.0,
                    );
                } else {
                    line(cx, shown, LABEL_SIZE, theme.foreground, x0, top, w, 24.0);
                }
            }
            Row::Flow { name, steps } => {
                // A flow row is its name and how many steps it has: a list of
                // names alone cannot say whether the flow was ever finished.
                line(cx, &name, LABEL_SIZE, theme.foreground, x0, top, w, 12.0);
                let count =
                    t(locale, "section.flows.stepCount").replace("{{count}}", &steps.to_string());
                line(
                    cx,
                    &count,
                    CAPTION_SIZE,
                    theme.muted_foreground,
                    x0,
                    top,
                    w,
                    25.0,
                );
            }
            Row::Overflow { count } => {
                line(
                    cx,
                    &format!("+{count}"),
                    CAPTION_SIZE,
                    theme.muted_foreground,
                    x0,
                    top,
                    w,
                    12.0,
                );
            }
        }
        top += height;
    }
    y + section_block_height(state)
}

/// The analytics line: the document's name and the state of its link, or `None`
/// when nothing is attached.
fn analytics_line(state: &SectionPanelState) -> Option<(String, Option<&'static str>)> {
    let attached = state.links.first()?;
    let name = if state.links.len() > 1 {
        format!("{} +{}", attached.link.name, state.links.len() - 1)
    } else {
        attached.link.name.clone()
    };
    let key = match attached.state {
        LinkState::InSync => Some("section.state.inSync"),
        LinkState::Broken { side } => Some(match side {
            MovedSide::Analytics => "section.state.analyticsMoved",
            MovedSide::Mockups => "section.state.mockupsMoved",
            // Both sides moved: saying only one of them would send a reader to
            // the wrong half of the section to look for the difference.
            MovedSide::Both => "section.state.bothMoved",
        }),
        LinkState::AssetMissing => Some("section.state.assetMissing"),
        // The document is there and the reader may not open it: saying "gone"
        // here would state something untrue about somebody else's file, and
        // nothing the reader could do (a restore, a re-link) would help them.
        LinkState::NotReadable => Some("section.state.notReadable"),
        // The read did not complete, so nothing is known about the document.
        // "Gone" would be a deletion nobody confirmed (issue #145) and
        // "changed since" a drift nobody looked for; the sentence says which
        // of the three this is not.
        LinkState::CheckFailed => Some("section.state.checkFailed"),
        // A link with nothing behind it is the caption's business, not a state
        // of a document's name.
        LinkState::NoAnalytics => None,
    };
    Some((name, key))
}

/// The summary's answered questions, as (label key, text) pairs.
///
/// Unanswered ones are left out rather than painted with a dash: the four
/// questions are answered in whatever order somebody had answers, and a column
/// of dashes would bury the two lines that were written.
fn summary_lines(state: &SectionPanelState) -> Vec<(SummaryField, &str)> {
    let summary = &state.properties.summary;
    SummaryField::ALL
        .into_iter()
        .map(|field| (field, field.read(summary)))
        .filter(|(_, value)| !value.trim().is_empty())
        .collect()
}

fn t(locale: Locale, key: &'static str) -> &'static str {
    op_i18n::translate(locale, key)
}

/// One line of text, fitted to the panel's width.
fn line(
    cx: &mut PaintCx<'_>,
    label: &str,
    size: f32,
    color: Color,
    x0: f32,
    top: f32,
    w: f32,
    offset: f32,
) {
    if label.is_empty() {
        return;
    }
    let fitted = text_metrics::fit_chrome(cx.backend, label, w - PAD_X * 2.0, size);
    let layout = TextLayout::single_run(
        &fitted,
        "system-ui",
        size,
        color.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend
        .draw_text(&layout, Point2D::new(x0 + PAD_X, top + offset));
}

/// A line drawn inside a rect the caller has already measured — what the
/// focused question shows instead of the stored answer.
fn line_in(
    cx: &mut PaintCx<'_>,
    label: &str,
    size: f32,
    color: Color,
    x: f32,
    top: f32,
    width: f32,
    offset: f32,
) {
    let fitted = text_metrics::fit_chrome(cx.backend, label, width, size);
    let layout = TextLayout::single_run(
        &fitted,
        "system-ui",
        size,
        color.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(&layout, Point2D::new(x, top + offset));
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::editor_ui_state::section_panel::SectionLink;
    use op_editor_core::section::{SectionDigest, SectionProperties, SectionSummary, UxFlow};
    use op_editor_core::NodeId;

    fn panel_with(properties: SectionProperties, links: Vec<SectionLink>) -> SectionPanelState {
        let node = NodeId::new("s1");
        let mut state = SectionPanelState::default();
        state.select(Some(node.clone()));
        state.apply(&node, properties, links);
        state
    }

    fn attached(state: LinkState) -> SectionLink {
        SectionLink {
            link: op_editor_core::section::AnalyticsLink::new(
                "abc",
                "Checkout analytics",
                SectionDigest::of_text("then"),
                SectionDigest::of_text("screens"),
                1,
                None,
            ),
            state,
        }
    }

    #[test]
    fn a_selection_that_is_not_a_section_paints_nothing() {
        assert_eq!(
            section_block_height(&SectionPanelState::default()),
            0.0,
            "a block with no height is a block that shifts nothing below it"
        );
    }

    #[test]
    fn a_section_nobody_has_read_is_shorter_than_one_with_a_summary() {
        let node = NodeId::new("s1");
        let mut unread = SectionPanelState::default();
        unread.select(Some(node.clone()));

        let read = panel_with(
            SectionProperties {
                summary: SectionSummary {
                    what_it_is: "Checkout".to_string(),
                    what_to_check: "The total matches".to_string(),
                    ..SectionSummary::default()
                },
                ..SectionProperties::empty()
            },
            Vec::new(),
        );

        assert!(section_block_height(&unread) > 0.0);
        assert!(
            section_block_height(&read) > section_block_height(&unread),
            "the read block has the fields the unread one deliberately does not"
        );
    }

    #[test]
    fn each_flow_row_makes_the_block_taller_until_the_list_is_capped() {
        let flow = |name: &str| UxFlow::new(name, name);
        let with_flows = |count: usize| {
            panel_with(
                SectionProperties {
                    flows: (0..count)
                        .map(|index| flow(&format!("flow {index}")))
                        .collect(),
                    ..SectionProperties::empty()
                },
                Vec::new(),
            )
        };

        let one = section_block_height(&with_flows(1));
        let three = section_block_height(&with_flows(3));
        let five = section_block_height(&with_flows(5));

        assert!(three > one);
        assert!(
            five > three,
            "the hidden flows are counted, so the reader is told there are more"
        );
        assert!(
            five - three < three - one,
            "but not with a row each: the list stays three rows and a count"
        );
    }

    #[test]
    fn a_broken_link_is_the_only_state_that_is_not_a_sync() {
        let node = NodeId::new("s1");
        let mut state = SectionPanelState::default();
        state.select(Some(node.clone()));
        state.apply(
            &node,
            SectionProperties::empty(),
            vec![attached(LinkState::Broken {
                side: MovedSide::Both,
            })],
        );

        let (name, key) = analytics_line(&state).expect("a link");
        assert_eq!(name, "Checkout analytics");
        assert_eq!(key, Some("section.state.bothMoved"));
    }

    #[test]
    fn a_reader_who_may_not_open_the_analytics_is_told_that_and_not_that_it_is_gone() {
        // Issue #110. The two facts leave the reader equally empty-handed and
        // are not the same: one is about somebody else's file, the other about
        // what this reader is allowed to open. The panel says which.
        let node = NodeId::new("s1");
        let line_for = |state| {
            let mut panel = SectionPanelState::default();
            panel.select(Some(node.clone()));
            panel.apply(&node, SectionProperties::empty(), vec![attached(state)]);
            analytics_line(&panel).expect("a link")
        };

        let (name, key) = line_for(LinkState::NotReadable);
        assert_eq!(name, "Checkout analytics");
        assert_eq!(key, Some("section.state.notReadable"));
        assert_ne!(
            key,
            line_for(LinkState::AssetMissing).1,
            "a refusal must not be painted as a deletion"
        );
    }

    #[test]
    fn a_check_that_did_not_complete_is_told_apart_from_a_deletion() {
        // Issue #145. A 5xx leaves the panel as empty-handed as a 404 does and
        // means something else entirely: nobody confirmed that anything was
        // deleted. The three sentences have to be three.
        let node = NodeId::new("s1");
        let line_for = |state| {
            let mut panel = SectionPanelState::default();
            panel.select(Some(node.clone()));
            panel.apply(&node, SectionProperties::empty(), vec![attached(state)]);
            analytics_line(&panel).expect("a link")
        };

        let (name, key) = line_for(LinkState::CheckFailed);
        assert_eq!(name, "Checkout analytics");
        assert_eq!(key, Some("section.state.checkFailed"));
        assert_ne!(
            key,
            line_for(LinkState::AssetMissing).1,
            "a failed read must not be painted as a deletion"
        );
        assert_ne!(key, line_for(LinkState::NotReadable).1, "nor as a refusal");
        assert_ne!(
            key,
            line_for(LinkState::InSync).1,
            "nor as a link that is fine"
        );
    }

    #[test]
    fn several_attached_documents_are_counted_in_the_line() {
        let node = NodeId::new("s1");
        let mut state = SectionPanelState::default();
        state.select(Some(node.clone()));
        state.apply(
            &node,
            SectionProperties::empty(),
            vec![
                attached(LinkState::InSync),
                SectionLink {
                    link: op_editor_core::section::AnalyticsLink::new(
                        "def",
                        "Second",
                        SectionDigest::of_text("a"),
                        SectionDigest::of_text("b"),
                        2,
                        None,
                    ),
                    state: LinkState::InSync,
                },
            ],
        );

        let (name, _) = analytics_line(&state).expect("links");
        assert_eq!(
            name, "Checkout analytics +1",
            "a reader is told there is more than one, not left to assume"
        );
    }

    #[test]
    fn the_summary_skips_the_questions_nobody_answered() {
        let state = panel_with(
            SectionProperties {
                summary: SectionSummary {
                    where_to_look: "The basket screen".to_string(),
                    ..SectionSummary::default()
                },
                ..SectionProperties::empty()
            },
            Vec::new(),
        );

        let lines = summary_lines(&state);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, SummaryField::WhereToLook);
    }
}

#[cfg(test)]
mod hit_tests {
    use super::*;
    use crate::widgets::property_panel::PropertyPanel;
    use crate::widgets::property_panel_inputs::{HEADER_HEIGHT, TAB_HEIGHT};
    use crate::Rect;
    use op_editor_core::editor_ui_state::section_panel::SummaryField;
    use op_editor_core::section::{SectionProperties, SectionSummary};
    use op_editor_core::{EditorState, NodeId};

    const VIEWPORT: (f32, f32) = (1440.0, 900.0);
    const PANEL_WIDTH: f32 = 280.0;

    /// A panel whose selection is a section carrying one answered question.
    fn panel_with_a_section() -> (PropertyPanel, Rect) {
        let mut state = EditorState::sample();
        let node = state.selection.anchor.clone();
        state.editor_ui.section_panel.select(Some(node.clone()));
        state.editor_ui.section_panel.apply(
            &node,
            SectionProperties {
                summary: SectionSummary {
                    what_it_is: "Checkout".to_string(),
                    ..SectionSummary::default()
                },
                ..SectionProperties::empty()
            },
            Vec::new(),
        );
        let panel_rect = Rect::xywh(
            VIEWPORT.0 - PANEL_WIDTH,
            crate::widgets::TOP_BAR_HEIGHT,
            PANEL_WIDTH,
            VIEWPORT.1 - crate::widgets::TOP_BAR_HEIGHT,
        );
        let panel = PropertyPanel::for_selection_at(&state, 0).expect("a selected section");
        (panel, panel_rect)
    }

    #[test]
    fn the_block_sits_directly_under_the_node_header() {
        let (panel, panel_rect) = panel_with_a_section();

        let block = panel.section_block_rect(panel_rect).expect("a block");

        assert_eq!(
            block.origin.y,
            panel_rect.origin.y + TAB_HEIGHT + HEADER_HEIGHT,
            "the same place paint puts it, with nothing scrolled"
        );
        assert_eq!(block.size.y, panel.section_block_height);
        assert!(block.size.y > 0.0);
    }

    #[test]
    fn a_question_is_clickable_where_it_is_painted() {
        let (panel, panel_rect) = panel_with_a_section();
        let block = panel.section_block_rect(panel_rect).expect("a block");

        let fields = section_field_rects(
            &panel.section_panel,
            block.origin.x,
            block.origin.y,
            block.size.x,
        );

        assert_eq!(fields.len(), 1, "one answered question, one rect");
        assert_eq!(fields[0].0, SummaryField::WhatItIs);
        let inside = Point2D::new(fields[0].1.origin.x + 4.0, fields[0].1.origin.y + 4.0);
        assert!(block.contains(inside), "the field is inside the block");
        assert!(fields[0].1.contains(inside));
    }

    #[test]
    fn a_selection_that_is_not_a_section_has_no_block() {
        let state = EditorState::sample();
        let panel_rect = Rect::xywh(
            VIEWPORT.0 - PANEL_WIDTH,
            crate::widgets::TOP_BAR_HEIGHT,
            PANEL_WIDTH,
            VIEWPORT.1 - crate::widgets::TOP_BAR_HEIGHT,
        );
        let panel = PropertyPanel::for_selection_at(&state, 0).expect("a selection");

        assert!(panel.section_block_rect(panel_rect).is_none());
        let _ = NodeId::new("unused");
    }
}
