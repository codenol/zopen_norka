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
//! ## Why a comment is a point on a page, not an element
//!
//! A thread used to name a `node_id` and hang off that element's bounds. That
//! made the comment reachable only where the element was: the small elements a
//! review is mostly about — an icon, a label, a 4 px gap — are exactly the ones
//! a pointer cannot reliably hit, so the comment about them could not be placed
//! at all. A comment is therefore a [point](CommentAnchor): the page it is on
//! and a coordinate in that page's own document space, which is where the
//! reviewer clicked. Nothing about a thread's position depends on the document
//! tree any more, so an element that moves, is renamed, or is deleted leaves
//! the conversation exactly where it was left.
//!
//! The coordinate is **document** space, and the page id is kept beside it: a
//! screen position is a property of the viewport, so a pin that stored one
//! would slide the moment somebody panned (see
//! `op_editor_ui::widgets::comment_pins` for the one place the two spaces are
//! converted).
//!
//! ## Why the rail is the mode, rather than a separate flag
//!
//! The right rail shows one thing at a time: the inspector, or the document's
//! conversations. Two flags (`pin_mode` and a `panel_open`) would be two
//! answers to one question, and every reader that consulted only one of them
//! would eventually paint the wrong occupant. So [`CommentsUiState::pin_mode`]
//! is the comment *tool* — the toolbar's icon, the rail's occupant and the
//! canvas click that drops a pin are all that one bit.

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

/// Where a comment sits: the page, and the point on it.
///
/// Document-space coordinates (`x` / `y`), in the page `page_id` names — the
/// same space the canvas paints in and the same pair the daemon stores. Kept
/// together rather than as three loose fields so a comment can never carry a
/// coordinate that belongs to another page.
///
/// `f64` rather than `f32`: the daemon stores the pair as REAL and hands it back
/// as it was written, and a narrowing round trip through the parse would move a
/// pin by a visible amount at any zoom past 1x — a document pixel is wider than
/// a screen pixel there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentAnchor {
    /// The page this point is on.
    pub page_id: String,
    pub x: f64,
    pub y: f64,
}

/// Furthest from the origin the daemon will store a coordinate.
///
/// Mirrored rather than imported for the reason [`MAX_COMMENT_CHARS`] is: it is
/// a wire contract shared with a crate this one does not depend on. It is
/// checked here so a click that could not be stored is refused where it is made
/// instead of coming back as a 400 after the reviewer has typed a paragraph.
pub const MAX_COMMENT_COORDINATE: f64 = 10_000_000.0;

impl CommentAnchor {
    pub fn new(page_id: impl Into<String>, x: f64, y: f64) -> Self {
        Self {
            page_id: page_id.into(),
            x,
            y,
        }
    }

    /// Whether this is a point a pin can be drawn at, and stored.
    ///
    /// A non-finite or out-of-range coordinate is refused rather than clamped:
    /// it would place a pin nobody can find, and the state that made one is a
    /// bug worth being able to see rather than hide behind a `0.0`.
    pub fn is_placeable(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.x.abs() <= MAX_COMMENT_COORDINATE
            && self.y.abs() <= MAX_COMMENT_COORDINATE
    }
}

/// One thread: its pin (when it has one), its comments, and whether it is
/// closed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentThread {
    pub id: i64,
    /// Where the pin sits, or `None` for a thread with no pin at all.
    ///
    /// The daemon migrated threads written under the old element-keyed format,
    /// which have no page and no coordinates, and it answers `null` for exactly
    /// that — so "there is nothing to draw" is a state this model has to be able
    /// to hold. Folding it into a coordinate would put a pin at the origin of
    /// some page and claim the comment was left there. The thread is still a
    /// thread: the rail lists it, marked, and a press on its row opens the
    /// conversation without moving the canvas.
    ///
    /// Independent of the document tree either way, so a thread outlives the
    /// element it was written about.
    pub anchor: Option<CommentAnchor>,
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
#[derive(Debug, Clone, PartialEq)]
pub enum CommentComposer {
    /// An existing thread is open.
    Thread(i64),
    /// A comment about the point in `anchor` that has not been written yet.
    ///
    /// The anchor is carried, not looked up: this is the click the reviewer
    /// just made, and it is what the composer is painted beside and what the
    /// write will send. A composer that only remembered "a new thread is being
    /// written" would have to be told the position twice.
    NewThread(CommentAnchor),
}

