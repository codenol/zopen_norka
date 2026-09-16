//! Drain the Share dialog's queued work (#56).
//!
//! The dialog has no HTTP and no clipboard: every press that needs either
//! queues a [`ShareAction`] and lets the host answer it. This module is that
//! host half for the browser — the same arrangement [`crate::collab_sync`]
//! uses for the collaboration panel, and deliberately the same shape: one
//! interval, `thread_local` latches, and every write through the shared
//! `RepaintContext`.
//!
//! ## Why the context is refreshed every tick
//!
//! Three things the dialog reads are facts about the TAB rather than about a
//! press: which account is asking, whether the document is theirs, and the
//! link Copy link copies. None of them exists at mount and all of them change
//! without the dialog noticing — a document opens, an account signs in — so
//! they are recomputed cheaply each tick and written only when they moved.
//!
//! ## Rights are read from the address, and that is the honest source
//!
//! The daemon refuses to re-share a document a caller was merely given
//! (`share_routes::handle`: grant and revoke edit the CALLER's own list), so
//! the dialog must not offer what the server will refuse. A tab showing its
//! own document — no `?tenant=` in the address — is its owner, which the
//! daemon answers with `roles ∪ EDITOR`; a tab showing somebody else's is a
//! visitor, whose floor is viewing. Both answers are the ones the routes
//! themselves would give, so the button and the refusal cannot disagree.
//!
//! ## One action in flight, and none dropped
//!
//! An Invite of three addresses is three requests. A single in-flight slot
//! keeps them ordered, and a slot that was taken but never answered is put
//! back rather than forgotten — the queue is a thing the user asked for.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_core::editor_ui_state::share::{
    ShareAction, ShareIssuedInvite, ShareNotice, ShareUiState,
};
use op_editor_core::{
    auth_routes, route, share_routes, AccountState, Rights, ShareGrant, ShareInviteRefusal,
    ShareLevel, ShareListSnapshot,
};

use crate::live_sync;
use crate::repaint_ctx::RepaintContext;

/// Tick cadence. There is nothing to be responsive about between presses, and
/// each tick costs one borrow and a string compare; 300 ms keeps a press from
/// waiting on a slow frame without spending anything measurable.
const TICK_MS: i32 = 300;

thread_local! {
    /// One request in flight at a time, so an Invite of three addresses
    /// cannot arrive out of order.
    static BUSY: Cell<bool> = const { Cell::new(false) };
    /// An action whose request never left (no XHR, refused URL), kept for the
    /// next tick rather than dropped.
    static RETRY: RefCell<Option<ShareAction>> = const { RefCell::new(None) };
}

/// Wire the Share dialog's host half onto the mounted shell. Called from mount.
pub(crate) fn start<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    let inner = inner.clone();
    let tick: Rc<dyn Fn()> = Rc::new(move || {
        refresh_context(&inner);
        drain(&inner, &base);
    });
    let _ = live_sync::start_interval(TICK_MS, tick);
}

