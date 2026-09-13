//! The conversation about a document: `/api/files/<key>/comments*`.
//!
//! Four routes, one per thing a review actually does — read what has been said,
//! open a thread on an element, answer in one, close or reopen one:
//!
//! | Route | Answers |
//! | --- | --- |
//! | `GET /api/files/<key>/comments` | every thread of the document, each with its comments |
//! | `POST /api/files/<key>/comments` | `{nodeId, text}` — a new thread and its first comment |
//! | `POST /api/files/<key>/comments/<id>/reply` | `{text}` — a comment on an existing thread |
//! | `POST /api/files/<key>/comments/<id>/resolve` | close it |
//! | `POST /api/files/<key>/comments/<id>/reopen` | open it again |
//!
//! ## Why these live inside the `/api/files` family
//!
//! A thread is reached through a document, so everything in front of these
//! handlers is a property of the key in the path and of nothing else: the access
//! gate, the key's shape, the store, and the check that the row belongs to an
//! account this caller may address. That preamble lives in
//! [`super::files_routes`], and these routes are written as part of its route
//! table so they inherit it by construction rather than by a second copy of it.
//!
//! ## Why `handle` takes no editor state
//!
//! There is no `&mut WebCanvasState` in the signature, and that is the design
//! rather than an oversight: a comment is a conversation ABOUT the document, not
//! part of it. Nothing here may touch `state.editor`, `state.version`, the
//! document's file or its row, and the shortest way to make that true is to
//! leave the whole of the daemon's editor state out of reach of every handler in
//! this file. A comment therefore cannot bump a version, dirty a document, or
//! invalidate a collaboration hash — the properties a reader of a design tool
//! would otherwise have to take on trust.
//!
//! ## What each route asks for
//!
//! The right is named in [`super::files_routes::required_action`], beside the
//! other routes of this family, and the answers are: a read for the list,
//! [`DocumentAction::Comment`] for everything that writes. Closing or reopening
//! asks that same right first and then a second question, which only the thread
//! itself can answer — is this YOUR thread, or may you edit the document
//! ([`RequestAccess::decide_thread_resolution`]). A guest who was given a link
//! to read is refused at the floor; a contributor who may comment is refused
//! only if the thread is somebody else's and they are not an editor.
//!
//! ## Who a comment is attributed to
//!
//! The verified identity, never the body: a request body a caller can write is
//! not a statement about who the caller is. The name is recorded as a snapshot
//! beside the id, so the history still reads as people after somebody is renamed
//! (see [`crate::document_comments::Author`]).

use super::request_access::{self, RequestAccess};
use super::WebReply;
use crate::document_comments::{
    self, Author, Comment, CommentThread, NewComment, ResolutionChange, ThreadAuthor,
    MAX_COMMENT_CHARS, MAX_NODE_ID_CHARS,
};
use crate::document_db::DocumentDb;
use crate::document_store::{self, DocumentStoreError};

/// Handle every `/api/files/<key>/comments*` request that reaches this far.
///
/// Reached from [`super::files_routes::handle`], which has already decided what
/// the caller may do with the document and that the document is theirs to
/// address; nothing here re-asks either question. `thread` and `action` come
/// from that route's own parser, so a shape this function does not recognise is
/// a 404 rather than an unreachable arm that silently grants.
pub(super) fn handle(
    method: &str,
    key: &str,
    thread: Option<&str>,
    action: &str,
    body: &str,
    store: &DocumentDb,
    access: &RequestAccess<'_>,
) -> WebReply {
    match (method, thread, action) {
        ("GET", None, "") => list(store, key),
        ("POST", None, "") => create(store, key, body, access),
        ("POST", Some(thread), "reply") => reply(store, key, thread, body, access),
        // Two routes rather than one route with a `resolved` flag: which of the
        // two a caller is doing is the whole of the change, and a boolean in a
        // body is a place where "close this" and "open this" differ by one
        // character of JSON.
        ("POST", Some(thread), "resolve") => set_resolution(store, key, thread, true, access),
        ("POST", Some(thread), "reopen") => set_resolution(store, key, thread, false, access),
        _ => super::not_found_reply(),
    }
}

/// Why a comment request could not be answered.
///
/// One variant per thing that can be wrong, rather than a formatted string: the
/// HTTP answer is the same 400 for all of them, and what differs is which part
/// of the request was wrong — which is what a client needs in order to put a
/// message next to the right field, and what a test can assert without matching
/// prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentRequestError {
    /// The body was not JSON.
    MalformedBody,
    /// No `text` string, or nothing but whitespace in it.
    MissingText,
    /// `text` is over [`MAX_COMMENT_CHARS`].
    TextTooLong,
    /// No `nodeId` string, or nothing but whitespace in it.
    MissingNodeId,
    /// `nodeId` is over [`MAX_NODE_ID_CHARS`].
    NodeIdTooLong,
    /// The id in the path is not a positive whole number.
    InvalidThreadId,
}

