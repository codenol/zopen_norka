//! The thread list panel — every conversation the document carries.
//!
//! ## What it is for
//!
//! A pin answers "what is being said about this element". The panel answers the
//! other half of a review: what is still outstanding, and where. So a row shows
//! who opened the thread, the first line of what they said, how much came after
//! it, whether it is still open — and, when the element it was pinned to is no
//! longer in the document, that there is no pin to jump to. That last mark is
//! the whole reason the panel exists rather than a list of pins: a thread whose
//! element was deleted is otherwise invisible, and it is exactly the thread
//! somebody needs to re-attach or close.
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

/// Width of the panel.
pub const PANEL_W: f32 = 304.0;
const PAD: f32 = 12.0;
const HEADER_H: f32 = 40.0;
/// The "comment on an element" toggle row.
const ARM_H: f32 = 34.0;
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
    /// 1-based position in the document's thread list — the pin's number.
    pub ordinal: usize,
    pub author: String,
    pub color: Option<Color>,
    pub excerpt: String,
    pub reply_count: usize,
    pub resolved: bool,
    /// False when the document no longer has the element the thread was pinned
    /// to, which the row says instead of offering a jump to nowhere.
    pub pinned: bool,
    pub created_at: u64,
}

/// Read the document's threads into rows.
///
/// `node_exists` answers whether the element a thread was pinned to is still in
/// the document. It is a parameter rather than a lookup here because the widget
/// layer reaches the node tree through the scene, which belongs to the caller —
/// and because a list that decided for itself what "exists" means would be the
/// second place that rule lives.
pub fn rows(
    ui: &CommentsUiState,
    locale: Locale,
    viewer_id: Option<&str>,
    node_exists: impl Fn(&str) -> bool,
) -> Vec<CommentListRow> {
    ui.threads
        .iter()
        .enumerate()
        .map(|(index, thread)| CommentListRow {
            thread_id: thread.id,
            ordinal: index + 1,
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
            pinned: node_exists(&thread.node_id),
            created_at: thread.created_at,
        })
        .collect()
}

/// What a press on the panel landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentsPanelHit {
    Close,
    /// The "comment on an element" toggle.
    ArmPin,
    /// A thread row — open it, and jump the canvas to its element.
    Row(i64),
    Inside,
    Outside,
}

/// The panel, with what it paints and the clock it dates rows by.
pub struct CommentsPanel {
    theme: Theme,
    locale: Locale,
    rows: Vec<CommentListRow>,
    /// Whether the next canvas click drops a pin.
    pin_mode: bool,
    loading: bool,
    /// The last failure, already worded for the reviewer.
    error: Option<String>,
    now_unix_ms: f64,
}

impl CommentsPanel {
    pub fn new(
        theme: Theme,
        locale: Locale,
        rows: Vec<CommentListRow>,
        pin_mode: bool,
        loading: bool,
        error: Option<String>,
        now_unix_ms: f64,
    ) -> Self {
        Self {
            theme,
            locale,
            rows,
            pin_mode,
            loading,
            error,
            now_unix_ms,
        }
    }

    /// The rows actually painted — the cap applied.
    pub fn visible_rows(&self) -> &[CommentListRow] {
        let end = self.rows.len().min(MAX_ROWS);
        &self.rows[..end]
    }

    /// Rows the cap left out.
    pub fn hidden_rows(&self) -> usize {
        self.rows.len().saturating_sub(MAX_ROWS)
    }

    pub fn height(&self) -> f32 {
        HEADER_H + ARM_H + self.visible_rows().len() as f32 * ROW_H + PAD
    }

    /// Where the panel sits: the canvas' right edge, under its top.
    pub fn rect_in_canvas(&self, canvas: Rect) -> Rect {
        let width = PANEL_W.min((canvas.size.x - 16.0).max(160.0));
        let height = self.height().min((canvas.size.y - 16.0).max(120.0));
        Rect::xywh(
            canvas.origin.x + canvas.size.x - width - 8.0,
            canvas.origin.y + 8.0,
            width,
            height,
        )
    }

    pub fn close_rect(rect: Rect) -> Rect {
        Rect::xywh(
            rect.origin.x + rect.size.x - PAD - CLOSE_SIZE,
            rect.origin.y + (HEADER_H - CLOSE_SIZE) / 2.0,
            CLOSE_SIZE,
            CLOSE_SIZE,
        )
    }

    pub fn arm_rect(rect: Rect) -> Rect {
        Rect::xywh(
            rect.origin.x + PAD,
            rect.origin.y + HEADER_H,
            rect.size.x - PAD * 2.0,
            ARM_H,
        )
    }

