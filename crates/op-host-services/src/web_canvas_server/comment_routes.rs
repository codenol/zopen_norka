//! The conversation about a document: `/api/files/<key>/comments*`.
//!
//! Four routes, one per thing a review actually does — read what has been said,
//! place a comment, answer in one, close or reopen one:
//!
//! | Route | Answers |
//! | --- | --- |
//! | `GET /api/files/<key>/comments` | every thread of the document, each with its comments |
//! | `POST /api/files/<key>/comments` | `{pageId, x, y, text}`; a thread and its first comment |
//! | `POST /api/files/<key>/comments/<id>/reply` | `{text}` — a comment on an existing thread |
//! | `POST /api/files/<key>/comments/<id>/resolve` | close it |
//! | `POST /api/files/<key>/comments/<id>/reopen` | open it again |
//!
//! A thread comes back as `{id, pageId, x, y, anchorHint, createdAt, resolved,
//! resolvedAt, resolvedByName, comments}`, where `pageId`/`x`/`y` are the pin
//! and are all `null` for a thread written before pins were coordinates.
//!
//! ## Why a comment is placed by coordinates
//!
//! The pin is a point on a page ([`crate::document_comments::Placement`]), and
//! the reasoning is there. What belongs here is the wire half of it: `x` and
//! `y` are the PAGE's coordinates, the ones the document is authored in, not
//! the ones the viewport draws it at. A client that sent screen coordinates
//! would put every comment wherever that reader happened to be scrolled to,
//! and the pins would move for the next person who opened the document.
//!
//! ## Why `nodeId` is refused rather than ignored
//!
//! A body carrying `nodeId` is a body written against the contract this route
//! had before, and it gets a 400 that names the field. Ignoring unknown fields
//! is the right default for fields this server never had — a client is allowed
//! to send more than this build reads. A RETIRED field is not that case: the
//! caller believes the pin is anchored to that element, and a 200 would let
//! them keep believing it while the field is dropped on the floor. The refusal
//! is the one place a client migrating to the new contract is told what
//! changed, in the answer to the request it is already sending, instead of
//! inferring it from a pin that lands somewhere unexpected.
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
//! would otherwise have to take on trust. It has a second consequence now that
//! pins are coordinates: this file cannot check that the page a pin names is a
//! page the document has, and it must not pretend to (see
//! [`crate::document_comments::list_threads`] on why there is no page filter).
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
    self, Author, Comment, CommentThread, NewComment, Placement, ResolutionChange, ThreadAuthor,
    MAX_COMMENT_CHARS, MAX_COORDINATE, MAX_PAGE_ID_CHARS,
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
#[derive(Debug, Clone, Copy, PartialEq)]
enum CommentRequestError {
    /// The body was not JSON.
    MalformedBody,
    /// No `text` string, or nothing but whitespace in it.
    MissingText,
    /// `text` is over [`MAX_COMMENT_CHARS`].
    TextTooLong,
    /// No `pageId` string, or nothing but whitespace in it.
    MissingPageId,
    /// `pageId` is over [`MAX_PAGE_ID_CHARS`].
    PageIdTooLong,
    /// A coordinate that cannot be a pin.
    ///
    /// The axis is carried because a client puts the message beside a field, and
    /// "x is not a number" is actionable where "a coordinate is not a number" is
    /// a search.
    BadCoordinate { axis: Axis, why: NotACoordinate },
    /// The body carried `nodeId`, which no longer places a pin.
    NodeIdRetired,
    /// The id in the path is not a positive whole number.
    InvalidThreadId,
}

/// Which coordinate a complaint is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    /// The name it has in the body, in this server's messages, and nowhere
    /// else — one spelling, so a client can match a message to a key.
    fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
        }
    }
}