/// Keep the tab-level facts the dialog reads up to date.
fn refresh_context<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    let base = crate::daemon_base::daemon_base();
    // No `?tenant=` means this tab is showing its own document; see the module
    // docs for why that is the same answer the routes give.
    let own_document = crate::daemon_base::tenant_param().is_none();
    let changed = {
        let state = context.host_mut().editor_state_mut();
        // The deployment's own account key, never the username: an access list
        // is keyed by the id, so a list that showed handles beside ids would be
        // two vocabularies in one column — and a grant made from a handle
        // grants nothing at all.
        let account_id = state.editor_ui.account.account_id().map(str::to_string);
        let self_account = account_id.clone();
        // The link a colleague can actually open. A document is reached through
        // its OWNER: without the tenant parameter the daemon answers
        // `tenant-not-shared` to anybody but the owner, which is exactly what a
        // copied link used to do.
        let link = state.editor_ui.file_key.as_deref().map(|key| {
            let path = format!("{base}{}{key}", route::DOCUMENT_PREFIX);
            match account_id.as_deref() {
                Some(owner) => format!(
                    "{path}?{}={owner}",
                    op_editor_core::share_routes::TENANT_QUERY
                ),
                // No account key: a local deployment, where there is nobody to
                // name and the plain path is the whole link.
                None => path,
            }
        });
        let own_rights = if own_document {
            Rights::EDITOR
        } else {
            Rights::VIEW_ONLY
        };
        let share = &mut state.editor_ui.share;
        let mut changed = false;
        if share.self_account != self_account {
            share.self_account = self_account;
            changed = true;
        }
        if share.link != link {
            share.link = link;
            changed = true;
        }
        // A tab showing somebody else's document knows whose it is from the
        // address; what that owner gives this account comes back with the
        // access list, and is written there (see `apply_list`).
        if !own_document && share.granted_level.is_none() {
            if let Some(level) = crate::daemon_base::tenant_param()
                .as_deref()
                .and_then(|owner| share.list.level_from(owner))
            {
                share.granted_level = Some(level);
                changed = true;
            }
        }
        if !share.rights_known || share.is_owner != own_document || share.own_rights != own_rights {
            share.is_owner = own_document;
            share.own_rights = own_rights;
            share.rights_known = true;
            changed = true;
        }
        changed
    };
    if changed {
        context.host_mut().mark_editor_state_dirty();
        let _ = context.repaint();
    }
}

/// Take the next queued action and perform it.
fn drain<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    if BUSY.get() {
        return;
    }
    let action = RETRY
        .with(|slot| slot.borrow_mut().take())
        .or_else(|| take_pending(inner));
    let Some(action) = action else {
        return;
    };
    if !perform(inner, base, &action) {
        RETRY.with(|slot| *slot.borrow_mut() = Some(action));
    }
}

fn take_pending<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) -> Option<ShareAction> {
    let mut context = inner.try_borrow_mut().ok()?;
    let share = &mut context.host_mut().editor_state_mut().editor_ui.share;
    if share.pending.is_empty() {
        return None;
    }
    Some(share.pending.remove(0))
}

/// Perform one action. `false` means the request never left, so the caller can
/// hold it for the next tick.
fn perform<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: &str,
    action: &ShareAction,
) -> bool {
    match action {
        // Neither of these needs a socket: the string is already in the state
        // and the clipboard is the browser's.
        ShareAction::CopyLink => {
            copy_link(inner, base, None);
            true
        }
        ShareAction::CopyInviteLink { path } => {
            copy_link(inner, base, Some(path));
            true
        }
        ShareAction::OpenSession => {
            open_session(inner);
            true
        }
        ShareAction::LoadList => {
            BUSY.set(true);
            let inner = inner.clone();
            let started = live_sync::get_with_status(
                &format!("{base}{}", share_routes::LIST),
                Rc::new(move |status, body| {
                    BUSY.set(false);
                    apply_list(&inner, status, &body);
                }),
            );
            if !started {
                BUSY.set(false);
            }
            started
        }
        ShareAction::Grant { account, level } => {
            let body = serde_json::json!({ "userId": account, "level": level.wire() }).to_string();
            post_mutation(inner, base, share_routes::GRANT, body, action.clone())
        }
        ShareAction::Revoke { account } => {
            let body = serde_json::json!({ "userId": account }).to_string();
            post_mutation(inner, base, share_routes::REVOKE, body, action.clone())
        }
        ShareAction::SetLinkAccess { enabled, level } => {
            let body = serde_json::json!({ "enabled": enabled, "level": level.wire() }).to_string();
            BUSY.set(true);
            let inner = inner.clone();
            let started = live_sync::post_json_with_status(
                &format!("{base}{}", share_routes::LINK_ACCESS),
                &body,
                Rc::new(move |status, response| {
                    BUSY.set(false);
                    apply_link_access(&inner, status, &response);
                }),
            );
            if !started {
                BUSY.set(false);
            }
            started
        }
        ShareAction::IssueInvitation { email, level } => {
            let roles = level.role_wires();
            let body = serde_json::json!({ "email": email, "roles": roles }).to_string();
            BUSY.set(true);
            let inner = inner.clone();
            let email = email.clone();
            let started = live_sync::post_json_with_status(
                &format!("{base}{}", auth_routes::ADMIN_INVITES),
                &body,
                Rc::new(move |status, response| {
                    BUSY.set(false);
                    apply_issued_invitation(&inner, status, &response, &email);
                }),
            );
            if !started {
                BUSY.set(false);
            }
            started
        }
    }
}

