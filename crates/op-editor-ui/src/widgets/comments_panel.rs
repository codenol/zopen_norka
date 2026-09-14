//! The thread list — every conversation the current page carries.
//!
//! ## What it is for
//!
//! A pin answers "what is being said here". The list answers the other half of
//! a review: what is still outstanding, and where. So a row shows who opened
//! the thread, the first line of what they said, how much came after it and
//! whether it is still open — and a press on it opens the thread in its
//! popover and asks the host to bring the canvas to that point.
//!
//! ## Why it lives in the rail and not in a box over the canvas
//!
//! It is the same kind of surface as the inspector: a fixed, scrollable column
//! of facts about the document. Floating it over the design covered the very
//! page being reviewed and had to be dismissed to see what it was about. The
//! rail has one occupant at a time — the comment tool selects this one (see
//! `CommentsUiState::rail_visible`) — so the panel takes the rail's rect from
//! its host and paints inside it, the same way the property panel does.
//!
//! ## Why a row's height is a constant
//!
//! `hit_test` cannot measure text, and a list that computed its own row heights
//! from a font would place its rows one way when painted and another way when
//! clicked. So the excerpt is one line, ellipsized to the panel's width at paint
//! time ([`comment_identity::excerpt`] collapses it to a single line first), and
//! every row is [`ROW_H`] tall — which also makes the list scrollable arithmetic
//! rather than a layout engine.

use op_editor_core::editor_ui_state::CommentsUiState;
use op_i18n::Locale;

use crate::theme::Theme;
use crate::widgets::comment_identity::{age_label, author_label, excerpt, role_colour};
use crate::widgets::comment_paint::{
    caption, chip, chip_width, panel as panel_frame, role_dot, text,
};
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

const PAD: f32 = 12.0;
const HEADER_H: f32 = 40.0;
/// The line that says how a comment is added at all.
///
/// The mode's own switch is the toolbar icon, which is where a tool is looked
/// for; this is the sentence that explains what the active mode does with a
/// canvas click, and it is not a button.
const HINT_H: f32 = 26.0;
/// One thread row.
pub const ROW_H: f32 = 58.0;
/// Most rows painted at once; the rest are counted, not scrolled.
///
/// A document's review is a list somebody reads, and past a screenful the answer
/// is a filter rather than a scrollbar — the same call the file browser makes
/// with its own cap.
const MAX_ROWS: usize = 12;
const CLOSE_SIZE: f32 = 20.0;
/// Characters of a comment body a row shows.
const EXCERPT_CHARS: usize = 64;

/// One thread as the list paints it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentListRow {
    pub thread_id: i64,
    /// The number its marker shows, or `None` for a thread with no pin.
    pub ordinal: Option<usize>,
    pub author: String,
    pub color: Option<Color>,
    pub excerpt: String,
    pub reply_count: usize,
    pub resolved: bool,
    pub created_at: u64,
}

/// Read one page's threads into rows.
///
/// The page filter and the numbering are applied here rather than by the caller
/// so a row's number and its marker's number can never disagree: both this
/// function and `comment_pins::threads_for_page` walk
/// [`CommentsUiState::pinned_on_page`], so the nth pinned row is the nth marker.
/// A thread with no pin is listed without a number.
pub fn rows(
    ui: &CommentsUiState,
    locale: Locale,
    viewer_id: Option<&str>,
    page_id: &str,
) -> Vec<CommentListRow> {
    ui.threads_on_page(page_id)
        .into_iter()
        .map(|thread| CommentListRow {
            thread_id: thread.id,
            ordinal: ui.ordinal(thread.id),
            author: thread
                .opener()
                .map(|author| author_label(author, viewer_id, locale))
                .unwrap_or_else(|| {
                    op_i18n::translate(locale, "comments.author.unknown").to_string()
                }),
            color: role_colour(thread.opener().and_then(|author| author.role.as_deref())),
            excerpt: thread
                .first()
                .map(|comment| excerpt(&comment.body, EXCERPT_CHARS))
                .unwrap_or_default(),
            reply_count: thread.reply_count(),
            resolved: thread.resolved,
            created_at: thread.created_at,
        })
        .collect()
}

/// What a press on the panel landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentsPanelHit {
    /// Leave the comment tool — the rail goes back to the inspector.
    Close,
    /// A thread row — open it, and bring the canvas to its pin.
    Row(i64),
    Inside,
    Outside,
}

/// The panel, with what it paints and the clock it dates rows by.
pub struct CommentsPanel {
    theme: Theme,
    locale: Locale,
    rows: Vec<CommentListRow>,
    loading: bool,
    /// The last failure, already worded for the reviewer.
    error: Option<String>,
    now_unix_ms: f64,
    /// Open threads on the document's other pages, which this list cannot show.
    elsewhere: usize,
}

