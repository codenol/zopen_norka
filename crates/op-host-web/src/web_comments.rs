//! Comment threads over the daemon's REST API.
//!
//! Five routes, one per thing a review does — read the conversation, open a
//! thread on an element, answer in one, close it, open it again
//! (`/api/files/<key>/comments*`). This module is the browser end of them: it
//! drains the writes the widget layer queued in `CommentsUiState`, performs
//! them, and installs the answers back into the same state.
//!
//! ## Why a 403 is not an error
//!
//! The daemon refuses a write for reasons that are ordinary in a review — a
//! read-only link, a document not shared with this account, a thread somebody
//! else opened on a document this caller may not edit. Treating those as
//! failures of this module would mean logging them, retrying them, or (worse)
//! showing the user a transport error for an answer the server stated plainly.
//! So the status is decoded into [`CommentApiError`], the API's own words are
//! kept, and the state records the refusal as a message beside the thread —
//! see [`CommentWriteError`] and `CommentsUiState::note_write_error`.
//!
//! ## Why the answers arrive through a thread-local
//!
//! An XHR callback can fire in the middle of an event, while the shell is
//! borrowed. Nothing here installs state from inside a callback: the callback
//! parks its answer in [`PENDING`] and the next frame's [`tick`] applies it
//! under a borrow it took itself. This is exactly what
//! `route_sync::tick_files` does for the file list, and for the same reason.
//!
//! ## Why the list is re-read rather than patched
//!
//! The daemon has no live signal for comments — deliberately, because a comment
//! is not a document change and must not move the document version. Writes
//! answer with the thread they touched, which is enough to update that one
//! thread; everything else (somebody else's new thread, somebody else's reply)
//! is only visible by asking again. So every write is followed by a re-read,
//! and opening the panel or the popover asks for one too.
//!
//! ## Why opening a document asks for one
//!
//! A conversation read at open is the difference between a marker that answers
//! "is there something here" and one that appears only after the tool is
//! already on. Until this, the client held the list only while the comment tool
//! was active, so a document full of open threads looked like a document nobody
//! had ever discussed: the chrome said "no comments" when it meant "not asked",
//! which is the one thing a marker must never say.
//!
//! The trigger is the (document key, identity epoch) pair, evaluated once per
//! frame in [`tick`]. The key is what the daemon files a conversation under, so
//! a different key is a different conversation; a different account changes what
//! the daemon answers — and whether it answers at all. One read per document,
//! never one per frame: see [`opened_read_wanted`] for the rule and [`OPENED`]
//! for what it remembers.
//!
//! Deliberately NOT a trigger: saving and autosaving. A save cannot change a
//! conversation (the threads are stored beside the document, not inside its
//! version), so a read there would be a request that can only answer what is
//! already held — and autosave fires every fifteen seconds during editing, which
//! is a stream. The one save that does change what is addressable is the one
//! that binds a document to a new key, and that arrives here as a key change,
//! which is already a trigger.

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::editor_ui_state::{
    Comment, CommentAnchor, CommentAuthor, CommentRequest, CommentThread, CommentWriteError,
    CommentsUiState,
};

use crate::repaint_ctx::RepaintContext;

thread_local! {
    /// An answer that arrived while the shell was borrowed.
    ///
    /// One slot, not a queue: a review is a sequence of writes and the answer to
    /// the last one is the one that describes the conversation. Holding two
    /// would apply an older thread over a newer one.
    static PENDING: RefCell<Option<Answered>> = const { RefCell::new(None) };

    /// The document whose conversation this tab has already asked for.
    ///
    /// One slot rather than a set: the open document is one document, and a tab
    /// that kept every key it had ever read would grow a string per document
    /// open for the life of the page. Re-opening a document is therefore a
    /// second read, which is correct — the reviewer navigated away and back, and
    /// the conversation may well have moved while they were elsewhere.
    ///
    /// Not cleared when a document is closed, only overwritten: an empty value
    /// would make the frame re-ask for the document that is still open.
    static OPENED: RefCell<Option<OpenedRead>> = const { RefCell::new(None) };
}

