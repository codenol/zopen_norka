//! A page row's open-comment marker: the count, the geometry, and the badge.
//!
//! What a reviewer can be wrong about here: reading a row's number as the whole
//! document's, seeing a marker on a page nobody commented on, and — the reason
//! this row has its own geometry — a long page name running under the marker or
//! the marker sliding under the × that appears on hover.

use super::comment_paint::{badge_ink, count_badge_at, BADGE_DIAMETER, BADGE_RING};
use super::layer_panel::LayerPanel;
use super::layer_panel_metrics::{
    delete_page_target, page_label_x, page_row_rect, page_row_tail, LayerPanelMetrics,
    PAGE_BADGE_GAP, PAGE_DELETE_AIR,
};
use super::layer_panel_paint::truncate_to_fit_measured;
use super::test_capture_backend::CaptureBackend;
use super::toolbar::Toolbar;
use super::{PaintCx, Widget};
use crate::theme::Theme;
use crate::{Color, Point2D, Rect, RenderBackend};
use op_editor_core::editor_ui_state::{Comment, CommentAnchor, CommentAuthor, CommentThread};
use op_editor_core::EditorState;

/// The narrow rail the marker has to fit in.
const PANEL_W: f32 = 180.0;

const LONG_NAME: &str = "Checkout · Shipping options · Empty cart";

fn comment(id: i64) -> Comment {
    Comment {
        id,
        author: CommentAuthor {
            id: Some("u1".to_string()),
            name: "Kay".to_string(),
            role: Some("ux_ui".to_string()),
        },
        body: "this label is clipped at 200 %".to_string(),
        created_at: 1_700_000_000,
    }
}

/// An open thread pinned on `page`, or a closed one when `resolved`.
fn thread(id: i64, page: &str, resolved: bool) -> CommentThread {
    CommentThread {
        id,
        anchor: Some(CommentAnchor::new(page, id as f64 * 10.0, id as f64 * 20.0)),
        created_at: 1_700_000_000,
        resolved,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![comment(id * 10)],
    }
}

/// A thread the daemon migrated from the old element-keyed format: no pin, so
/// no page can draw it.
fn migrated_thread(id: i64) -> CommentThread {
    CommentThread {
        id,
        anchor: None,
        created_at: 1_699_000_000,
        resolved: false,
        resolved_at: None,
        resolved_by: None,
        resolved_by_name: None,
        comments: vec![comment(id * 10)],
    }
}

