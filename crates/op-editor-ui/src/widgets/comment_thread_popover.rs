//! The thread popover — one conversation, opened from a pin.
//!
//! ## What it is made of
//!
//! A header (who opened it, how long ago, and a close button), the opening
//! comment, up to [`MAX_VISIBLE_REPLIES`] replies with a "+N more" line when
//! there are more, a field to answer in, and one action button whose label is
//! the thread's own state: `Resolve` while it is open, `Reopen` once it is
//! closed. A closed thread also shows who closed it, because "resolved" without
//! a name is a fact nobody can follow up on.
//!
//! ## Why the height is arithmetic and not measurement
//!
//! `hit_test` must not need a render backend — the same rule the file browser's
//! cards follow — so the popover's height is computed from a **character
//! budget** per line, and paint wraps its text into the lines that budget
//! already paid for. The budget is deliberately an upper bound: paint clamps
//! what it draws to the lines the height reserved, so a comment full of wide
//! glyphs is ellipsized rather than painted over the field below it, and the
//! buttons stay where the hit-test says they are.
//!
//! ## Why a thread being written looks the same as one that exists
//!
//! A canvas click opens the composer before there is a thread to show, and the
//! reviewer should not have to learn a second panel for that: same frame, same
//! field, same position — beside the point the comment will be pinned to. The
//! difference is two things the model decides rather than a second widget: the
//! header says it is a new comment, and there is nothing to resolve yet.
//!
//! ## Why the box is kept inside the canvas
//!
//! A pin can sit at the very edge of the viewport, and a popover hung to its
//! right would leave the window. [`CommentThreadPopover::rect_at`] therefore
//! flips the box to the other side of the marker and then clamps both axes into
//! the canvas region, so whatever a reviewer clicks, the field they have to type
//! into is on screen. A pin that has been panned out of the canvas entirely
//! anchors the same way and the box lands against the nearest edge.

use op_editor_core::editor_ui_state::{CommentComposer, CommentsUiState};
use op_i18n::Locale;

use crate::theme::Theme;
use crate::widgets::comment_identity::{age_label, author_label, role_colour};
use crate::widgets::comment_paint::{
    button, caption, chip, chip_width, input, panel as panel_frame, role_dot, text,
};
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

/// Width of the popover.
pub const POPOVER_W: f32 = 304.0;
const PAD: f32 = 12.0;
const HEADER_H: f32 = 38.0;
/// One line of comment text.
const LINE_H: f32 = 16.0;
/// The author/age line above each comment.
const AUTHOR_H: f32 = 15.0;
/// Vertical gap between two comments.
const COMMENT_GAP: f32 = 10.0;
/// The resolved banner under the header.
const RESOLVED_H: f32 = 24.0;
const COMPOSER_H: f32 = 32.0;
const ACTION_H: f32 = 30.0;
const CLOSE_SIZE: f32 = 20.0;
const SEND_W: f32 = 68.0;
const ACTION_W: f32 = 116.0;

/// Characters one line of comment text is budgeted for.
///
/// Derived from the panel's inner width at the 12 px body size the chrome uses:
/// roughly 0.55 em per Latin character over a 280 px run. It is a budget, not a
/// measurement — see the module notes.
pub const CHARS_PER_LINE: usize = 38;
/// Most lines the opening comment is given.
const MAX_BODY_LINES: usize = 6;
/// Most lines each reply is given.
const MAX_REPLY_LINES: usize = 3;
/// Most replies shown before they are counted instead.
const MAX_VISIBLE_REPLIES: usize = 3;

/// One comment, read for paint.
///
/// Owned rather than borrowed from the state: the popover is rebuilt every frame
/// from a conversation of a few short strings, and owning it keeps the press
/// path free to mutate the state it was built from (see
/// `comments_flow::press_popover`).
#[derive(Debug, Clone, PartialEq)]
pub struct CommentView {
    /// Already resolved to a name — "You", "Local operator", "Unknown", or the
    /// name the server recorded.
    pub author: String,
    pub color: Option<Color>,
    pub body: String,
    pub created_at: u64,
}

