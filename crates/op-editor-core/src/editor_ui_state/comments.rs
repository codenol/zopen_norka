//! Comment threads — the conversation pinned to a document's elements.
//!
//! This is the paint-side model of what the daemon's `/api/files/<key>/comments`
//! routes answer, plus the small amount of state the chrome needs to hold a
//! review in progress: which thread is open, what is being typed, and whether
//! the next canvas click should drop a pin.
//!
//! ## Why the widget layer asks and the host performs
//!
//! Nothing here opens a socket. The widget layer is platform-free, so a click
//! that means "write this comment" lands in [`CommentsUiState::pending`] as a
//! [`CommentRequest`] and the host drains it on the next frame — the same shape
//! `server_files_open_request` and `copy_link_requested` already use. A queue
//! rather than one `Option` per verb because a review is a sequence: resolve a
//! thread and reply into another in the same frame is ordinary, and one slot
//! would silently drop the first.
//!
//! ## Why the list is replaced, never merged, on a reload
//!
//! The daemon has no live signal for comments (deliberately: a comment is not a
//! document change and must not move the document version). A reload is
//! therefore the only way to see what somebody else said, and a reload answers
//! the whole conversation. Merging that into what is already held would keep a
//! thread the server no longer has, and there is no way to tell "deleted" from
//! "not in this answer" from a partial one. [`CommentsUiState::install_threads`]
//! replaces the list and re-opens the open thread by id, or closes it if it is
//! gone.
//!
//! ## Why a thread with no pin is still a thread
//!
//! `node_id` names an element, and the element can be deleted while the
//! conversation about it stays worth reading — Figma's "unattached" comments.
//! So nothing here filters by whether the node exists: the pin is a property of
//! the paint layer (see `op_editor_ui::widgets::comment_pins`), which drops the
//! marker and leaves the thread in the list. Dropping the thread itself would
//! lose the discussion the moment its subject is renamed away.

/// Longest comment accepted, in characters — the daemon's own ceiling.
///
/// Mirrored rather than imported: the bound is a wire contract shared with a
/// crate this one does not depend on, and a client that let a 4,001-character
/// comment leave would learn about the limit from a 400. Characters, not bytes,
/// for the reason the server states: a byte limit gives a Latin comment three
/// times the room of a Russian one.
pub const MAX_COMMENT_CHARS: usize = 4_000;

/// Who wrote a comment, as the moment recorded them.
///
/// `id` is `None` for the daemon's local operator — a deployment with no
/// accounts attributes a comment to nobody rather than inventing a name for
/// somebody. `role` is the wire string the hub sent, kept verbatim: the colour
/// it resolves to is presentation, and an unknown role must stay a neutral
/// colour rather than being dropped or guessed at here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentAuthor {
    pub id: Option<String>,
    pub name: String,
    pub role: Option<String>,
}

impl CommentAuthor {
    /// True when this comment has no account behind it.
    pub fn is_local_operator(&self) -> bool {
        self.id.is_none()
    }
}

/// One comment in a thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comment {
    pub id: i64,
    pub author: CommentAuthor,
    pub body: String,
    /// Unix seconds, as the daemon stamped it.
    ///
    /// Kept as an absolute stamp rather than a pre-computed age: the panel
    /// repaints from the same value many times, and an age computed once at
    /// install time would freeze at the frame the answer arrived.
    pub created_at: u64,
}

/// One thread: the pin, its comments, and whether it is closed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentThread {
    pub id: i64,
    /// The element the pin sits on. May name a node the document no longer has.
    pub node_id: String,
    pub created_at: u64,
    pub resolved: bool,
    /// Set exactly when `resolved` is — the daemon writes the pair together.
    pub resolved_at: Option<u64>,
    pub resolved_by: Option<String>,
    /// The resolver's name at the time; `None` for an open thread.
    pub resolved_by_name: Option<String>,
    /// Oldest first.
    pub comments: Vec<Comment>,
}

