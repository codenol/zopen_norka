//! The copy-status strip — which copy of the document the canvas is showing.
//!
//! ## Why it exists
//!
//! Issue #191, measured in the ordinary flow: reload while a recoverable draft
//! exists, send a chat turn, and the turn's result is applied to the daemon's
//! document while the canvas keeps painting something else. The transcript says
//! `<!-- APPLIED -->`, the screen does not change, and nothing tells the person
//! that the canvas and the daemon are two different copies. The recovery bar is
//! the only thing in that strip, and it answers a question about the *draft* —
//! so the one surface that could have explained the screen instead talks about
//! work the person has already half forgotten.
//!
//! This strip is the missing sentence. It shares the recovery bar's strip — the
//! free slot at the top of the canvas — and paints **only when the canvas is
//! not the daemon's current copy**, so a truthful editor says nothing at all.
//!
//! ## What it does not do
//!
//! It does not decide anything. Whether the daemon's copy or this tab's copy
//! should win is #169, and it is the operator's decision; wave 1 exists so the
//! situation is *knowable* — by the code, which has the standing
//! ([`CopyStanding`]) it can act on, and by the person, who has this strip.
//! There is no button here on purpose: a control that resolved the divergence
//! would be the decision this wave must not make.
//!
//! ## Not pressable, on purpose
//!
//! Unlike the recovery bar, this surface takes no press at all: it is a
//! statement, so it registers in no hit-test ladder and a click on it reaches
//! the canvas underneath. A hint that swallowed canvas clicks would be its own
//! small version of the bug it exists to report.

use op_editor_core::editor_ui_state::{CopyOrigin, CopyStanding, DocumentCopyStatus};
use op_editor_core::EditorState;

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::recovery_banner::RecoveryBanner;
use crate::widgets::relative_age::relative_age_label;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect, TextLayout};

/// Height of the strip: one line of chrome text in a compact bar.
pub const COPY_STATUS_HEIGHT: f32 = 28.0;
/// Corner radius, matching the other floating surfaces in this strip.
pub const COPY_STATUS_RADIUS: f32 = 8.0;
/// Message font size. One step below the recovery bar's: this line is a
/// statement about the screen, not a question the user has to answer.
const FONT_SIZE: f32 = 11.0;
/// Padding inside the leading and trailing edges.
const PAD_X: f32 = 12.0;
/// The warning rail down the leading edge — what makes this read as a warning
/// rather than as another notice.
const RAIL_W: f32 = 3.0;
/// Gap kept below the surface it stacks under.
const STACK_GAP: f32 = 6.0;
/// Distance from the canvas's top edge, matching the recovery bar's, for the
/// case where there is no recovery bar to stack under.
const TOP_INSET: f32 = 16.0;
/// Left-edge clearance for the vertical Toolbar column — the same reserve the
/// recovery bar keeps, so the two line up.
const VERTICAL_TOOLBAR_RESERVE: f32 = 56.0;
/// The narrowest strip that still says anything. Below this the sentence
/// ellipsises into a fragment that names neither the versions nor the reason,
/// which is worse than silence: a warning nobody can read is a warning nobody
/// can act on.
const MIN_WIDTH: f32 = 320.0;

/// The strip, resolved against a live editor state.
pub struct CopyStatusBar {
    theme: Theme,
    status: DocumentCopyStatus,
    locale: op_editor_core::editor_ui_state::Locale,
    /// Wall clock (unix ms) the chrome is holding, for the age phrase.
    now_unix_ms: f64,
}

impl CopyStatusBar {
    /// `None` whenever the canvas IS the daemon's copy — or when nothing has
    /// been observed yet, which is not the same thing and must not be painted
    /// as a warning.
    ///
    /// Unlike the recovery bar there is no scrim predicate here, and that is
    /// deliberate rather than an omission: this surface takes no press, so a
    /// modal that covers it leaves nothing that looks pressable and is not. The
    /// band it paints in (below every dropdown, modal and floating panel) is
    /// what does the covering, exactly as it does for the bar.
    pub fn for_editor(state: &EditorState) -> Option<Self> {
        let status = state.editor_ui.document_copy.clone();
        // `is_divergence`, NOT `!canvas_is_daemons_copy()`: a tab that has
        // observed nothing is also "not the daemon's copy", and warning about a
        // divergence nobody observed is how a warning becomes noise.
        if !status.standing().is_divergence() {
            return None;
        }
        Some(Self {
            theme: theme_for(&state.editor_ui),
            status,
            locale: state.editor_ui.effective_locale(),
            now_unix_ms: state.editor_ui.now_unix_ms,
        })
    }

