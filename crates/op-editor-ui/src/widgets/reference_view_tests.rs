//! Reference-card coverage (issue #63).
//!
//! The card exists so the picture a turn was asked to match can be looked at
//! beside the result. These tests pin the two things that make it worth
//! anything: it paints *that picture* (not a frame, not a placeholder), and it
//! lands beside the canvas — inside the canvas region, clear of both rails.

use crate::widgets::host_canvas_geometry;
use crate::widgets::reference_view::{ReferenceView, ReferenceViewHit};
use crate::widgets::PaintCx;
use crate::{Color, ImageDrawMode, Point2D, Rect, RenderBackend, TextLayout};
use op_editor_core::{ChatImage, ChatMessage, EditorState};

const VIEWPORT_W: f32 = 1440.0;
const VIEWPORT_H: f32 = 900.0;
/// Minimal PNG signature — enough bytes for the card's "no picture" guard.
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Recording backend — captures image draws and rect fills.
#[derive(Default)]
struct ImageRecorder {
    images: Vec<(Rect, u64, usize)>,
    rects: Vec<Rect>,
}

impl RenderBackend for ImageRecorder {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, _: Color) {
        self.rects.push(rect);
    }
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, _: &TextLayout, _: Point2D) {}
    fn clip_rect(&mut self, _: Rect) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn scale(&mut self, _: Point2D, _: Point2D) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _: f32, _: Color) {
        self.rects.push(rect);
    }
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn draw_image(&mut self, rect: Rect, image_id: u64, encoded: &[u8]) {
        self.images.push((rect, image_id, encoded.len()));
    }
    fn draw_image_with_mode(&mut self, rect: Rect, id: u64, encoded: &[u8], _: ImageDrawMode) {
        self.draw_image(rect, id, encoded);
    }
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
    fn measure_text_weighted(&mut self, text: &str, font_size: f32, _: u16) -> f32 {
        text.chars().count() as f32 * font_size * 0.5
    }
}

/// A document whose chat holds one user turn with reference picture `id`.
fn state_with_reference(id: u64) -> EditorState {
    let mut state = EditorState::default();
    let mut message = ChatMessage::user("сделай экран как на картинке");
    message.images.push(ChatImage {
        id,
        name: "reference.png".to_string(),
        media_type: "image/png".to_string(),
        data: PNG.to_vec(),
    });
    state.chat.messages.push(message);
    state
}

fn paint_card(state: &EditorState) -> ImageRecorder {
    let mut backend = ImageRecorder::default();
    let rect = ReferenceView::card_rect(state, VIEWPORT_W, VIEWPORT_H).expect("card is showing");
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        let view = ReferenceView::from_state(state).expect("card resolves from open state");
        view.paint(&mut cx, rect);
    }
    backend
}

/// The picture the turn was asked to match is the picture the card paints —
/// and it is painted from the transcript's bytes, not from a placeholder.
///
/// This is the assertion the whole feature rests on: a card that paints a
/// frame and an icon would still look like "a reference view" in a screenshot
/// while showing nothing a designer could compare against.
#[test]
fn the_card_paints_the_reference_the_turn_was_asked_to_match() {
    let mut state = state_with_reference(4242);
    state.editor_ui.reference_view.toggle(4242);

    let recorder = paint_card(&state);

    assert_eq!(
        recorder.images.len(),
        1,
        "the card draws exactly one picture: {:#?}",
        recorder.images
    );
    assert_eq!(recorder.images[0].1, 4242, "the transcript's own image id");
    assert_eq!(
        recorder.images[0].2,
        PNG.len(),
        "the bytes drawn are the attachment's, not a stand-in"
    );
}

