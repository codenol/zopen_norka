//! The recovery banner — the daemon's draft, offered back at launch (#26).
//!
//! A document with no server key has no file to save into, so autosave writes
//! it into the daemon's single draft slot (`op_host_web::web_autosave`). This
//! is the surface that asks about it on the next launch, and it is the only
//! thing standing between the user and work they did not know they still had:
//! the offer must be *answerable*, which is why it is not the transient toast
//! (single slot, no actions, self-expiring — see `editor_toast`) but a banner
//! with two buttons and no clock.
//!
//! ## Placement: the free slot at the top of the canvas
//!
//! Top-centre, in the same strip the align toolbar and the toast use, and
//! *stacked under both* — the banner is the persistent surface, so a transient
//! notice about the document must never be the thing that gets covered:
//!
//! - the bottom band is structurally busy (the vertical Toolbar column, the
//!   minimized chat dock, the StatusBar and the post-import diagnostics card),
//!   and a bottom-centre bar would collide with at least one of them;
//! - a full-width strip docked under the TopBar would cut across the rail
//!   headers and the toolbar column, which is worse than covering canvas
//!   pixels;
//! - at launch — the only moment this banner appears — nothing is selected and
//!   no toast is normally up, so it lands in the empty top slot on its own.
//!
//! ## Geometry is fixed on purpose
//!
//! Every rect here is a pure function of the canvas and the two surfaces above
//! it. Nothing is measured, so the host caches no paint-time metrics for the
//! hit-test to re-read (the toast does, and pays for it with a cache that can
//! go stale between a paint and a press). A long localized sentence is
//! ellipsised into its own slot instead of widening the bar, which also keeps
//! the bar from jumping around as the age phrase changes from "just now" to
//! "1m ago".

use op_editor_core::editor_ui_state::{EditorUiState, RecoveryDraft};
use op_editor_core::EditorState;

use crate::theme::Theme;
use crate::widgets::align_toolbar::ALIGN_TOOLBAR_HEIGHT;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::editor_toast::TOAST_HEIGHT;
use crate::widgets::relative_age::relative_age_label;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect, TextLayout};

/// Bar height: one line of chrome text with two 30 px buttons beside it.
pub const RECOVERY_BANNER_HEIGHT: f32 = 48.0;
/// Corner radius, matching the other floating surfaces.
pub const RECOVERY_BANNER_RADIUS: f32 = 10.0;
/// Message font size — the chrome body size.
const FONT_SIZE: f32 = 12.0;
/// Button label size. One step down so a long translation ("Wiederherstellen")
/// still fits the fixed button width in every locale.
const BUTTON_FONT_SIZE: f32 = 11.5;
/// Widest the bar grows however wide the canvas is. Sized so the longest of
/// the fifteen sentences ("Nicht gespeicherte Arbeit gefunden: 3h ago") still
/// fits its slot without an ellipsis at the chrome font size.
const MAX_WIDTH: f32 = 680.0;
/// Gap kept between the bar and the canvas's trailing edge. The leading gap is
/// `VERTICAL_TOOLBAR_RESERVE`, because that edge belongs to the tools.
const SIDE_MARGIN: f32 = 24.0;
/// Padding inside the leading and trailing edges.
const PAD_X: f32 = 16.0;
/// Fixed button width. Fixed rather than measured so the hit-test needs no
/// paint-time metrics; the label is ellipsised into whatever is left.
const BUTTON_W: f32 = 152.0;
const BUTTON_H: f32 = 30.0;
const BUTTON_RADIUS: f32 = 6.0;
const BUTTON_GAP: f32 = 8.0;
/// Gap between the message slot and the button pair.
const MESSAGE_GAP: f32 = 16.0;
/// The message slot never shrinks below this, or the bar is a pair of buttons
/// with an ellipsis in front of them.
const MIN_MESSAGE_W: f32 = 120.0;
/// Distance from the canvas's top edge, matching the align toolbar's own.
const TOP_INSET: f32 = 16.0;
/// Gap kept below a surface the banner stacks under.
const STACK_GAP: f32 = 8.0;
/// Left-edge clearance for the vertical Toolbar column — the same reserve the
/// align toolbar and the toast keep, so no floating surface covers the tools.
const VERTICAL_TOOLBAR_RESERVE: f32 = 56.0;

