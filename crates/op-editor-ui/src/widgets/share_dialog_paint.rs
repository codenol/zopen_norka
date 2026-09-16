//! The Share dialog's paint pass.
//!
//! Every string arrives already decided (see [`super::share_dialog_model`]);
//! this file only decides where it goes and how it reads. The one judgement it
//! does make is emphasis: a press that would be honoured is a filled primary
//! button, a press that would be refused is dimmed but still pressable (so the
//! person can read WHY), and a refusal is painted in the destructive tone
//! rather than the muted one, because "nothing happened, and here is why" must
//! not look like a caption.

use op_editor_core::editor_ui_state::share::{ShareLevelTarget, ShareRow, ShareUiState};
use op_editor_core::ShareLevel;
use op_i18n::Locale;

use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::share_dialog_layout::ShareDialogLayout;
use crate::widgets::share_dialog_model::ShareDialogModel;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Color, Point2D, Rect, TextLayout};

const TITLE_SIZE: f32 = 15.0;
const BODY_SIZE: f32 = 12.0;
const LABEL_SIZE: f32 = 11.0;
const CAPTION_SIZE: f32 = 10.5;

/// Paint the whole dialog: scrim, card, and the level picker on top.
pub(super) fn paint(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    locale: Locale,
    ui: &ShareUiState,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
    viewport: (f32, f32),
) {
    // The scrim covers the editor because the dialog is modal: a canvas that
    // still looks editable while a decision about access is open invites a
    // click that lands on the document instead of the dialog.
    cx.backend.fill_rect(
        Rect::xywh(0.0, 0.0, viewport.0, viewport.1),
        Color::BLACK.with_alpha(0.42),
    );
    let card = layout.card;
    cx.backend.fill_drop_shadow(
        Rect::xywh(card.origin.x, card.origin.y + 6.0, card.size.x, card.size.y),
        14.0,
        26.0,
        Color::BLACK.with_alpha(if theme.background.r < 0.5 { 0.5 } else { 0.18 }),
    );
    cx.backend.fill_round_rect(card, 14.0, theme.popover);
    cx.backend.stroke_round_rect(card, 14.0, theme.border, 1.0);

    header(cx, theme, ui, model, layout);
    invite_row(cx, theme, ui, model, layout);
    notice(cx, theme, model, layout);
    invitation_links(cx, theme, locale, model, layout);
    access_list(cx, theme, ui, model, layout);
    footer(cx, theme, ui, model, layout);

    if layout.level_menu.is_some() {
        level_picker(cx, theme, ui, model, layout);
    }
}

fn header(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &ShareUiState,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    draw_icon(
        cx.backend,
        Icon::Share,
        Point2D::new(layout.title.origin.x, layout.title.origin.y + 3.0),
        16.0,
        theme.muted_foreground,
        1.5,
    );
    text(
        cx,
        &model.title,
        TITLE_SIZE,
        theme.foreground,
        Point2D::new(layout.title.origin.x + 24.0, layout.title.origin.y + 16.0),
        600,
    );

    // Copy link is a quiet button: it changes nothing, and it is the one
    // control here that stays usable while a request is in flight.
    cx.backend.fill_round_rect(
        layout.copy_link,
        7.0,
        if ui.hover == Some(ShareRow::CopyLink) {
            theme.button_hover
        } else {
            theme.muted
        },
    );
    cx.backend
        .stroke_round_rect(layout.copy_link, 7.0, theme.border, 1.0);
    draw_icon(
        cx.backend,
        Icon::Copy,
        Point2D::new(
            layout.copy_link.origin.x + 10.0,
            layout.copy_link.origin.y + (layout.copy_link.size.y - 12.0) / 2.0,
        ),
        12.0,
        theme.foreground,
        1.4,
    );
    text(
        cx,
        &model.copy_link,
        11.0,
        theme.foreground,
        Point2D::new(
            layout.copy_link.origin.x + 28.0,
            jian_widgets::centered_text_baseline_y(layout.copy_link, 11.0),
        ),
        500,
    );

    draw_icon(
        cx.backend,
        Icon::Close,
        Point2D::new(layout.close.origin.x + 5.0, layout.close.origin.y + 5.0),
        14.0,
        if ui.hover == Some(ShareRow::Close) {
            theme.foreground
        } else {
            theme.muted_foreground
        },
        1.5,
    );
}