impl CommentThread {
    /// The comment that opened the thread, if it has any.
    ///
    /// `Option` rather than an index because the daemon's list is a LEFT JOIN:
    /// a thread with no comments is a shape a hand-repaired database can be in,
    /// and a panel that indexed `comments[0]` would panic on it.
    pub fn first(&self) -> Option<&Comment> {
        self.comments.first()
    }

    /// How many comments came after the opening one.
    pub fn reply_count(&self) -> usize {
        self.comments.len().saturating_sub(1)
    }

    /// The author a pin and a list row are coloured by: whoever opened it.
    pub fn opener(&self) -> Option<&CommentAuthor> {
        self.first().map(|comment| &comment.author)
    }

    /// Who closed the thread, as a name to show.
    pub fn resolved_by_label(&self) -> Option<&str> {
        self.resolved_by_name
            .as_deref()
            .filter(|name| !name.is_empty())
    }
}

/// What the popover is composing.
///
/// Two shapes and not "an id or nothing", because a new thread has no id yet —
/// it has the node the click landed on, and the write that creates it needs
/// exactly that. Keeping them one enum is what stops a half-built state (a
/// draft with neither an id nor a node) from being representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentComposer {
    /// An existing thread is open.
    Thread(i64),
    /// A comment about `node_id` that has not been written yet.
    NewThread(String),
}

/// A write the widget layer asked for and the host must perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentRequest {
    /// Re-read the document's threads.
    ///
    /// Asked for on open and after every write: the daemon pushes no signal for
    /// comments, so a client that never re-reads shows a conversation that
    /// stopped at the moment it loaded.
    Reload,
    /// Open a thread on `node_id` with `text` as its first comment.
    Create { node_id: String, text: String },
    /// Add `text` to an existing thread.
    Reply { thread_id: i64, text: String },
    /// Close a thread.
    Resolve { thread_id: i64 },
    /// Open a closed thread again.
    Reopen { thread_id: i64 },
}

/// How a write ended, as the host reports it back.
///
/// A named enum rather than a `Result<(), String>` so the three outcomes the
/// chrome treats differently stay distinct: a refusal is a normal answer a
/// contributor will see (403 `read-only-role`, `tenant-not-shared`), a transport
/// failure is a retry, and a missing thread is a reload — the conversation moved
/// on under this client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentWriteError {
    /// The caller may not do this (403). Expected, not exceptional.
    Refused,
    /// The thread is gone (404) — the list is stale.
    Gone,
    /// The request was rejected as malformed (400).
    Rejected(String),
    /// There is no document to comment on: the editor holds a document that was
    /// never saved, so it has no key for the daemon to store a thread under.
    UnsavedDocument,
    /// The request never completed. Retryable.
    Transport(String),
}

impl CommentWriteError {
    /// The i18n key whose text explains this failure to the user.
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::Refused => "comments.error.refused",
            Self::Gone => "comments.error.gone",
            Self::Rejected(_) => "comments.error.rejected",
            Self::UnsavedDocument => "comments.error.unsaved",
            Self::Transport(_) => "comments.error.transport",
        }
    }
}

/// Comment state for the document in the editor.
#[derive(Debug, Clone, Default)]
pub struct CommentsUiState {
    /// This client's account id, as the host's identity projection knows it.
    ///
    /// `None` in every build that has no account-id projection — the local
    /// daemon, and the browser until it learns its own id. It affects exactly
    /// one word: an author whose id equals this one is shown as *you* rather
    /// than by name. It is identity, not document state, so a document change
    /// does NOT clear it.
    pub viewer_id: Option<String>,
    /// The document's threads, oldest first — the server's own order.
    ///
    /// The order is deliberately kept rather than re-sorted by node or by
    /// activity: the pin number shown in the canvas is this list's index, and a
    /// number that moved because somebody replied would make the pins refer to
    /// different threads between two frames.
    pub threads: Vec<CommentThread>,
    /// A list request is in flight.
    pub loading: bool,
    /// The last failure, already localized by the host or the route.
    pub error: Option<String>,
    /// The open thread, if the popover is showing one.
    pub open_thread: Option<i64>,
    /// The comment being typed into the open thread.
    pub reply_draft: String,
    /// The comment being typed for the thread a pin click will create.
    pub new_draft: String,
    /// Whether the next canvas click on an element drops a pin.
    pub pin_mode: bool,
    /// The element a pending new-thread draft belongs to.
    pub pin_node: Option<String>,
    /// Whether the thread list panel is showing.
    pub panel_open: bool,
    /// Whether the comment field owns the keyboard.
    ///
    /// The host routes typed text to exactly one surface, so the comment
    /// composer has to say when it is the one being typed into — the join field
    /// and the property inputs each carry the same flag for the same reason.
    pub composer_focused: bool,
    /// Writes and reads the host has yet to perform.
    pending: Vec<CommentRequest>,
}