/// The card sits beside the canvas — inside the canvas region, clear of the
/// layer rail and the right rail. A card that covered a rail would cost the
/// user the panel they were working in to look at a picture.
#[test]
fn the_card_is_placed_beside_the_canvas_and_clear_of_both_rails() {
    let mut state = state_with_reference(7);
    state.editor_ui.reference_view.toggle(7);

    let rect = ReferenceView::card_rect(&state, VIEWPORT_W, VIEWPORT_H).expect("card is showing");
    let (left, top, width, height) =
        host_canvas_geometry::canvas_region(&state, VIEWPORT_W, VIEWPORT_H);
    let region = Rect::xywh(left, top, width, height);

    assert!(
        region.contains(rect.origin)
            && rect.origin.x + rect.size.x <= region.origin.x + region.size.x + 0.01
            && rect.origin.y + rect.size.y <= region.origin.y + region.size.y + 0.01,
        "card {rect:?} must sit inside the canvas region {region:?}"
    );
    assert!(
        rect.origin.x >= left,
        "the card must not sit on the layer panel (canvas starts at {left})"
    );
}

/// A card nobody opened paints nothing — the feature is a view, never an
/// always-on overlay over the user's canvas.
#[test]
fn a_shut_card_paints_no_picture() {
    let state = state_with_reference(11);
    assert!(
        ReferenceView::from_state(&state).is_none(),
        "the card is closed by default"
    );
    assert!(
        ReferenceView::card_rect(&state, VIEWPORT_W, VIEWPORT_H).is_none(),
        "a shut card has no rect, so nothing can be painted or hit in its place"
    );
}

/// A shut card swallows no press.
///
/// The card covers part of the canvas, so an invisible one that still claimed
/// its rect would eat clicks meant for the nodes underneath — including the
/// second click of a drill-down, which is how this was found
/// (`op-host-web::widget_host::canvas_hierarchy_tests`).
#[test]
fn a_shut_card_swallows_no_press() {
    let state = state_with_reference(12);
    assert!(ReferenceView::card_rect(&state, VIEWPORT_W, VIEWPORT_H).is_none());
}

/// Clicking the same thumbnail again hides the card; clicking another
/// reference opens *that* one. A user comparing two references must not have
/// the second click read as "close".
#[test]
fn clicking_the_same_reference_twice_hides_it_while_another_opens_it() {
    let mut state = state_with_reference(21);
    let mut second = ChatMessage::user("и вот эту");
    second.images.push(ChatImage {
        id: 22,
        name: "other.png".to_string(),
        media_type: "image/png".to_string(),
        data: PNG.to_vec(),
    });
    state.chat.messages.push(second);

    state.editor_ui.reference_view.toggle(21);
    assert_eq!(state.editor_ui.reference_view.image, Some(21));
    state.editor_ui.reference_view.toggle(21);
    assert!(!state.editor_ui.reference_view.open, "same picture closes");

    state.editor_ui.reference_view.toggle(21);
    state.editor_ui.reference_view.toggle(22);
    assert!(state.editor_ui.reference_view.open, "another picture opens");
    assert_eq!(state.editor_ui.reference_view.image, Some(22));
}

/// A picture the transcript no longer holds resolves to nothing: New Chat
/// clears the messages, and the card must go with them rather than keep
/// painting a frame for a conversation that is over.
#[test]
fn a_reference_the_transcript_dropped_paints_nothing() {
    let mut state = state_with_reference(33);
    state.editor_ui.reference_view.toggle(33);
    assert!(ReferenceView::from_state(&state).is_some());

    state.chat.new_chat();

    assert!(
        ReferenceView::from_state(&state).is_none(),
        "a cleared transcript leaves no reference to show"
    );
}

/// The card's press targets: `×` closes, the body is consumed but inert, and
/// a press outside belongs to whatever is underneath.
#[test]
fn the_close_button_and_the_card_body_are_distinct_targets() {
    let rect = Rect::xywh(100.0, 100.0, 300.0, 260.0);
    let close = ReferenceView::close_rect(rect);

    assert_eq!(
        ReferenceView::hit_test(
            rect,
            Point2D::new(close.origin.x + 4.0, close.origin.y + 4.0)
        ),
        Some(ReferenceViewHit::Close)
    );
    assert_eq!(
        ReferenceView::hit_test(rect, Point2D::new(150.0, 300.0)),
        Some(ReferenceViewHit::Inside)
    );
    assert_eq!(
        ReferenceView::hit_test(rect, Point2D::new(99.0, 300.0)),
        None,
        "outside the card the press belongs to the canvas"
    );
}

