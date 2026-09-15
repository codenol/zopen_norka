//! What a press on the Share dialog does.
//!
//! ## Why the flow lives beside the widget and not in a host
//!
//! Both hosts must answer the same press the same way, and the answer is not
//! platform-specific: it is a state transition plus, sometimes, one queued
//! request. Putting it here means a host's press arm is one call, and it means
//! the transitions are testable without a window.
//!
//! ## Why a press that would be refused is still honoured as a press
//!
//! Invite queues nothing it cannot justify, and records the refusal instead —
//! the button dims, but it stays pressable, because the refusal sentence is
//! the thing the person needs and a disabled control cannot explain itself.

use op_editor_core::editor_ui_state::share::{
    ShareAction, ShareLevelTarget, ShareNotice, ShareUiState,
};
use op_editor_core::{ShareInviteRefusal, ShareLevel};

use crate::widgets::share_dialog::ShareDialogHit;

/// Characters the invite field accepts.
///
/// An address or an account id: letters, digits and the punctuation both use.
/// Everything else (control characters, quotes, backslashes) is dropped at the
/// door rather than escaped later — this is a one-line field and the server
/// refuses a body that is not an address anyway.
pub fn invite_char_allowed(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(
            character,
            '@' | '.' | '_' | '-' | '+' | ',' | ';' | ' ' | '!' | '#' | '$' | '%' | '&' | '\''
        )
}

/// Longest text the field holds.
///
/// The parse caps entries, not the field: somebody pasting a list must be able
/// to see what they pasted and be told it is too long, rather than watch it
/// silently stop accepting characters.
pub const MAX_INVITE_FIELD_CHARS: usize = 2048;

/// Handle one press. Returns whether the dialog consumed it.
///
/// `level` is the option a [`ShareDialogHit::LevelOption`] press names; the
/// caller resolves it from the widget, which is the layer that knows where the
/// popover's rows are.
pub fn apply_share_hit(
    ui: &mut ShareUiState,
    hit: ShareDialogHit,
    level: Option<ShareLevel>,
) -> bool {
    match hit {
        ShareDialogHit::Outside | ShareDialogHit::Inside => {
            // A press off the controls closes the picker and blurs the field.
            // It does NOT close the dialog: an accidental click beside the
            // card would otherwise discard a half-typed invitation.
            ui.level_picker = None;
            ui.invite_focused = false;
            ui.hover = None;
            true
        }
        ShareDialogHit::Close => {
            ui.close();
            true
        }
        ShareDialogHit::CopyLink => {
            ui.request(ShareAction::CopyLink);
            ui.level_picker = None;
            true
        }
        ShareDialogHit::CopyInviteLink(index) => {
            let path = match ui.notice.as_ref() {
                Some(ShareNotice::Issued(invites)) => {
                    invites.get(index).map(|invite| invite.path.clone())
                }
                _ => None,
            };
            if let Some(path) = path {
                ui.request(ShareAction::CopyInviteLink { path });
            }
            true
        }
        ShareDialogHit::InviteField => {
            ui.invite_focused = true;
            ui.level_picker = None;
            let end = ui.invite_input.text().len();
            ui.invite_input.set_caret(end, 0);
            true
        }
        ShareDialogHit::Invite => {
            press_invite(ui);
            true
        }
        ShareDialogHit::LinkAccess => {
            press_link_access(ui);
            true
        }
        ShareDialogHit::LinkLevel => {
            ui.level_picker = toggle_picker(ui.level_picker, ShareLevelTarget::Link);
            true
        }
        ShareDialogHit::PersonLevel(index) => {
            ui.level_picker = toggle_picker(ui.level_picker, ShareLevelTarget::Person(index));
            true
        }
        ShareDialogHit::PersonRemove(index) => {
            let account = ui.people().get(index).map(|grant| grant.account.clone());
            if let Some(account) = account {
                ui.busy = true;
                ui.request(ShareAction::Revoke { account });
            }
            ui.level_picker = None;
            true
        }
        ShareDialogHit::OpenSession => {
            // The dialog closes because the panel is a different surface, not
            // an overlay on this one.
            ui.close();
            ui.request(ShareAction::OpenSession);
            true
        }
        ShareDialogHit::LevelOption(index) => {
            press_level_option(ui, index, level);
            true
        }
    }
}

/// The Invite press.
fn press_invite(ui: &mut ShareUiState) {
    ui.level_picker = None;
    let plan = match ui.plan_invite() {
        Ok(plan) => plan,
        Err(refusal) => {
            ui.record_refusal(refusal);
            return;
        }
    };
    let level = ui.invite_level;
    ui.busy = true;
    for email in plan.invitations {
        ui.request(ShareAction::IssueInvitation { email, level });
    }
    for account in plan.grants {
        ui.request(ShareAction::Grant { account, level });
    }
}