impl CommentsUiState {
    /// Note which account this client is, for "is this mine" questions.
    pub fn set_viewer_id(&mut self, viewer_id: Option<String>) {
        self.viewer_id = viewer_id;
    }

    /// Forget everything that belongs to the previous document.
    ///
    /// Called from `clear_document_derived`, because a thread is about a node of
    /// one document and painting the previous document's conversation over the
    /// next one is the kind of "my change does nothing" that is really "your
    /// change is about something that no longer exists".
    pub fn clear_for_document(&mut self) {
        self.threads.clear();
        self.loading = false;
        self.error = None;
        self.open_thread = None;
        self.reply_draft.clear();
        self.new_draft.clear();
        self.pin_mode = false;
        self.pin_node = None;
        self.composer_focused = false;
        // `viewer_id` is deliberately kept: it is who this client is, not
        // something the document said.
        // `panel_open` is chrome, not document state, and survives — the panel
        // a reviewer opened is still the panel they want on the next file.
        self.pending.clear();
    }

    /// Replace the list with a freshly read answer.
    ///
    /// Closes the popover when its thread is no longer in the answer rather than
    /// leaving it painting a thread the server has forgotten.
    pub fn install_threads(&mut self, threads: Vec<CommentThread>) {
        self.threads = threads;
        self.loading = false;
        self.error = None;
        if let Some(open) = self.open_thread {
            if self.thread(open).is_none() {
                self.open_thread = None;
                self.reply_draft.clear();
            }
        }
    }

    pub fn set_loading(&mut self) {
        self.loading = true;
        self.error = None;
    }

    /// End an in-flight read without touching the list or the error.
    ///
    /// Separate from [`Self::install_threads`] because the two are different
    /// outcomes of the same request: this one says "the wait is over" and
    /// nothing else, which is what a request that could not be sent at all
    /// needs.
    pub fn set_loading_done(&mut self) {
        self.loading = false;
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.loading = false;
        self.error = Some(error.into());
    }

    /// Install one thread the server just answered with.
    ///
    /// The writing routes answer the whole thread, so this is the cheap path
    /// after a write: no second round trip, and no window in which the panel
    /// shows the comment the user just sent only as a draft. Replaced in place
    /// when the id is known, appended at the end otherwise — the server's order
    /// is by creation, and a new thread is the newest.
    pub fn upsert_thread(&mut self, thread: CommentThread) {
        self.error = None;
        match self.threads.iter_mut().find(|held| held.id == thread.id) {
            Some(held) => *held = thread,
            None => self.threads.push(thread),
        }
    }

    pub fn thread(&self, id: i64) -> Option<&CommentThread> {
        self.threads.iter().find(|thread| thread.id == id)
    }

    /// The thread the list panel paints, in the order it paints them.
    pub fn thread_ids(&self) -> Vec<i64> {
        self.threads.iter().map(|thread| thread.id).collect()
    }

    /// 1-based position of a thread in the list — the number a pin shows.
    ///
    /// `None` for a thread the list does not hold, which is how a pin whose
    /// thread arrived after the snapshot that drew the canvas is skipped instead
    /// of drawn unnumbered.
    pub fn ordinal(&self, id: i64) -> Option<usize> {
        self.threads
            .iter()
            .position(|thread| thread.id == id)
            .map(|index| index + 1)
    }