    /// The sentence, localized, with the two versions interpolated.
    ///
    /// Built from the *standing*, not from the raw numbers: the numbers alone
    /// cannot say whether the divergence is a latched conflict, unpushed local
    /// work, or a daemon this tab has stopped hearing from, and those are the
    /// differences a person needs to act.
    pub fn message(&self) -> String {
        let standing = self.status.standing();
        let (key, pairs): (&'static str, Vec<(&str, String)>) = match standing {
            CopyStanding::Behind { shown, daemon } => (
                "copy.status.behind",
                vec![("shown", shown.to_string()), ("daemon", daemon.to_string())],
            ),
            CopyStanding::ConflictLatched { shown, daemon } => (
                "copy.status.conflict",
                vec![("shown", shown.to_string()), ("daemon", daemon.to_string())],
            ),
            CopyStanding::LocalEdits { shown } => {
                ("copy.status.localEdits", vec![("shown", shown.to_string())])
            }
            CopyStanding::DaemonSilent { .. } => match self.stored_age_secs() {
                Some(age) => (
                    "copy.status.silent",
                    vec![("when", relative_age_label(self.locale, age))],
                ),
                // No stored copy means the browser is not holding anything of
                // its own either: a different sentence, because claiming "the
                // copy this browser kept" when it kept none is a lie about the
                // one thing this strip exists to be honest about.
                None => ("copy.status.silentNoCopy", Vec::new()),
            },
            // Neither of these paints (`for_editor` refuses), so the fallback is
            // unreachable in paint and exists only to keep the match total.
            CopyStanding::InStep | CopyStanding::Unpulled => {
                ("copy.status.silentNoCopy", Vec::new())
            }
        };
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        match op_i18n::translate_dynamic(self.locale, key) {
            Some(template) => op_i18n::interpolate(template, &borrowed),
            // A key with no entry anywhere says so rather than painting an
            // empty bar: a raw key is a better bug report than a blank box.
            None => key.to_string(),
        }
    }

    /// How long ago the browser's own store wrote the copy it keeps, when it
    /// holds one. `None` when there is nothing stored, or when the clock cannot
    /// support the subtraction.
    fn stored_age_secs(&self) -> Option<u64> {
        let saved_at_ms = self.status.stored.as_ref()?.saved_at_ms;
        if saved_at_ms == 0 {
            return None;
        }
        let now_ms = self.now_unix_ms.max(0.0) as u64;
        if now_ms == 0 || saved_at_ms > now_ms {
            return Some(0);
        }
        Some((now_ms - saved_at_ms) / 1_000)
    }

    /// The copy the browser keeps, as a short "whose copy" clause: `None` when
    /// the store holds nothing, so the strip never names a copy that is not
    /// there.
    pub fn stored_origin(&self) -> Option<CopyOrigin> {
        self.status.stored.as_ref().map(|copy| copy.origin)
    }

    /// The strip's rect inside the canvas, or `None` when there is no room.
    ///
    /// Stacked directly under the recovery bar when one is up — the bar asks
    /// the question, and this line is the answer about the screen behind it, so
    /// it goes second rather than displacing it.
    ///
    /// `banner` is the rect the recovery bar was just painted at, so the two
    /// cannot disagree about where the strip begins — the bar's own slot moves
    /// with the align toolbar and the toast, and re-deriving it here would be a
    /// second copy of that arithmetic.
    pub fn rect_in_canvas(canvas: Rect, banner: Option<Rect>) -> Option<Rect> {
        let width = RecoveryBanner::strip_width(canvas);
        if width < MIN_WIDTH {
            return None;
        }
        let min_x = canvas.origin.x + VERTICAL_TOOLBAR_RESERVE;
        let max_x = canvas.origin.x + canvas.size.x - width;
        let centred = canvas.origin.x + (canvas.size.x - width) / 2.0;
        let y = match banner {
            Some(rect) => rect.origin.y + rect.size.y + STACK_GAP,
            None => canvas.origin.y + TOP_INSET,
        };
        if y + COPY_STATUS_HEIGHT > canvas.origin.y + canvas.size.y {
            return None;
        }
        Some(Rect::xywh(
            centred.clamp(min_x, max_x),
            y,
            width,
            COPY_STATUS_HEIGHT,
        ))
    }

    /// Surface, warning rail, and the sentence.
    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = &self.theme;
        cx.backend
            .fill_round_rect(rect, COPY_STATUS_RADIUS, theme.popover);
        cx.backend
            .stroke_round_rect(rect, COPY_STATUS_RADIUS, theme.status_warning, 1.0);
        // A rail rather than a filled warning surface: this strip sits directly
        // above the document, and a loud bar over the canvas is a worse way to
        // say "look at the versions" than a quiet line that is always there.
        let rail = Rect::xywh(rect.origin.x, rect.origin.y, RAIL_W, rect.size.y);
        cx.backend
            .fill_round_rect(rail, COPY_STATUS_RADIUS, theme.status_warning);

        let slot = Rect::xywh(
            rect.origin.x + PAD_X,
            rect.origin.y,
            (rect.size.x - PAD_X * 2.0).max(0.0),
            rect.size.y,
        );
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
    }
}