fn invite_row(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &ShareUiState,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    text(
        cx,
        &model.invite_label,
        LABEL_SIZE,
        theme.muted_foreground,
        Point2D::new(
            layout.invite_label.origin.x,
            layout.invite_label.origin.y + 12.0,
        ),
        500,
    );

    cx.backend
        .fill_round_rect(layout.invite_field, 8.0, theme.input);
    cx.backend.stroke_round_rect(
        layout.invite_field,
        8.0,
        if ui.invite_focused {
            theme.ring
        } else {
            theme.border
        },
        1.0,
    );
    let (shown, color) = if model.invite_value.is_empty() {
        (&model.invite_placeholder, theme.muted_foreground)
    } else {
        (&model.invite_value, theme.foreground)
    };
    let fitted = text_metrics::fit_chrome(
        cx.backend,
        shown,
        (layout.invite_field.size.x - 20.0).max(0.0),
        BODY_SIZE,
    );
    text(
        cx,
        &fitted,
        BODY_SIZE,
        color,
        Point2D::new(
            layout.invite_field.origin.x + 10.0,
            jian_widgets::centered_text_baseline_y(layout.invite_field, BODY_SIZE),
        ),
        400,
    );

    // A button that would be refused is dimmed, not hidden: the person can
    // still press it and read why, and a control that vanishes leaves them
    // guessing which right they are missing.
    let button = layout.invite_button;
    cx.backend.fill_round_rect(
        button,
        8.0,
        theme
            .primary
            .with_alpha(if model.invite_enabled { 1.0 } else { 0.4 }),
    );
    if ui.hover == Some(ShareRow::Invite) && model.invite_enabled {
        cx.backend
            .fill_round_rect(button, 8.0, theme.primary_foreground.with_alpha(0.08));
    }
    let button_w =
        text_metrics::measure_chrome_weighted(cx.backend, &model.invite_button, 11.5, 600);
    text(
        cx,
        &model.invite_button,
        11.5,
        theme
            .primary_foreground
            .with_alpha(if model.invite_enabled { 1.0 } else { 0.6 }),
        Point2D::new(
            button.origin.x + (button.size.x - button_w) / 2.0,
            jian_widgets::centered_text_baseline_y(button, 11.5),
        ),
        600,
    );

    level_control(
        cx,
        theme,
        &model.invite_level_label,
        layout.invite_level,
        ui.hover == Some(ShareRow::LinkLevel),
        ui.level_picker == Some(ShareLevelTarget::Invite),
        ui.can_invite(),
    );
    // The level's meaning, beside the level: a name like "Can comment and
    // invite" is a label, and this sentence is what it does.
    let hint_x = layout.invite_level.origin.x + layout.invite_level.size.x + 10.0;
    let hint_width = (layout.card.origin.x + layout.card.size.x - 20.0 - hint_x).max(0.0);
    let hint = text_metrics::fit_chrome(
        cx.backend,
        &model.invite_level_hint,
        hint_width,
        CAPTION_SIZE,
    );
    text(
        cx,
        &hint,
        CAPTION_SIZE,
        theme.muted_foreground,
        Point2D::new(
            hint_x,
            layout.invite_level.origin.y + layout.invite_level.size.y - 9.0,
        ),
        400,
    );
}

fn notice(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    // The band carries the refusal, the confirmation, or — when there is
    // nothing to report — the sentence about mail. That last case is why the
    // band is never empty: "what does Invite send" is the question this dialog
    // exists to answer honestly, and it is answered before anybody presses.
    let (message, color) = match (&model.notice, model.notice_is_refusal) {
        (Some(message), true) => (message.clone(), theme.destructive),
        (Some(message), false) => (message.clone(), theme.foreground),
        (None, _) => (model.no_mail_hint.clone(), theme.muted_foreground),
    };
    for (index, line) in wrap_two_lines(cx.backend, &message, layout.notice.size.x, LABEL_SIZE)
        .iter()
        .enumerate()
    {
        text(
            cx,
            line,
            LABEL_SIZE,
            color,
            Point2D::new(
                layout.notice.origin.x,
                layout.notice.origin.y + 12.0 + index as f32 * 15.0,
            ),
            400,
        );
    }
    if let Some(reason) = &model.invite_blocked {
        let fitted =
            text_metrics::fit_chrome(cx.backend, reason, layout.notice.size.x, CAPTION_SIZE);
        text(
            cx,
            &fitted,
            CAPTION_SIZE,
            theme.muted_foreground,
            Point2D::new(
                layout.notice.origin.x,
                layout.notice.origin.y + layout.notice.size.y - 3.0,
            ),
            400,
        );
    }
}

