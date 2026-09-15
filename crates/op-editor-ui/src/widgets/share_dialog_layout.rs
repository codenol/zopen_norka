//! Where every part of the Share dialog sits.
//!
//! One function, walked by paint, hit-test and the level picker alike. The
//! alternative — each of the three computing its own y-walk — is how a button
//! ends up a few pixels away from where it is drawn, and it shows up only on
//! the screen somebody clicks it on.
//!
//! ## Why the card has a fixed height for a given list
//!
//! Nothing in this dialog wraps: the invite field is one line, a person row is
//! one line plus a caption, and the notice is capped at two lines. That is what
//! makes the layout arithmetic instead of a text engine, and it is why a row's
//! height is a constant a hit-test can trust.

use op_editor_core::ShareLevel;

use crate::widgets::top_bar_geometry::estimated_text_width;
use crate::{Point2D, Rect};

/// Card width. Figma's share sheet is ~450; this is a little wider because a
/// person row carries a level label AND an attribution sentence on one line.
pub const CARD_W: f32 = 480.0;
/// Inner padding, left and right.
pub const PAD: f32 = 20.0;
/// Header band — title, Copy link, close.
pub const HEADER_H: f32 = 52.0;
const BODY_TOP_GAP: f32 = 14.0;
const INVITE_LABEL_H: f32 = 18.0;
const INVITE_LABEL_GAP: f32 = 6.0;
pub const INPUT_H: f32 = 36.0;
const INVITE_BUTTON_W: f32 = 84.0;
const INVITE_BUTTON_GAP: f32 = 8.0;
const LEVEL_ROW_H: f32 = 32.0;
const NOTICE_H: f32 = 36.0;
const SECTION_H: f32 = 32.0;
const ROW_H: f32 = 46.0;
const OVERFLOW_H: f32 = 22.0;
const FOOTER_H: f32 = 42.0;
const BODY_BOTTOM_GAP: f32 = 10.0;
/// Most person rows painted. Past this the rest are counted.
///
/// A cap rather than a scroll area, the same call the comments list makes:
/// past a screenful the answer is "who are these people" rather than a
/// scrollbar, and a dialog that grows past the window has no bottom to reach.
pub const MAX_PERSON_ROWS: usize = 6;
/// Side of every square control in the card (close, level chevrons, remove).
pub const CONTROL: f32 = 24.0;
/// Width of the level control on a person row.
const PERSON_LEVEL_W: f32 = 148.0;
/// Width of the level control beside the link switch, and of the invite level.
const WIDE_LEVEL_W: f32 = 208.0;

/// Level picker popover.
pub const LEVEL_MENU_W: f32 = 252.0;
pub const LEVEL_OPTION_H: f32 = 40.0;
const LEVEL_MENU_PAD: f32 = 6.0;
/// One invitation-link row, painted after an invitation was issued.
///
/// The product sends no mail, so the link IS the deliverable and it needs a
/// row of its own with a copy control — a notice sentence saying "created"
/// would leave the person with nothing to pass on.
pub const INVITE_LINK_H: f32 = 34.0;
/// Most invitation links shown at once; the rest are counted.
pub const MAX_INVITE_LINK_ROWS: usize = 3;

/// What the layout needs to know about the state it is laying out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareLayoutSpec {
    /// How many people rows to place (already capped at [`MAX_PERSON_ROWS`]).
    pub person_rows: usize,
    /// Whether anybody beyond the cap holds access.
    pub overflow: bool,
    /// Invitation links from the last press (capped at [`MAX_INVITE_LINK_ROWS`]).
    pub invite_links: usize,
    /// Open picker: how many options it offers and which is current.
    pub level_menu: Option<LevelMenuSpec>,
}

/// The open level picker: its size and which option is ticked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelMenuSpec {
    pub options: usize,
    pub selected: usize,
    /// Which control opened it, so the popover can hang off the right edge.
    pub anchor: LevelAnchor,
}

/// Which control a level picker was opened from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelAnchor {
    Invite,
    Link,
    Person(usize),
}

/// One person's row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharePersonRect {
    /// Index into the state's list, so a hit carries the subject and not a
    /// screen position.
    pub index: usize,
    pub row: Rect,
    /// The level control that opens the picker.
    pub level: Rect,
    /// The remove control.
    pub remove: Rect,
}