/// What a document open was read under, so the same one is not read twice.
///
/// The identity epoch is part of the identity of a conversation, not decoration:
/// the daemon answers threads per account (the same key is a 403 for an account
/// the document is not shared with), so the same key under a different account is
/// a different answer and has to be asked for again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenedRead {
    key: String,
    epoch: u64,
}

/// Whether the document now open is one whose conversation has not been read.
///
/// Pure, so the rule — which is the whole of "one request per document" — can be
/// asserted without a window. `Some(read)` means "ask for this, and remember it";
/// `None` means "nothing to do this frame".
///
/// The three refusals are deliberate:
/// - no transport: a host without the daemon's comment client has no
///   conversation to read (see `CommentsUiState::transport`);
/// - no key: a document that was never stored has no conversation and no
///   address to ask about;
/// - already read: the same key under the same identity, which is every frame
///   after the first.
pub(crate) fn opened_read_wanted(
    already: Option<&OpenedRead>,
    key: Option<&str>,
    epoch: u64,
    transport: bool,
) -> Option<OpenedRead> {
    if !transport {
        return None;
    }
    let key = key?;
    let wanted = OpenedRead {
        key: key.to_string(),
        epoch,
    };
    (already != Some(&wanted)).then_some(wanted)
}

/// What one request was, so its answer can be applied in the right way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnswerKind {
    /// A list, or any write — a write is followed by a fresh list, so most
    /// answers are this.
    List,
    /// A thread the daemon just wrote, which the popover should show.
    Written,
}

/// An answer from the daemon, parked until a frame can install it.
#[derive(Debug)]
struct Answered {
    kind: AnswerKind,
    /// The document the request was issued for.
    ///
    /// Carried with the answer rather than read from the state when it lands:
    /// a list that arrives after the reviewer opened another document still
    /// describes the document it was asked about, and installing it under the
    /// now-open key would make the next replacement of that key keep a
    /// conversation that is not its own.
    key: Option<String>,
    result: Result<Answer, CommentApiError>,
}

/// The two shapes the API answers with.
#[derive(Debug)]
enum Answer {
    /// Every thread of the document.
    Threads(Vec<CommentThread>),
    /// The thread that was just written.
    Thread(Box<CommentThread>),
}

/// Why a comment request could not be answered.
///
/// One variant per thing that can be wrong, because a client does something
/// different about each: a refusal is shown and dropped, a missing thread asks
/// for a fresh list, a transport failure is the only one worth retrying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommentApiError {
    /// The request never left (no XHR, a bad URL).
    RequestFailed,
    /// 403 — the caller may not read or write this document's comments.
    Refused,
    /// 404 — the key names no document, or the thread is gone.
    NotFound,
    /// 400 — the request was malformed. Carries the daemon's own message.
    Rejected(String),
    /// Any other non-200 status.
    Http(u16),
    /// The body was not the JSON envelope this family answers with.
    Malformed,
    /// The envelope was readable but carried no usable thread.
    MalformedThread,
}

impl std::fmt::Display for CommentApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestFailed => write!(f, "the comment request could not start"),
            Self::Refused => write!(f, "the server refused the comment request"),
            Self::NotFound => write!(f, "the document or thread no longer exists"),
            Self::Rejected(detail) => write!(f, "the comment was rejected: {detail}"),
            Self::Http(status) => write!(f, "the comment request answered {status}"),
            Self::Malformed => write!(f, "the comment response was not readable"),
            Self::MalformedThread => write!(f, "the comment response carried no thread"),
        }
    }
}

impl std::error::Error for CommentApiError {}

impl CommentApiError {
    /// How the state should record this failure.
    ///
    /// The mapping is the whole point of keeping the two enums apart: the wire
    /// speaks HTTP, the chrome speaks "what do I show the reviewer", and a 403
    /// is a sentence rather than a malfunction in both.
    pub(crate) fn to_write_error(&self) -> CommentWriteError {
        match self {
            Self::Refused => CommentWriteError::Refused,
            Self::NotFound => CommentWriteError::Gone,
            Self::Rejected(detail) => CommentWriteError::Rejected(detail.clone()),
            Self::RequestFailed | Self::Http(_) | Self::Malformed | Self::MalformedThread => {
                CommentWriteError::Transport(self.to_string())
            }
        }
    }
}