/// The affordance: a click on the reference thumbnail in the transcript opens
/// the card. Without this the card would be state nobody could reach, which is
/// how the picture ended up un-comparable in the first place.
#[test]
fn clicking_a_transcript_reference_thumbnail_opens_the_card() {
    let mut state = state_with_reference(91);

    let step = crate::widgets::chat_click_flow::apply_chat_hit(
        &mut state,
        crate::widgets::AIChatHit::ShowReference(0, 0),
        0,
    );

    assert_ne!(
        step,
        crate::widgets::chat_click_flow::ChatClickStep::Clean,
        "opening the card is a state change the host must repaint for"
    );
    assert!(state.editor_ui.reference_view.open);
    assert_eq!(state.editor_ui.reference_view.image, Some(91));
    assert!(
        ReferenceView::from_state(&state).is_some(),
        "the card is paintable right after the click"
    );

    // A second click on the same thumbnail closes it again.
    crate::widgets::chat_click_flow::apply_chat_hit(
        &mut state,
        crate::widgets::AIChatHit::ShowReference(0, 0),
        0,
    );
    assert!(!state.editor_ui.reference_view.open);
}

/// The transcript's own hit-test resolves a click on an attached picture to
/// `ShowReference` — the wiring between "the picture I can see" and "the card
/// that shows it".
///
/// Asserted through the real builder rather than a hand-made rect, because the
/// rect the user clicks is the one `build_transcript` laid out: if the two ever
/// disagree, the thumbnail looks clickable and is not.
#[test]
fn a_click_on_the_transcripts_picture_resolves_to_show_reference() {
    let state = state_with_reference(77);
    let body = Rect::xywh(0.0, 0.0, 340.0, 320.0);
    let canonical = crate::widgets::ai_chat_transcript_cache::unowned_for_tests(
        &state.chat.messages,
        body,
        op_editor_core::Locale::EnUs,
    );
    let item = &canonical.items[0];
    assert_eq!(item.images.len(), 1, "the message carries one picture");
    let thumb = item.images[0];
    let point = Point2D::new(
        thumb.origin.x + thumb.size.x / 2.0,
        thumb.origin.y + thumb.size.y / 2.0,
    );

    let hit = crate::widgets::ai_chat_transcript_hit::transcript_hit(
        &canonical, body, point.x, point.y, 0.0,
    );
    assert_eq!(
        hit,
        Some(crate::widgets::ai_chat_transcript_hit::TranscriptHit::ShowReference(0, 0)),
        "a click inside the thumbnail is the reference affordance ({thumb:?})"
    );
}

/// The FIRST attachment of a process carries id `0` — the id counter starts at
/// zero — and the card has to show it like any other picture.
///
/// This is how the feature failed in the browser: `0` had been treated as "no
/// picture", so the very first reference a session ever attached opened
/// nothing, and the card stayed invisible while the click resolved correctly.
#[test]
fn the_first_attachment_id_zero_is_a_picture_like_any_other() {
    let mut state = state_with_reference(0);

    let step = crate::widgets::chat_click_flow::apply_chat_hit(
        &mut state,
        crate::widgets::AIChatHit::ShowReference(0, 0),
        0,
    );
    assert_ne!(step, crate::widgets::chat_click_flow::ChatClickStep::Clean);
    assert!(
        state.editor_ui.reference_view.open,
        "id 0 is a real picture"
    );
    assert_eq!(state.editor_ui.reference_view.image, Some(0));

    let recorder = paint_card(&state);
    assert_eq!(recorder.images.len(), 1, "id 0 paints the picture too");
    assert_eq!(recorder.images[0].1, 0);
}