/// The banner's two answers, left to right.
///
/// The affirmative one is rightmost, where every desktop dialog puts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryBannerAction {
    /// Let the draft go (`DELETE /api/recovery`).
    Discard,
    /// Adopt the draft as the open document (`POST /api/recovery/restore`).
    Restore,
}

/// What a press landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryBannerHit {
    /// The "restore" button.
    Restore,
    /// The "discard" button.
    Discard,
    /// Inside the bar but on no button — consumed, so a press aimed at the
    /// banner never falls through to the canvas behind it.
    Inside,
    /// Outside — NOT consumed. The banner is a notice, not a modal: it never
    /// takes a press the user aimed somewhere else.
    Outside,
}

/// The bar, resolved against a live editor state.
pub struct RecoveryBanner<'a> {
    theme: Theme,
    ui: &'a EditorUiState,
    draft: RecoveryDraft,
}

impl<'a> RecoveryBanner<'a> {
    /// `None` whenever nothing should paint.
    ///
    /// A scrim modal suppresses the bar rather than being painted over by it:
    /// the banner belongs below every modal, and a bar the user can see
    /// through a scrim but not press is a worse lie than no bar.
    pub fn for_editor(state: &'a EditorState, now_ms: u64) -> Option<Self> {
        if scrim_owns_screen(state, now_ms) {
            return None;
        }
        let ui = &state.editor_ui;
        Some(Self {
            theme: theme_for(ui),
            ui,
            draft: ui.visible_recovery_offer()?,
        })
    }

    /// The localized sentence, with the draft's age interpolated.
    pub fn message(&self) -> String {
        let when = relative_age_label(self.ui.effective_locale(), self.age_secs());
        match op_i18n::translate_dynamic(self.ui.effective_locale(), "recovery.banner.title") {
            Some(template) => op_i18n::interpolate(template, &[("when", when.as_str())]),
            // A key with no entry anywhere falls back to the key itself rather
            // than an empty bar: a raw key is a better bug report than a blank
            // box, and it is the only signal a missing translation would give.
            None => "recovery.banner.title".to_string(),
        }
    }