/// Decode a status + body into the answer the API promised.
fn decode_status(status: u16, body: &str) -> Result<serde_json::Value, CommentApiError> {
    match status {
        200 => {}
        // The daemon's three refusals, each stated by the server rather than
        // inferred here: a read-only role, a document not shared with this
        // account, and a thread that is not the caller's to close.
        403 => return Err(CommentApiError::Refused),
        404 => return Err(CommentApiError::NotFound),
        400 => {
            return Err(CommentApiError::Rejected(
                error_message(body).unwrap_or_else(|| "the server refused the request".to_string()),
            ))
        }
        other => return Err(CommentApiError::Http(other)),
    }
    serde_json::from_str::<serde_json::Value>(body).map_err(|_| CommentApiError::Malformed)
}

/// The daemon's own `error` string, when it sent one.
fn error_message(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed
        .get("error")
        .and_then(|error| error.as_str())
        .map(str::to_string)
}

/// Read the `threads` array of a list answer.
fn decode_threads(status: u16, body: &str) -> Result<Vec<CommentThread>, CommentApiError> {
    let parsed = decode_status(status, body)?;
    if parsed.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        return Err(CommentApiError::Malformed);
    }
    let threads = parsed
        .get("threads")
        .and_then(|threads| threads.as_array())
        .ok_or(CommentApiError::Malformed)?;
    Ok(threads.iter().filter_map(parse_thread).collect())
}

/// Read the `thread` object of a write answer.
fn decode_thread(status: u16, body: &str) -> Result<CommentThread, CommentApiError> {
    let parsed = decode_status(status, body)?;
    if parsed.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        return Err(CommentApiError::Malformed);
    }
    parsed
        .get("thread")
        .and_then(parse_thread)
        .ok_or(CommentApiError::MalformedThread)
}

/// One thread from the wire.
///
/// Every field is read with a fallback rather than a `?`: the browser cannot
/// repair a server, and a thread with an unreadable `createdAt` is still a
/// thread with comments worth reading. The one field that IS required is the id
/// — without it the thread cannot be opened, replied to, or closed, so a
/// response missing it is a malformed answer rather than a degraded one.
fn parse_thread(value: &serde_json::Value) -> Option<CommentThread> {
    let id = value.get("id").and_then(|id| id.as_i64())?;
    Some(CommentThread {
        id,
        anchor: parse_anchor(value),
        created_at: u64_field(value, "createdAt"),
        resolved: value
            .get("resolved")
            .and_then(|resolved| resolved.as_bool())
            .unwrap_or(false),
        resolved_at: value.get("resolvedAt").and_then(|at| at.as_u64()),
        resolved_by: value
            .get("resolvedBy")
            .and_then(|by| by.as_str())
            .map(str::to_string),
        // `resolvedByName` is `null` for an open thread and empty for the local
        // operator; both mean "no name to paint", and the empty string is kept
        // as `None` so no caller has to remember to filter it.
        resolved_by_name: value
            .get("resolvedByName")
            .and_then(|name| name.as_str())
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        comments: value
            .get("comments")
            .and_then(|comments| comments.as_array())
            .map(|comments| comments.iter().filter_map(parse_comment).collect())
            .unwrap_or_default(),
    })
}

fn parse_comment(value: &serde_json::Value) -> Option<Comment> {
    Some(Comment {
        // A comment the client cannot name is still readable; `0` keeps it in
        // the list rather than dropping somebody's words on a parse quirk.
        id: value.get("id").and_then(|id| id.as_i64()).unwrap_or(0),
        author: CommentAuthor {
            // `authorId` is `null` for the daemon's local operator, which is
            // information the chrome uses (their comments read as "you"), so
            // the null is preserved rather than folded into an empty string.
            id: value
                .get("authorId")
                .and_then(|id| id.as_str())
                .filter(|id| !id.is_empty())
                .map(str::to_string),
            // Empty for the local operator: the daemon has no account behind
            // them and refuses to invent a name.
            name: value
                .get("authorName")
                .and_then(|name| name.as_str())
                .unwrap_or("")
                .to_string(),
            // The role is a wire string the widget layer resolves to a colour,
            // so an unknown one travels through untouched.
            role: value
                .get("authorRole")
                .and_then(|role| role.as_str())
                .filter(|role| !role.is_empty())
                .map(str::to_string),
        },
        body: value
            .get("body")
            .and_then(|body| body.as_str())
            .unwrap_or("")
            .to_string(),
        created_at: u64_field(value, "createdAt"),
    })
}

