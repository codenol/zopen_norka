//! The minimized AI chat bar: placement inside the canvas region and
//! the single click that brings the panel back.

use super::WidgetHostNative;
use op_editor_ui::widgets::host_canvas_geometry::{AICHAT_INSET_BOTTOM, AICHAT_INSET_LEFT};
use op_editor_ui::widgets::AI_CHAT_MINIMIZED_HEIGHT;

const VIEWPORT: (f32, f32) = (1200.0, 800.0);

fn minimized_host() -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().chat.minimize();
    host
}

/// The expanded rect, then the same host's minimized rect.
fn expanded_then_minimized(
    host: &mut WidgetHostNative,
) -> (op_editor_ui::Rect, op_editor_ui::Rect) {
    let expanded = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("panel placed");
    host.editor_state_mut().chat.minimize();
    let minimized = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("bar placed");
    (expanded, minimized)
}

#[test]
fn the_bar_docks_to_the_canvas_bottom_left_at_the_panel_width() {
    let host = minimized_host();
    let (cx0, cy0, _cw, ch) = host.canvas_region(VIEWPORT.0, VIEWPORT.1);

    let bar = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("bar placed");

    assert_eq!(bar.size.x, host.editor_state().chat.panel_width);
    assert_eq!(bar.size.y, AI_CHAT_MINIMIZED_HEIGHT);
    // The gutters are the geometry module's own constants, not literals: the
    // bottom one was raised from 12 to 52 so the bar clears a short viewport's
    // edge, and a test that hard-codes the old number only tells us it moved.
    assert_eq!(bar.origin.x, cx0 + AICHAT_INSET_LEFT);
    assert_eq!(
        bar.origin.y,
        cy0 + ch - AI_CHAT_MINIMIZED_HEIGHT - AICHAT_INSET_BOTTOM
    );
}

/// Minimizing changes the panel's HEIGHT. The bar used to carry a width
/// constant of its own, so collapsing a panel the user had widened pulled
/// the bar's edges visibly inward — the reported bug.
#[test]
fn minimizing_keeps_the_panel_x_and_width_to_the_pixel() {
    for anchor in [
        op_editor_core::ChatAnchor::BottomLeft,
        op_editor_core::ChatAnchor::BottomRight,
        op_editor_core::ChatAnchor::TopRight,
    ] {
        for width in [360.0_f32, 520.0, 288.0] {
            let mut host = WidgetHostNative::new();
            {
                let state = host.editor_state_mut();
                state.chat.anchor = anchor;
                state.chat.panel_width = width;
            }

            let (expanded, minimized) = expanded_then_minimized(&mut host);

            assert_eq!(
                minimized.size.x, expanded.size.x,
                "{anchor:?} @ {width}: the bar must be exactly as wide as the panel"
            );
            assert_eq!(
                minimized.origin.x, expanded.origin.x,
                "{anchor:?} @ {width}: the bar's left edge must not move"
            );
        }
    }
}

/// The same guarantee after a drag: the expanded panel honours a stored
/// position, so the bar has to as well or its edges jump sideways.
#[test]
fn minimizing_a_dragged_panel_keeps_its_x_and_width() {
    let mut host = WidgetHostNative::new();
    {
        let state = host.editor_state_mut();
        state.chat.panel_position = Some((600.0, 100.0));
        state.chat.anchor = op_editor_core::ChatAnchor::TopRight;
    }

    let (expanded, minimized) = expanded_then_minimized(&mut host);

    assert_eq!(minimized.origin.x, expanded.origin.x);
    assert_eq!(minimized.size.x, expanded.size.x);
}

#[test]
fn the_bar_still_ignores_the_dragged_y_and_docks_to_the_canvas_floor() {
    // Minimized is a dock, not a floating window: a position left over
    // from dragging the expanded panel must not lift the bar off the
    // canvas floor. Only the HORIZONTAL half of that position is honoured.
    let mut host = minimized_host();
    {
        let state = host.editor_state_mut();
        state.chat.panel_position = Some((600.0, 100.0));
        state.chat.anchor = op_editor_core::ChatAnchor::TopRight;
    }
    let (_cx0, cy0, _cw, ch) = host.canvas_region(VIEWPORT.0, VIEWPORT.1);

    let bar = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("bar placed");

    assert_eq!(
        bar.origin.y,
        cy0 + ch - AI_CHAT_MINIMIZED_HEIGHT - AICHAT_INSET_BOTTOM
    );
    assert_eq!(bar.origin.x, 600.0);
}

#[test]
fn expanding_restores_the_full_panel_and_focuses_the_input() {
    let mut host = minimized_host();
    let bar = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("bar placed");

    // Anywhere on the bar — including over the model label, which is not
    // a control in this form.
    assert!(host.apply_click(
        bar.origin.x + bar.size.x - 90.0,
        bar.origin.y + bar.size.y / 2.0,
        VIEWPORT.0,
        VIEWPORT.1
    ));

    assert!(!host.editor_state().chat.is_minimized());
    assert!(
        host.editor_state().chat.focused,
        "the bar reads as an input, so expanding hands over the caret"
    );
    let panel = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("panel placed");
    assert!(panel.size.y > AI_CHAT_MINIMIZED_HEIGHT);
}

#[test]
fn a_press_over_the_vacated_panel_area_reaches_the_canvas() {
    // The minimized form must not keep claiming the expanded panel's
    // footprint — the panel used to reach this far up the canvas, and
    // everything outside the bar now belongs to the canvas again.
    let mut host = minimized_host();
    let bar = host
        .ai_chat_rect(VIEWPORT.0, VIEWPORT.1)
        .expect("bar placed");
    let vacated_y = bar.origin.y - 120.0;

    assert!(host.apply_press(bar.origin.x + 40.0, vacated_y, VIEWPORT.0, VIEWPORT.1));

    assert!(
        host.editor_state().chat.is_minimized(),
        "a press over the vacated panel area must not expand the chat"
    );
}