impl std::fmt::Display for CommentRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // The wording matches the family's other 400s ("Missing name
            // string"), so a client already reading those needs no new shape.
            Self::MalformedBody => f.write_str("Expected a JSON object"),
            Self::MissingText => f.write_str("Missing text string"),
            Self::TextTooLong => write!(
                f,
                "Comment text is longer than {MAX_COMMENT_CHARS} characters"
            ),
            Self::MissingNodeId => f.write_str("Missing nodeId string"),
            Self::NodeIdTooLong => {
                write!(f, "nodeId is longer than {MAX_NODE_ID_CHARS} characters")
            }
            Self::InvalidThreadId => f.write_str("Invalid comment thread id"),
        }
    }
}

impl std::error::Error for CommentRequestError {}

/// The daemon's standard 400 for a request this family could not read.
fn bad_request(error: CommentRequestError) -> WebReply {
    WebReply {
        status: "400 Bad Request",
        body: crate::mcp_serve::rest_error_body(&error.to_string()),
    }
}

/// The 404 for a thread its own document does not have.
///
/// Its own wording rather than the store's `NotFound` — which says "document not
/// found" — because the two are different statements about different things: the
/// document WAS found, and it is the row the key named; what is missing is the
/// thread the path asked for. A client that showed the store's message would
/// send whoever reads it looking for the wrong thing.
fn thread_not_found() -> WebReply {
    WebReply {
        status: "404 Not Found",
        body: crate::mcp_serve::rest_error_body("No such comment thread"),
    }
}

/// Parse a request body as JSON.
fn parse_body(body: &str) -> Result<serde_json::Value, CommentRequestError> {
    serde_json::from_str::<serde_json::Value>(body).map_err(|_| CommentRequestError::MalformedBody)
}

