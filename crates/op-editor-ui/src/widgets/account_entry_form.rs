//! Getting into an account, on screen: the password form, the invitation
//! acceptance form, and the explanation a deployment with no accounts gets
//! instead of one.
//!
//! ## What this widget decides, and what it does not
//!
//! It decides nothing about WHO is signed in and nothing about whether a form
//! should exist at all — that is `EditorUiState::account_entry_mode`, computed
//! from the daemon's own `/api/auth/status` answer. [`AccountEntryForm::for_editor`]
//! returns `None` for the `Hidden` mode, so a host that paints whatever this
//! gives it cannot show a sign-in form on a deployment that has no accounts, on
//! a tab that is already signed in, or in the frame before the status answer
//! arrived.
//!
//! ## Why the geometry is a function of the mode
//!
//! The three bodies have different heights (two fields, four fields, none), and
//! paint and hit-test both walk the same [`layout`] result. A rect computed
//! twice is a rect that eventually disagrees with itself — the classic version
//! of that bug is a submit button a few pixels away from where it is drawn,
//! which only shows up on the screen it is clicked on.
//!
//! ## Why the password is painted as bullets
//!
//! The widget is handed the account-entry state, which deliberately holds no
//! password once one has been sent (see `op_editor_core::account_entry_state`).
//! While one is being typed it is masked here rather than passed around, so a
//! screenshot, a recording, or an over-the-shoulder glance gets the same
//! nothing it would get from any other password field.

use op_editor_core::{AccountEntryMode, AccountEntryState, AccountField};

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::text_metrics;
use crate::widgets::{LayoutBox, LayoutCx, PaintCx, Widget, WidgetId};
use crate::{Color, Point2D, Rect, TextLayout};
use op_editor_core::editor_ui_state::Locale;
use op_editor_core::EditorState;

/// The card is one width for every body: the form does not resize as somebody
/// types, and the explanation reads as the same surface as the form it replaces.
pub const CARD_WIDTH: f32 = 428.0;
const PAD_X: f32 = 28.0;
const PAD_TOP: f32 = 26.0;
const PAD_BOTTOM: f32 = 26.0;
const TITLE_H: f32 = 30.0;
const SUBTITLE_H: f32 = 22.0;
const HEADER_GAP: f32 = 20.0;
const LABEL_H: f32 = 16.0;
const LABEL_GAP: f32 = 5.0;
const INPUT_H: f32 = 42.0;
const FIELD_GAP: f32 = 13.0;
const ERROR_H: f32 = 20.0;
const ERROR_GAP: f32 = 12.0;
const BUTTON_H: f32 = 44.0;
const BUTTON_GAP: f32 = 16.0;
const BODY_LINE_H: f32 = 19.0;
/// The explanation is short and fixed, so the card can size to it exactly
/// instead of guessing at a scroll area.
const BODY_LINES: f32 = 4.0;
const RADIUS: f32 = 16.0;
const INPUT_RADIUS: f32 = 9.0;

/// What a press inside the account card landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountEntryHit {
    /// A text field — the host should put the caret in it.
    Field(AccountField),
    /// The submit button.
    Submit,
    /// The card itself: consumed, but no action.
    Inside,
    /// Anywhere else: the scrim. Consumed as well — the form is a gate, not a
    /// dialog, and a click outside it must not reach the editor behind it.
    Outside,
}

/// One field's rectangle, paired with the field it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldRect {
    pub field: AccountField,
    pub label: Rect,
    pub input: Rect,
}

/// Everything the card's paint and hit-test need.
///
/// Fields are a `Vec` because the two bodies have different counts and the
/// alternative — a fixed array with a live length — only moves the count
/// somewhere else to be kept right.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountEntryLayout {
    pub card: Rect,
    pub title: Rect,
    pub subtitle: Rect,
    pub fields: Vec<FieldRect>,
    /// The explanation body — painted only for [`AccountEntryMode::Unprovisioned`].
    pub body: Rect,
    /// `None` when there is nothing to submit (the explanation, and a request
    /// already in flight does not remove the button, it disables it).
    pub submit: Option<Rect>,
    pub error: Rect,
}