/// The pin's place, or `None` for a thread that has none.
///
/// `pageId` / `x` / `y` are nullable on the wire, and a `null` there means
/// exactly one thing: this thread has no pin. That is the state the daemon
/// migrated threads from the old element-keyed format are in, and it has to
/// stay distinguishable from a pin at the origin of some page — a fallback of
/// `""` / `0.0` would paint a migrated conversation in the top-left corner of
/// every document and claim the reviewer left it there.
///
/// It is also the *only* distinction needed: an empty page id is treated as no
/// page (a coordinate needs a page to be in), and a coordinate outside the range
/// the daemon accepts is refused here to match the refusal it would get back
/// from a write.
fn parse_anchor(value: &serde_json::Value) -> Option<CommentAnchor> {
    let page_id = value
        .get("pageId")
        .and_then(|page| page.as_str())
        .filter(|page| !page.is_empty())?
        .to_string();
    let x = json_f64(value.get("x")?)?;
    let y = json_f64(value.get("y")?)?;
    let anchor = CommentAnchor::new(page_id, x, y);
    anchor.is_placeable().then_some(anchor)
}

fn u64_field(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|field| field.as_u64()).unwrap_or(0)
}

/// A JSON number, accepted in either shape, at full `f64` precision.
///
/// `as_f64` alone covers integers on paper, but a `{"x": 12}` written by a
/// server whose column happens to be an integer is a shape worth surviving.
/// Nothing is rounded: the daemon stores the pair as REAL, and a document pixel
/// is wider than a screen pixel past zoom 1, so a rounded coordinate moves the
/// pin by a visible amount.
fn json_f64(field: &serde_json::Value) -> Option<f64> {
    field
        .as_f64()
        .or_else(|| field.as_i64().map(|int| int as f64))
}

/// The daemon path for one comment request.
///
/// Split from the URL so it can be asserted without a window: `daemon_url`
/// reads the page's origin, which a unit test has none of.
fn comments_path(key: &str, suffix: &str) -> String {
    format!("/api/files/{key}/comments{suffix}")
}

/// The full URL for one comment request.
fn comments_url(key: &str, suffix: &str) -> String {
    crate::daemon_base::daemon_url(&comments_path(key, suffix))
}

/// Park an answer for the next frame.
fn park(kind: AnswerKind, key: Option<String>, result: Result<Answer, CommentApiError>) {
    PENDING.with(|pending| *pending.borrow_mut() = Some(Answered { kind, key, result }));
}

/// Read the document's conversation again.
fn fetch_threads(key: String) {
    let url = comments_url(&key, "");
    let key_for_answer = key.clone();
    let on_response: Rc<dyn Fn(u16, String)> = Rc::new(move |status, body| {
        park(
            AnswerKind::List,
            Some(key_for_answer.clone()),
            decode_threads(status, &body).map(Answer::Threads),
        );
        crate::repaint_coalescer::request();
    });
    if !crate::live_sync::get_with_status(&url, on_response) {
        park(
            AnswerKind::List,
            Some(key),
            Err(CommentApiError::RequestFailed),
        );
        crate::repaint_coalescer::request();
    }
}

/// Send one write and park the thread it answers with.
fn post_thread(key: &str, suffix: &str, body: Option<String>) {
    let url = comments_url(key, suffix);
    let on_response: Rc<dyn Fn(u16, String)> = Rc::new(move |status, body| {
        park(
            AnswerKind::Written,
            // A write answers one thread, not a list: it does not say what
            // document the held list is about, so it does not claim to.
            None,
            decode_thread(status, &body).map(|thread| Answer::Thread(Box::new(thread))),
        );
        crate::repaint_coalescer::request();
    });
    // The two routes that carry nothing (`resolve` / `reopen`) still POST a body:
    // the family's handlers parse JSON for every write, and an empty body is a
    // 400 there rather than a body-less success.
    let body = body.unwrap_or_else(|| "{}".to_string());
    if !crate::live_sync::post_json_with_status(&url, &body, on_response) {
        park(
            AnswerKind::Written,
            None,
            Err(CommentApiError::RequestFailed),
        );
        crate::repaint_coalescer::request();
    }
}