/// Grant, revoke — one request shape, one answer shape.
fn post_mutation<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: &str,
    path: &str,
    body: String,
    action: ShareAction,
) -> bool {
    BUSY.set(true);
    let inner = inner.clone();
    let started = live_sync::post_json_with_status(
        &format!("{base}{path}"),
        &body,
        Rc::new(move |status, response| {
            BUSY.set(false);
            apply_mutation(&inner, status, &response, &action);
        }),
    );
    if !started {
        BUSY.set(false);
    }
    started
}

/// `GET /api/share/list` answered.
fn apply_list<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, status: u16, body: &str) {
    let snapshot = ShareListSnapshot::parse(body);
    let link_access = parse_link_access(body);
    write(inner, |share| {
        if status >= 400 {
            // A refusal is as unreadable as a network failure, and the dialog
            // must say "we could not read the list" rather than render an empty
            // one it cannot vouch for.
            share.apply_list(ShareListSnapshot::default());
        } else {
            share.apply_list(snapshot);
            if let Some((enabled, level)) = link_access {
                share.link_enabled = enabled;
                share.link_level = level;
            }
            // What this account holds in the document on screen, when the
            // document is somebody else's: the dialog's own row says so.
            if let Some(owner) = crate::daemon_base::tenant_param() {
                share.granted_level = share.list.level_from(&owner);
            }
        }
    });
}

/// Grant and revoke answered with the whole access list.
fn apply_mutation<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    status: u16,
    body: &str,
    action: &ShareAction,
) {
    if status >= 400 {
        refuse(inner, body, action);
        return;
    }
    let grants = parse_shared_with(body);
    write(inner, |share| {
        match action {
            ShareAction::Grant { account, .. } => {
                share.list.shared_with = grants.clone();
                match grants.iter().find(|grant| &grant.account == account) {
                    Some(grant) => share.record_granted(vec![grant.clone()]),
                    // The server answered success without the account in the
                    // list: the honest reading is that the change did not
                    // happen, so nothing is claimed and the list is refreshed
                    // on the next open.
                    None => share.busy = false,
                }
            }
            ShareAction::Revoke { account } => {
                share.list.shared_with = grants.clone();
                let index = share
                    .list
                    .shared_with
                    .iter()
                    .position(|grant| &grant.account == account);
                if let Some(index) = index {
                    share.forget_person(index);
                }
                share.busy = false;
            }
            _ => share.busy = false,
        }
    });
}

/// `POST /api/share/link` answered with the level the document now hands out.
fn apply_link_access<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, status: u16, body: &str) {
    if status >= 400 {
        refuse(inner, body, &ShareAction::LoadList);
        return;
    }
    let answer = parse_link_access(body).unwrap_or((false, ShareLevel::DEFAULT));
    write(inner, |share| {
        share.link_enabled = answer.0;
        share.link_level = answer.1;
        share.busy = false;
        share.notice = Some(ShareNotice::LinkAccess {
            enabled: answer.0,
            level: answer.1,
        });
    });
}

/// `POST /api/auth/admin/invites` answered with the link only a human can carry.
fn apply_issued_invitation<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    status: u16,
    body: &str,
    email: &str,
) {
    if status >= 400 {
        refuse(
            inner,
            body,
            &ShareAction::IssueInvitation {
                email: email.to_string(),
                level: ShareLevel::DEFAULT,
            },
        );
        return;
    }
    let path = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| {
            parsed
                .get("path")
                .and_then(|path| path.as_str())
                .map(str::to_string)
        });
    let Some(path) = path else {
        refuse(inner, body, &ShareAction::LoadList);
        return;
    };
    let invite = ShareIssuedInvite {
        email: email.to_string(),
        path,
    };
    write(inner, |share| share.record_issued(vec![invite]));
}