impl CommentsPanel {
    pub fn new(
        theme: Theme,
        locale: Locale,
        rows: Vec<CommentListRow>,
        loading: bool,
        error: Option<String>,
        now_unix_ms: f64,
        elsewhere: usize,
    ) -> Self {
        Self {
            theme,
            locale,
            rows,
            loading,
            error,
            now_unix_ms,
            elsewhere,
        }
    }

    /// Rows the rail has room for, cap included.
    ///
    /// The room is taken from the rect so a short rail clips its list rather
    /// than painting it out of the panel; paint and hit-test both derive the row
    /// rects from this, so the two can only agree.
    fn visible_count(&self, rect: Rect) -> usize {
        let room = ((rect.size.y - HEADER_H - HINT_H - PAD) / ROW_H).floor();
        let room = if room.is_finite() && room > 0.0 {
            room as usize
        } else {
            0
        };
        self.rows.len().min(MAX_ROWS).min(room)
    }

    /// Rows the cap left out, including the ones with no room.
    pub fn hidden_rows(&self, rect: Rect) -> usize {
        self.rows.len().saturating_sub(self.visible_count(rect))
    }

    /// Open threads this list does not show because they are on another page.
    pub fn elsewhere(&self) -> usize {
        self.elsewhere
    }

    pub fn close_rect(rect: Rect) -> Rect {
        Rect::xywh(
            rect.origin.x + rect.size.x - PAD - CLOSE_SIZE,
            rect.origin.y + (HEADER_H - CLOSE_SIZE) / 2.0,
            CLOSE_SIZE,
            CLOSE_SIZE,
        )
    }

    /// The row the hint line occupies.
    pub fn hint_rect(rect: Rect) -> Rect {
        Rect::xywh(
            rect.origin.x + PAD,
            rect.origin.y + HEADER_H,
            (rect.size.x - PAD * 2.0).max(20.0),
            HINT_H,
        )
    }

    /// Every row's rect, in paint order.
    pub fn row_rects(&self, rect: Rect) -> Vec<Rect> {
        let top = rect.origin.y + HEADER_H + HINT_H;
        (0..self.visible_count(rect))
            .map(|index| {
                Rect::xywh(
                    rect.origin.x + PAD / 2.0,
                    top + index as f32 * ROW_H,
                    rect.size.x - PAD,
                    ROW_H,
                )
            })
            .collect()
    }

    pub fn hit_test(&self, rect: Rect, point: Point2D) -> CommentsPanelHit {
        if !contains(rect, point) {
            return CommentsPanelHit::Outside;
        }
        if contains(Self::close_rect(rect), point) {
            return CommentsPanelHit::Close;
        }
        for (index, row) in self.row_rects(rect).into_iter().enumerate() {
            if contains(row, point) {
                return CommentsPanelHit::Row(self.rows[index].thread_id);
            }
        }
        CommentsPanelHit::Inside
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = &self.theme;
        panel_frame(cx, theme, rect);

        let close = Self::close_rect(rect);
        let title = op_i18n::translate(self.locale, "comments.panel.title");
        let title = crate::widgets::text_metrics::fit_chrome(
            cx.backend,
            title,
            (close.origin.x - rect.origin.x - PAD * 2.0).max(20.0),
            12.0,
        );
        text(
            cx,
            &title,
            12.0,
            theme.foreground,
            Point2D::new(rect.origin.x + PAD, rect.origin.y + 25.0),
            600,
        );
        draw_icon(
            cx.backend,
            Icon::Close,
            Point2D::new(close.origin.x + 4.0, close.origin.y + 4.0),
            close.size.x - 8.0,
            theme.muted_foreground,
            1.4,
        );

        // How a comment is added: the tool is active, so the next canvas click
        // is the answer, and this is the only place that says so.
        let hint = op_i18n::translate(self.locale, "comments.tool.hint");
        let hint_width = (rect.size.x - PAD * 2.0).max(20.0);
        let hint = crate::widgets::text_metrics::fit_chrome(cx.backend, hint, hint_width, 11.0);
        caption(
            cx,
            theme,
            &hint,
            hint_width,
            Point2D::new(rect.origin.x + PAD, rect.origin.y + HEADER_H + 16.0),
            11.0,
        );

        // The three states a list can be in that are not a list.
        if self.rows.is_empty() {
            let key = if self.error.is_some() {
                "comments.panel.error"
            } else if self.loading {
                "comments.panel.loading"
            } else {
                "comments.panel.empty"
            };
            let message = op_i18n::translate(self.locale, key);
            let message =
                crate::widgets::text_metrics::fit_chrome(cx.backend, message, hint_width, 12.0);
            caption(
                cx,
                theme,
                &message,
                hint_width,
                Point2D::new(
                    rect.origin.x + PAD,
                    rect.origin.y + HEADER_H + HINT_H + 22.0,
                ),
                12.0,
            );
            return;
        }

        let row_rects = self.row_rects(rect);
        for (index, row) in self.rows.iter().take(row_rects.len()).enumerate() {
            self.paint_row(cx, row_rects[index], row);
        }
        if self.hidden_rows(rect) > 0 {
            let more = op_i18n::translate_with(
                self.locale,
                "comments.panel.more",
                &[("count", &self.hidden_rows(rect).to_string())],
            );
            let last = row_rects[row_rects.len() - 1];
            caption(
                cx,
                theme,
                &more,
                hint_width,
                Point2D::new(rect.origin.x + PAD, last.origin.y + last.size.y + 14.0),
                10.0,
            );
        }
        if self.elsewhere > 0 {
            // The honest end of a page-scoped list: this page's review is not
            // the whole review, and the reviewer is told so without being handed
            // rows that would have nowhere to point on this page.
            let elsewhere = op_i18n::translate_with(
                self.locale,
                "comments.panel.otherPages",
                &[("count", &self.elsewhere.to_string())],
            );
            let y = rect.origin.y + rect.size.y - PAD;
            caption(
                cx,
                theme,
                &elsewhere,
                hint_width,
                Point2D::new(rect.origin.x + PAD, y),
                10.0,
            );
        }
    }

