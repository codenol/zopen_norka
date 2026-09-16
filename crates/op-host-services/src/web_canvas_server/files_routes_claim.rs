//! `POST /api/files/<key>/claim` — the recovery of a document that arrived
//! outside the store (issue #46).
//!
//! A sibling of `files_routes` rather than part of it for the reason the
//! conversation routes are: the spine stays about the file and its family, and
//! this route asks a question the rest of the family does not ([`DocumentAction::
//! Claim`] — the deployment's authority rather than a document's). It is reached
//! only from the spine, after the gate and the key's shape, and it is the one
//! per-key route whose ownership check the spine deliberately skips: the row it
//! names has no owner, which is exactly what that check refuses.

use super::files_routes::{entry_json, ok_json, store_error_reply};
use super::*;
use crate::document_db::DocumentDb;
use crate::document_store::{self, ClaimOutcome};

/// `POST /api/files/<key>/claim` — bind a document that arrived outside the
/// store to the calling account (issue #46).
///
/// The recovery path this daemon did not have. Every row in the store has an
/// owner except what a deployment inherited: rows the legacy `index.json` import
/// brought over, rows a daemon that ran without accounts created, and `.op`
/// files placed in the documents directory by hand. Online, an unattributed row
/// is reachable by nobody — [`RequestAccess::reaches_stored_document`] refuses a
/// NULL owner on purpose, and that refusal is the leak it closes — so those
/// documents were on disk and unreachable, and the account that had been working
/// on them had no way to say so. A deployment that ran locally and switched to
/// online showed an empty file list with every file still in place.
///
/// What a claim is NOT is the reverse of that refusal. It does not hand a
/// document to whoever names its key:
///
/// * it needs a caller who may manage the deployment's accounts
///   ([`DocumentAction::Claim`], the right the account list is kept for), so an
///   ordinary account cannot take an unattributed file merely by producing a
///   stale link to it;
/// * it works on exactly one key per request, so nothing happens by accident;
/// * and it adopts only a row that belongs to NOBODY
///   ([`ClaimOutcome::Foreign`] is a refusal, not a takeover).
///
/// Which keys are worth claiming is the operator's own step, and deliberately
/// not a listing route: the keys are in the directory (`<key>.op`), in the
/// `index.json` the database imported, and in the address bar of whoever worked
/// on the file.
///
/// The account attributed is the verified caller and never the body: a body a
/// caller can write is not a statement about who the caller is — the rule
/// `files_routes::create_document` already states. The reply carries the row, so
/// the caller sees the document land in its list without a second request.
pub(super) fn claim_document(
    store: &DocumentDb,
    key: &str,
    access: &RequestAccess<'_>,
) -> WebReply {
    // A deployment with no accounts has nothing to attribute to and nothing to
    // hide either — its list already shows every row of its directory — so this
    // is not a route it has. The same not-found the other account-shaped routes
    // answer with in that mode.
    let Some(owner) = access.caller_id() else {
        return not_found_reply();
    };
    match document_store::claim(store, key, owner) {
        Ok(ClaimOutcome::Claimed(entry)) | Ok(ClaimOutcome::AlreadyOwned(entry)) => {
            ok_json(serde_json::json!({ "ok": true, "file": entry_json(&entry) }))
        }
        // Somebody else's document, with the answer a stranger already gets for
        // it: the same statement about the same document.
        Ok(ClaimOutcome::Foreign(_)) => request_access::refusal_reply(AccessRefusal::NotShared),
        Err(error) => store_error_reply(error),
    }
}