/// The thread a popover is showing.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadView {
    pub id: i64,
    pub resolved: bool,
    pub resolved_by: Option<String>,
    /// Oldest first.
    pub comments: Vec<CommentView>,
}

/// What the popover is about.
///
/// A comment being written carries no position: the popover is placed against
/// the anchor the click recorded (see `comments_flow::popover_anchor`), and the
/// header only has to say that this is a new one.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentComposerView {
    /// An existing thread.
    Thread(Box<ThreadView>),
    /// A comment that has not been written yet.
    NewThread,
}

/// Everything the popover paints and decides from.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentPopoverModel {
    pub locale: Locale,
    pub composer: CommentComposerView,
    /// The text in the field, shared with the state's draft.
    pub draft: String,
    /// Whether the field owns the keyboard.
    pub focused: bool,
    /// Whether the field is offering a reply (`Reply…`) or a new comment
    /// (`Write a comment…`).
    pub reply_placeholder: bool,
}

impl CommentPopoverModel {
    /// Read the model out of the comment state, or `None` when nothing is open.
    pub fn for_comments(
        ui: &CommentsUiState,
        locale: Locale,
        viewer_id: Option<&str>,
        focused: bool,
    ) -> Option<Self> {
        let view = match ui.composer()? {
            CommentComposer::Thread(id) => {
                let thread = ui.thread(id)?;
                CommentComposerView::Thread(Box::new(ThreadView {
                    id: thread.id,
                    resolved: thread.resolved,
                    resolved_by: thread.resolved_by_label().map(|name| name.to_string()),
                    comments: thread
                        .comments
                        .iter()
                        .map(|comment| CommentView {
                            author: author_label(&comment.author, viewer_id, locale),
                            color: role_colour(comment.author.role.as_deref()),
                            body: comment.body.clone(),
                            created_at: comment.created_at,
                        })
                        .collect(),
                }))
            }
            CommentComposer::NewThread(_) => CommentComposerView::NewThread,
        };
        let reply_placeholder = matches!(view, CommentComposerView::Thread(_));
        Some(Self {
            locale,
            composer: view,
            draft: ui.draft().to_string(),
            focused,
            reply_placeholder,
        })
    }

    /// The thread this popover would close or reopen.
    pub fn thread_id(&self) -> Option<i64> {
        match &self.composer {
            CommentComposerView::Thread(thread) => Some(thread.id),
            CommentComposerView::NewThread => None,
        }
    }

    pub fn resolved(&self) -> bool {
        match &self.composer {
            CommentComposerView::Thread(thread) => thread.resolved,
            CommentComposerView::NewThread => false,
        }
    }

    /// Whether the field's text can be sent.
    pub fn can_send(&self) -> bool {
        let draft = self.draft.trim();
        !draft.is_empty()
            && draft.chars().count() <= op_editor_core::editor_ui_state::MAX_COMMENT_CHARS
    }

    /// The comments that are painted, each with the lines its body is budgeted.
    fn visible(&self) -> Vec<(usize, &CommentView, usize)> {
        let CommentComposerView::Thread(thread) = &self.composer else {
            return Vec::new();
        };
        let mut out: Vec<(usize, &CommentView, usize)> = Vec::new();
        for (index, comment) in thread.comments.iter().enumerate() {
            let max_lines = if index == 0 {
                MAX_BODY_LINES
            } else {
                MAX_REPLY_LINES
            };
            if index > MAX_VISIBLE_REPLIES {
                break;
            }
            out.push((index, comment, budget_lines(&comment.body, max_lines)));
        }
        out
    }

    /// How many replies the popover counts instead of showing.
    fn hidden_replies(&self) -> usize {
        let CommentComposerView::Thread(thread) = &self.composer else {
            return 0;
        };
        thread
            .comments
            .len()
            .saturating_sub(1)
            .saturating_sub(MAX_VISIBLE_REPLIES)
    }
}

