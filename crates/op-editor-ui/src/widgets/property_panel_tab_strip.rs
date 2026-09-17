//! The inspector's pinned tab strip.
//!
//! Split out of `property_panel_sections.rs` when the section's second tab
//! pushed that file past the 800-line cap. What lives here is the geometry the
//! paint pass and the press arm share: the rects, the hit-test over them, and
//! the labels on them. Keeping the three in one file is the point — the strip
//! has grown a second shape (a section's «Обзор» | «Дизайн») and a rect computed
//! twice would eventually be computed differently.

use crate::theme::Theme;
use crate::widgets::property_panel_inputs::TAB_HEIGHT;
use crate::widgets::property_panel_sections::{PropertyLabels, TabStripState};

/// Which optional tabs a selection offers, beside the Design tab every
/// selection has. Three loose booleans per call made the strip's own signatures
/// eight arguments long and unreadable at the call sites; naming the set says
/// what a caller is actually deciding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabStripTabs {
    pub interact: bool,
    pub code: bool,
    /// A section replaces both of the above: it offers «Обзор» | «Дизайн».
    pub section: bool,
}

impl TabStripTabs {
    /// The ordinary strip: the optional tabs a layout allows, no section.
    pub const fn ordinary(interact: bool, code: bool) -> Self {
        Self {
            interact,
            code,
            section: false,
        }
    }

    /// A section's strip, which is the same wherever it is shown.
    pub const fn of_section() -> Self {
        Self {
            interact: false,
            code: false,
            section: true,
        }
    }
}

impl TabStripState {
    pub fn tabs(&self) -> TabStripTabs {
        TabStripTabs {
            interact: self.show_interact,
            code: self.show_code,
            section: self.section,
        }
    }
}
use crate::widgets::PaintCx;
use crate::{Point2D, Rect, TextLayout};

fn tab_label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|c| if c.is_ascii() { 7.0 } else { 13.0 })
        .sum()
}

/// The tab rects (Design, [Interact when `show_interact`], Code) for
/// the pinned strip at panel top-left `(x, y)`, in paint order.
/// Single source of truth shared by paint + hit-test — a click always
/// lands on what's drawn because both walk this same vec.
pub fn tab_strip_rects(
    labels: &PropertyLabels,
    x: f32,
    y: f32,
    tabs: TabStripTabs,
    touch_controls: bool,
) -> Vec<(op_editor_core::PropertyTab, Rect)> {
    use op_editor_core::PropertyTab;
    let pad = 14.0;
    let tab_height = if touch_controls { 30.0 } else { 26.0 };
    let tab_y = y + (TAB_HEIGHT - tab_height) / 2.0;
    let mut cursor_x = x + pad;
    let mut rects = Vec::with_capacity(3);
    // A section's strip is its own: «Обзор» — everything about flow and
    // analytics — then «Дизайн», which carries only design. There is no code to
    // inspect on a section and no widget interactions to wire, so neither tab
    // appears.
    if tabs.section {
        let overview_w = (tab_label_width(labels.tab_overview) + 24.0).max(48.0);
        rects.push((
            PropertyTab::Overview,
            Rect {
                origin: Point2D::new(cursor_x, tab_y),
                size: Point2D::new(overview_w, tab_height),
            },
        ));
        cursor_x += overview_w + 6.0;
        let design_w = (tab_label_width(labels.tab_design) + 24.0).max(48.0);
        rects.push((
            PropertyTab::Design,
            Rect {
                origin: Point2D::new(cursor_x, tab_y),
                size: Point2D::new(design_w, tab_height),
            },
        ));
        return rects;
    }
    let design_w = (tab_label_width(labels.tab_design) + 24.0).max(48.0);
    rects.push((
        PropertyTab::Design,
        Rect {
            origin: Point2D::new(cursor_x, tab_y),
            size: Point2D::new(design_w, tab_height),
        },
    ));
    cursor_x += design_w + 6.0;
    if tabs.interact {
        let interact_w = (tab_label_width(labels.tab_interact) + 24.0).max(48.0);
        rects.push((
            PropertyTab::Interact,
            Rect {
                origin: Point2D::new(cursor_x, tab_y),
                size: Point2D::new(interact_w, tab_height),
            },
        ));
        cursor_x += interact_w + 6.0;
    }
    if tabs.code {
        let code_w = (tab_label_width(labels.tab_code) + 24.0).max(48.0);
        rects.push((
            PropertyTab::Code,
            Rect {
                origin: Point2D::new(cursor_x, tab_y),
                size: Point2D::new(code_w, tab_height),
            },
        ));
    }
    rects
}

/// Hit-test the pinned tab strip. `x`/`y` are the panel's top-left
/// (unscrolled — the strip is pinned). Returns the tab the point
/// lands on, or `None`. Geometry comes from [`tab_strip_rects`], the
/// same source `paint_tab_strip` uses, so clicks match the painted
/// tabs.
pub fn tab_strip_hit(
    labels: &PropertyLabels,
    x: f32,
    y: f32,
    point: Point2D,
    tabs: TabStripTabs,
    touch_controls: bool,
) -> Option<op_editor_core::PropertyTab> {
    tab_strip_rects(labels, x, y, tabs, touch_controls)
        .into_iter()
        .find(|(_, rect)| rect.contains(point))
        .map(|(tab, _)| tab)
}

pub fn paint_tab_strip(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    labels: &PropertyLabels,
    state: TabStripState,
    x: f32,
    y: f32,
    width: f32,
) -> f32 {
    use op_editor_core::PropertyTab;
    let active = state.active;
    let hover = state.hover;
    let label_for = |tab: PropertyTab| -> &'static str {
        match tab {
            PropertyTab::Design => labels.tab_design,
            PropertyTab::Interact => labels.tab_interact,
            PropertyTab::Code => labels.tab_code,
            PropertyTab::Overview => labels.tab_overview,
        }
    };
    for (tab, rect) in tab_strip_rects(labels, x, y, state.tabs(), state.touch_controls) {
        let is_active = tab == active;
        let is_hovered = hover == Some(tab) && !is_active;
        if is_active || is_hovered {
            cx.backend.fill_round_rect(rect, 6.0, theme.muted);
        }
        let color = if is_active {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        let label = TextLayout::single_run(
            label_for(tab),
            "system-ui",
            13.0,
            (color).to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &label,
            Point2D::new(
                rect.origin.x + 12.0,
                jian_widgets::centered_text_baseline_y(rect, 13.0),
            ),
        );
    }
    cx.backend.fill_rect(
        Rect {
            origin: Point2D::new(x, y + TAB_HEIGHT - 1.0),
            size: Point2D::new(width, 1.0),
        },
        theme.border,
    );
    y + TAB_HEIGHT
}