/// The fields a mode collects, top to bottom.
pub fn fields_for(mode: AccountEntryMode) -> &'static [AccountField] {
    // Literal slices rather than a shared `const` array: a promoted literal is
    // `'static` for certain, and the field order is this widget's to state.
    match mode {
        AccountEntryMode::SignIn => &[AccountField::Username, AccountField::Password],
        AccountEntryMode::Invite => &[
            AccountField::InviteUsername,
            AccountField::InvitePassword,
            AccountField::InvitePasswordConfirm,
            AccountField::InviteDisplayName,
        ],
        // The explanation asks for nothing: a form for a deployment with no
        // accounts could only ever refuse, so there is no field to type into.
        AccountEntryMode::Unprovisioned | AccountEntryMode::Hidden => &[],
    }
}

/// The card's height for a mode.
pub fn card_height(mode: AccountEntryMode) -> f32 {
    let fields = fields_for(mode).len() as f32;
    let body = if matches!(mode, AccountEntryMode::Unprovisioned) {
        BODY_LINES * BODY_LINE_H + ERROR_GAP
    } else {
        0.0
    };
    // The error line is reserved in EVERY body, painted or not: a form that
    // grows by 32px the moment it refuses an attempt moves the button out from
    // under the pointer that just pressed it.
    let reserved_error = ERROR_GAP + ERROR_H;
    let button = if matches!(mode, AccountEntryMode::Unprovisioned) {
        0.0
    } else {
        BUTTON_GAP + BUTTON_H
    };
    PAD_TOP
        + TITLE_H
        + SUBTITLE_H
        + HEADER_GAP
        + fields * (LABEL_H + LABEL_GAP + INPUT_H)
        + (fields - 1.0).max(0.0) * FIELD_GAP
        + body
        + reserved_error
        + button
        + PAD_BOTTOM
}

/// The card's rectangle, centred in the viewport.
///
/// Centred rather than anchored to the TopBar: this is the only surface a
/// signed-out visitor may use, and the eye should land on it.
pub fn card_rect(viewport_w: f32, viewport_h: f32, mode: AccountEntryMode) -> Rect {
    // Phone widths: keep a 16px gutter rather than centring a card wider than
    // the window it is in.
    let width = CARD_WIDTH.min((viewport_w - 32.0).max(0.0));
    let height = card_height(mode).min((viewport_h - 16.0).max(0.0));
    Rect::xywh(
        ((viewport_w - width) / 2.0).max(0.0),
        ((viewport_h - height) / 2.0).max(0.0),
        width,
        height,
    )
}

/// Where every part of the card sits.
pub fn layout(viewport_w: f32, viewport_h: f32, mode: AccountEntryMode) -> AccountEntryLayout {
    let card = card_rect(viewport_w, viewport_h, mode);
    let left = card.origin.x + PAD_X;
    let width = (card.size.x - PAD_X * 2.0).max(0.0);
    let mut y = card.origin.y + PAD_TOP;
    let band = |y: f32, height: f32| Rect::xywh(left, y, width, height);

    let title = band(y, TITLE_H);
    y += TITLE_H;
    let subtitle = band(y, SUBTITLE_H);
    y += SUBTITLE_H + HEADER_GAP;

    let mut fields = Vec::with_capacity(fields_for(mode).len());
    for field in fields_for(mode) {
        let label = band(y, LABEL_H);
        let input = band(y + LABEL_H + LABEL_GAP, INPUT_H);
        fields.push(FieldRect {
            field: *field,
            label,
            input,
        });
        y += LABEL_H + LABEL_GAP + INPUT_H + FIELD_GAP;
    }
    if !fields.is_empty() {
        // The trailing gap belongs between fields, not after the last one.
        y -= FIELD_GAP;
    }

    let body = if matches!(mode, AccountEntryMode::Unprovisioned) {
        let rect = band(y + ERROR_GAP, BODY_LINES * BODY_LINE_H);
        y += ERROR_GAP + BODY_LINES * BODY_LINE_H;
        rect
    } else {
        band(y, 0.0)
    };

    y += ERROR_GAP;
    let error = band(y, ERROR_H);
    y += ERROR_H;

    let submit =
        (!matches!(mode, AccountEntryMode::Unprovisioned)).then(|| band(y + BUTTON_GAP, BUTTON_H));

    AccountEntryLayout {
        card,
        title,
        subtitle,
        fields,
        body,
        submit,
        error,
    }
}