/// A document of `(page id, page name)` in order, with a comment client — the
/// shape a host that carries the daemon's transport hands the chrome.
fn state_with_pages(pages: &[(&str, &str)]) -> EditorState {
    let entries = pages
        .iter()
        .map(|(id, name)| format!(r#"{{"id":"{id}","name":"{name}","children":[]}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(r#"{{"version":"1.0.0","children":[],"pages":[{entries}]}}"#);
    let doc = jian_ops_schema::load_str(&source)
        .expect("page-marker fixture parses")
        .value;
    let mut state = EditorState::from_document(doc);
    state.editor_ui.comments.transport = true;
    state
}

fn panel_rect(panel: &LayerPanel) -> Rect {
    Rect::xywh(0.0, 0.0, PANEL_W, panel.intrinsic_height())
}

fn paint(panel: &LayerPanel, rect: Rect) -> CaptureBackend {
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    panel.paint(&mut cx, rect);
    backend
}

/// Every count-badge oval a surface painted, as `(size, radius, colour)`.
///
/// `RenderBackend::fill_oval` reaches a backend as a round rect with a circular
/// radius, so the two ovals a badge is made of — the ring, then the circle over
/// it — are recognisable by their radius alone. Comparing this between two
/// surfaces is what "the same badge" means when the same number is painted in
/// two different places.
fn badge_ovals(backend: &CaptureBackend) -> Vec<(Point2D, f32, Color)> {
    let circle = BADGE_DIAMETER / 2.0;
    let ring = (BADGE_DIAMETER + BADGE_RING * 2.0) / 2.0;
    backend
        .round_fills
        .iter()
        .filter(|(_, radius, _)| *radius == circle || *radius == ring)
        .map(|(rect, radius, color)| (rect.size, *radius, *color))
        .collect()
}

fn painted_texts(backend: &CaptureBackend) -> Vec<String> {
    backend
        .texts
        .iter()
        .map(|(content, _)| content.clone())
        .collect()
}

#[test]
fn a_page_row_shows_the_open_threads_pinned_on_it() {
    let mut state = state_with_pages(&[("p1", "Page 1"), ("p2", "Cover")]);
    state.editor_ui.comments.install_threads(vec![
        thread(1, "p1", false),
        thread(2, "p1", false),
        thread(3, "p2", false),
        // Closed: a page's outstanding work is what is still open, and a marker
        // that outlived its resolution would point at a finished conversation.
        thread(4, "p2", true),
    ]);
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(&*panel.page_comments, &[2, 1]);
}

#[test]
fn a_thread_pinned_on_another_page_is_not_this_rows_count() {
    let mut state = state_with_pages(&[("p1", "Page 1"), ("p2", "Cover")]);
    state
        .editor_ui
        .comments
        .install_threads(vec![thread(1, "p2", false)]);
    let panel = LayerPanel::from_editor(&state);
    // The page id is the whole membership test: p1 holds nothing, so its row
    // shows nothing at all — not zero, nothing.
    assert_eq!(&*panel.page_comments, &[0, 1]);
}

#[test]
fn a_thread_with_no_pin_is_counted_on_no_page_row() {
    // The migrated thread is real, open, and listed by the rail on every page —
    // but no page draws it, so no page's marker may claim it. This is the one
    // place the row's number and the toolbar badge differ, and this test is what
    // says so on purpose (see `comments_page_counts`).
    let mut state = state_with_pages(&[("p1", "Page 1"), ("p2", "Cover")]);
    state.editor_ui.comments.install_threads(vec![
        thread(1, "p1", false),
        thread(2, "p1", false),
        migrated_thread(3),
    ]);
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(&*panel.page_comments, &[2, 0]);
    // The badge still counts it, because the rail the badge opens still lists it.
    assert_eq!(Toolbar::for_editor(&state).comments_open, 3);
}

#[test]
fn the_row_marker_agrees_with_the_toolbar_badge_on_the_active_page() {
    for active in [0usize, 1] {
        let mut state = state_with_pages(&[("p1", "Page 1"), ("p2", "Cover")]);
        state.editor_ui.comments.install_threads(vec![
            thread(1, "p1", false),
            thread(2, "p1", false),
            thread(3, "p2", false),
        ]);
        assert!(state.set_active_page(active));
        let panel = LayerPanel::from_editor(&state);
        let toolbar = Toolbar::for_editor(&state);
        assert_eq!(
            panel.page_comments[active], toolbar.comments_open,
            "the page row the reviewer is looking at and the toolbar badge must be one number"
        );
    }
}

#[test]
fn a_page_with_nothing_open_paints_no_marker() {
    let mut state = state_with_pages(&[("p1", "Page 1")]);
    state
        .editor_ui
        .comments
        .install_threads(vec![thread(1, "p1", true)]);
    let panel = LayerPanel::from_editor(&state);
    assert_eq!(&*panel.page_comments, &[0]);
    let backend = paint(&panel, panel_rect(&panel));
    assert!(
        badge_ovals(&backend).is_empty(),
        "a page with no open thread must paint no badge at all"
    );
    assert!(
        !painted_texts(&backend).iter().any(|text| text == "0"),
        "an unreviewed page must not show a zero"
    );
}

#[test]
fn a_page_with_open_threads_paints_the_badge_the_toolbar_paints() {
    let mut state = state_with_pages(&[("p1", "Page 1")]);
    state.editor_ui.comments.install_threads(vec![
        thread(1, "p1", false),
        thread(2, "p1", false),
        thread(3, "p1", false),
    ]);
    let panel = LayerPanel::from_editor(&state);
    let rect = panel_rect(&panel);
    let panel_backend = paint(&panel, rect);

    let toolbar = Toolbar::for_editor(&state);
    let mut toolbar_backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut toolbar_backend,
    };
    toolbar.paint(
        &mut cx,
        Rect::xywh(0.0, 0.0, super::toolbar::TOOLBAR_WIDTH, 400.0),
    );

    let marker = badge_ovals(&panel_backend);
    let icon_badge = badge_ovals(&toolbar_backend);
    assert_eq!(
        marker.len(),
        2,
        "a marker is the ring plus the circle, nothing else"
    );
    assert_eq!(
        marker, icon_badge,
        "the page marker and the toolbar badge must be the same badge"
    );
    for backend in [&panel_backend, &toolbar_backend] {
        assert!(
            painted_texts(backend).iter().any(|text| text == "3"),
            "both surfaces must print the count itself"
        );
    }
}

#[test]
fn the_marker_sits_inside_the_row_and_clear_of_the_name() {
    // The narrow rail, a name far longer than it, and three open threads: the
    // name ellipsizes before the marker, the marker stays off the × that appears
    // on hover, and everything stays inside the row.
    let mut state = state_with_pages(&[("p1", LONG_NAME)]);
    state.editor_ui.comments.install_threads(vec![
        thread(1, "p1", false),
        thread(2, "p1", false),
        thread(3, "p1", false),
    ]);
    let panel = LayerPanel::from_editor(&state);
    let rect = panel_rect(&panel);
    let metrics = LayerPanelMetrics::DESKTOP;
    let row_y = panel.regions(rect).pages_rows_top;
    let row = page_row_rect(rect, row_y, metrics);
    let tail = page_row_tail(rect, row_y, metrics, 3, true);
    let slot = tail
        .badge
        .expect("a row with open threads reserves a marker");
    let ink = badge_ink(slot);

    // Inside the row it belongs to.
    assert!(
        ink.origin.x >= row.origin.x
            && ink.origin.x + ink.size.x <= row.origin.x + row.size.x
            && ink.origin.y >= row.origin.y
            && ink.origin.y + ink.size.y <= row.origin.y + row.size.y,
        "marker ink {ink:?} escapes its row {row:?}"
    );
    // Clear of the × the row shows on hover, which the marker must never cover.
    let delete_left = delete_page_target(rect, row_y, metrics).origin.x;
    assert!(
        ink.origin.x + ink.size.x <= delete_left,
        "marker ink ends at {} but the delete affordance starts at {delete_left}",
        ink.origin.x + ink.size.x
    );
    // Clear of the name's box, and the ellipsized name really does stop short of
    // the ink — measured with the backend the painter measures with.
    let label_x = page_label_x(row);
    let available = tail.label_right - label_x;
    assert!(
        available > 0.0,
        "a name must keep some room on a 180 px row"
    );
    assert!(tail.label_right < ink.origin.x);
    let mut backend = CaptureBackend::default();
    let display = truncate_to_fit_measured(&mut backend, LONG_NAME, metrics.row_font, available);
    assert!(
        display.ends_with('…'),
        "an over-long page name must be ellipsized, got {display:?}"
    );
    let painted_w = backend.measure_text_family(&display, metrics.row_font, "system-ui");
    assert!(
        painted_w <= available + 0.01,
        "the name paints {painted_w} px into a {available} px budget"
    );
    assert!(
        label_x + painted_w < ink.origin.x,
        "the name reaches {} and the marker ink starts at {}",
        label_x + painted_w,
        ink.origin.x
    );
}

#[test]
fn a_row_without_a_marker_keeps_the_whole_name_budget() {
    // The reservation is conditional: a page nobody commented on gets the width
    // back, so introducing markers does not silently shorten every name.
    let rect = Rect::xywh(0.0, 0.0, PANEL_W, 400.0);
    let metrics = LayerPanelMetrics::DESKTOP;
    let row_y = 0.0;
    let plain = page_row_tail(rect, row_y, metrics, 0, true);
    let marked = page_row_tail(rect, row_y, metrics, 3, true);
    assert!(plain.badge.is_none());
    let delete_left = delete_page_target(rect, row_y, metrics).origin.x;
    assert_eq!(plain.label_right, delete_left - PAGE_DELETE_AIR);
    let ink = badge_ink(marked.badge.expect("a row with threads reserves a slot"));
    // The name stops one gap before the marker's ink, and the ink stops short of
    // the × the row shows on hover — neither of them moved to make room.
    assert_eq!(marked.label_right, ink.origin.x - PAGE_BADGE_GAP);
    assert!(
        ink.origin.x + ink.size.x < delete_left,
        "marker ink ends at {} but the delete affordance starts at {delete_left}",
        ink.origin.x + ink.size.x
    );
    assert!(marked.label_right < plain.label_right);
}

#[test]
fn the_name_the_panel_painted_stops_before_the_marker_it_painted() {
    // The end-to-end form of the geometry test above: what the panel actually
    // emits on a 180 px row — the ellipsized name and the badge — must not
    // overlap, and the badge must stay visible inside the row instead of being
    // clipped away by the panel's edge.
    let mut state = state_with_pages(&[("p1", LONG_NAME)]);
    state.editor_ui.comments.install_threads(vec![
        thread(1, "p1", false),
        thread(2, "p1", false),
        thread(3, "p1", false),
    ]);
    let panel = LayerPanel::from_editor(&state);
    let rect = panel_rect(&panel);
    let mut backend = paint(&panel, rect);

    let metrics = LayerPanelMetrics::DESKTOP;
    let row_y = panel.regions(rect).pages_rows_top;
    let row = page_row_rect(rect, row_y, metrics);
    let (name, origin) = backend
        .texts
        .iter()
        .find(|(content, _)| content.starts_with("Checkout"))
        .expect("the page name paints")
        .clone();
    assert!(name.ends_with('…'), "the name must be ellipsized: {name:?}");
    let name_right = origin.x + backend.measure_text_family(&name, metrics.row_font, "system-ui");

    let circle = backend
        .round_fills
        .iter()
        .find(|(_, radius, _)| *radius == BADGE_DIAMETER / 2.0)
        .expect("the marker paints on a page with open threads");
    let ink = badge_ink(circle.0);
    assert!(
        name_right < ink.origin.x,
        "the name reaches {name_right} and the marker ink starts at {}",
        ink.origin.x
    );
    assert!(
        ink.origin.x >= row.origin.x
            && ink.origin.x + ink.size.x <= row.origin.x + row.size.x
            && ink.origin.y >= row.origin.y
            && ink.origin.y + ink.size.y <= row.origin.y + row.size.y,
        "marker ink {ink:?} escapes its row {row:?}"
    );
}

#[test]
fn the_marker_is_drawn_at_the_slot_the_row_reserved() {
    // The reservation and the ink are one decision: a marker painted anywhere
    // else would collide with the name the row truncated to make room for it.
    let slot = Rect::xywh(30.0, 40.0, BADGE_DIAMETER, BADGE_DIAMETER);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    count_badge_at(&mut cx, &Theme::dark(), slot, 3);
    let ovals = badge_ovals(&backend);
    assert_eq!(ovals.len(), 2);
    let circle = backend
        .round_fills
        .iter()
        .find(|(_, radius, _)| *radius == BADGE_DIAMETER / 2.0)
        .expect("the badge's circle");
    assert_eq!(circle.0, slot, "the circle fills the slot it was given");
    // Nothing at all for a zero — not an empty circle, not a "0".
    let mut quiet = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut quiet,
    };
    count_badge_at(&mut cx, &Theme::dark(), slot, 0);
    assert!(badge_ovals(&quiet).is_empty());
    assert!(quiet.texts.is_empty());
}