/// Perform one queued request against `key`.
fn dispatch(request: CommentRequest, key: &str) {
    // The host target has no `window`: every `web_sys` call is a wasm import
    // that panics off wasm32, so a native test cannot let a request reach the
    // XHR. It stops at this seam instead, which is where "the frame put this on
    // the wire" is the question being asked — see `tests::hold_wire`.
    #[cfg(test)]
    if hold_wire::holding() {
        hold_wire::record(&request, key);
        return;
    }
    match request {
        CommentRequest::Reload => fetch_threads(key.to_string()),
        CommentRequest::Create { anchor, text } => {
            // The daemon's create route takes the place, not an element: a
            // comment is about a point on a page (see `CommentAnchor`), so the
            // page id and the two coordinates are the whole address.
            let body = serde_json::json!({
                "pageId": anchor.page_id,
                "x": anchor.x,
                "y": anchor.y,
                "text": text,
            })
            .to_string();
            post_thread(key, "", Some(body));
        }
        CommentRequest::Reply { thread_id, text } => {
            let body = serde_json::json!({ "text": text }).to_string();
            post_thread(key, &format!("/{thread_id}/reply"), Some(body));
        }
        CommentRequest::Resolve { thread_id } => {
            post_thread(key, &format!("/{thread_id}/resolve"), None);
        }
        CommentRequest::Reopen { thread_id } => {
            post_thread(key, &format!("/{thread_id}/reopen"), None);
        }
    }
}

/// Apply a write's failure to the state.
fn note_failure(ui: &mut CommentsUiState, error: &CommentApiError) {
    // A missing key is not an HTTP failure at all — there is no document to
    // address — so it gets its own sentence rather than a transport one.
    if matches!(error, CommentApiError::RequestFailed) {
        ui.note_write_error(CommentWriteError::Transport(error.to_string()));
        return;
    }
    ui.note_write_error(error.to_write_error());
}

/// One frame of comment traffic: install what arrived, note a document that has
/// not been read yet, then send what is queued.
///
/// A request that cannot be sent this frame is not taken from the state at all
/// — the drain happens under a borrow this function took, and a borrow it could
/// not take leaves the queue where the next frame will find it. That is why
/// there is no retry queue here.
pub(crate) fn tick<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let Ok(mut borrowed) = inner.try_borrow_mut() else {
        return;
    };
    let mut dirty = false;

    // 1. Install an answer that arrived while the shell was borrowed.
    if let Some(Answered { kind, key, result }) =
        PENDING.with(|pending| pending.borrow_mut().take())
    {
        let editor = borrowed.host_mut().editor_state_mut();
        apply(&mut editor.editor_ui.comments, key, kind, result);
        dirty = true;
    }

    // 2. A document can become open with a conversation nobody has read (see the
    //    module notes). The request goes through the same queue and the same
    //    drain below as one the widget layer asked for, so there is still
    //    exactly one read per document open however many surfaces want it.
    {
        let epoch = crate::identity_epoch::epoch();
        let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
        let wanted = OPENED.with(|opened| {
            opened_read_wanted(
                opened.borrow().as_ref(),
                ui.file_key.as_deref(),
                epoch,
                ui.comments.transport,
            )
        });
        if let Some(read) = wanted {
            OPENED.with(|opened| *opened.borrow_mut() = Some(read));
            ui.comments.request_reload();
        }
    }

    // 3. Send what the widget layer queued.
    let (key, requests) = {
        let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
        let requests = ui.comments.take_requests();
        if requests.contains(&CommentRequest::Reload) {
            ui.comments.set_loading();
            dirty = true;
        }
        (ui.file_key.clone(), requests)
    };
    if requests.is_empty() {
        if dirty {
            borrowed.host_mut().mark_editor_state_dirty();
            let _ = borrowed.repaint();
        }
        return;
    }

    // A document with no key has no conversation to read and nowhere to store a
    // comment. Nothing is sent, and a write is answered with the reason rather
    // than silence.
    let Some(key) = key else {
        let had_write = requests
            .iter()
            .any(|request| !matches!(request, CommentRequest::Reload));
        {
            let ui = &mut borrowed.host_mut().editor_state_mut().editor_ui;
            ui.comments.set_loading_done();
            if had_write {
                ui.comments
                    .note_write_error(CommentWriteError::UnsavedDocument);
            }
        }
        borrowed.host_mut().mark_editor_state_dirty();
        let _ = borrowed.repaint();
        return;
    };

    // Every write is followed by a re-read: the answer to a write is that one
    // thread, and the rest of the conversation has no live signal at all.
    let mut follow_up = false;
    for request in requests {
        let reload = matches!(request, CommentRequest::Reload);
        dispatch(request, &key);
        follow_up |= !reload;
    }
    if follow_up {
        dispatch(CommentRequest::Reload, &key);
    }
    borrowed.host_mut().mark_editor_state_dirty();
    let _ = borrowed.repaint();
}