    pub fn restore_label(&self) -> &'static str {
        op_i18n::translate(self.ui.effective_locale(), "recovery.banner.restore")
    }

    pub fn discard_label(&self) -> &'static str {
        op_i18n::translate(self.ui.effective_locale(), "recovery.banner.discard")
    }

    /// How long ago the daemon wrote the draft.
    ///
    /// An unusable clock (the host has not set one, or the server's stamp is
    /// ahead of it) reports the shortest phrase rather than a negative age:
    /// the banner never invents a duration it cannot support.
    fn age_secs(&self) -> u64 {
        let now = (self.ui.now_unix_ms / 1000.0) as u64;
        if now == 0 || self.draft.saved_at > now {
            return 0;
        }
        now - self.draft.saved_at
    }

    /// Narrowest bar that still holds a sentence and both buttons, derived
    /// from the parts rather than written down: a bar below this paints
    /// nothing, and the derivation is what keeps that promise true when a
    /// padding or a button width changes.
    fn min_width() -> f32 {
        PAD_X * 2.0 + MIN_MESSAGE_W + MESSAGE_GAP + Self::button_span_width()
    }

    /// The bar's width inside `canvas`, or `None` when there is no room.
    ///
    /// Width is a function of the canvas, never of the message: a measured
    /// width would move the bar (and every hit rect with it) whenever the age
    /// phrase changes length.
    ///
    /// The Toolbar column's clearance is subtracted here rather than checked
    /// later, so a narrow canvas gives a narrower bar instead of no bar at
    /// all — the offer is the one surface whose absence costs the user work.
    fn width_for_canvas(canvas: Rect) -> f32 {
        (canvas.size.x - SIDE_MARGIN - VERTICAL_TOOLBAR_RESERVE).min(MAX_WIDTH)
    }

    /// Place the bar in the canvas region.
    ///
    /// `align_toolbar_visible` / `toast_visible` push it below those two — the
    /// strip holds exactly three floating surfaces, and the persistent one
    /// goes last so a transient notice never moves under it.
    ///
    /// `None` when the canvas cannot hold the bar: a clipped bar with
    /// hit-test geometry that disagrees with its paint is worse than no bar,
    /// the same rule the toast and align toolbar follow.
    pub fn rect_in_canvas(
        canvas: Rect,
        align_toolbar_visible: bool,
        toast_visible: bool,
    ) -> Option<Rect> {
        let width = Self::width_for_canvas(canvas);
        if width < Self::min_width() {
            return None;
        }
        // `min_x <= max_x` holds for every width this function can return
        // (see `width_for_canvas`), so this clamp is a centring that gives way
        // to the tool column on a cramped canvas, never a rejection.
        let min_x = canvas.origin.x + VERTICAL_TOOLBAR_RESERVE;
        let max_x = canvas.origin.x + canvas.size.x - width;
        let centred = canvas.origin.x + (canvas.size.x - width) / 2.0;
        let mut y = canvas.origin.y + TOP_INSET;
        if align_toolbar_visible {
            y += ALIGN_TOOLBAR_HEIGHT + STACK_GAP;
        }
        if toast_visible {
            y += TOAST_HEIGHT + STACK_GAP;
        }
        if y + RECOVERY_BANNER_HEIGHT > canvas.origin.y + canvas.size.y {
            return None;
        }
        Some(Rect::xywh(
            centred.clamp(min_x, max_x),
            y,
            width,
            RECOVERY_BANNER_HEIGHT,
        ))
    }

    /// Where the message paints, and the budget it is ellipsised into.
    pub fn message_rect(rect: Rect) -> Rect {
        let buttons = Self::button_span_width();
        Rect::xywh(
            rect.origin.x + PAD_X,
            rect.origin.y,
            (rect.size.x - PAD_X * 2.0 - buttons - MESSAGE_GAP).max(0.0),
            rect.size.y,
        )
    }

    /// Width taken by the trailing button pair, gap included.
    const fn button_span_width() -> f32 {
        BUTTON_W * 2.0 + BUTTON_GAP
    }

    /// One button's rect, laid out from the trailing edge.
    pub fn button_rect(rect: Rect, action: RecoveryBannerAction) -> Rect {
        let left = rect.origin.x + rect.size.x - PAD_X - Self::button_span_width();
        let x = match action {
            RecoveryBannerAction::Discard => left,
            RecoveryBannerAction::Restore => left + BUTTON_W + BUTTON_GAP,
        };
        Rect::xywh(
            x,
            rect.origin.y + (rect.size.y - BUTTON_H) / 2.0,
            BUTTON_W,
            BUTTON_H,
        )
    }

    /// Route a point. Paint and hit-test derive from the same rects, so a
    /// press lands where the button is drawn.
    pub fn hit_test(rect: Rect, point: Point2D) -> RecoveryBannerHit {
        if !rect.contains(point) {
            return RecoveryBannerHit::Outside;
        }
        for action in [RecoveryBannerAction::Restore, RecoveryBannerAction::Discard] {
            if Self::button_rect(rect, action).contains(point) {
                return match action {
                    RecoveryBannerAction::Restore => RecoveryBannerHit::Restore,
                    RecoveryBannerAction::Discard => RecoveryBannerHit::Discard,
                };
            }
        }
        RecoveryBannerHit::Inside
    }

    /// Surface, sentence, and the two answers.
    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = &self.theme;
        // `popover`, matching every other floating notice in the editor.
        cx.backend
            .fill_round_rect(rect, RECOVERY_BANNER_RADIUS, theme.popover);
        cx.backend
            .stroke_round_rect(rect, RECOVERY_BANNER_RADIUS, theme.border, 1.0);

        let slot = Self::message_rect(rect);
        let message = text_metrics::fit_chrome(cx.backend, &self.message(), slot.size.x, FONT_SIZE);
        let text = TextLayout::single_run(
            &message,
            text_metrics::CHROME_FONT_FAMILY,
            FONT_SIZE,
            theme.popover_foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &text,
            Point2D::new(
                slot.origin.x,
                jian_widgets::centered_text_baseline_y(rect, FONT_SIZE),
            ),
        );

        for action in [RecoveryBannerAction::Discard, RecoveryBannerAction::Restore] {
            self.paint_button(cx, Self::button_rect(rect, action), action);
        }
    }

    /// One answer. Restore is the filled button — it is the reason the bar is
    /// on screen, and the destructive-looking outline belongs to the answer
    /// that throws work away.
    fn paint_button(&self, cx: &mut PaintCx<'_>, rect: Rect, action: RecoveryBannerAction) {
        let theme = &self.theme;
        let (label, background, foreground) = match action {
            RecoveryBannerAction::Restore => (
                self.restore_label(),
                theme.primary,
                theme.primary_foreground,
            ),
            RecoveryBannerAction::Discard => {
                (self.discard_label(), theme.muted, theme.muted_foreground)
            }
        };
        cx.backend.fill_round_rect(rect, BUTTON_RADIUS, background);
        let inner = (rect.size.x - 16.0).max(0.0);
        let fitted = text_metrics::fit_chrome(cx.backend, label, inner, BUTTON_FONT_SIZE);
        let x = text_metrics::centered_text_x(cx.backend, &fitted, BUTTON_FONT_SIZE, rect);
        let text = TextLayout::single_run(
            &fitted,
            text_metrics::CHROME_FONT_FAMILY,
            BUTTON_FONT_SIZE,
            foreground.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &text,
            Point2D::new(
                x,
                jian_widgets::centered_text_baseline_y(rect, BUTTON_FONT_SIZE),
            ),
        );
    }
}