    /// Every thread whose pin sits on `node_id`, oldest first.
    pub fn threads_on_node(&self, node_id: &str) -> Vec<&CommentThread> {
        self.threads
            .iter()
            .filter(|thread| thread.node_id == node_id)
            .collect()
    }

    pub fn open_count(&self) -> usize {
        self.threads
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
    }

    pub fn is_open(&self, id: i64) -> bool {
        self.open_thread == Some(id)
    }

    /// Show an existing thread, replacing whatever the popover showed.
    pub fn open(&mut self, id: i64) {
        if self.open_thread == Some(id) {
            return;
        }
        self.open_thread = Some(id);
        self.pin_node = None;
        // Opening a thread puts the cursor in the field: the reviewer clicked a
        // pin to say something, and making them click a second time to type is
        // a step with no decision in it.
        self.composer_focused = true;
        // The reply draft belongs to the thread it was typed into: carrying it
        // to another thread would send one reviewer's sentence into somebody
        // else's conversation.
        self.reply_draft.clear();
    }

    /// Close the popover.
    ///
    /// Both drafts are discarded. A draft kept after the popover closes is
    /// invisible text that a later reopen would resurrect — the reviewer would
    /// see a sentence they no longer remember typing, in a field they did not
    /// put it in.
    pub fn close(&mut self) {
        self.open_thread = None;
        self.pin_node = None;
        self.composer_focused = false;
        self.reply_draft.clear();
        self.new_draft.clear();
    }

    /// Give the comment field the keyboard.
    pub fn focus_composer(&mut self) {
        if self.composer().is_some() {
            self.composer_focused = true;
        }
    }

    /// Whether the comment field owns the keyboard right now.
    ///
    /// The host routes a keystroke to exactly one surface, so "is the comment
    /// field the one being typed into" is one question with one answer, and it
    /// lives here rather than being re-derived from two flags by every host.
    ///
    /// It is `composer_focused` AND a live composer: the flag alone can outlive
    /// the popover (a close clears it, but a reader that trusted the flag
    /// without the composer would keep stealing keystrokes from a field that is
    /// no longer on screen).
    ///
    /// This is the answer the host's "a text input owns the keyboard" rule must
    /// consult — not just the keyboard ladder but also the hidden IME capture
    /// input, which is only focused while such an input exists. A comment field
    /// that never reports itself owns nothing: a composed character (an IME
    /// commit, a dead key, an emoji picker insertion) has nowhere to land, and a
    /// bare letter naming a tool switches the tool instead of being typed.
    pub fn takes_keyboard(&self) -> bool {
        self.composer_focused && self.composer().is_some()
    }

    /// Take the keyboard away from the comment field.
    pub fn blur_composer(&mut self) {
        self.composer_focused = false;
    }

    /// What the popover is showing, if anything.
    ///
    /// An open thread wins over a pending pin: a reviewer who opened a thread
    /// and then armed pin mode asked for a new pin, and the click that lands
    /// calls [`Self::begin_thread_on`], which clears the open thread.
    pub fn composer(&self) -> Option<CommentComposer> {
        if let Some(id) = self.open_thread {
            return Some(CommentComposer::Thread(id));
        }
        self.pin_node
            .as_ref()
            .map(|node| CommentComposer::NewThread(node.clone()))
    }

    pub fn set_pin_mode(&mut self, on: bool) {
        self.pin_mode = on;
        if !on {
            self.pin_node = None;
            self.new_draft.clear();
        }
    }

    pub fn toggle_pin_mode(&mut self) {
        self.set_pin_mode(!self.pin_mode);
    }