/// The drawn form.
pub struct AccountEntryForm<'a> {
    pub id: WidgetId,
    pub theme: Theme,
    locale: Locale,
    mode: AccountEntryMode,
    entry: &'a AccountEntryState,
    /// The viewport the form is painted into.
    ///
    /// Held rather than passed to `paint`, because `Widget::paint` receives the
    /// WIDGET's rect and the scrim this surface draws has to cover the whole
    /// window — a scrim sized to the card would leave the editor behind it
    /// looking live.
    viewport: (f32, f32),
}

impl<'a> AccountEntryForm<'a> {
    /// The form to paint for a viewport, or `None` when this state shows none.
    pub fn for_editor(state: &'a EditorState, viewport_w: f32, viewport_h: f32) -> Option<Self> {
        let mode = state.editor_ui.account_entry_mode();
        if mode == AccountEntryMode::Hidden {
            return None;
        }
        Some(Self {
            id: WidgetId::new(5700),
            theme: theme_for(&state.editor_ui),
            locale: state.editor_ui.effective_locale(),
            mode,
            entry: &state.editor_ui.account_entry,
            viewport: (viewport_w, viewport_h),
        })
    }

    pub fn mode(&self) -> AccountEntryMode {
        self.mode
    }

    /// Where this form's card sits in the viewport.
    pub fn rect(&self) -> Rect {
        card_rect(self.viewport.0, self.viewport.1, self.mode)
    }

    /// The layout paint and hit-test both walk.
    pub fn layout(&self) -> AccountEntryLayout {
        layout(self.viewport.0, self.viewport.1, self.mode)
    }

    /// What a press landed on.
    ///
    /// The scrim is part of the surface: [`AccountEntryHit::Outside`] is an
    /// answer, not a miss, so the host can swallow a press that would otherwise
    /// reach the canvas behind the form.
    pub fn hit_test(&self, point: Point2D) -> AccountEntryHit {
        let layout = self.layout();
        if layout.card.contains(point) {
            if let Some(submit) = layout.submit {
                if submit.contains(point) {
                    // A submit while a request is in flight is the same answer
                    // as a press on the card: there is nothing to send twice.
                    return if self.entry.submitting {
                        AccountEntryHit::Inside
                    } else {
                        AccountEntryHit::Submit
                    };
                }
            }
            for field in &layout.fields {
                if field.input.contains(point) {
                    return AccountEntryHit::Field(field.field);
                }
            }
            return AccountEntryHit::Inside;
        }
        AccountEntryHit::Outside
    }
}