/// Paint the strip from a live state, returning the rect it used.
///
/// The host calls this immediately after the recovery bar, in the same band,
/// and passes the rect that bar was painted at (or `None` when no bar is up),
/// so the two read as one strip and this line never covers the question above
/// it. It registers in no press ladder: the strip is a statement.
pub fn paint(
    cx: &mut PaintCx<'_>,
    state: &EditorState,
    canvas: Rect,
    banner: Option<Rect>,
) -> Option<Rect> {
    let bar = CopyStatusBar::for_editor(state)?;
    let rect = CopyStatusBar::rect_in_canvas(canvas, banner)?;
    bar.paint(cx, rect);
    Some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::host_canvas_geometry::canvas_rect;
    use crate::widgets::recovery_banner::RECOVERY_BANNER_HEIGHT;

    const VIEWPORT: (f32, f32) = (1440.0, 900.0);

    /// A canvas holding daemon version `shown`, with the probe reporting
    /// `daemon`. The two are recorded separately because they are observed
    /// separately: the shown version comes from the sync client, the daemon's
    /// from the version probe.
    fn state(shown: u64, daemon: Option<u64>) -> EditorState {
        let mut state = EditorState::new();
        state.editor_ui.locale = op_editor_core::editor_ui_state::Locale::EnUs;
        state
            .editor_ui
            .document_copy
            .note_shown_version(Some(shown));
        state.editor_ui.document_copy.note_daemon_version(daemon);
        state
    }

    fn message(state: &EditorState) -> Option<String> {
        CopyStatusBar::for_editor(state).map(|bar| bar.message())
    }

    #[test]
    fn a_canvas_that_is_the_daemons_copy_says_nothing() {
        // The only standing that paints nothing. A strip that spoke while the
        // canvas was correct would train the person to ignore the one that
        // matters.
        assert!(message(&state(12, Some(12))).is_none());
    }

    #[test]
    fn a_fresh_tab_announces_no_divergence_it_has_not_observed() {
        assert!(message(&EditorState::new()).is_none());
    }

    #[test]
    fn an_ai_turn_the_canvas_has_not_painted_is_stated_with_both_versions() {
        // Issue #191 as the sentence the person reads: the versions are the
        // evidence, and without them "the daemon has something newer" is not
        // checkable.
        let message = message(&state(12, Some(14))).expect("a divergence is stated");
        assert!(message.contains("12"), "{message}");
        assert!(message.contains("14"), "{message}");
        assert!(!message.contains("copy.status"), "translated: {message}");
    }

    #[test]
    fn a_latched_conflict_names_the_conflict_and_not_merely_the_versions() {
        let mut state = state(12, Some(14));
        state.editor_ui.document_copy.note_conflict(Some(14));

        let message = message(&state).expect("a divergence is stated");
        assert!(
            message.contains("conflict"),
            "the reason a tab stops following the daemon is not the same news as a stale pull: {message}"
        );
    }

    #[test]
    fn unpushed_local_edits_are_stated_as_this_tabs_own_copy() {
        let mut state = state(12, Some(12));
        state.editor_ui.document_copy.note_local_edits(true);

        let message = message(&state).expect("a divergence is stated");
        assert!(message.contains("12"), "{message}");
        assert!(message.contains("daemon"), "{message}");
    }

    #[test]
    fn a_silent_daemon_is_not_stated_as_agreement() {
        let state = state(12, None);
        let message = message(&state).expect("silence is a divergence worth stating");
        assert!(message.contains("not answering"), "{message}");
    }

    #[test]
    fn the_strip_paints_under_the_recovery_bar_and_never_over_it() {
        // The bar asks the question; the strip is the answer about the screen
        // behind it. Painting it over the bar would hide the one surface whose
        // absence costs the user work.
        let state = EditorState::new();
        let canvas = canvas_rect(&state, VIEWPORT.0, VIEWPORT.1);

        let banner = Rect::xywh(
            canvas.origin.x + 40.0,
            canvas.origin.y + TOP_INSET,
            canvas.size.x - 80.0,
            RECOVERY_BANNER_HEIGHT,
        );
        let with_banner =
            CopyStatusBar::rect_in_canvas(canvas, Some(banner)).expect("room in a full window");
        let without_banner =
            CopyStatusBar::rect_in_canvas(canvas, None).expect("room in a full window");

        assert!(
            with_banner.origin.y >= banner.origin.y + banner.size.y,
            "the strip must not cover the recovery bar"
        );
        assert!(
            without_banner.origin.y < with_banner.origin.y,
            "with no bar up it takes the slot the bar would have used"
        );
    }

    #[test]
    fn a_canvas_too_narrow_for_the_sentence_paints_nothing() {
        // The sentence names two versions and a reason; a strip that cannot
        // hold it states nothing a person could act on.
        assert!(CopyStatusBar::rect_in_canvas(Rect::xywh(0.0, 0.0, 300.0, 600.0), None).is_none());
        assert!(CopyStatusBar::rect_in_canvas(Rect::xywh(0.0, 0.0, 700.0, 600.0), None).is_some());
    }
}
