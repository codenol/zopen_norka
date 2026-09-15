//! Small paint primitives shared by the collaboration panel body.

use super::*;
use crate::widgets::collab_ui::{role_label, CollabAvatarModel};
use crate::widgets::text_metrics;
use crate::{Color, RenderBackend, TextLayout};

impl CollabPanel<'_> {
    pub fn rect_at(&self, anchor: Rect, viewport: Rect) -> Rect {
        let width = COLLAB_PANEL_WIDTH.min((viewport.size.x - 16.0).max(180.0));
        let right = anchor.origin.x + anchor.size.x;
        let x = (right - width)
            .max(viewport.origin.x + 8.0)
            .min(viewport.origin.x + viewport.size.x - width - 8.0);
        let preferred_y = anchor.origin.y + anchor.size.y + 6.0;
        let height = self
            .panel_height()
            .min((viewport.origin.y + viewport.size.y - preferred_y - 8.0).max(120.0));
        Rect::xywh(x, preferred_y, width, height)
    }

    pub fn panel_height(&self) -> f32 {
        let notice = if self.model.notice.is_some() {
            NOTICE_HEIGHT + 8.0
        } else {
            0.0
        };
        let body = match &self.model.screen {
            CollabPanelScreen::Unavailable
            | CollabPanelScreen::SignInRequired
            | CollabPanelScreen::SignInUnavailable => 82.0,
            CollabPanelScreen::Home => 66.0,
            // Message + the service-region selector (label + option row).
            CollabPanelScreen::Create => 66.0 + REGION_SECTION_HEIGHT,
            CollabPanelScreen::Progress { .. } => 70.0,
            CollabPanelScreen::ConfirmOwner(confirm) => {
                CONFIRM_OWNER_HEAD_HEIGHT
                    + (confirm.authoritative.len() + usize::from(confirm.claimed_name.is_some()))
                        as f32
                        * CONFIRM_OWNER_ROW_HEIGHT
            }
            CollabPanelScreen::Join { discovered, .. } => {
                let visible_rows = discovered.len().clamp(1, MAX_VISIBLE_ENDPOINTS);
                106.0 + visible_rows as f32 * ROW_HEIGHT
            }
            CollabPanelScreen::Session {
                invite,
                connection,
                share_endpoint,
                participants,
                admission_request,
                ..
            } => {
                58.0 + if connection.is_some() {
                    CONNECTION_PATH_HEIGHT
                } else {
                    0.0
                } + if invite.is_some() { INVITE_HEIGHT } else { 0.0 }
                    + if share_endpoint.is_some() {
                        SHARE_ENDPOINT_HEIGHT
                    } else {
                        0.0
                    }
                    + if admission_request.is_some() {
                        ADMISSION_HEIGHT
                    } else {
                        0.0
                    }
                    + participants.len().min(MAX_VISIBLE_PARTICIPANTS) as f32 * ROW_HEIGHT
            }
        };
        HEADER_HEIGHT + notice + body + self.actions_height() + PAD
    }

    /// Notice text wrapped to the bubble: up to two lines, greedy per-char
    /// breaking (CJK-safe), with the second line ellipsized when the message
    /// still doesn't fit. A single-line message stays vertically centred.
    pub(super) fn paint_notice_text(&self, cx: &mut PaintCx<'_>, notice: &str, bubble: Rect) {
        const FONT: f32 = 11.0;
        let max_width = bubble.size.x - 18.0;
        let split = {
            let mut end = notice.len();
            for (index, _) in notice.char_indices().skip(1) {
                if text_metrics::measure_chrome(cx.backend, &notice[..index], FONT) > max_width {
                    end = notice
                        .char_indices()
                        .take_while(|(byte, _)| *byte < index)
                        .last()
                        .map(|(byte, _)| byte)
                        .unwrap_or(index);
                    break;
                }
            }
            end
        };
        let first = &notice[..split];
        let rest = notice[split..].trim_start();
        if rest.is_empty() {
            paint_text(
                cx,
                first,
                FONT,
                self.theme.foreground,
                Point2D::new(bubble.origin.x + 9.0, bubble.origin.y + 25.0),
                400,
            );
            return;
        }
        let second = crate::util::ellipsize_to_width(rest, max_width, |text| {
            text_metrics::measure_chrome(cx.backend, text, FONT)
        });
        paint_text(
            cx,
            first,
            FONT,
            self.theme.foreground,
            Point2D::new(bubble.origin.x + 9.0, bubble.origin.y + 17.0),
            400,
        );
        paint_text(
            cx,
            &second,
            FONT,
            self.theme.foreground,
            Point2D::new(bubble.origin.x + 9.0, bubble.origin.y + 33.0),
            400,
        );
    }

    /// One line of copy, ellipsized to the body column.
    pub(super) fn paint_message(
        &self,
        cx: &mut PaintCx<'_>,
        rect: Rect,
        body_top: f32,
        key: &'static str,
    ) {
        let message = op_i18n::translate(self.ui.effective_locale(), key);
        let message = text_metrics::fit_chrome(
            cx.backend,
            message,
            (rect.size.x - PAD * 2.0).max(0.0),
            12.0,
        );
        paint_text(
            cx,
            &message,
            12.0,
            self.theme.muted_foreground,
            Point2D::new(rect.origin.x + PAD, body_top + 29.0),
            400,
        );
    }

    /// Paint an explanation for a screen that carries no control at all.
    ///
    /// [`Self::paint_message`] ellipsizes, which is right for a caption whose
    /// tail is a detail and wrong here: this sentence IS the reason a row is
    /// missing, and a translation a word longer than the English would have the
    /// reason cut off — the same silent half-truth the missing row was. The
    /// height an explanation screen reserves holds three lines of this face, so
    /// the sentence is wrapped instead of truncated; only what still does not
    /// fit after three lines is ellipsized, and that is a signal rather than a
    /// silent amputation.
    pub(super) fn paint_explanation(
        &self,
        cx: &mut PaintCx<'_>,
        rect: Rect,
        body_top: f32,
        key: &'static str,
    ) {
        const FONT: f32 = 12.0;
        // Room for three lines inside the 82 px body these screens reserve.
        const LINE_GAP: f32 = 18.0;
        const MAX_LINES: usize = 3;
        let mut rest = op_i18n::translate(self.ui.effective_locale(), key);
        let max_width = (rect.size.x - PAD * 2.0).max(0.0);
        let origin_x = rect.origin.x + PAD;
        let mut line = 0;
        while !rest.is_empty() && line < MAX_LINES {
            let last = line + 1 == MAX_LINES;
            let split = if last {
                rest.len()
            } else {
                wrap_split(cx.backend, rest, max_width, FONT)
            };
            let (head, tail) = rest.split_at(split);
            let head = head.trim_end();
            let painted = if last {
                text_metrics::fit_chrome(cx.backend, head, max_width, FONT)
            } else {
                head.to_string()
            };
            paint_text(
                cx,
                &painted,
                FONT,
                self.theme.muted_foreground,
                Point2D::new(origin_x, body_top + 29.0 + line as f32 * LINE_GAP),
                400,
            );
            rest = tail.trim_start();
            line += 1;
        }
    }
}