/// The i18n key for a refusal. One key per reason, and one reason for a wrong
/// name and a wrong password — see `AccountEntryError::Rejected`.
///
/// A throttled sign-in has TWO keys rather than one, because the wait it must
/// state is only quotable when the daemon's `Retry-After` reached this shell:
/// one sentence names the number, the other says what happened and asks for a
/// wait it does not put a figure on. Inventing a figure for the second case
/// would be the same kind of untruth as calling a lockout "unavailable"
/// (issue #151).
pub fn error_key(error: op_editor_core::AccountEntryError) -> &'static str {
    use op_editor_core::AccountEntryError as E;
    match error {
        E::Rejected => "account.entry.errorRejected",
        E::Disabled => "account.entry.errorDisabled",
        E::Unprovisioned => "account.entry.errorUnavailable",
        E::WeakPassword => "account.entry.errorWeakPassword",
        E::InvalidInput => "account.entry.errorInvalid",
        E::UsernameTaken => "account.entry.errorUsernameTaken",
        E::InviteNotFound => "account.entry.errorInviteNotFound",
        E::InviteExpired => "account.entry.errorInviteExpired",
        E::InviteAlreadyAccepted => "account.entry.errorInviteAccepted",
        E::EmptyFields => "account.entry.errorEmptyFields",
        E::PasswordMismatch => "account.entry.errorPasswordMismatch",
        E::TooManyAttempts {
            retry_after_secs: Some(_),
        } => "account.entry.errorTooManyAttempts",
        E::TooManyAttempts {
            retry_after_secs: None,
        } => "account.entry.errorTooManyAttemptsUnstated",
        E::Unavailable => "account.entry.errorUnavailable",
    }
}

/// The sentence a refusal paints, with the wait substituted when the daemon
/// stated one.
///
/// Separate from [`error_key`] because exactly one refusal carries a number: a
/// `&'static str` key cannot express it, and substituting at the paint site
/// would put the placeholder's name in two places instead of one.
pub fn error_text(locale: Locale, error: op_editor_core::AccountEntryError) -> String {
    use op_editor_core::AccountEntryError as E;
    match error {
        E::TooManyAttempts {
            retry_after_secs: Some(secs),
        } => op_i18n::translate_with(locale, error_key(error), &[("seconds", &secs.to_string())]),
        other => t(locale, error_key(other)).to_string(),
    }
}

/// The label key for one field.
pub fn field_label_key(field: AccountField) -> &'static str {
    match field {
        AccountField::Username | AccountField::InviteUsername => "account.entry.username",
        AccountField::Password | AccountField::InvitePassword => "account.entry.password",
        AccountField::InvitePasswordConfirm => "account.entry.inviteConfirm",
        AccountField::InviteDisplayName => "account.entry.inviteDisplayName",
    }
}

/// What a secret field paints instead of its text.
const BULLET: char = '•';

/// The text a field shows: the real one for a name, bullets for a password.
fn painted_text(field: AccountField, text: &str) -> String {
    if !field.is_secret() {
        return text.to_string();
    }
    std::iter::repeat_n(BULLET, text.chars().count()).collect()
}