/// Fold one answer into the state.
fn apply(
    ui: &mut CommentsUiState,
    key: Option<String>,
    kind: AnswerKind,
    result: Result<Answer, CommentApiError>,
) {
    match (kind, result) {
        (_, Ok(Answer::Threads(threads))) => ui.install_threads_for_key(key, threads),
        (_, Ok(Answer::Thread(thread))) => {
            let id = thread.id;
            let open = ui.open_thread;
            ui.upsert_thread(*thread);
            ui.set_loading_done();
            match open {
                // The thread that was already open is the one that was written
                // into, so it stays open and now shows the new comment.
                Some(_) => {}
                // Nothing was open: this answer is the thread a composer was
                // waiting for — the canvas click that had no thread yet, or a
                // write into a thread that was opened and then closed. It
                // opens, because seeing what was just written is what the
                // reviewer asked for, and the pin it carries is the point they
                // clicked.
                None => ui.open(id),
            }
        }
        (_, Err(error)) => note_failure(ui, &error),
    }
}

/// Recording in place of sending, for the tests that assert what a frame sends.
///
/// A host-target test cannot use the transport at all: `web_sys` is a set of
/// wasm imports and every one of them panics off wasm32, so `XmlHttpRequest`
/// is not something a native test can call. Holding the wire lets the REAL frame
/// path run — the open-document gate, the widget queue, the drain, the write's
/// follow-up read — and asserts on what it would have sent, which is the part
/// that can be wrong: a read that never happens, a second read for the same
/// document, a read for the wrong key.
#[cfg(test)]
pub(crate) mod hold_wire {
    use std::cell::{Cell, RefCell};

    use op_editor_core::editor_ui_state::CommentRequest;

    thread_local! {
        static HOLDING: Cell<bool> = const { Cell::new(false) };
        static SENT: RefCell<Vec<(CommentRequest, String)>> = const { RefCell::new(Vec::new()) };
    }

    /// Start holding, and forget what a previous hold recorded.
    ///
    /// There is no counterpart: a test never hands the wire back, because the
    /// transport cannot run on this target at all.
    pub(crate) fn hold() {
        HOLDING.with(|holding| holding.set(true));
        SENT.with(|sent| sent.borrow_mut().clear());
    }

    pub(crate) fn holding() -> bool {
        HOLDING.with(Cell::get)
    }

    pub(crate) fn record(request: &CommentRequest, key: &str) {
        SENT.with(|sent| sent.borrow_mut().push((request.clone(), key.to_string())));
    }

    /// Everything the frames since `hold` asked to send, in order.
    pub(crate) fn sent() -> Vec<(CommentRequest, String)> {
        SENT.with(|sent| sent.borrow().clone())
    }

    /// Just the reads, which is what most of these tests are about.
    pub(crate) fn reads() -> Vec<String> {
        sent()
            .into_iter()
            .filter(|(request, _)| *request == CommentRequest::Reload)
            .map(|(_, key)| key)
            .collect()
    }
}

#[cfg(test)]
#[path = "web_comments_tests.rs"]
mod tests;