fn invitation_links(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    locale: Locale,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    let _ = locale;
    for link in &layout.invite_links {
        let Some((email, path)) = model.issued_links.get(link.index) else {
            continue;
        };
        cx.backend
            .fill_round_rect(link.row, 8.0, theme.muted.with_alpha(0.6));
        text(
            cx,
            email,
            LABEL_SIZE,
            theme.foreground,
            Point2D::new(link.row.origin.x + 10.0, link.row.origin.y + 13.0),
            500,
        );
        let fitted = text_metrics::fit_chrome(
            cx.backend,
            path,
            (link.copy.origin.x - link.row.origin.x - 18.0).max(0.0),
            CAPTION_SIZE,
        );
        text(
            cx,
            &fitted,
            CAPTION_SIZE,
            theme.muted_foreground,
            Point2D::new(link.row.origin.x + 10.0, link.row.origin.y + 27.0),
            400,
        );
        // The link IS the deliverable — with no mail in the product it is the
        // only thing this press produced, so it has to be copyable from where
        // it appears rather than from a sentence describing it.
        cx.backend.fill_round_rect(link.copy, 6.0, theme.secondary);
        let fitted = text_metrics::fit_chrome(
            cx.backend,
            &model.copy_invite_label,
            link.copy.size.x - 12.0,
            10.0,
        );
        let width = text_metrics::measure_chrome_weighted(cx.backend, &fitted, 10.0, 500);
        text(
            cx,
            &fitted,
            10.0,
            theme.secondary_foreground,
            Point2D::new(
                link.copy.origin.x + (link.copy.size.x - width) / 2.0,
                jian_widgets::centered_text_baseline_y(link.copy, 10.0),
            ),
            500,
        );
    }
}

fn access_list(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &ShareUiState,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    text(
        cx,
        &model.section,
        LABEL_SIZE,
        theme.muted_foreground,
        Point2D::new(layout.section.origin.x, layout.section.origin.y + 16.0),
        600,
    );

    // The caller's own row: the one row this deployment can label without a
    // directory to look an account up in.
    avatar(cx, theme, layout.you, &model.you_label);
    text(
        cx,
        &model.you_label,
        BODY_SIZE,
        theme.foreground,
        Point2D::new(layout.you.origin.x + 40.0, layout.you.origin.y + 20.0),
        500,
    );
    text(
        cx,
        &model.you_caption,
        CAPTION_SIZE,
        theme.muted_foreground,
        Point2D::new(layout.you.origin.x + 40.0, layout.you.origin.y + 34.0),
        400,
    );

    draw_icon(
        cx.backend,
        Icon::Globe,
        Point2D::new(
            layout.link_row.origin.x + 8.0,
            layout.link_row.origin.y + 14.0,
        ),
        16.0,
        if model.link_enabled {
            theme.foreground
        } else {
            theme.muted_foreground
        },
        1.5,
    );
    text(
        cx,
        &model.link_label,
        BODY_SIZE,
        theme.foreground,
        Point2D::new(layout.link_label.origin.x, layout.link_row.origin.y + 20.0),
        500,
    );
    // The caption is the honest half of this row, twice over: there is no
    // anonymous access on this deployment, so "anyone with the link" always
    // means "anyone who can sign in"; and the level the link hands out is a
    // power the whole deployment holds, which the flat caption this replaced
    // never said (#131). It is drawn at full contrast when that power includes
    // changing the document — the state a person has to be able to see without
    // reading a sentence twice.
    let caption = text_metrics::fit_chrome(
        cx.backend,
        &model.link_caption,
        layout.link_label.size.x,
        CAPTION_SIZE,
    );
    text(
        cx,
        &caption,
        CAPTION_SIZE,
        if model.link_level_writes {
            theme.foreground
        } else {
            theme.muted_foreground
        },
        Point2D::new(layout.link_label.origin.x, layout.link_row.origin.y + 34.0),
        400,
    );
    let relevelable = ui.is_owner || ShareLevel::from_rights(ui.own_rights).manages_this_document();
    level_control(
        cx,
        theme,
        &model.link_level_label,
        layout.link_level,
        ui.hover == Some(ShareRow::LinkLevel),
        ui.level_picker == Some(ShareLevelTarget::Link),
        relevelable,
    );
    switch(
        cx,
        theme,
        layout.link_switch,
        model.link_enabled,
        ui.hover == Some(ShareRow::LinkAccess),
    );

    for row in &layout.people {
        let Some(person) = model.people.get(row.index) else {
            continue;
        };
        avatar(cx, theme, row.row, &person.label);
        text(
            cx,
            &person.label,
            BODY_SIZE,
            theme.foreground,
            Point2D::new(row.row.origin.x + 40.0, row.row.origin.y + 20.0),
            500,
        );
        let caption = text_metrics::fit_chrome(
            cx.backend,
            &person.attribution,
            (row.level.origin.x - row.row.origin.x - 48.0).max(0.0),
            CAPTION_SIZE,
        );
        text(
            cx,
            &caption,
            CAPTION_SIZE,
            theme.muted_foreground,
            Point2D::new(row.row.origin.x + 40.0, row.row.origin.y + 34.0),
            400,
        );
        level_control(
            cx,
            theme,
            &person.level_label,
            row.level,
            ui.hover == Some(ShareRow::PersonLevel(row.index)),
            ui.level_picker == Some(ShareLevelTarget::Person(row.index)),
            relevelable,
        );
        if relevelable {
            let hovering = ui.hover == Some(ShareRow::PersonRemove(row.index));
            draw_icon(
                cx.backend,
                Icon::Trash,
                Point2D::new(row.remove.origin.x + 4.0, row.remove.origin.y + 5.0),
                13.0,
                if hovering {
                    theme.destructive
                } else {
                    theme.muted_foreground
                },
                1.5,
            );
        }
    }

    if let (Some(text_value), Some(rect)) = (&model.overflow_text, layout.overflow) {
        text(
            cx,
            text_value,
            CAPTION_SIZE,
            theme.muted_foreground,
            Point2D::new(rect.origin.x, rect.origin.y + 14.0),
            400,
        );
    }
}