fn t(locale: Locale, key: &'static str) -> &'static str {
    op_i18n::translate(locale, key)
}

impl Widget for AccountEntryForm<'_> {
    fn id(&self) -> WidgetId {
        self.id
    }

    fn layout(&self, _cx: &LayoutCx) -> LayoutBox {
        LayoutBox {
            rect: Rect::xywh(0.0, 0.0, CARD_WIDTH, card_height(self.mode)),
        }
    }

    fn paint(&self, cx: &mut PaintCx<'_>, _rect: Rect) {
        let layout = self.layout();
        let card = layout.card;

        // The scrim: everything behind a signed-out visit is unusable anyway
        // (no route answers without a session), so it is dimmed rather than
        // left looking available.
        cx.backend.fill_rect(
            Rect::xywh(0.0, 0.0, self.viewport.0, self.viewport.1),
            Color::BLACK.with_alpha(0.45),
        );
        cx.backend.fill_drop_shadow(
            Rect::xywh(card.origin.x, card.origin.y + 6.0, card.size.x, card.size.y),
            RADIUS,
            22.0,
            Color::BLACK.with_alpha(if self.theme.background.r < 0.5 {
                0.46
            } else {
                0.16
            }),
        );
        cx.backend.fill_round_rect(card, RADIUS, self.theme.popover);
        cx.backend
            .stroke_round_rect(card, RADIUS, self.theme.border, 1.0);

        let (title_key, subtitle_key) = match self.mode {
            AccountEntryMode::Invite => {
                ("account.entry.inviteTitle", "account.entry.inviteSubtitle")
            }
            AccountEntryMode::Unprovisioned => (
                "account.entry.needsFirstAdminTitle",
                "account.entry.needsFirstAdminBody",
            ),
            _ => ("account.entry.signInTitle", "account.entry.signInSubtitle"),
        };
        draw_left(
            cx.backend,
            t(self.locale, title_key),
            layout.title.origin,
            layout.title.size.x,
            17.0,
            650,
            self.theme.foreground,
        );
        if !matches!(self.mode, AccountEntryMode::Unprovisioned) {
            draw_left(
                cx.backend,
                t(self.locale, subtitle_key),
                layout.subtitle.origin,
                layout.subtitle.size.x,
                12.0,
                400,
                self.theme.muted_foreground,
            );
        } else {
            // The explanation is a paragraph, not a one-line subtitle: it names
            // the two ways out (the environment variables, and `op admin
            // create`) and has to wrap to be read.
            paint_wrapped(
                cx.backend,
                t(self.locale, subtitle_key),
                layout.body,
                12.0,
                self.theme.muted_foreground,
                1.55,
            );
        }

        for field in &layout.fields {
            self.paint_field(cx, field);
        }

        if let Some(error) = self.entry.error {
            // Through `error_text`, not `error_key`: a throttled sign-in is the
            // one refusal that says how long the wait is, and that sentence only
            // exists once the number is in it.
            draw_left(
                cx.backend,
                &error_text(self.locale, error),
                layout.error.origin,
                layout.error.size.x,
                11.5,
                500,
                self.theme.destructive,
            );
        }

        if let Some(submit) = layout.submit {
            self.paint_submit(cx, submit);
        }
    }

    fn access_node(&self) -> accesskit::Node {
        let mut node = accesskit::Node::new(accesskit::Role::Dialog);
        node.set_label(t(self.locale, "account.entry.signInTitle"));
        node
    }
}