/// Byte index to break `text` at so its first line fits `max_width`.
///
/// Breaks at the last space of the fitting prefix, so an English sentence
/// splits between words; a script that does not space its words (CJK) has no
/// such space and is broken at the width itself, which is where those scripts
/// wrap. `text.len()` means the whole sentence fits one line and must not be
/// broken at a space it happens to contain.
fn wrap_split(
    backend: &mut dyn RenderBackend,
    text: &str,
    max_width: f32,
    font_size: f32,
) -> usize {
    // The character boundaries are walked rather than the byte range: the
    // first prefix that overflows ends the fitting line at the boundary BEFORE
    // the character that caused it.
    let mut fitted = 0;
    let mut overflowed = false;
    for (index, _) in text.char_indices().skip(1) {
        if text_metrics::measure_chrome(backend, &text[..index], font_size) > max_width {
            overflowed = true;
            break;
        }
        fitted = index;
    }
    if !overflowed {
        return text.len();
    }
    match text[..fitted].rfind(' ') {
        Some(space) if space > 0 => space,
        _ => fitted,
    }
}

pub(super) fn paint_participant(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &EditorUiState,
    participant: &CollabAvatarModel,
    x: f32,
    y: f32,
    width: f32,
) {
    let avatar = Rect::xywh(x, y + 6.0, 22.0, 22.0);
    crate::widgets::collab_avatar_paint::paint_collab_avatar(
        cx,
        participant,
        avatar,
        9.0,
        y + 20.0,
    );
    paint_text(
        cx,
        &participant.display_name,
        12.0,
        theme.foreground,
        Point2D::new(x + 31.0, y + 21.0),
        if participant.is_self { 600 } else { 400 },
    );
    let role = role_label(ui, participant.role);
    let role_w = text_metrics::measure_chrome(cx.backend, role, 10.0);
    paint_text(
        cx,
        role,
        10.0,
        theme.muted_foreground,
        Point2D::new(x + width - role_w, y + 20.0),
        400,
    );
}

pub(super) fn paint_button(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    label: &str,
    primary: bool,
    enabled: bool,
    hovered: bool,
) {
    let background = if primary {
        theme.primary
    } else {
        theme.secondary
    };
    cx.backend.fill_round_rect(
        rect,
        6.0,
        background.with_alpha(if enabled { 1.0 } else { 0.5 }),
    );
    if hovered && enabled {
        cx.backend.fill_round_rect(rect, 6.0, theme.button_hover);
        cx.backend
            .stroke_round_rect(rect, 6.0, theme.foreground.with_alpha(0.12), 1.0);
    }
    let color = if primary {
        theme.primary_foreground
    } else {
        theme.secondary_foreground
    };
    let width = text_metrics::measure_chrome_weighted(cx.backend, label, 11.0, 500);
    // Centre against the button's own height. A hardcoded baseline was tuned
    // for the 32 px action row and left every 28 px button (admission
    // decisions, service-region options) painting its label low.
    paint_text(
        cx,
        label,
        11.0,
        color.with_alpha(if enabled { 1.0 } else { 0.6 }),
        Point2D::new(
            rect.origin.x + (rect.size.x - width) / 2.0,
            jian_widgets::centered_text_baseline_y(rect, 11.0),
        ),
        500,
    );
}

pub(super) fn paint_text(
    cx: &mut PaintCx<'_>,
    text: &str,
    size: f32,
    color: Color,
    origin: Point2D,
    weight: u16,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    cx.backend.draw_text(&layout, origin);
}