/// Every rectangle the dialog paints or hits.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareDialogLayout {
    pub card: Rect,
    pub header: Rect,
    pub title: Rect,
    pub copy_link: Rect,
    pub close: Rect,
    pub invite_label: Rect,
    pub invite_field: Rect,
    pub invite_button: Rect,
    /// The level the next invitation carries.
    pub invite_level: Rect,
    pub notice: Rect,
    pub section: Rect,
    /// The caller's own row.
    pub you: Rect,
    pub link_row: Rect,
    pub link_label: Rect,
    pub link_level: Rect,
    pub link_switch: Rect,
    pub people: Vec<SharePersonRect>,
    /// Invitation links from the last press: the row and its copy control.
    pub invite_links: Vec<ShareInviteLinkRect>,
    /// "+N more" line, when the list was capped.
    pub overflow: Option<Rect>,
    pub footer: Rect,
    pub level_menu: Option<Rect>,
    /// Options of the open picker, in the order they are offered.
    pub level_options: Vec<(ShareLevel, Rect)>,
}

/// One issued invitation's row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShareInviteLinkRect {
    /// Index into the model's `issued_links`.
    pub index: usize,
    pub row: Rect,
    /// The control that copies this link.
    pub copy: Rect,
}

impl ShareDialogLayout {
    /// The row a point is inside, if any.
    pub fn row_at(&self, point: Point2D) -> Option<SharePersonRect> {
        self.people
            .iter()
            .copied()
            .find(|row| row.row.contains(point))
    }
}

/// The card's height for a spec.
pub fn card_height(spec: ShareLayoutSpec) -> f32 {
    let people = spec.person_rows as f32 * ROW_H;
    let overflow = if spec.overflow { OVERFLOW_H } else { 0.0 };
    let links = spec.invite_links as f32 * INVITE_LINK_H;
    HEADER_H
        + BODY_TOP_GAP
        + INVITE_LABEL_H
        + INVITE_LABEL_GAP
        + INPUT_H
        + 8.0
        + LEVEL_ROW_H
        + 8.0
        + NOTICE_H
        + links
        + SECTION_H
        + ROW_H
        + ROW_H
        + people
        + overflow
        + FOOTER_H
        + BODY_BOTTOM_GAP
}

/// The card, centred in the viewport.
///
/// Centred like the sign-in card rather than anchored under the Share button:
/// this is a modal decision about one document, and the level picker inside it
/// opens sideways from rows near both edges, so anchoring it under a control
/// that sits at the right end of the top bar would push every popover off
/// screen.
pub fn card_rect(viewport_w: f32, viewport_h: f32, spec: ShareLayoutSpec) -> Rect {
    let width = CARD_W.min((viewport_w - 32.0).max(0.0));
    let height = card_height(spec).min((viewport_h - 16.0).max(0.0));
    Rect::xywh(
        ((viewport_w - width) / 2.0).max(0.0),
        ((viewport_h - height) / 2.0).max(0.0),
        width,
        height,
    )
}