impl AccountEntryForm<'_> {
    fn paint_field(&self, cx: &mut PaintCx<'_>, field: &FieldRect) {
        draw_left(
            cx.backend,
            t(self.locale, field_label_key(field.field)),
            field.label.origin,
            field.label.size.x,
            11.5,
            500,
            self.theme.muted_foreground,
        );

        let focused = self.entry.focus == Some(field.field);
        cx.backend
            .fill_round_rect(field.input, INPUT_RADIUS, self.theme.muted.with_alpha(0.55));
        cx.backend.stroke_round_rect(
            field.input,
            INPUT_RADIUS,
            if focused {
                self.theme.primary
            } else {
                self.theme.border
            },
            if focused { 1.5 } else { 1.0 },
        );

        let text_size = 13.0;
        let text_x = field.input.origin.x + 12.0;
        let max_w = (field.input.size.x - 24.0).max(0.0);
        // The project's one centring rule — `rect.y + h/2 + font_size * 0.35` —
        // and not `rect.y + h/2 + font_size`, which put this text 8.45px below
        // the middle of a 42px field. `draw_left` takes the TOP of the run and
        // adds `font_size` to reach the baseline, so the centred baseline comes
        // back off it again. The submit button in this same file centres
        // correctly (`h/2 + 4.5` for a 13px label, which is 0.35 of the font),
        // which is how the fields could look wrong next to a button that looks
        // right.
        let baseline = jian_widgets::centered_text_baseline_y(field.input, text_size);
        let text = painted_text(field.field, self.entry.field(field.field));
        if text.is_empty() {
            // The caret marks the focused field even while it is empty; an
            // empty field with no caret reads as a field that does not work.
            if focused {
                paint_caret(cx.backend, text_x, field.input, self.theme.foreground);
            }
            return;
        }
        let fitted = text_metrics::fit_chrome(cx.backend, &text, max_w, text_size);
        draw_left(
            cx.backend,
            &fitted,
            Point2D::new(text_x, baseline - text_size),
            max_w,
            text_size,
            400,
            self.theme.foreground,
        );
        if focused {
            let width = text_metrics::measure_chrome(cx.backend, &fitted, text_size);
            paint_caret(
                cx.backend,
                (text_x + width).min(field.input.origin.x + field.input.size.x - 10.0),
                field.input,
                self.theme.foreground,
            );
        }
    }

    fn paint_submit(&self, cx: &mut PaintCx<'_>, submit: Rect) {
        let label = if self.entry.submitting {
            match self.mode {
                AccountEntryMode::Invite => t(self.locale, "account.entry.acceptingInvite"),
                _ => t(self.locale, "account.entry.signingIn"),
            }
        } else {
            match self.mode {
                AccountEntryMode::Invite => t(self.locale, "account.entry.acceptInvite"),
                _ => t(self.locale, "account.entry.signIn"),
            }
        };
        let fill = if self.entry.submitting {
            self.theme.primary.with_alpha(0.55)
        } else {
            self.theme.primary
        };
        cx.backend.fill_round_rect(submit, INPUT_RADIUS, fill);
        let width = text_metrics::measure_chrome_weighted(cx.backend, label, 13.0, 600);
        let layout = TextLayout::single_run(
            label,
            text_metrics::CHROME_FONT_FAMILY,
            13.0,
            self.theme.primary_foreground.to_jian(),
            Point2D::ZERO,
        )
        .with_font_weight(600);
        cx.backend.draw_text(
            &layout,
            Point2D::new(
                submit.origin.x + (submit.size.x - width) / 2.0,
                submit.origin.y + submit.size.y / 2.0 + 4.5,
            ),
        );
    }
}

/// A left-aligned run, ellipsized to the band it is given.
fn draw_left(
    backend: &mut dyn crate::RenderBackend,
    text: &str,
    origin: Point2D,
    max_w: f32,
    font_size: f32,
    weight: u16,
    color: Color,
) {
    let fitted = text_metrics::fit_chrome(backend, text, max_w, font_size);
    let layout = TextLayout::single_run(
        &fitted,
        text_metrics::CHROME_FONT_FAMILY,
        font_size,
        color.to_jian(),
        Point2D::ZERO,
    )
    .with_font_weight(weight);
    backend.draw_text(&layout, Point2D::new(origin.x, origin.y + font_size));
}

/// A wrapped paragraph, clipped to its band.
fn paint_wrapped(
    backend: &mut dyn crate::RenderBackend,
    text: &str,
    band: Rect,
    font_size: f32,
    color: Color,
    line_gap: f32,
) {
    let lines =
        super::canvas_viewport_overlay::wrap_text(backend, text, font_size, band.size.x, 400, 0.0);
    for (index, line) in lines.iter().enumerate() {
        let y = band.origin.y + (index as f32) * (font_size + line_gap) + font_size;
        if y > band.origin.y + band.size.y + font_size {
            break;
        }
        let layout = TextLayout::single_run(
            line,
            text_metrics::CHROME_FONT_FAMILY,
            font_size,
            color.to_jian(),
            Point2D::ZERO,
        );
        backend.draw_text(&layout, Point2D::new(band.origin.x, y));
    }
}

/// The text caret: a one-pixel rule beside the glyphs, drawn from `x`.
fn paint_caret(backend: &mut dyn crate::RenderBackend, x: f32, input: Rect, color: Color) {
    backend.fill_rect(
        Rect::xywh(
            x,
            input.origin.y + 10.0,
            1.0,
            (input.size.y - 20.0).max(0.0),
        ),
        color.with_alpha(0.85),
    );
}

#[cfg(test)]
#[path = "account_entry_form_tests.rs"]
mod tests;