fn footer(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &ShareUiState,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    // The live-session screen used to be what the top-bar chip opened; now that
    // the chip opens this dialog, this row is what keeps it reachable.
    cx.backend.fill_round_rect(
        layout.footer,
        8.0,
        if ui.hover == Some(ShareRow::OpenSession) {
            theme.button_hover
        } else {
            theme.muted.with_alpha(0.5)
        },
    );
    let width = text_metrics::measure_chrome_weighted(cx.backend, &model.footer, 11.0, 500);
    text(
        cx,
        &model.footer,
        11.0,
        theme.foreground,
        Point2D::new(
            layout.footer.origin.x + (layout.footer.size.x - width) / 2.0,
            jian_widgets::centered_text_baseline_y(layout.footer, 11.0),
        ),
        500,
    );
}

fn level_picker(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &ShareUiState,
    model: &ShareDialogModel,
    layout: &ShareDialogLayout,
) {
    let Some(menu) = layout.level_menu else {
        return;
    };
    cx.backend.fill_drop_shadow(
        Rect::xywh(menu.origin.x, menu.origin.y + 3.0, menu.size.x, menu.size.y),
        10.0,
        18.0,
        Color::BLACK.with_alpha(0.32),
    );
    cx.backend.fill_round_rect(menu, 10.0, theme.popover);
    cx.backend.stroke_round_rect(menu, 10.0, theme.border, 1.0);

    let selected = model.spec.level_menu.map(|menu| menu.selected);
    for (index, (_, rect)) in layout.level_options.iter().enumerate() {
        let Some((_, label, hint)) = model.level_options.get(index) else {
            continue;
        };
        if selected == Some(index) {
            cx.backend
                .fill_round_rect(*rect, 7.0, theme.accent.with_alpha(0.5));
        } else if ui.hover == Some(ShareRow::LevelOption(index)) {
            cx.backend.fill_round_rect(*rect, 7.0, theme.button_hover);
        }
        text(
            cx,
            label,
            BODY_SIZE,
            theme.foreground,
            Point2D::new(rect.origin.x + 10.0, rect.origin.y + 15.0),
            500,
        );
        let fitted = text_metrics::fit_chrome(
            cx.backend,
            hint,
            (rect.size.x - 46.0).max(0.0),
            CAPTION_SIZE,
        );
        text(
            cx,
            &fitted,
            CAPTION_SIZE,
            theme.muted_foreground,
            Point2D::new(rect.origin.x + 10.0, rect.origin.y + 29.0),
            400,
        );
        if selected == Some(index) {
            draw_icon(
                cx.backend,
                Icon::Check,
                Point2D::new(rect.origin.x + rect.size.x - 22.0, rect.origin.y + 13.0),
                13.0,
                theme.foreground,
                1.8,
            );
        }
    }
}

