//! Narrow touch-panel text regressions.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;

#[test]
fn unavailable_message_fits_a_320pt_phone_in_every_locale() {
    for locale in op_i18n::Locale::ALL {
        let mut ui = EditorUiState {
            touch: true,
            locale,
            ..Default::default()
        };
        ui.collab.panel.open = true;
        let panel = CollabPanel::for_editor_ui(&ui).expect("open panel");
        let rect = panel.rect_at(
            Rect::xywh(268.0, 0.0, 44.0, 44.0),
            Rect::xywh(0.0, 0.0, 320.0, 568.0),
        );
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        panel.paint(&mut cx, rect);
        let message = backend.texts[1].0.clone();
        assert!(
            text_metrics::measure_chrome(&mut backend, &message, 12.0)
                <= rect.size.x - PAD * 2.0 + 0.01,
            "{} overflows: {message}",
            locale.code()
        );
    }
}

/// The sign-in explanation has no control to fall back on, so its sentence is
/// the whole answer — and it is painted two lines at a time precisely because
/// a translation a word longer than the English would otherwise lose the half
/// that says why the row is missing.
///
/// Asserts what actually reached the screen: every painted line fits the
/// column, and none of them carries the ellipsis that means something was cut.
#[test]
fn the_sign_in_explanation_survives_a_320pt_phone_in_every_locale() {
    for locale in op_i18n::Locale::ALL {
        let mut ui = EditorUiState {
            touch: true,
            locale,
            ..Default::default()
        };
        ui.collab.availability = op_editor_core::CollabAvailability::SignInRequired;
        ui.collab.panel.open = true;
        let panel = CollabPanel::for_editor_ui(&ui).expect("open panel");
        let rect = panel.rect_at(
            Rect::xywh(268.0, 0.0, 44.0, 44.0),
            Rect::xywh(0.0, 0.0, 320.0, 568.0),
        );
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        panel.paint(&mut cx, rect);
        // The title, then the explanation — one line where the translation is
        // short, three where it is not. Never four: the fourth would be cut.
        let painted: Vec<String> = backend.texts.iter().map(|(text, _)| text.clone()).collect();
        assert!(
            (2..=4).contains(&painted.len()),
            "{} painted {} lines: {painted:?}",
            locale.code(),
            painted.len()
        );
        for line in &painted[1..] {
            assert!(
                !line.contains('…'),
                "{} truncated its explanation: {line}",
                locale.code()
            );
            assert!(
                text_metrics::measure_chrome(&mut backend, line, 12.0)
                    <= rect.size.x - PAD * 2.0 + 0.01,
                "{} overflows: {line}",
                locale.code()
            );
        }
    }
}