/// Whether an overlay that paints a full-viewport scrim currently owns the
/// screen.
///
/// The banner paints below the whole menu / modal / panel band, so a scrim
/// covers it. Painting it anyway would show a bar through the dialog that hit
/// its own press tier rather than the dialog's; suppressing it here means one
/// predicate decides both, and paint and press cannot disagree.
///
/// **Every new full-viewport scrim belongs in this list.** A missed one is not
/// a crash: it is a bar that looks pressable under a modal and is not.
fn scrim_owns_screen(state: &EditorState, now_ms: u64) -> bool {
    let ui = &state.editor_ui;
    ui.figma_import_open
        || ui.figma_import_in_progress
        || ui.export_dialog_open
        || ui.agent_settings_open
        || (ui.account_ui_available && ui.login_modal_open)
        || crate::widgets::MissingFontsPanel::for_editor(state).is_some()
        || crate::widgets::SceneTemplatePanel::for_editor_at(state, now_ms).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_capture_backend::CaptureBackend;
    use crate::widgets::PaintCx;
    use op_editor_core::editor_ui_state::Locale;

    const VIEWPORT: (f32, f32) = (1440.0, 900.0);

    /// A state with a draft on offer, a known clock, and English chrome.
    fn state_with_offer(saved_at: u64) -> EditorState {
        let mut state = EditorState::new();
        state.editor_ui.locale = Locale::EnUs;
        state.editor_ui.now_unix_ms = 1_800_000_000_000.0;
        state.editor_ui.note_recovery_probe(Some(RecoveryDraft {
            saved_at,
            size: 4_096,
        }));
        state
    }

    fn canvas(state: &EditorState) -> Rect {
        crate::widgets::host_canvas_geometry::canvas_rect(state, VIEWPORT.0, VIEWPORT.1)
    }

    fn banner_rect(state: &EditorState) -> Option<Rect> {
        RecoveryBanner::rect_in_canvas(canvas(state), false, false)
    }

    #[test]
    fn nothing_paints_without_an_offer() {
        // A fresh editor has nothing to offer, and `for_editor` is the only
        // gate — the geometry itself does not depend on the offer, so a host
        // that asks twice gets the same answer.
        let state = EditorState::new();
        assert!(RecoveryBanner::for_editor(&state, 0).is_none());
        assert!(banner_rect(&state).is_some());
    }

    #[test]
    fn an_answered_offer_never_paints_again() {
        let mut state = state_with_offer(1_799_999_000);
        assert!(RecoveryBanner::for_editor(&state, 0).is_some());
        state.editor_ui.answer_recovery_offer();
        assert!(
            RecoveryBanner::for_editor(&state, 0).is_none(),
            "the bar must not come back to a user who already answered"
        );
    }

    #[test]
    fn a_scrim_modal_suppresses_the_bar_rather_than_being_covered_by_it() {
        // The bar paints under the whole modal band; leaving it up would put a
        // pressable-looking notice over a dialog that owns the press.
        for open in [
            |ui: &mut EditorUiState| ui.figma_import_open = true,
            |ui: &mut EditorUiState| ui.export_dialog_open = true,
            |ui: &mut EditorUiState| ui.agent_settings_open = true,
        ] {
            let mut state = state_with_offer(1_799_999_000);
            open(&mut state.editor_ui);
            assert!(
                RecoveryBanner::for_editor(&state, 0).is_none(),
                "a scrim must hide the offer"
            );
        }
    }

    #[test]
    fn the_bar_sits_at_the_top_of_the_canvas_clear_of_the_tool_column() {
        let state = state_with_offer(1_799_999_000);
        let rect = banner_rect(&state).expect("a bar");
        let canvas = canvas(&state);

        assert!(rect.origin.y >= canvas.origin.y);
        assert!(
            rect.origin.y + RECOVERY_BANNER_HEIGHT < canvas.origin.y + canvas.size.y / 2.0,
            "it belongs in the top half"
        );
        assert!(
            rect.origin.x >= canvas.origin.x + VERTICAL_TOOLBAR_RESERVE,
            "the vertical Toolbar column keeps its clearance"
        );
        assert!(rect.origin.x + rect.size.x <= canvas.origin.x + canvas.size.x);
    }

    #[test]
    fn the_bar_stacks_below_the_align_toolbar_and_the_toast() {
        let state = state_with_offer(1_799_999_000);
        let canvas = canvas(&state);
        let alone = RecoveryBanner::rect_in_canvas(canvas, false, false).expect("a bar");
        let below_align = RecoveryBanner::rect_in_canvas(canvas, true, false).expect("a bar");
        let below_both = RecoveryBanner::rect_in_canvas(canvas, true, true).expect("a bar");

        assert_eq!(
            below_align.origin.y,
            alone.origin.y + ALIGN_TOOLBAR_HEIGHT + STACK_GAP
        );
        assert_eq!(
            below_both.origin.y,
            below_align.origin.y + TOAST_HEIGHT + STACK_GAP,
            "a transient notice is never the surface that gets covered"
        );
        // Stacking moves it down; it must not move sideways or resize.
        assert_eq!(below_both.size, alone.size);
        assert_eq!(below_both.origin.x, alone.origin.x);
    }

    #[test]
    fn a_canvas_too_narrow_for_the_bar_paints_nothing() {
        // The bar needs its own width plus the tool column's clearance; one
        // pixel less and there is no honest place to put it.
        let minimum = RecoveryBanner::min_width() + SIDE_MARGIN + VERTICAL_TOOLBAR_RESERVE;
        let narrow = Rect::xywh(0.0, 0.0, minimum - 1.0, 600.0);
        assert!(
            RecoveryBanner::rect_in_canvas(narrow, false, false).is_none(),
            "a clipped bar with stale hit-test geometry is worse than no bar"
        );
        let just_fits = Rect::xywh(0.0, 0.0, minimum, 600.0);
        let rect = RecoveryBanner::rect_in_canvas(just_fits, false, false).expect("a bar");
        assert_eq!(rect.size.x, RecoveryBanner::min_width());
        assert!(
            rect.origin.x >= just_fits.origin.x + VERTICAL_TOOLBAR_RESERVE,
            "the tool column keeps its clearance even when the bar stops being centred"
        );
    }

    #[test]
    fn a_canvas_too_short_for_the_stacked_bar_paints_nothing() {
        let short = Rect::xywh(
            0.0,
            0.0,
            1_000.0,
            TOP_INSET + ALIGN_TOOLBAR_HEIGHT + TOAST_HEIGHT + 10.0,
        );
        assert!(RecoveryBanner::rect_in_canvas(short, false, false).is_some());
        assert!(
            RecoveryBanner::rect_in_canvas(short, true, true).is_none(),
            "the bar never rides off the canvas it is anchored to"
        );
    }

    #[test]
    fn the_buttons_sit_inside_the_bar_at_the_trailing_edge() {
        let state = state_with_offer(1_799_999_000);
        let rect = banner_rect(&state).expect("a bar");
        let discard = RecoveryBanner::button_rect(rect, RecoveryBannerAction::Discard);
        let restore = RecoveryBanner::button_rect(rect, RecoveryBannerAction::Restore);

        assert_eq!(discard.size, Point2D::new(BUTTON_W, BUTTON_H));
        assert_eq!(
            discard.origin.x + discard.size.x + BUTTON_GAP,
            restore.origin.x
        );
        assert_eq!(
            restore.origin.x + restore.size.x,
            rect.origin.x + rect.size.x - PAD_X
        );
        assert!(
            discard.origin.y > rect.origin.y
                && restore.origin.y + restore.size.y < rect.origin.y + rect.size.y
        );
        // The default repair is rightmost, where a desktop dialog puts it.
        assert!(restore.origin.x > discard.origin.x);
    }

    #[test]
    fn the_message_slot_keeps_room_for_a_sentence_and_the_buttons_clear_it() {
        let state = state_with_offer(1_799_999_000);
        let rect = banner_rect(&state).expect("a bar");
        let slot = RecoveryBanner::message_rect(rect);
        let discard = RecoveryBanner::button_rect(rect, RecoveryBannerAction::Discard);

        assert!(
            slot.size.x >= MIN_MESSAGE_W,
            "at MIN_WIDTH the sentence still gets {MIN_MESSAGE_W}px, got {}",
            slot.size.x
        );
        assert!(
            slot.origin.x + slot.size.x + MESSAGE_GAP <= discard.origin.x,
            "a long sentence is ellipsised, never painted under a button"
        );
    }

    #[test]
    fn a_press_resolves_where_the_buttons_are_drawn() {
        let state = state_with_offer(1_799_999_000);
        let rect = banner_rect(&state).expect("a bar");
        let centre =
            |r: Rect| Point2D::new(r.origin.x + r.size.x / 2.0, r.origin.y + r.size.y / 2.0);

        assert_eq!(
            RecoveryBanner::hit_test(
                rect,
                centre(RecoveryBanner::button_rect(
                    rect,
                    RecoveryBannerAction::Restore
                ))
            ),
            RecoveryBannerHit::Restore
        );
        assert_eq!(
            RecoveryBanner::hit_test(
                rect,
                centre(RecoveryBanner::button_rect(
                    rect,
                    RecoveryBannerAction::Discard
                ))
            ),
            RecoveryBannerHit::Discard
        );
        assert_eq!(
            RecoveryBanner::hit_test(rect, centre(RecoveryBanner::message_rect(rect))),
            RecoveryBannerHit::Inside,
            "a press on the sentence is consumed, not passed to the canvas"
        );
        assert_eq!(
            RecoveryBanner::hit_test(rect, Point2D::new(rect.origin.x - 1.0, rect.origin.y)),
            RecoveryBannerHit::Outside,
            "the bar is a notice, not a modal"
        );
    }

    #[test]
    fn the_sentence_names_the_age_of_the_draft() {
        let state = state_with_offer(1_800_000_000 - 600);
        let banner = RecoveryBanner::for_editor(&state, 0).expect("a bar");
        assert_eq!(banner.message(), "Unsaved work found: 10m ago");
        assert_eq!(banner.restore_label(), "Restore");
        assert_eq!(banner.discard_label(), "Discard");
    }

    #[test]
    fn the_sentence_is_localized() {
        let mut state = state_with_offer(1_800_000_000 - 7_200);
        state.editor_ui.locale = Locale::Ru;
        let banner = RecoveryBanner::for_editor(&state, 0).expect("a bar");
        assert_eq!(banner.message(), "Найдена несохранённая работа: 2ч назад");
        assert_eq!(banner.restore_label(), "Восстановить");
        assert_eq!(banner.discard_label(), "Отказаться");
    }

    #[test]
    fn an_unusable_clock_never_reports_a_negative_age() {
        // The host may not have set a wall clock yet, and a daemon running
        // ahead of the tab would otherwise produce "…ago" from the future.
        let mut state = state_with_offer(1_900_000_000);
        state.editor_ui.now_unix_ms = 0.0;
        let banner = RecoveryBanner::for_editor(&state, 0).expect("a bar");
        assert_eq!(banner.message(), "Unsaved work found: just now");

        state.editor_ui.now_unix_ms = 1_800_000_000_000.0;
        let banner = RecoveryBanner::for_editor(&state, 0).expect("a bar");
        assert_eq!(banner.message(), "Unsaved work found: just now");
    }

    #[test]
    fn paint_draws_the_sentence_and_both_answers() {
        let state = state_with_offer(1_800_000_000 - 60);
        let banner = RecoveryBanner::for_editor(&state, 0).expect("a bar");
        let rect = banner_rect(&state).expect("a bar");
        let mut backend = CaptureBackend::default();
        {
            let mut cx = PaintCx {
                backend: &mut backend,
            };
            banner.paint(&mut cx, rect);
        }
        let painted: Vec<&str> = backend
            .texts
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        assert_eq!(
            painted,
            vec!["Unsaved work found: 1m ago", "Discard", "Restore"]
        );
    }
}