/// An avatar circle with the first character of the label.
///
/// A letter rather than one of the operator's role colours: the roles behind a
/// grant are not what this list is about (the LEVEL is), and colouring by
/// account id would be a decoration that means nothing.
fn avatar(cx: &mut PaintCx<'_>, theme: &Theme, row: Rect, label: &str) {
    let bounds = Rect::xywh(row.origin.x + 6.0, row.origin.y + 11.0, 26.0, 26.0);
    cx.backend.fill_oval(bounds, theme.muted);
    let initial = label
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    let width = text_metrics::measure_chrome_weighted(cx.backend, &initial, 11.0, 600);
    text(
        cx,
        &initial,
        11.0,
        theme.muted_foreground,
        Point2D::new(
            bounds.origin.x + (bounds.size.x - width) / 2.0,
            bounds.origin.y + 17.0,
        ),
        600,
    );
}

fn level_control(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    label: &str,
    rect: Rect,
    hovered: bool,
    open: bool,
    enabled: bool,
) {
    cx.backend.fill_round_rect(
        rect,
        7.0,
        if open {
            theme.accent
        } else if hovered && enabled {
            theme.button_hover
        } else {
            theme.muted
        },
    );
    cx.backend
        .stroke_round_rect(rect, 7.0, if open { theme.ring } else { theme.border }, 1.0);
    let fitted = text_metrics::fit_chrome(cx.backend, label, (rect.size.x - 34.0).max(0.0), 11.0);
    text(
        cx,
        &fitted,
        11.0,
        if enabled {
            theme.foreground
        } else {
            theme.muted_foreground
        },
        Point2D::new(
            rect.origin.x + 10.0,
            jian_widgets::centered_text_baseline_y(rect, 11.0),
        ),
        500,
    );
    draw_icon(
        cx.backend,
        Icon::ChevronDown,
        Point2D::new(
            rect.origin.x + rect.size.x - 19.0,
            rect.origin.y + (rect.size.y - 12.0) / 2.0,
        ),
        12.0,
        theme.muted_foreground,
        1.5,
    );
}

fn switch(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect, on: bool, hovered: bool) {
    let track = Rect::xywh(
        rect.origin.x,
        rect.origin.y + (rect.size.y - 16.0) / 2.0,
        30.0,
        16.0,
    );
    cx.backend.fill_round_rect(
        track,
        8.0,
        if on {
            theme.primary
        } else if hovered {
            theme.button_hover
        } else {
            theme.muted
        },
    );
    if !on {
        cx.backend.stroke_round_rect(track, 8.0, theme.border, 1.0);
    }
    cx.backend.fill_oval(
        Rect::xywh(
            if on {
                track.origin.x + track.size.x - 14.0
            } else {
                track.origin.x + 2.0
            },
            track.origin.y + 2.0,
            12.0,
            12.0,
        ),
        theme.primary_foreground,
    );
}

/// Wrap a message into at most two lines at `width`.
///
/// Two, not more: the notice band has a fixed height, so a message needing a
/// third line would be clipped into a lie. The second line ellipsizes, which
/// reads as "there is more" rather than as nothing.
fn wrap_two_lines(
    backend: &mut dyn crate::RenderBackend,
    message: &str,
    width: f32,
    size: f32,
) -> Vec<String> {
    if text_metrics::measure_chrome(backend, message, size) <= width {
        return vec![message.to_string()];
    }
    let mut first = String::new();
    let mut rest = String::new();
    let mut over = false;
    for word in message.split(' ') {
        if !over {
            let candidate = if first.is_empty() {
                word.to_string()
            } else {
                format!("{first} {word}")
            };
            if text_metrics::measure_chrome(backend, &candidate, size) <= width {
                first = candidate;
                continue;
            }
            over = true;
        }
        if !rest.is_empty() {
            rest.push(' ');
        }
        rest.push_str(word);
    }
    vec![first, text_metrics::fit_chrome(backend, &rest, width, size)]
}

fn text(cx: &mut PaintCx<'_>, value: &str, size: f32, color: Color, origin: Point2D, weight: u16) {
    let layout = TextLayout::single_run(value, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    cx.backend.draw_text(&layout, origin);
}