/// The server refused. Say which refusal this build can name, and carry the
/// code when it cannot — see [`ShareInviteRefusal::RefusedByServer`].
fn refuse<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, body: &str, action: &ShareAction) {
    let code = error_code(body);
    let refusal = refusal_for(code.as_deref(), action, body);
    write(inner, |share| share.record_refusal(refusal));
}

/// How many accounts the daemon said this document already shares with, from a
/// `share-limit-reached` body.
///
/// `None` when the body names no number: the daemon always sends one, so a
/// refusal without it comes from a build this side does not understand, and a
/// sentence with an invented figure in it would be worse than the code.
fn share_limit(body: &str) -> Option<usize> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("limit")?.as_u64())
        .and_then(|limit| usize::try_from(limit).ok())
}

/// Which refusal this build can name, for a code the daemon answered with.
///
/// The strings matched below are the DAEMON's codes — `share_routes::ShareError::code`,
/// `AccessRefusal::code` and the literals beside them — because they are the
/// wire vocabulary and this side reads the wire. The dialog's own codes
/// ([`ShareInviteRefusal::code`]) are a second list; where the two spell a fact
/// the same way one arm covers both, and where they do not, the daemon's
/// spelling is the one that must be matched.
///
/// Issue #142 is what one drift costs: this function read `share-with-self`
/// while the daemon answers `cannot-share-with-self`, so the sentence written
/// for exactly that refusal — "that account is you" — was unreachable and the
/// person was shown the raw code instead. Renaming a code in `share_routes`
/// means renaming a line here: `share_routes_tests` pins the daemon's spelling
/// and `a_refusal_code_maps_onto_the_sentence_this_build_has` pins this one.
fn refusal_for(code: Option<&str>, action: &ShareAction, body: &str) -> ShareInviteRefusal {
    let account = match action {
        ShareAction::Grant { account, .. } | ShareAction::Revoke { account } => Some(account),
        _ => None,
    };
    match code {
        // What the share routes answer for a caller whose roles do not carry
        // the invite right (`AccessRefusal::ReadOnly`). `invite-role-required`
        // is the dialog's own spelling of the same fact, and no daemon route
        // emits it — kept so a stale tab cannot lose the sentence it has.
        Some("read-only-role") | Some("invite-role-required") => ShareInviteRefusal::NoInviteRight,
        // `admin-role-required` is what the invitation route answers
        // (`AccessRefusal::NotAnAdministrator`) — the one route of this dialog
        // that has an account list to be refused. The `-for-email` spelling is
        // the dialog's own.
        Some("admin-role-required") | Some("admin-role-required-for-email") => {
            ShareInviteRefusal::NotAnAdministratorForEmail
        }
        // The daemon's spelling of "that account is you", beside the dialog's
        // own name for the same sentence. A repeat grant is not refused at all
        // (it answers `changed:false`), so `already-has-access` arrives from
        // nowhere today.
        Some("cannot-share-with-self") | Some("already-has-access") => {
            ShareInviteRefusal::AlreadyOnList {
                account: account.cloned().unwrap_or_default(),
            }
        }
        Some("level-above-your-own") => match action {
            ShareAction::Grant { level, .. } => ShareInviteRefusal::LevelAboveOwn {
                level: *level,
                own: ShareLevel::DEFAULT,
            },
            _ => ShareInviteRefusal::RefusedByServer {
                code: "level-above-your-own".to_string(),
            },
        },
        // A typo in the invite field — the mistake most likely to be made in
        // this dialog, and the one that used to be answered with the code
        // itself (issue #146). The entry that named nobody travels with the
        // sentence, because "check the spelling" is only useful next to the
        // spelling.
        Some("unknown-account") => match account.filter(|entry| !entry.trim().is_empty()) {
            Some(entry) => ShareInviteRefusal::UnknownAccount {
                account: entry.clone(),
            },
            // A refusal about an entry this dialog cannot see — it arrived on a
            // request made from somewhere else. Naming nobody in a sentence
            // that is about a name would be worse than the code.
            None => ShareInviteRefusal::RefusedByServer {
                code: "unknown-account".to_string(),
            },
        },
        // The 257th account on one document. The number comes from the daemon's
        // answer rather than from a constant repeated on this side (#146).
        Some("share-limit-reached") => match share_limit(body) {
            Some(limit) => ShareInviteRefusal::ShareLimitReached { limit },
            None => ShareInviteRefusal::RefusedByServer {
                code: "share-limit-reached".to_string(),
            },
        },
        // A code this build has no sentence for. The daemon's codes without one
        // are `payload-too-large`, `malformed-share-request`,
        // `tenant-not-shared`, `missing-document`, `account-lookup-unavailable`
        // and `share-not-persisted` — a request this dialog cannot build, a
        // write that could not be persisted, or something the deployment
        // learned since. Writing sentences for them is a decision about the
        // dialog's wording (and fifteen locales), not a spelling to guess at,
        // so the code is carried for a log line instead.
        other => ShareInviteRefusal::RefusedByServer {
            code: other.unwrap_or("unknown").to_string(),
        },
    }
}