/// What is wrong with a coordinate that is present but unusable.
///
/// Three answers rather than one, because they are three different mistakes and
/// the person reading the 400 can act on them differently: a typo (a string, a
/// `null`, a key that is not there at all), a value the wire cannot mean (see
/// [`NotACoordinate::NotFinite`]), and a number that is simply not a place in
/// this editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotACoordinate {
    /// Absent, or not a JSON number: a string, a boolean, `null`, an object.
    NotANumber,
    /// A number that is not a position: an infinity or a NaN.
    ///
    /// Unreachable from a well-formed JSON body — JSON has no literal for
    /// either, and the parser refuses the ones that would overflow — and checked
    /// anyway. It costs one comparison to make "the store never holds a
    /// coordinate that cannot be drawn" a property of this function rather than
    /// a property of serde_json's number handling, which is somebody else's
    /// code and somebody else's version.
    NotFinite,
    /// Further from the origin than [`MAX_COORDINATE`].
    TooFar,
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
            Self::MissingPageId => f.write_str("Missing pageId string"),
            Self::PageIdTooLong => {
                write!(f, "pageId is longer than {MAX_PAGE_ID_CHARS} characters")
            }
            Self::BadCoordinate { axis, why } => match why {
                NotACoordinate::NotANumber => {
                    write!(f, "{} must be a number", axis.name())
                }
                NotACoordinate::NotFinite => {
                    write!(f, "{} must be a finite number", axis.name())
                }
                // The bound is printed as the integer it is: a client showing
                // this to a person should not have to explain what `1e7` means.
                NotACoordinate::TooFar => write!(
                    f,
                    "{} is further from the origin than {MAX_COORDINATE}",
                    axis.name()
                ),
            },
            Self::NodeIdRetired => {
                f.write_str("nodeId is no longer accepted: place a comment with pageId, x and y")
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

/// The `pageId` field: the page the pin's coordinates are relative to.
///
/// A name the client and the document agree on, like the node id it replaced:
/// pages live inside the `.op` file, so this server stores the string and does
/// not know whether the document has a page by that name. What it does refuse is
/// a missing one — coordinates without a page are not a place, because the same
/// two numbers exist on every page.
fn page_id_field(value: &serde_json::Value) -> Result<String, CommentRequestError> {
    let page_id = value
        .get("pageId")
        .and_then(|page| page.as_str())
        .map(str::trim)
        .filter(|page| !page.is_empty())
        .ok_or(CommentRequestError::MissingPageId)?;
    if page_id.chars().count() > MAX_PAGE_ID_CHARS {
        return Err(CommentRequestError::PageIdTooLong);
    }
    Ok(page_id.to_string())
}

/// One coordinate of a pin.
fn coordinate_field(value: &serde_json::Value, axis: Axis) -> Result<f64, CommentRequestError> {
    let bad = |why| CommentRequestError::BadCoordinate { axis, why };
    // `as_f64` and not `as_i64`: a coordinate is a measurement, so `12` and
    // `12.5` are the same kind of answer and an integer-only reader would have
    // to grow a second arm to accept the second one.
    let number = value
        .get(axis.name())
        .and_then(|raw| raw.as_f64())
        .ok_or_else(|| bad(NotACoordinate::NotANumber))?;
    coordinate(number, axis)
}

/// The check one coordinate must pass, on its own.
///
/// Split from the body-walking above so it can be tested with values a JSON
/// request cannot carry — an infinity, a NaN — rather than only with the ones
/// the wire allows.
fn coordinate(value: f64, axis: Axis) -> Result<f64, CommentRequestError> {
    let bad = |why| CommentRequestError::BadCoordinate { axis, why };
    if !value.is_finite() {
        return Err(bad(NotACoordinate::NotFinite));
    }
    // Rejected rather than clamped: a client sending a position past the bound
    // has a bug, and moving its pin to the edge of the world would hide the bug
    // behind a comment that is now in the wrong place.
    if value.abs() > MAX_COORDINATE {
        return Err(bad(NotACoordinate::TooFar));
    }
    Ok(value)
}

/// The whole pin: the page and the point, read as one value.
///
/// One function rather than three calls at the route, because the three are one
/// fact — a page without a point is not a place, and the schema refuses it
/// (migration 3's CHECK). Taking them apart here would let this layer hand the
/// store half of one.
fn placement_field(value: &serde_json::Value) -> Result<Placement, CommentRequestError> {
    Ok(Placement {
        page_id: page_id_field(value)?,
        x: coordinate_field(value, Axis::X)?,
        y: coordinate_field(value, Axis::Y)?,
    })
}

/// Refuse a body written against the old, element-anchored contract.
///
/// Asked before the fields it replaces, so a body carrying `nodeId` and no
/// `pageId` is answered with what changed rather than with "Missing pageId
/// string" — the second is true and tells the caller nothing about why the
/// request they have been sending for months stopped working.
fn reject_retired_node_id(value: &serde_json::Value) -> Result<(), CommentRequestError> {
    match value.get("nodeId") {
        // `null` is not sent by anything: it is what a client that modelled the
        // old field as optional emits when it has nothing to put in it, which
        // is not an attempt to pin an element.
        Some(node_id) if !node_id.is_null() => Err(CommentRequestError::NodeIdRetired),
        _ => Ok(()),
    }
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
///
/// The pin is three flat keys rather than a nested object, and a thread with no
/// pin answers `null` in all three rather than omitting them: a client reads one
/// shape either way, and `pageId === null` is the check that says "this thread
/// has no pin to draw" without a second question. The other nulls in this object
/// (`resolvedAt` on an open thread) have been read that way since the first
/// version of the route.
fn thread_json(thread: &CommentThread) -> serde_json::Value {
    let (page_id, x, y) = match &thread.placement {
        Some(placement) => (
            Some(placement.page_id.as_str()),
            Some(placement.x),
            Some(placement.y),
        ),
        None => (None, None, None),
    };
    serde_json::json!({
        "id": thread.id,
        "pageId": page_id,
        "x": x,
        "y": y,
        // Sent because it is the only thing that says what a thread without
        // coordinates was about — those are the rows migration 3 carried over
        // from the element-anchored schema, and a client cannot place them.
        "anchorHint": thread.anchor_hint,
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

/// `POST /api/files/<key>/comments` — place a comment at a point on a page.
fn create(store: &DocumentDb, key: &str, body: &str, access: &RequestAccess<'_>) -> WebReply {
    let value = match parse_body(body) {
        Ok(value) => value,
        Err(error) => return bad_request(error),
    };
    if let Err(error) = reject_retired_node_id(&value) {
        return bad_request(error);
    }
    let placement = match placement_field(&value) {
        Ok(placement) => placement,
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
        placement,
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