/// Where every part of the card sits.
pub fn layout(viewport_w: f32, viewport_h: f32, spec: ShareLayoutSpec) -> ShareDialogLayout {
    let card = card_rect(viewport_w, viewport_h, spec);
    let left = card.origin.x + PAD;
    let inner_w = (card.size.x - PAD * 2.0).max(0.0);
    let band = |y: f32, height: f32| Rect::xywh(left, y, inner_w, height);
    let right =
        |y: f32, height: f32, width: f32| Rect::xywh(left + inner_w - width, y, width, height);

    // Header: title on the left, Copy link and close on the right. Copy link
    // is measured against the localized string, so a longer translation pushes
    // the title instead of overlapping it.
    let header = band(card.origin.y, HEADER_H);
    let copy_w = (estimated_text_width("Copy link", 12.0) + 48.0).max(96.0);
    let close = right(card.origin.y + (HEADER_H - CONTROL) / 2.0, CONTROL, CONTROL);
    let copy_link = Rect::xywh(
        close.origin.x - 8.0 - copy_w,
        card.origin.y + (HEADER_H - 30.0) / 2.0,
        copy_w,
        30.0,
    );
    let title = Rect::xywh(
        left,
        card.origin.y + (HEADER_H - 24.0) / 2.0,
        (copy_link.origin.x - left - 8.0).max(0.0),
        24.0,
    );

    let mut y = card.origin.y + HEADER_H + BODY_TOP_GAP;
    let invite_label = band(y, INVITE_LABEL_H);
    y += INVITE_LABEL_H + INVITE_LABEL_GAP;
    let invite_field = Rect::xywh(
        left,
        y,
        (inner_w - INVITE_BUTTON_W - INVITE_BUTTON_GAP).max(0.0),
        INPUT_H,
    );
    let invite_button = right(y, INPUT_H, INVITE_BUTTON_W);
    y += INPUT_H + 8.0;

    // The invite level sits under the field: it applies to whatever the next
    // press sends, which is the field above it rather than any person row.
    let invite_level = Rect::xywh(left, y, WIDE_LEVEL_W.min(inner_w), LEVEL_ROW_H - 4.0);
    y += 8.0 + LEVEL_ROW_H;

    // The notice band is reserved whether or not there is a notice: a card that
    // grows the moment it refuses moves the button out from under the pointer
    // that just pressed it.
    let notice = band(y, NOTICE_H);
    y += NOTICE_H;

    // Issued invitation links, one row each. Painted above the access list
    // because they are the answer to the press that was just made, and the
    // list below is unchanged by it.
    let mut invite_links = Vec::with_capacity(spec.invite_links);
    for index in 0..spec.invite_links {
        let row = band(y, INVITE_LINK_H);
        let copy = right(
            row.origin.y + (INVITE_LINK_H - CONTROL) / 2.0,
            CONTROL,
            CONTROL + 46.0,
        );
        invite_links.push(ShareInviteLinkRect { index, row, copy });
        y += INVITE_LINK_H;
    }

    let section = band(y, SECTION_H);
    y += SECTION_H;

    let you = band(y, ROW_H);
    y += ROW_H;

    let link_row = band(y, ROW_H);
    let link_level = Rect::xywh(
        link_row.origin.x + link_row.size.x - 46.0 - PERSON_LEVEL_W,
        link_row.origin.y + (ROW_H - 28.0) / 2.0,
        PERSON_LEVEL_W,
        28.0,
    );
    let link_switch = Rect::xywh(
        link_row.origin.x + link_row.size.x - 42.0,
        link_row.origin.y + (ROW_H - 22.0) / 2.0,
        42.0,
        22.0,
    );
    let link_label = Rect::xywh(
        link_row.origin.x + 40.0,
        link_row.origin.y + 8.0,
        (link_level.origin.x - link_row.origin.x - 48.0).max(0.0),
        ROW_H - 16.0,
    );
    y += ROW_H;

    let mut people = Vec::with_capacity(spec.person_rows);
    for index in 0..spec.person_rows {
        let row = band(y, ROW_H);
        let remove = right(
            row.origin.y + (ROW_H - CONTROL) / 2.0,
            CONTROL,
            CONTROL - 4.0,
        );
        let level = Rect::xywh(
            remove.origin.x - 6.0 - PERSON_LEVEL_W,
            row.origin.y + (ROW_H - 28.0) / 2.0,
            PERSON_LEVEL_W,
            28.0,
        );
        people.push(SharePersonRect {
            index,
            row,
            level,
            remove,
        });
        y += ROW_H;
    }
    let overflow = spec.overflow.then(|| band(y, OVERFLOW_H));
    if spec.overflow {
        y += OVERFLOW_H;
    }

    let footer = band(y + 4.0, FOOTER_H - 4.0);

    let (level_menu, level_options) = match spec.level_menu {
        Some(menu) => {
            let height = LEVEL_MENU_PAD * 2.0 + menu.options as f32 * LEVEL_OPTION_H;
            let width = LEVEL_MENU_W.min(inner_w);
            let anchor = match menu.anchor {
                LevelAnchor::Invite => invite_level,
                LevelAnchor::Link => link_level,
                LevelAnchor::Person(index) => people
                    .iter()
                    .find(|row| row.index == index)
                    .map(|row| row.level)
                    .unwrap_or(invite_level),
            };
            // Above the control when there is no room below: the card is
            // centred, so a picker hanging off a bottom row would otherwise be
            // half off screen.
            let below = anchor.origin.y + anchor.size.y + 4.0;
            let top = if below + height <= card.origin.y + card.size.y {
                below
            } else {
                (anchor.origin.y - 4.0 - height).max(card.origin.y + 4.0)
            };
            let menu_rect = Rect::xywh(
                (anchor.origin.x + anchor.size.x - width).max(left),
                top,
                width,
                height,
            );
            let options = ShareLevel::ALL
                .into_iter()
                .take(menu.options)
                .enumerate()
                .map(|(index, level)| {
                    (
                        level,
                        Rect::xywh(
                            menu_rect.origin.x + LEVEL_MENU_PAD,
                            menu_rect.origin.y + LEVEL_MENU_PAD + index as f32 * LEVEL_OPTION_H,
                            menu_rect.size.x - LEVEL_MENU_PAD * 2.0,
                            LEVEL_OPTION_H,
                        ),
                    )
                })
                .collect();
            (Some(menu_rect), options)
        }
        None => (None, Vec::new()),
    };

    ShareDialogLayout {
        card,
        header,
        title,
        copy_link,
        close,
        invite_label,
        invite_field,
        invite_button,
        invite_level,
        notice,
        section,
        you,
        link_row,
        link_label,
        link_level,
        link_switch,
        people,
        invite_links,
        overflow,
        footer,
        level_menu,
        level_options,
    }
}

/// Width of the card at a viewport — for hosts sizing an overlay.
pub fn card_width(viewport_w: f32) -> f32 {
    CARD_W.min((viewport_w - 32.0).max(0.0))
}