/// Put a link on the clipboard: the document's, or one invitation's.
fn copy_link<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: &str,
    invite_path: Option<&str>,
) {
    let mut context = match inner.try_borrow_mut() {
        Ok(context) => context,
        Err(_) => return,
    };
    let text = {
        let share = &context.host_mut().editor_state().editor_ui.share;
        match invite_path {
            Some(path) => link_with_base(base, path),
            None => share.link.clone().unwrap_or_default(),
        }
    };
    if text.is_empty() {
        // Nothing to copy: a button that appears to work while the clipboard
        // stays empty is the one outcome this dialog must not produce, so it
        // says what is missing instead.
        write_locked(&mut *context, |share| {
            share.notice = Some(ShareNotice::NoDocumentLink);
        });
        return;
    }
    crate::web_clipboard::copy_text(&text);
    write_locked(&mut *context, |share| {
        share.notice = Some(ShareNotice::CopiedLink);
    });
}

/// The dialog's footer row opens the live collaboration panel, which used to be
/// the top-bar chip's own surface.
fn open_session<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    write(inner, |share| {
        share.busy = false;
    });
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    context
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .collab
        .panel
        .open = true;
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

/// Read the `linkAccess` field: the level the document hands out, or `null`.
fn parse_link_access(body: &str) -> Option<(bool, ShareLevel)> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    match parsed.get("linkAccess") {
        Some(serde_json::Value::String(level)) => {
            ShareLevel::from_wire(level).ok().map(|level| (true, level))
        }
        Some(serde_json::Value::Null) => Some((false, ShareLevel::DEFAULT)),
        _ => None,
    }
}

/// The `sharedWith` array of a grant / revoke / list answer.
fn parse_shared_with(body: &str) -> Vec<ShareGrant> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| {
            parsed
                .get("sharedWith")
                .and_then(|value| value.as_array())
                .map(|entries| entries.iter().filter_map(ShareGrant::from_json).collect())
        })
        .unwrap_or_default()
}

/// The daemon's machine code for a refusal body.
fn error_code(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| {
            parsed
                .get("error")
                .and_then(|error| error.as_str())
                .map(str::to_string)
        })
}

/// An absolute link for a path the daemon answered with.
fn link_with_base(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    format!("{base}{path}")
}

/// Mutate the dialog's state on the shared host and repaint once.
fn write<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    apply: impl FnOnce(&mut ShareUiState),
) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    write_locked(&mut *context, apply);
}

/// The same, for a caller that already holds the borrow (the clipboard path,
/// which reads the link and writes the notice around one non-borrowing call).
fn write_locked<C: RepaintContext + 'static>(
    context: &mut C,
    apply: impl FnOnce(&mut ShareUiState),
) {
    apply(&mut context.host_mut().editor_state_mut().editor_ui.share);
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

#[cfg(test)]
#[path = "share_sync_tests.rs"]
mod tests;