/// How many lines a body is budgeted, capped.
pub fn budget_lines(body: &str, max_lines: usize) -> usize {
    let chars = body.chars().count();
    if chars == 0 {
        // An empty body still occupies its own line, so the row does not
        // collapse into the author line above it.
        return 1;
    }
    chars.div_ceil(CHARS_PER_LINE).clamp(1, max_lines)
}

/// What a press inside the popover landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentPopoverHit {
    Close,
    /// The reply field: give it the keyboard.
    FocusInput,
    /// Send the draft.
    Send,
    /// Close or reopen the thread.
    Resolution,
    /// Anywhere on the panel that is not an action.
    Inside,
    /// Outside the panel — the press that closes it.
    Outside,
}

/// The popover, with the theme and clock it paints against.
pub struct CommentThreadPopover {
    model: CommentPopoverModel,
    theme: Theme,
    now_unix_ms: f64,
}

impl CommentThreadPopover {
    pub fn new(model: CommentPopoverModel, theme: Theme, now_unix_ms: f64) -> Self {
        Self {
            model,
            theme,
            now_unix_ms,
        }
    }

    pub fn model(&self) -> &CommentPopoverModel {
        &self.model
    }

    /// The popover's height for its current contents.
    pub fn height(&self) -> f32 {
        let mut height = HEADER_H;
        if self.model.resolved() {
            height += RESOLVED_H;
        }
        height += PAD;
        let visible = self.model.visible();
        if visible.is_empty() {
            // The thread being written: one line saying what the comment is
            // about, so the reviewer knows which element they clicked.
            height += AUTHOR_H + LINE_H + COMMENT_GAP;
        }
        for (_, _, lines) in &visible {
            height += AUTHOR_H + *lines as f32 * LINE_H + COMMENT_GAP;
        }
        if self.model.hidden_replies() > 0 {
            height += AUTHOR_H;
        }
        height += COMPOSER_H + 8.0 + ACTION_H + PAD;
        height
    }

    /// Where the popover paints, anchored under its pin and kept inside the
    /// canvas.
    ///
    /// `anchor` is the pin's own rect, so the popover follows the point the
    /// comment was left at as the canvas pans — and, for a comment being
    /// written, the point the pin is about to occupy. A thread whose anchor is
    /// not on the page being shown passes the canvas' own corner instead: the
    /// conversation is still readable, it just has nowhere on this page to hang
    /// from.
    pub fn rect_at(&self, anchor: Rect, canvas: Rect) -> Rect {
        let width = POPOVER_W.min((canvas.size.x - 16.0).max(160.0));
        let x = anchor.origin.x + anchor.size.x + 8.0;
        let x = if x + width > canvas.origin.x + canvas.size.x - 8.0 {
            // No room on the right: flip to the pin's other side rather than
            // overlapping it, which would hide the marker the click just used.
            anchor.origin.x - width - 8.0
        } else {
            x
        };
        let height = self.height().min((canvas.size.y - 16.0).max(140.0));
        let x = x.clamp(
            canvas.origin.x + 8.0,
            (canvas.origin.x + canvas.size.x - width - 8.0).max(canvas.origin.x + 8.0),
        );
        let y = anchor.origin.y.clamp(
            canvas.origin.y + 8.0,
            (canvas.origin.y + canvas.size.y - height - 8.0).max(canvas.origin.y + 8.0),
        );
        Rect::xywh(x, y, width, height)
    }

    pub fn close_rect(rect: Rect) -> Rect {
        Rect::xywh(
            rect.origin.x + rect.size.x - PAD - CLOSE_SIZE,
            rect.origin.y + (HEADER_H - CLOSE_SIZE) / 2.0,
            CLOSE_SIZE,
            CLOSE_SIZE,
        )
    }

    pub fn input_rect(rect: Rect) -> Rect {
        let y = rect.origin.y + rect.size.y - PAD - ACTION_H - 8.0 - COMPOSER_H;
        Rect::xywh(
            rect.origin.x + PAD,
            y,
            (rect.size.x - PAD * 3.0 - SEND_W).max(40.0),
            COMPOSER_H,
        )
    }

