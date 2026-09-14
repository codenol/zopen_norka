//! Which document a held conversation belongs to, and when it stops belonging.
//!
//! Split out of `comments.rs` (a sibling `impl CommentsUiState`, declared with
//! `#[path]` so the file stays a sibling rather than a directory) at the 800-line
//! cap. The cluster is cohesive: it is the life of the LIST — installing one, and
//! the two ways it ends — as opposed to the review state around it (the open
//! thread, the drafts, the mode).
//!
//! The reason it needs its own key at all is the daemon's own model: a
//! conversation is filed under the document's key
//! (`/api/files/<key>/comments`), so the key is what says whether the list and
//! the document on screen are the same subject. Everything else in the module
//! follows from that one fact.

use super::{CommentThread, CommentsUiState};

impl CommentsUiState {
    /// Forget everything that belongs to the previous document.
    ///
    /// Called from `clear_document_derived`, because a thread is about a node of
    /// one document and painting the previous document's conversation over the
    /// next one is the kind of "my change does nothing" that is really "your
    /// change is about something that no longer exists".
    ///
    /// ## Why a replacement under the same key is not a new conversation
    ///
    /// `key` is the document the editor is installing. When it is the key the
    /// held list was read for, the list IS this document's conversation and is
    /// kept: the daemon stores threads under the key, so the same key is the
    /// same conversation however many times its content is replaced (a sync
    /// apply from an AI turn, an external MCP write, a collaboration commit).
    ///
    /// That is not an optimisation, it is what makes reading at open work at
    /// all. Opening a document happens in two steps — the key is adopted when
    /// the daemon accepts the open, the content arrives a round trip later — and
    /// the read the first step asks for is answered by the small comment list
    /// well before the document itself lands. Wiping on arrival would throw away
    /// the answer that was just fetched, and the markers would stay invisible
    /// until the reviewer opened the tool, which is exactly the symptom this
    /// exists to remove.
    ///
    /// Everything else — a different key, or no key at all — is a different
    /// conversation and is dropped.
    pub fn clear_for_document(&mut self, key: Option<&str>) {
        let same_document = self.document_key.is_some() && self.document_key.as_deref() == key;
        if same_document {
            return;
        }
        self.forget_threads();
    }

    /// Forget the conversation outright, whoever and whatever it was about.
    ///
    /// For the cases where the document is not what changed but who is looking
    /// at it: the daemon answers a conversation per account (a document not
    /// shared with the caller is a 403, and the authors are account ids), so a
    /// tab that has just stopped speaking for the account whose words it is
    /// holding has no business showing them for a frame — and the read that
    /// replaces them is a round trip away.
    pub fn forget_threads(&mut self) {
        self.threads.clear();
        self.loading = false;
        self.error = None;
        self.open_thread = None;
        self.reply_draft.clear();
        self.new_draft.clear();
        self.pin_mode = false;
        self.pending_pin = None;
        self.composer_focused = false;
        self.document_key = None;
        // `viewer_id` is deliberately kept: it is who this client is, not
        // something the document said.
        // `pin_mode` is NOT kept, unlike the old panel flag: the mode is
        // attached to the page a click landed on, and the next document's
        // coordinate space is not that one. A reviewer who wants the comment
        // rail back picks the tool again, which is one click and unambiguous.
        self.pending.clear();
    }

    /// The key whose conversation is held, for the host that reads the list.
    ///
    /// Exposed so "the list is empty" can be told apart from "nobody asked": an
    /// empty list for a key that was read is a document nobody commented on, and
    /// an empty list under no key is a document nobody has asked about.
    pub fn document_key(&self) -> Option<&str> {
        self.document_key.as_deref()
    }

    /// Replace the list with a freshly read answer.
    ///
    /// Closes the popover when its thread is no longer in the answer rather than
    /// leaving it painting a thread the server has forgotten.
    ///
    /// The key is NOT touched: [`Self::install_threads_for_key`] is the entry
    /// point for an answer, and it is the only install that knows what document
    /// it is about. This one is for a caller holding threads that already belong
    /// to the key in force — fixtures, tests, and a restore that kept the list.
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

    /// Install the answer to a read of `key`'s conversation.
    ///
    /// The key is recorded with the list so [`Self::clear_for_document`] can
    /// tell "the same document was replaced" from "another document is open" —
    /// and it is the key the request was ISSUED for, never the key that happens
    /// to be open when the answer lands: a reply that arrives after the reviewer
    /// moved to another document still describes the old one, and claiming it
    /// for the new key would paint one document's pins on another.
    pub fn install_threads_for_key(&mut self, key: Option<String>, threads: Vec<CommentThread>) {
        self.install_threads(threads);
        self.document_key = key;
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
}
