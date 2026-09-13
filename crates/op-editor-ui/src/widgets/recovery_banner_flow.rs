//! Recovery-banner flow shared by the widget hosts.
//!
//! Mirrors `editor_toast_flow`: placement, press and dismissal are
//! `EditorState` mutation plus a widget-layer hit-test, so a host's arm is
//! only "resolve against this viewport, run the platform tail". Both hosts
//! behave identically because there is one implementation — today only the
//! browser paints it, because only the browser writes a draft
//! (`op_host_web::web_autosave`), and a desktop offer would have nothing to
//! offer.
//!
//! The banner is non-modal: [`press`] returns `false` for a point outside it,
//! so the canvas underneath keeps receiving presses while the bar is up. It is
//! likewise off the Escape ladder — a notice that asks a question the user can
//! also simply ignore does not belong in the keyboard's dismissal cascade.

use op_editor_core::editor_ui_state::RecoveryRequest;
use op_editor_core::EditorState;

use crate::widgets::recovery_banner::{RecoveryBanner, RecoveryBannerHit};
use crate::widgets::{AlignToolbar, PaintCx};
use crate::{Point2D, Rect};

/// Where the banner paints this frame, if at all.
pub fn resolve<'a>(
    state: &'a EditorState,
    viewport_width: f32,
    viewport_height: f32,
    now_ms: u64,
) -> Option<(RecoveryBanner<'a>, Rect)> {
    let banner = RecoveryBanner::for_editor(state, now_ms)?;
    let canvas =
        crate::widgets::host_canvas_geometry::canvas_rect(state, viewport_width, viewport_height);
    // The two surfaces the bar stacks under. Both are read from the same state
    // the paint pass reads, so a press resolves the rect the paint drew.
    let align_visible = AlignToolbar::for_canvas_region(canvas, state).is_some();
    let toast_visible = state.editor_ui.visible_toast(now_ms).is_some();
    let rect = RecoveryBanner::rect_in_canvas(canvas, align_visible, toast_visible)?;
    Some((banner, rect))
}

/// Paint the bar, if one is due. Hosts call this below every menu, modal and
/// floating panel, and above the canvas.
///
/// Returns the painted rect, for the hosts' hit-test parity.
pub fn paint(
    cx: &mut PaintCx<'_>,
    state: &EditorState,
    viewport_width: f32,
    viewport_height: f32,
    now_ms: u64,
) -> Option<Rect> {
    let (banner, rect) = resolve(state, viewport_width, viewport_height, now_ms)?;
    banner.paint(cx, rect);
    Some(rect)
}