/// The `text` field, trimmed, within the bound the record sets.
///
/// Trimmed because surrounding whitespace is not something anybody means to
/// write, and a comment of nothing but whitespace is a pin that opens onto an
/// empty panel — refused here rather than stored and puzzled over later.
fn text_field(value: &serde_json::Value) -> Result<String, CommentRequestError> {
    let text = value
        .get("text")
        .and_then(|text| text.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or(CommentRequestError::MissingText)?;
    // Counted in characters, not bytes: the bound is about how much prose a
    // thread carries, and a byte limit would quietly give a Latin comment three
    // times the room of a Russian one.
    if text.chars().count() > MAX_COMMENT_CHARS {
        return Err(CommentRequestError::TextTooLong);
    }
    Ok(text.to_string())
}

/// The `nodeId` field: the element the pin sits on.
fn node_id_field(value: &serde_json::Value) -> Result<String, CommentRequestError> {
    let node_id = value
        .get("nodeId")
        .and_then(|node| node.as_str())
        .map(str::trim)
        .filter(|node| !node.is_empty())
        .ok_or(CommentRequestError::MissingNodeId)?;
    if node_id.chars().count() > MAX_NODE_ID_CHARS {
        return Err(CommentRequestError::NodeIdTooLong);
    }
    Ok(node_id.to_string())
}

/// The thread id in the path.
///
/// A positive whole number, refused as a bad request rather than looked up and
/// missed: `abc` is a malformed URL, and answering it with "no such thread"
/// would describe a thread nobody could have named.
fn thread_id(raw: &str) -> Result<i64, CommentRequestError> {
    raw.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or(CommentRequestError::InvalidThreadId)
}

/// Who to record as the author of what this request writes.
///
/// The verified caller: their account id, and the name the hub gave for them.
/// The local daemon has no accounts, so its comments are attributed to nobody by
/// id and carry no name — an empty one, rather than a name invented here on
/// somebody's behalf (see [`Author::name`]).
fn author<'a>(access: &'a RequestAccess<'_>) -> Author<'a> {
    Author {
        role: access.caller_role(),
        id: access.caller_id(),
        name: access.caller_name().unwrap_or(""),
    }
}

/// One thread as the browser sees it.
fn thread_json(thread: &CommentThread) -> serde_json::Value {
    serde_json::json!({
        "id": thread.id,
        "nodeId": thread.node_id,
        "createdAt": thread.created_at,
        "resolved": thread.resolved,
        "resolvedAt": thread.resolved_at,
        "resolvedBy": thread.resolved_by,
        "resolvedByName": thread.resolved_by_name,
        "comments": thread.comments.iter().map(comment_json).collect::<Vec<_>>(),
    })
}

/// One comment as the browser sees it.
///
/// `authorId` IS sent, unlike the file list's owner id. The browser has its own
/// account projection, and the one question it cannot answer from a name is "is
/// this mine" — which decides whether a thread offers to close itself, and which
/// is what the resolver's half of the rule asks. Everyone who receives this
/// already reaches the document; a name alone could not tell two people with the
/// same one apart.
fn comment_json(comment: &Comment) -> serde_json::Value {
    serde_json::json!({
        "id": comment.id,
        "authorId": comment.author_id,
        "authorName": comment.author_name,
            "authorRole": comment.author_role,
        "body": comment.body,
        "createdAt": comment.created_at,
    })
}

/// The one reply shape every writing route answers with.
fn thread_created(thread: CommentThread) -> WebReply {
    super::files_routes::ok_json(serde_json::json!({ "ok": true, "thread": thread_json(&thread) }))
}

/// `GET /api/files/<key>/comments` — the whole conversation.
///
/// Every thread with every comment, in one answer. A document's discussion is
/// small (a review, not a forum), and a client that has to page a conversation
/// cannot draw the pins it is about: it needs to know which elements are spoken
/// for before it can show any of them.
fn list(store: &DocumentDb, key: &str) -> WebReply {
    match document_comments::list_threads(store, key) {
        Ok(Some(threads)) => super::files_routes::ok_json(serde_json::json!({
            "ok": true,
            "threads": threads.iter().map(thread_json).collect::<Vec<_>>(),
        })),
        // No row carries the key: a conversation belongs to a STORED document,
        // and there is nothing here to have one. The same 404 the family
        // answers for any other route on a key that names no document.
        Ok(None) => super::files_routes::store_error_reply(DocumentStoreError::NotFound),
        Err(error) => super::files_routes::store_error_reply(error),
    }
}

/// `POST /api/files/<key>/comments` — open a thread on an element.
fn create(store: &DocumentDb, key: &str, body: &str, access: &RequestAccess<'_>) -> WebReply {
    let value = match parse_body(body) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    let node_id = match node_id_field(&value) {
        Ok(node_id) => node_id,
        Err(error) => return bad_request(error),
    };
    let text = match text_field(&value) {
        Ok(text) => text,
        Err(error) => return bad_request(error),
    };
    let comment = NewComment {
        author: author(access),
        body: &text,
    };
    match document_comments::create_thread(
        store,
        key,
        &node_id,
        comment,
        document_store::now_secs(),
    ) {
        Ok(Some(thread)) => thread_created(thread),
        Ok(None) => super::files_routes::store_error_reply(DocumentStoreError::NotFound),
        Err(error) => super::files_routes::store_error_reply(error),
    }
}

/// `POST /api/files/<key>/comments/<id>/reply` — answer in a thread.
fn reply(
    store: &DocumentDb,
    key: &str,
    thread: &str,
    body: &str,
    access: &RequestAccess<'_>,
) -> WebReply {
    let id = match thread_id(thread) {
        Ok(id) => id,
        Err(error) => return bad_request(error),
    };
    let value = match parse_body(body) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    let text = match text_field(&value) {
        Ok(text) => text,
        Err(error) => return bad_request(error),
    };
    let comment = NewComment {
        author: author(access),
        body: &text,
    };
    match document_comments::add_reply(store, key, id, comment, document_store::now_secs()) {
        Ok(Some(thread)) => thread_created(thread),
        Ok(None) => thread_not_found(),
        Err(error) => super::files_routes::store_error_reply(error),
    }
}

/// `resolve` / `reopen` — the two answers to one question about a thread.
///
/// The thread is read first and the second question asked of the caller before
/// anything is written, because the answer depends on who opened it — a fact
/// that lives in the thread rather than in the path. The read is not a
/// substitute for the write's own check: `set_resolved` looks the thread up
/// again under this document's key, so a thread deleted between the two is a
/// 404 and never a write.
fn set_resolution(
    store: &DocumentDb,
    key: &str,
    thread: &str,
    resolved: bool,
    access: &RequestAccess<'_>,
) -> WebReply {
    let id = match thread_id(thread) {
        Ok(id) => id,
        Err(error) => return bad_request(error),
    };
    let opened_by = match document_comments::thread_author(store, key, id) {
        Ok(ThreadAuthor::NoSuchThread) => return thread_not_found(),
        Ok(ThreadAuthor::Account(account)) => Some(account),
        // Opened by the local operator: no account stands behind it, so the
        // author half of the rule cannot answer for it (see
        // `RequestAccess::decide_thread_resolution`).
        Ok(ThreadAuthor::LocalOperator) => None,
        Err(error) => return super::files_routes::store_error_reply(error),
    };
    if let Err(refusal) = access.decide_thread_resolution(opened_by.as_deref()) {
        return request_access::refusal_reply(refusal);
    }
    let change = if resolved {
        ResolutionChange::Resolve {
            author: author(access),
            at: document_store::now_secs(),
        }
    } else {
        ResolutionChange::Reopen
    };
    match document_comments::set_resolved(store, key, id, change) {
        Ok(Some(thread)) => thread_created(thread),
        Ok(None) => thread_not_found(),
        Err(error) => super::files_routes::store_error_reply(error),
    }
}

#[cfg(test)]
#[path = "comment_routes_tests.rs"]
mod tests;