    pub fn send_rect(rect: Rect) -> Rect {
        let input = Self::input_rect(rect);
        Rect::xywh(
            input.origin.x + input.size.x + 8.0,
            input.origin.y,
            SEND_W,
            COMPOSER_H,
        )
    }

    pub fn resolution_rect(rect: Rect) -> Rect {
        Rect::xywh(
            rect.origin.x + PAD,
            rect.origin.y + rect.size.y - PAD - ACTION_H,
            ACTION_W.min(rect.size.x - PAD * 2.0),
            ACTION_H,
        )
    }

    /// What a press at `point` means.
    pub fn hit_test(&self, rect: Rect, point: Point2D) -> CommentPopoverHit {
        if !contains(rect, point) {
            return CommentPopoverHit::Outside;
        }
        if contains(Self::close_rect(rect), point) {
            return CommentPopoverHit::Close;
        }
        if contains(Self::send_rect(rect), point) {
            return CommentPopoverHit::Send;
        }
        if contains(Self::input_rect(rect), point) {
            return CommentPopoverHit::FocusInput;
        }
        if self.model.thread_id().is_some() && contains(Self::resolution_rect(rect), point) {
            return CommentPopoverHit::Resolution;
        }
        CommentPopoverHit::Inside
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let theme = &self.theme;
        panel_frame(cx, theme, rect);
        let mut y = rect.origin.y + PAD;

        // Header: the thread's opener, or the element a new comment is about.
        let (title, color) = match &self.model.composer {
            CommentComposerView::Thread(thread) => match thread.comments.first() {
                Some(first) => (first.author.clone(), first.color),
                None => (
                    op_i18n::translate(self.model.locale, "comments.panel.title").to_string(),
                    None,
                ),
            },
            CommentComposerView::NewThread => (
                op_i18n::translate(self.model.locale, "comments.composer.newTitle").to_string(),
                None,
            ),
        };
        let dot = Point2D::new(rect.origin.x + PAD + 6.0, y + 12.0);
        role_dot(cx, theme, dot, 5.0, color);
        let author_x = dot.x + 12.0;
        let close = Self::close_rect(rect);
        let title_width = (close.origin.x - 8.0 - author_x).max(20.0);
        let title = crate::widgets::text_metrics::fit_chrome(cx.backend, &title, title_width, 12.0);
        text(
            cx,
            &title,
            12.0,
            theme.foreground,
            Point2D::new(author_x, y + 16.0),
            600,
        );

        if let CommentComposerView::Thread(thread) = &self.model.composer {
            if let Some(first) = thread.comments.first() {
                let age = age_label(first.created_at, self.now_unix_ms, self.model.locale);
                let age_width =
                    crate::widgets::text_metrics::measure_chrome(cx.backend, &age, 10.0);
                if age_width + 12.0 < close.origin.x - author_x - title_width {
                    caption(
                        cx,
                        theme,
                        &age,
                        age_width,
                        Point2D::new(close.origin.x - 8.0 - age_width, y + 16.0),
                        10.0,
                    );
                }
            }
        }
        let close_color = theme.muted_foreground;
        draw_icon(
            cx.backend,
            Icon::Close,
            Point2D::new(close.origin.x + 4.0, close.origin.y + 4.0),
            close.size.x - 8.0,
            close_color,
            1.4,
        );
        y += HEADER_H;

        if self.model.resolved() {
            let label = op_i18n::translate(self.model.locale, "comments.panel.resolved");
            let chip_rect = Rect::xywh(rect.origin.x + PAD, y + 2.0, chip_width(cx, label), 18.0);
            chip(cx, chip_rect, label, theme.status_success);
            if let CommentComposerView::Thread(thread) = &self.model.composer {
                if let Some(name) = &thread.resolved_by {
                    let resolved_by = op_i18n::translate_with(
                        self.model.locale,
                        "comments.resolvedBy",
                        &[("name", name)],
                    );
                    caption(
                        cx,
                        theme,
                        &resolved_by,
                        (rect.size.x - PAD * 3.0 - chip_rect.size.x).max(20.0),
                        Point2D::new(chip_rect.origin.x + chip_rect.size.x + 8.0, y + 15.0),
                        10.0,
                    );
                }
            }
            y += RESOLVED_H;
        }

        y += PAD / 2.0;
        let body_width = (rect.size.x - PAD * 2.0).max(40.0);
        for (index, comment, lines) in self.model.visible() {
            let author_y = y + AUTHOR_H - 4.0;
            role_dot(
                cx,
                theme,
                Point2D::new(rect.origin.x + PAD + 4.0, author_y - 4.0),
                4.0,
                comment.color,
            );
            text(
                cx,
                &comment.author,
                11.0,
                theme.foreground,
                Point2D::new(rect.origin.x + PAD + 14.0, author_y),
                if index == 0 { 600 } else { 500 },
            );
            let age = age_label(comment.created_at, self.now_unix_ms, self.model.locale);
            let age_width = crate::widgets::text_metrics::measure_chrome(cx.backend, &age, 10.0);
            caption(
                cx,
                theme,
                &age,
                age_width,
                Point2D::new(rect.origin.x + rect.size.x - PAD - age_width, author_y),
                10.0,
            );
            y += AUTHOR_H;
            y = self.paint_body(cx, &comment.body, rect.origin.x + PAD, y, body_width, lines);
            y += COMMENT_GAP;
        }

        if self.model.hidden_replies() > 0 {
            let more = op_i18n::translate_with(
                self.model.locale,
                "comments.panel.more",
                &[("count", &self.model.hidden_replies().to_string())],
            );
            caption(
                cx,
                theme,
                &more,
                body_width,
                Point2D::new(rect.origin.x + PAD, y + AUTHOR_H - 4.0),
                10.0,
            );
        }

        // The composer and the one action.
        let placeholder_key = if self.model.reply_placeholder {
            "comments.composer.replyPlaceholder"
        } else {
            "comments.composer.placeholder"
        };
        let placeholder = op_i18n::translate(self.model.locale, placeholder_key);
        input(
            cx,
            theme,
            Self::input_rect(rect),
            placeholder,
            &self.model.draft,
            self.model.focused,
        );
        let send_label = op_i18n::translate(self.model.locale, "comments.composer.send");
        button(
            cx,
            theme,
            Self::send_rect(rect),
            send_label,
            true,
            self.model.can_send(),
            false,
        );
        if self.model.thread_id().is_some() {
            let (key, primary) = if self.model.resolved() {
                ("comments.action.reopen", false)
            } else {
                ("comments.action.resolve", false)
            };
            let label = op_i18n::translate(self.model.locale, key);
            button(
                cx,
                theme,
                Self::resolution_rect(rect),
                label,
                primary,
                true,
                false,
            );
        }
    }