/// Route a press. `true` only when the point landed on the bar, so an outside
/// press falls through to the tiers below.
///
/// Takes the resolved `rect` rather than re-deriving it: the host caches the
/// rect its paint pass used, because the bar's vertical slot depends on
/// whether the align toolbar or a toast is up, and those can change between
/// the paint and the press.
///
/// An answer is recorded as a *request* rather than performed here — the
/// widget layer owns no transport. The host's frame tick drains it.
pub fn press(state: &mut EditorState, rect: Option<Rect>, point: Point2D, now_ms: u64) -> bool {
    let Some(rect) = rect else {
        return false;
    };
    // Re-checked here, not trusted from the cache: the offer may have been
    // answered (or a scrim modal opened over it) between the paint and this
    // press, and a stale rect must not eat a click aimed at the canvas.
    if RecoveryBanner::for_editor(state, now_ms).is_none() {
        return false;
    }
    match RecoveryBanner::hit_test(rect, point) {
        RecoveryBannerHit::Restore => {
            state.editor_ui.request_recovery(RecoveryRequest::Restore);
            true
        }
        RecoveryBannerHit::Discard => {
            state.editor_ui.request_recovery(RecoveryRequest::Discard);
            true
        }
        RecoveryBannerHit::Inside => true,
        RecoveryBannerHit::Outside => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_capture_backend::CaptureBackend;
    use op_editor_core::editor_ui_state::{Locale, RecoveryDraft};

    const VIEWPORT: (f32, f32) = (1440.0, 900.0);

    fn state_with_offer() -> EditorState {
        let mut state = EditorState::new();
        state.editor_ui.locale = Locale::EnUs;
        state.editor_ui.now_unix_ms = 1_800_000_000_000.0;
        state.editor_ui.note_recovery_probe(Some(RecoveryDraft {
            saved_at: 1_799_999_000,
            size: 2_048,
        }));
        state
    }

    fn painted_rect(state: &EditorState) -> Option<Rect> {
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        paint(&mut cx, state, VIEWPORT.0, VIEWPORT.1, 0)
    }

    #[test]
    fn nothing_paints_without_an_offer() {
        assert!(painted_rect(&EditorState::new()).is_none());
    }

    #[test]
    fn an_offer_in_a_full_window_paints_a_bar() {
        let state = state_with_offer();
        let rect = painted_rect(&state).expect("a bar");
        let canvas =
            crate::widgets::host_canvas_geometry::canvas_rect(&state, VIEWPORT.0, VIEWPORT.1);
        assert!(canvas.contains(rect.origin));
        assert!(rect.origin.y > canvas.origin.y);
    }

    #[test]
    fn a_press_on_restore_asks_the_host_to_restore_and_hides_the_bar() {
        let mut state = state_with_offer();
        let rect = painted_rect(&state).expect("a bar");
        let restore =
            RecoveryBanner::button_rect(rect, crate::widgets::RecoveryBannerAction::Restore);
        let point = Point2D::new(
            restore.origin.x + restore.size.x / 2.0,
            restore.origin.y + restore.size.y / 2.0,
        );

        assert!(press(&mut state, Some(rect), point, 0));
        assert_eq!(
            state.editor_ui.recovery_request,
            Some(RecoveryRequest::Restore)
        );
        assert!(
            painted_rect(&state).is_none(),
            "an answered question must not stay on screen"
        );
    }

    #[test]
    fn a_press_on_discard_asks_the_host_to_drop_the_draft() {
        let mut state = state_with_offer();
        let rect = painted_rect(&state).expect("a bar");
        let discard =
            RecoveryBanner::button_rect(rect, crate::widgets::RecoveryBannerAction::Discard);
        let point = Point2D::new(
            discard.origin.x + discard.size.x / 2.0,
            discard.origin.y + discard.size.y / 2.0,
        );

        assert!(press(&mut state, Some(rect), point, 0));
        assert_eq!(
            state.editor_ui.recovery_request,
            Some(RecoveryRequest::Discard)
        );
    }

    #[test]
    fn a_press_on_the_sentence_is_swallowed_but_asks_nothing() {
        let mut state = state_with_offer();
        let rect = painted_rect(&state).expect("a bar");
        let inside = Point2D::new(rect.origin.x + 8.0, rect.origin.y + rect.size.y / 2.0);

        assert!(press(&mut state, Some(rect), inside, 0));
        assert_eq!(state.editor_ui.recovery_request, None);
    }

    #[test]
    fn an_outside_press_falls_through_to_the_canvas() {
        let mut state = state_with_offer();
        let rect = painted_rect(&state).expect("a bar");
        let outside = Point2D::new(rect.origin.x - 40.0, rect.origin.y + rect.size.y / 2.0);

        assert!(!press(&mut state, Some(rect), outside, 0));
        assert_eq!(state.editor_ui.recovery_request, None);
    }

    #[test]
    fn a_stale_rect_from_a_finished_offer_eats_nothing() {
        // The host caches the rect the paint used; the user may have answered
        // from another path (or a modal may have covered the bar) since.
        let mut state = state_with_offer();
        let rect = painted_rect(&state).expect("a bar");
        state.editor_ui.answer_recovery_offer();

        let restore =
            RecoveryBanner::button_rect(rect, crate::widgets::RecoveryBannerAction::Restore);
        let point = Point2D::new(
            restore.origin.x + restore.size.x / 2.0,
            restore.origin.y + restore.size.y / 2.0,
        );
        assert!(!press(&mut state, Some(rect), point, 0));
        assert_eq!(state.editor_ui.recovery_request, None);
    }
}