    /// A canvas click landed on `node_id` while pin mode was armed: open the
    /// composer for a comment about it.
    ///
    /// Pin mode stays armed — a review is often several pins in a row, and
    /// disarming after one would make the second comment a trip back to the
    /// toolbar. The pin is what the mode is for; the mode ends when the reviewer
    /// says so (or when the thread is written, see [`Self::submit_new_thread`]).
    pub fn begin_thread_on(&mut self, node_id: impl Into<String>) {
        let node_id = node_id.into();
        if node_id.is_empty() {
            return;
        }
        self.open_thread = None;
        self.reply_draft.clear();
        self.pin_node = Some(node_id);
        self.new_draft.clear();
        self.composer_focused = true;
    }

    /// Discard the popover's uncommitted text and close it.
    pub fn cancel_composer(&mut self) {
        self.close();
    }

    /// The draft the composer's input is editing, empty when nothing is open.
    pub fn draft(&self) -> &str {
        match self.composer() {
            Some(CommentComposer::Thread(_)) => &self.reply_draft,
            Some(CommentComposer::NewThread(_)) => &self.new_draft,
            None => "",
        }
    }

    pub fn draft_mut(&mut self) -> &mut String {
        match self.composer() {
            Some(CommentComposer::Thread(_)) => &mut self.reply_draft,
            _ => &mut self.new_draft,
        }
    }

    /// Whether the current draft can be sent.
    ///
    /// Trimmed, because a comment of spaces is a pin that opens onto nothing —
    /// the daemon refuses it too, and refusing it here saves a round trip that
    /// could only end in a 400.
    pub fn can_send(&self) -> bool {
        let draft = self.draft().trim();
        !draft.is_empty() && draft.chars().count() <= MAX_COMMENT_CHARS
    }

    /// Ask the host to re-read the list, at most once per pending reload.
    pub fn request_reload(&mut self) {
        if !self.pending.contains(&CommentRequest::Reload) {
            self.pending.push(CommentRequest::Reload);
        }
    }

    /// Send the current draft. Returns whether a request was queued.
    pub fn send(&mut self) -> bool {
        if !self.can_send() {
            return false;
        }
        match self.composer() {
            Some(CommentComposer::Thread(id)) => {
                let text = self.reply_draft.trim().to_string();
                self.reply_draft.clear();
                self.pending.push(CommentRequest::Reply {
                    thread_id: id,
                    text,
                });
                true
            }
            Some(CommentComposer::NewThread(node_id)) => {
                let text = self.new_draft.trim().to_string();
                self.new_draft.clear();
                self.pin_node = None;
                self.composer_focused = false;
                // The pin is placed by the server's answer (the thread's id),
                // and a mode still armed would drop a second pin on the next
                // click while the reviewer is trying to read the first.
                self.pin_mode = false;
                self.pending.push(CommentRequest::Create { node_id, text });
                true
            }
            None => false,
        }
    }

    pub fn resolve(&mut self, thread_id: i64) {
        self.pending.push(CommentRequest::Resolve { thread_id });
    }

    pub fn reopen(&mut self, thread_id: i64) {
        self.pending.push(CommentRequest::Reopen { thread_id });
    }

    /// Record a write's failure beside the thread it was about.
    ///
    /// The draft is NOT restored: the server refused the text, and putting it
    /// back in a field the user believes they sent is how the same comment is
    /// typed twice. The message names the reason; retyping is the user's call.
    pub fn note_write_error(&mut self, error: CommentWriteError) {
        // A failure ends any read that was in flight: leaving the spinner up
        // beside a message is a screen that looks stuck rather than refused.
        self.loading = false;
        self.error = Some(error.message_key().to_string());
        // A gone thread means this client's list is stale, and only a reload can
        // tell the difference between "somebody deleted it" and "we never saw
        // it": ask for one.
        if error == CommentWriteError::Gone {
            self.request_reload();
        }
    }

    /// Hand the host every request queued since the last drain.
    pub fn take_requests(&mut self) -> Vec<CommentRequest> {
        std::mem::take(&mut self.pending)
    }

    /// Whether any request is waiting — cheap enough for a per-frame check.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
#[path = "comments_tests.rs"]
mod tests;