/// The "Anyone with the link" switch.
fn press_link_access(ui: &mut ShareUiState) {
    if let Err(refusal) = ShareInviteRefusal::check_invite_right(ui.own_rights) {
        ui.record_refusal(refusal);
        return;
    }
    if let Err(refusal) = ShareInviteRefusal::check_level(ui.own_rights, ui.link_level) {
        ui.record_refusal(refusal);
        return;
    }
    // The switch does not move here. `link_enabled` is written from the
    // server's answer, because a switch that reads "on" while the document is
    // still closed is worse than one that lags a request.
    ui.busy = true;
    ui.request(ShareAction::SetLinkAccess {
        enabled: !ui.link_enabled,
        level: ui.link_level,
    });
}

/// The picker's option press.
fn press_level_option(ui: &mut ShareUiState, option: usize, level: Option<ShareLevel>) {
    let Some(level) = level else {
        ui.level_picker = None;
        return;
    };
    let _ = option;
    let target = ui.level_picker;
    let outcome = match target {
        Some(ShareLevelTarget::Invite) | None => ui.set_invite_level(level),
        Some(ShareLevelTarget::Link) => {
            let result = ui.set_link_level(level);
            if result.is_ok() {
                // A level change on an ALREADY-ON switch has to reach the
                // server, or the control would claim a level the document does
                // not hand out. With the switch off there is nothing to tell
                // the server yet — the level travels with the switch.
                if ui.link_enabled {
                    ui.busy = true;
                    ui.request(ShareAction::SetLinkAccess {
                        enabled: true,
                        level,
                    });
                }
            }
            result
        }
        Some(ShareLevelTarget::Person(index)) => ui.set_person_level(index, level).and_then(|()| {
            // Re-levelling somebody is a grant of the same account at a new
            // level; the server treats it as an upsert.
            let account = ui.people().get(index).map(|grant| grant.account.clone());
            match account {
                Some(account) => {
                    ui.busy = true;
                    ui.request(ShareAction::Grant { account, level });
                    Ok(())
                }
                None => Ok(()),
            }
        }),
    };
    if let Err(refusal) = outcome {
        ui.record_refusal(refusal);
    }
}

fn toggle_picker(
    current: Option<ShareLevelTarget>,
    target: ShareLevelTarget,
) -> Option<ShareLevelTarget> {
    if current == Some(target) {
        None
    } else {
        Some(target)
    }
}

/// Typed-character routing for the invite field.
///
/// `None` means the field is not focused and the key belongs to somebody else.
pub fn invite_field_text(ui: &mut ShareUiState, character: char, now_ms: u64) -> Option<bool> {
    if !ui.invite_focused {
        return None;
    }
    if character.is_control() || !invite_char_allowed(character) {
        return Some(false);
    }
    let selected = ui
        .invite_input
        .highlight_range()
        .map(|(start, end)| end - start)
        .unwrap_or(0);
    if ui.invite_input.text().chars().count() - selected >= MAX_INVITE_FIELD_CHARS {
        return Some(false);
    }
    let mut buffer = [0_u8; 4];
    ui.invite_input
        .insert_str(character.encode_utf8(&mut buffer), now_ms);
    ui.notice = None;
    Some(true)
}

/// Backspace on the focused invite field.
pub fn invite_field_backspace(ui: &mut ShareUiState, now_ms: u64) -> Option<bool> {
    if !ui.invite_focused {
        return None;
    }
    let before = ui.invite_input.text().to_owned();
    ui.invite_input.backspace(now_ms);
    let changed = ui.invite_input.text() != before;
    if changed {
        ui.notice = None;
    }
    Some(changed)
}

/// Enter on the focused invite field: the same press as the button.
pub fn invite_field_submit(ui: &mut ShareUiState) -> Option<bool> {
    if !ui.invite_focused {
        return None;
    }
    press_invite(ui);
    Some(true)
}

/// Clipboard paste into the invite field, filtered.
pub fn invite_field_paste(ui: &mut ShareUiState, text: &str, now_ms: u64) -> Option<bool> {
    if !ui.invite_focused {
        return None;
    }
    let sanitized: String = text
        .chars()
        .filter(|character| !character.is_control() && invite_char_allowed(*character))
        .take(MAX_INVITE_FIELD_CHARS)
        .collect();
    if sanitized.is_empty() {
        return Some(false);
    }
    ui.invite_input.set_text(sanitized);
    ui.invite_input.touch(now_ms);
    ui.notice = None;
    Some(true)
}