/// A write the widget layer asked for and the host must perform.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentRequest {
    /// Re-read the document's threads.
    ///
    /// Asked for on open and after every write: the daemon pushes no signal for
    /// comments, so a client that never re-reads shows a conversation that
    /// stopped at the moment it loaded.
    Reload,
    /// Open a thread at `anchor` with `text` as its first comment.
    Create { anchor: CommentAnchor, text: String },
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
    /// Whether this host carries the daemon's comment client at all.
    ///
    /// Declared by the host that owns the transport (`op_host_web::web_comments`),
    /// not derived here: a host without it can neither read the conversation nor
    /// write one, so the comment tool has nothing to select and the toolbar must
    /// not offer it. Defaults to `false`, which is the honest answer for every
    /// host that has not said otherwise.
    pub transport: bool,
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
    /// The comment being typed for the thread a canvas click will create.
    pub new_draft: String,
    /// Whether the comment tool is active.
    ///
    /// One bit, three effects: the toolbar icon reads it as its active state,
    /// the right rail shows the thread list instead of the inspector, and a
    /// canvas click drops a pin. Tools behave this way — the mode ends when the
    /// reviewer picks it again or picks another tool (see
    /// `host_keyboard_transitions::set_active_tool`) — so it is deliberately not
    /// a flag per surface.
    pub pin_mode: bool,
    /// The point a pending new-thread draft belongs to.
    ///
    /// Set by the canvas click and read by the composer, which paints beside
    /// it and sends it. `None` while an existing thread is open instead.
    pub pending_pin: Option<CommentAnchor>,
    /// Whether the comment field owns the keyboard.
    ///
    /// The host routes typed text to exactly one surface, so the comment
    /// composer has to say when it is the one being typed into — the join field
    /// and the property inputs each carry the same flag for the same reason.
    pub composer_focused: bool,
    /// The stored document [`Self::threads`] is the conversation of.
    ///
    /// The daemon files a conversation under the document's key
    /// (`/api/files/<key>/comments`), so the key is what makes the list and the
    /// document the same subject. It is written when a list answer is installed
    /// — by the answer's own key, not by whatever key is open a frame later —
    /// and read by [`Self::clear_for_document`], which is the only thing that
    /// may now throw the list away. `None` means "this list belongs to no
    /// document", which is the honest state after a wipe and for a host that has
    /// never read anything.
    document_key: Option<String>,
    /// Writes and reads the host has yet to perform.
    pending: Vec<CommentRequest>,
}

