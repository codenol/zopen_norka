//! Press coverage for the web host's launch-time recovery banner (#26).
//!
//! Drives the real composition pass (`paint_editor`) against a recording
//! backend and then the real press ladder, because the two halves are what can
//! drift: the bar's vertical slot moves with the align toolbar and the toast,
//! so a press must hit-test the rect the paint actually used. Asserting on the
//! shared flow alone would not catch the host caching (or not caching) it.

use super::WidgetHost;
use op_editor_core::editor_ui_state::{Locale, RecoveryDraft, RecoveryRequest};
use op_editor_ui::widgets::{RecoveryBanner, RecoveryBannerAction};
use op_editor_ui::{Color, Point2D, Rect, RenderBackend, TextLayout};

const W: f32 = 1440.0;
const H: f32 = 900.0;

/// Recording backend — enough of the trait for a full composition pass.
#[derive(Default)]
struct CaptureBackend {
    round_fills: Vec<Rect>,
}

impl RenderBackend for CaptureBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: Color) {}
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, _: &TextLayout, _: Point2D) {}
    fn clip_rect(&mut self, _: Rect) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _: f32, _: Color) {
        self.round_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

/// A host holding an unanswered recovery offer, with a known locale.
fn host_with_offer() -> WidgetHost {
    let mut host = WidgetHost::new();
    host.editor_state_mut().editor_ui.locale = Locale::EnUs;
    host.editor_state_mut()
        .editor_ui
        .note_recovery_probe(Some(RecoveryDraft {
            saved_at: 1_799_999_000,
            size: 4_096,
        }));
    host
}

/// Paint one frame and return the rect the host cached for the press arm.
fn paint_and_cache(host: &mut WidgetHost) -> Option<Rect> {
    let mut backend = CaptureBackend::default();
    host.paint_editor(&mut backend, W, H);
    host.recovery_banner_rect
}

fn centre(rect: Rect) -> (f32, f32) {
    (
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn no_offer_means_no_cached_rect_to_press() {
    let mut host = WidgetHost::new();
    assert!(paint_and_cache(&mut host).is_none());
    // The ladder still runs; what matters is that nothing asked for a draft
    // the daemon never mentioned.
    host.apply_press(400.0, 80.0, W, H);
    assert_eq!(host.editor_state().editor_ui.recovery_request, None);
}

#[test]
fn a_frame_with_an_offer_caches_the_rect_it_painted() {
    let mut host = host_with_offer();
    let rect = paint_and_cache(&mut host).expect("the paint pass caches the bar");

    // The bar is on screen: something painted its own background there.
    let canvas =
        op_editor_ui::widgets::host_canvas_geometry::canvas_rect(host.editor_state(), W, H);
    assert!(canvas.contains(rect.origin));
    assert!(rect.origin.y >= canvas.origin.y);
}

#[test]
fn a_press_on_restore_records_the_request_and_hides_the_bar() {
    let mut host = host_with_offer();
    let rect = paint_and_cache(&mut host).expect("a bar");
    let (x, y) = centre(RecoveryBanner::button_rect(
        rect,
        RecoveryBannerAction::Restore,
    ));

    assert!(host.apply_press(x, y, W, H), "the press is consumed");
    assert_eq!(
        host.editor_state().editor_ui.recovery_request,
        Some(RecoveryRequest::Restore),
        "the press can only record the ask — the frame tick makes the request"
    );
    assert!(
        paint_and_cache(&mut host).is_none(),
        "an answered question must not paint again"
    );
}

#[test]
fn a_press_on_discard_records_the_request_too() {
    let mut host = host_with_offer();
    let rect = paint_and_cache(&mut host).expect("a bar");
    let (x, y) = centre(RecoveryBanner::button_rect(
        rect,
        RecoveryBannerAction::Discard,
    ));

    assert!(host.apply_press(x, y, W, H));
    assert_eq!(
        host.editor_state().editor_ui.recovery_request,
        Some(RecoveryRequest::Discard)
    );
}

#[test]
fn a_press_beside_the_bar_still_reaches_the_canvas() {
    // Non-modal: the offer never takes a click aimed somewhere else.
    let mut host = host_with_offer();
    let rect = paint_and_cache(&mut host).expect("a bar");
    let (_, y) = centre(rect);

    assert!(host.apply_press(rect.origin.x - 40.0, y, W, H));
    assert_eq!(host.editor_state().editor_ui.recovery_request, None);
    assert!(
        paint_and_cache(&mut host).is_some(),
        "an unanswered offer stays on screen"
    );
}

#[test]
fn a_scrim_modal_takes_the_press_the_bar_would_have_had() {
    // The bar paints below every modal, so it must not answer for one: the
    // offer is suppressed while a scrim owns the screen, and the click belongs
    // to the dialog.
    let mut host = host_with_offer();
    let rect = paint_and_cache(&mut host).expect("a bar");
    let (x, y) = centre(RecoveryBanner::button_rect(
        rect,
        RecoveryBannerAction::Restore,
    ));

    host.editor_state_mut().editor_ui.figma_import_open = true;
    assert!(
        paint_and_cache(&mut host).is_none(),
        "the scrim hides the bar"
    );
    host.apply_press(x, y, W, H);
    assert_eq!(
        host.editor_state().editor_ui.recovery_request,
        None,
        "a press aimed at the dialog must not answer the offer behind it"
    );
}