    /// Every row's rect, in paint order.
    pub fn row_rects(&self, rect: Rect) -> Vec<Rect> {
        let top = rect.origin.y + HEADER_H + ARM_H;
        self.visible_rows()
            .iter()
            .enumerate()
            .map(|(index, _)| {
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
        if contains(Self::arm_rect(rect), point) {
            return CommentsPanelHit::ArmPin;
        }
        for (index, row) in self.row_rects(rect).into_iter().enumerate() {
            if contains(row, point) {
                return CommentsPanelHit::Row(self.visible_rows()[index].thread_id);
            }
        }
        CommentsPanelHit::Inside
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = &self.theme;
        panel_frame(cx, theme, rect);

        let title = op_i18n::translate(self.locale, "comments.panel.title");
        let close = Self::close_rect(rect);
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

        // The toggle to arm pin mode: the panel is where a reviewer goes to
        // start a comment, so the switch that starts one lives here.
        let arm = Self::arm_rect(rect);
        cx.backend.fill_round_rect(
            arm,
            6.0,
            if self.pin_mode {
                theme.primary.with_alpha(0.22)
            } else {
                theme.secondary
            },
        );
        if self.pin_mode {
            cx.backend
                .stroke_round_rect(arm, 6.0, theme.primary.with_alpha(0.7), 1.0);
        }
        let hint_key = if self.pin_mode {
            "comments.pin.armed"
        } else {
            "comments.pin.arm"
        };
        let hint = op_i18n::translate(self.locale, hint_key);
        let hint = crate::widgets::text_metrics::fit_chrome(
            cx.backend,
            hint,
            (arm.size.x - 20.0).max(20.0),
            11.0,
        );
        text(
            cx,
            &hint,
            11.0,
            if self.pin_mode {
                theme.primary
            } else {
                theme.secondary_foreground
            },
            Point2D::new(
                arm.origin.x + 10.0,
                jian_widgets::centered_text_baseline_y(arm, 11.0),
            ),
            if self.pin_mode { 600 } else { 500 },
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
            let width = (rect.size.x - PAD * 2.0).max(20.0);
            let message =
                crate::widgets::text_metrics::fit_chrome(cx.backend, message, width, 12.0);
            caption(
                cx,
                theme,
                &message,
                width,
                Point2D::new(rect.origin.x + PAD, rect.origin.y + HEADER_H + ARM_H + 22.0),
                12.0,
            );
            return;
        }

        let row_rects = self.row_rects(rect);
        for (index, row) in self.visible_rows().iter().enumerate() {
            self.paint_row(cx, row_rects[index], row);
        }
        if self.hidden_rows() > 0 {
            let more = op_i18n::translate_with(
                self.locale,
                "comments.panel.more",
                &[("count", &self.hidden_rows().to_string())],
            );
            let last = row_rects[row_rects.len() - 1];
            caption(
                cx,
                theme,
                &more,
                (rect.size.x - PAD * 2.0).max(20.0),
                Point2D::new(rect.origin.x + PAD, last.origin.y + last.size.y + 14.0),
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

        // The row's marks, left to right: state, replies, and the absence of a
        // pin — the one that says this thread is not on the canvas any more.
        let mut x = rect.origin.x + 8.0;
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
            x += replies_w + 6.0;
        }
        if !row.pinned {
            let unpinned = op_i18n::translate(self.locale, "comments.panel.unpinned");
            let unpinned_w = chip_width(cx, unpinned);
            chip(
                cx,
                Rect::xywh(x, rect.origin.y + 40.0, unpinned_w, 14.0),
                unpinned,
                theme.status_warning,
            );
        }
    }
}

/// The collapsed form of the panel: the pill that opens it.
///
/// A review affordance has to be reachable without a keyboard shortcut and
/// without a menu somebody has to know about, so it is a button in the canvas'
/// own corner — the corner the panel itself opens into, which is what makes the
/// two read as one surface: closing the panel leaves the button where the
/// reviewer's cursor already is.
///
/// The badge is the count of OPEN threads, not of all of them: "three things
/// are still being discussed" is the reason to press it, and a closed thread is
/// not.
pub struct CommentsToggle;

/// Size of the pill.
pub const TOGGLE_W: f32 = 34.0;
pub const TOGGLE_H: f32 = 30.0;
/// Inset from the canvas corner — the same 8 px the panel uses, so the button
/// and the panel's close button do not move under the cursor.
const TOGGLE_INSET: f32 = 8.0;

impl CommentsToggle {
    pub fn rect_in_canvas(canvas: Rect) -> Rect {
        Rect::xywh(
            canvas.origin.x + canvas.size.x - TOGGLE_W - TOGGLE_INSET,
            canvas.origin.y + TOGGLE_INSET,
            TOGGLE_W,
            TOGGLE_H,
        )
    }

    pub fn contains(rect: Rect, point: Point2D) -> bool {
        contains(rect, point)
    }

    pub fn paint(
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        rect: Rect,
        open_threads: usize,
        loading: bool,
    ) {
        cx.backend.fill_round_rect(rect, 8.0, theme.popover);
        cx.backend
            .stroke_round_rect(rect, 8.0, theme.border.with_alpha(0.9), 1.0);
        draw_icon(
            cx.backend,
            Icon::MessageCircle,
            Point2D::new(rect.origin.x + 8.0, rect.origin.y + 7.0),
            16.0,
            if loading {
                theme.muted_foreground
            } else {
                theme.foreground
            },
            1.5,
        );
        if open_threads > 0 {
            // A count, not a dot: how much is outstanding is the one thing the
            // button can say before it is opened.
            let label = if open_threads > 9 {
                "9+".to_string()
            } else {
                open_threads.to_string()
            };
            let badge = Rect::xywh(
                rect.origin.x + rect.size.x - 7.0,
                rect.origin.y - 5.0,
                16.0,
                16.0,
            );
            cx.backend.fill_oval(badge, theme.primary);
            let width = crate::widgets::text_metrics::measure_chrome_weighted(
                cx.backend, &label, 10.0, 600,
            );
            text(
                cx,
                &label,
                10.0,
                theme.primary_foreground,
                Point2D::new(
                    badge.origin.x + (badge.size.x - width) / 2.0,
                    jian_widgets::centered_text_baseline_y(badge, 10.0),
                ),
                600,
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