impl CommentsUiState {
    /// Note which account this client is, for "is this mine" questions.
    pub fn set_viewer_id(&mut self, viewer_id: Option<String>) {
        self.viewer_id = viewer_id;
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

    /// Every thread the rail lists for `page_id`, in the server's order.
    ///
    /// That is the page's own pinned threads **plus** the pin-less ones. A
    /// thread the daemon migrated from the old element-keyed format belongs to
    /// no page, and a strictly page-scoped list would hide it from every page —
    /// a conversation nobody can find is worse than one listed without a marker
    /// to jump to. They carry no number and no pin; the row says so.
    ///
    /// The list is page-scoped for the same reason the pins are: the number a
    /// marker shows is its index among the markers actually drawn. Threads
    /// *pinned* on other pages are counted instead of listed (see
    /// [`Self::open_count_elsewhere`]) — a reviewer is told they exist without
    /// being handed rows that lead to a page they are not looking at.
    pub fn threads_on_page(&self, page_id: &str) -> Vec<&CommentThread> {
        self.threads
            .iter()
            .filter(|thread| match thread.anchor.as_ref() {
                Some(anchor) => anchor.page_id == page_id,
                None => true,
            })
            .collect()
    }

    /// 1-based position of a thread among its page's pins, or `None`.
    ///
    /// `None` for a thread the rail cannot point at: one with no pin at all, or
    /// one the list does not hold — which is how a marker whose thread arrived
    /// after the snapshot that drew the canvas is skipped instead of drawn
    /// unnumbered.
    pub fn ordinal(&self, id: i64) -> Option<usize> {
        let thread = self.thread(id)?;
        let page_id = thread.anchor.as_ref()?.page_id.as_str();
        self.pinned_on_page(page_id)
            .iter()
            .position(|held| held.id == id)
            .map(|index| index + 1)
    }

    /// The threads of `page_id` that have a drawable pin, in the server's order.
    ///
    /// This is the sequence both the rail's numbering and the canvas' marker
    /// placement walk, which is what makes a row's number and its marker's
    /// number the same number. "Drawable" is part of the filter rather than a
    /// second check at the marker: a coordinate outside the range the daemon
    /// stores is a thread with no pin, and numbering it would leave a gap in the
    /// numbers painted on the canvas.
    pub fn pinned_on_page(&self, page_id: &str) -> Vec<&CommentThread> {
        self.threads
            .iter()
            .filter(|thread| {
                thread
                    .anchor
                    .as_ref()
                    .is_some_and(|anchor| anchor.page_id == page_id && anchor.is_placeable())
            })
            .collect()
    }

    /// Open threads the rail lists for `page_id` — what the toolbar badge shows.
    ///
    /// The badge counts what pressing it opens, so it includes the pin-less
    /// threads for the reason the list does; anything else would be a number
    /// that disagrees with the first thing the reviewer sees.
    ///
    /// Both this and the per-page marker a page list paints are answered by
    /// [`Self::page_comment_counts`] — one count with two readings, never two
    /// counts of the same page. That function's notes say why the marker shows
    /// strictly fewer threads than this in a document whose comments predate
    /// coordinates.
    pub fn open_count_on_page(&self, page_id: &str) -> usize {
        self.page_comment_counts(page_id).listed()
    }

    /// Open threads pinned on pages other than `page_id`.
    ///
    /// The rail lists one page; a count of the rest is how a reviewer learns
    /// that the review does not end with it, without a list whose rows would
    /// each have to say which page they belong to. A pin-less thread is not
    /// "elsewhere" — it is nowhere, and it is already in the list.
    pub fn open_count_elsewhere(&self, page_id: &str) -> usize {
        self.threads
            .iter()
            .filter(|thread| {
                !thread.resolved
                    && thread
                        .anchor
                        .as_ref()
                        .is_some_and(|anchor| anchor.page_id != page_id)
            })
            .count()
    }

    /// Every open thread of the document.
    pub fn open_count(&self) -> usize {
        self.threads
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
    }

    /// Whether the right rail is showing the document's conversations.
    ///
    /// The rail has one occupant at a time, and the comment tool selects it —
    /// see the module notes on why this is the mode bit and not a second flag.
    pub fn rail_visible(&self) -> bool {
        self.pin_mode
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
        self.pending_pin = None;
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
        self.pending_pin = None;
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
    /// and then clicked the canvas asked for a new pin, and the click that lands
    /// calls [`Self::begin_thread_at`], which clears the open thread.
    pub fn composer(&self) -> Option<CommentComposer> {
        if let Some(id) = self.open_thread {
            return Some(CommentComposer::Thread(id));
        }
        self.pending_pin
            .as_ref()
            .map(|anchor| CommentComposer::NewThread(anchor.clone()))
    }

    pub fn set_pin_mode(&mut self, on: bool) {
        if on {
            self.begin_mode();
        } else {
            self.end_mode();
        }
    }

    pub fn toggle_pin_mode(&mut self) {
        self.set_pin_mode(!self.pin_mode);
    }

    /// Activate the comment tool: show the rail, drop a pin on the next click.
    ///
    /// The reload is part of turning it on, not an afterthought: the daemon
    /// pushes no signal for comments, so the list a client holds is only as
    /// fresh as its last read — a rail that opened onto somebody else's
    /// yesterday would be worse than one that opened onto a spinner.
    pub fn begin_mode(&mut self) {
        // A host with no comment client has no list to show and no write to
        // send: refusing here means a stray call can never blank a rail.
        if !self.transport {
            return;
        }
        self.pin_mode = true;
        self.request_reload();
    }

    /// Leave the comment tool: the rail goes back to the inspector.
    ///
    /// Anything half-written goes with it. A pin waiting for a click and a
    /// draft waiting for a send belong to the mode; keeping them would leave a
    /// composer on screen after the rail that explains it has gone back to
    /// properties, which is how a comment arrives with no visible reason.
    pub fn end_mode(&mut self) {
        self.pin_mode = false;
        self.close();
    }

    /// A canvas click landed at `anchor` while the comment tool was active:
    /// open the composer for a comment about that point.
    ///
    /// The point is taken as given — element or empty space, the pin goes where
    /// the reviewer clicked (see the module notes). The mode stays active: a
    /// review is often several pins in a row, and leaving it after one would
    /// make the second comment a trip back to the toolbar.
    pub fn begin_thread_at(&mut self, anchor: CommentAnchor) {
        if !anchor.is_placeable() {
            return;
        }
        self.open_thread = None;
        self.reply_draft.clear();
        self.pending_pin = Some(anchor);
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
            Some(CommentComposer::NewThread(anchor)) => {
                let text = self.new_draft.trim().to_string();
                self.new_draft.clear();
                self.pending_pin = None;
                self.composer_focused = false;
                // The tool stays active: the pin is placed by the server's
                // answer (the thread's coordinates), and a review is a sequence
                // of comments — the reviewer leaves the mode by picking another
                // tool, exactly as with every other tool in the column.
                self.pending.push(CommentRequest::Create { anchor, text });
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

// Which document a held conversation belongs to, and when it stops belonging.
// A sibling file rather than a directory, like the test module below: the crate
// convention keeps the split flat and the import paths unchanged.
#[path = "comments_document.rs"]
mod comments_document;

#[cfg(test)]
#[path = "comments_tests.rs"]
mod tests;