    /// Wrap one body into the lines its height already paid for and draw them.
    fn paint_body(
        &self,
        cx: &mut PaintCx<'_>,
        body: &str,
        x: f32,
        y: f32,
        width: f32,
        lines: usize,
    ) -> f32 {
        // `wrap_text` measures with the real backend, so a line here can be
        // shorter than the character budget predicted — never longer than the
        // budget's line COUNT, which is what the height was computed from.
        let wrapped = crate::widgets::canvas_viewport_overlay::wrap_text(
            cx.backend, body, 12.0, width, 400, 0.0,
        );
        let clipped = wrapped.len() > lines;
        for (index, line) in wrapped.iter().take(lines).enumerate() {
            let value = if clipped && index + 1 == lines {
                crate::widgets::text_metrics::fit_chrome(
                    cx.backend,
                    &format!("{line} …"),
                    width,
                    12.0,
                )
            } else {
                line.clone()
            };
            text(
                cx,
                &value,
                12.0,
                self.theme.foreground.with_alpha(0.92),
                Point2D::new(x, y + 11.0 + index as f32 * LINE_H),
                400,
            );
        }
        y + lines as f32 * LINE_H
    }
}

fn contains(rect: Rect, point: Point2D) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.x
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.y
}

#[cfg(test)]
#[path = "comment_thread_popover_tests.rs"]
mod tests;