    fn paint_row(&self, cx: &mut PaintCx<'_>, rect: Rect, row: &CommentListRow) {
        let theme = &self.theme;
        if row.resolved {
            // A closed thread is dimmed, not hidden: it still has to be findable.
            cx.backend
                .fill_round_rect(rect, 6.0, theme.card.with_alpha(0.45));
        }
        let dot = Point2D::new(rect.origin.x + 10.0, rect.origin.y + 15.0);
        role_dot(cx, theme, dot, 4.0, row.color);
        let author = crate::widgets::text_metrics::fit_chrome(
            cx.backend,
            &row.author,
            (rect.size.x - 90.0).max(20.0),
            11.0,
        );
        text(
            cx,
            &author,
            11.0,
            theme
                .foreground
                .with_alpha(if row.resolved { 0.75 } else { 1.0 }),
            Point2D::new(dot.x + 10.0, rect.origin.y + 19.0),
            600,
        );
        let age = age_label(row.created_at, self.now_unix_ms, self.locale);
        let age_width = crate::widgets::text_metrics::measure_chrome(cx.backend, &age, 10.0);
        caption(
            cx,
            theme,
            &age,
            age_width,
            Point2D::new(
                rect.origin.x + rect.size.x - 8.0 - age_width,
                rect.origin.y + 19.0,
            ),
            10.0,
        );

        let body = crate::widgets::text_metrics::fit_chrome(
            cx.backend,
            &row.excerpt,
            (rect.size.x - 16.0).max(20.0),
            12.0,
        );
        text(
            cx,
            &body,
            12.0,
            theme.muted_foreground,
            Point2D::new(rect.origin.x + 8.0, rect.origin.y + 36.0),
            400,
        );

        // The row's marks, left to right: what the row points at on the canvas —
        // its marker's number, or the plain statement that it has none — then
        // state and replies.
        let mut x = rect.origin.x + 8.0;
        let (mark, mark_color) = match row.ordinal {
            Some(ordinal) => (
                op_i18n::translate_with(
                    self.locale,
                    "comments.panel.pin",
                    &[("number", &ordinal.to_string())],
                ),
                theme.muted_foreground,
            ),
            None => (
                op_i18n::translate(self.locale, "comments.panel.unpinned").to_string(),
                theme.status_warning,
            ),
        };
        let mark_w = chip_width(cx, &mark);
        chip(
            cx,
            Rect::xywh(x, rect.origin.y + 40.0, mark_w, 14.0),
            &mark,
            mark_color,
        );
        x += mark_w + 6.0;
        let state_key = if row.resolved {
            "comments.panel.resolved"
        } else {
            "comments.panel.open"
        };
        let state = op_i18n::translate(self.locale, state_key);
        let state_w = chip_width(cx, state);
        chip(
            cx,
            Rect::xywh(x, rect.origin.y + 40.0, state_w, 14.0),
            state,
            if row.resolved {
                theme.status_success
            } else {
                theme.primary
            },
        );
        x += state_w + 6.0;
        if row.reply_count > 0 {
            let replies = op_i18n::translate_with(
                self.locale,
                "comments.panel.replyCount",
                &[("count", &row.reply_count.to_string())],
            );
            let replies_w = chip_width(cx, &replies);
            chip(
                cx,
                Rect::xywh(x, rect.origin.y + 40.0, replies_w, 14.0),
                &replies,
                theme.muted_foreground,
            );
        }
    }
}

fn contains(rect: Rect, point: Point2D) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.x
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.y
}

#[cfg(test)]
#[path = "comments_panel_tests.rs"]
mod tests;
