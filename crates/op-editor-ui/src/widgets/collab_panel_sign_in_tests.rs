//! Resolution of the collaboration panel's sign-in row (issue #83).
//!
//! The row and the press it answers are one control, so these tests only ever
//! ask the question the panel asks itself: does a press at this coordinate
//! resolve the row the paint promised? Reachability is measured with a real
//! hit test over the panel, never assumed from the model.

use super::*;
use crate::widgets::collab_ui::apply_panel_hit;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::CollabAvailability;

fn viewport() -> Rect {
    Rect::xywh(0.0, 0.0, 1_000.0, 800.0)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

/// Every point of the panel that resolves the sign-in row, by actually
/// hit-testing the panel on a grid.
///
/// Reachability is measured, never assumed: "the row is there" and "the row is
/// gone" are both answers about what a press at a coordinate does, and only a
/// real hit test can give them.
fn sign_in_row_points(panel: &CollabPanel<'_>, rect: Rect) -> Vec<Point2D> {
    let mut points = Vec::new();
    let mut y = rect.origin.y;
    while y <= rect.origin.y + rect.size.y {
        let mut x = rect.origin.x;
        while x <= rect.origin.x + rect.size.x {
            let point = Point2D::new(x, y);
            if panel.hit_test(rect, point) == Some(CollabPanelHit::OpenSignIn) {
                points.push(point);
            }
            x += 4.0;
        }
        y += 4.0;
    }
    points
}

/// Issue #83: the row and the press it answers are one control, so a panel
/// that paints "Sign in" must be a panel where pressing it does something —
/// and a deployment that cannot sign anybody in must paint no row, because a
/// button that silently does nothing is worse than an explanation.
#[test]
fn sign_in_row_exists_only_where_a_press_on_it_can_be_answered() {
    // A deployment whose runtime wants an account and whose chrome HAS a
    // sign-in surface: the row is there, and this is the hit test that proves
    // it rather than assuming it.
    let mut ui = EditorUiState {
        locale: op_editor_core::Locale::EnUs,
        account_ui_available: true,
        ..Default::default()
    };
    ui.collab.availability = CollabAvailability::SignInRequired;
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());
    let row = panel.sign_in_rect(rect, panel.body_top(rect));

    assert_eq!(
        panel.hit_test(rect, center(row)),
        Some(CollabPanelHit::OpenSignIn),
        "a painted row must be the row a press resolves"
    );
    assert!(
        !sign_in_row_points(&panel, rect).is_empty(),
        "and the press that resolves it is what the row's paint promise means"
    );
    let mut backend = CaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    assert!(
        backend
            .round_fills
            .iter()
            .any(|(filled, _, _)| *filled == row),
        "the row a press resolves is the row the panel paints"
    );
    assert!(apply_panel_hit(&mut ui, CollabPanelHit::OpenSignIn));
    assert!(ui.login_modal_open, "and that press asks for a sign-in");

    // The same runtime, in a deployment that offers nobody a sign-in: the
    // local `--serve-web` daemon answers `available:false` on
    // `/api/auth/status` while its collaboration runtime still reports
    // `SignInRequired`, and the panel used to paint the row there anyway. The
    // press was refused by `apply_panel_hit`, so the row was dead chrome.
    let mut ui = EditorUiState {
        locale: op_editor_core::Locale::EnUs,
        ..Default::default()
    };
    ui.collab.availability = CollabAvailability::SignInRequired;
    ui.collab.panel.open = true;
    let panel = CollabPanel::for_editor_ui(&ui).unwrap();
    let rect = panel.rect_at(Rect::xywh(600.0, 8.0, 100.0, 26.0), viewport());

    assert_eq!(
        panel.model.screen,
        CollabPanelScreen::SignInUnavailable,
        "the panel explains why instead of offering a door that is not there"
    );
    assert!(
        sign_in_row_points(&panel, rect).is_empty(),
        "no point of the panel may resolve a sign-in row it cannot answer"
    );
    let mut backend = CaptureBackend::default();
    panel.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        rect,
    );
    let dead_row = panel.sign_in_rect(rect, panel.body_top(rect));
    assert!(
        !backend
            .round_fills
            .iter()
            .any(|(filled, _, _)| *filled == dead_row),
        "and nothing paints a button where that row used to be"
    );
    let painted: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert_eq!(
        painted.first().copied(),
        Some("Collaboration"),
        "the title, then nothing but the explanation: {painted:?}"
    );
    // Rejoined, the painted lines are the whole sentence — no ellipsis ate the
    // half that says why the row is missing.
    let explanation = painted[1..].join(" ");
    assert_eq!(
        explanation,
        op_i18n::translate(
            op_editor_core::Locale::EnUs,
            "collab.join.signInUnavailable"
        ),
        "the explanation reaches the screen whole"
    );
    assert!(
        !explanation.contains('…'),
        "two lines carry the reason instead of one line truncating it: {explanation:?}"
    );
    assert!(
        !apply_panel_hit(&mut ui, CollabPanelHit::OpenSignIn),
        "and no press may claim to have opened a sign-in that does not exist"
    );
    assert!(!ui.login_modal_open);
}
