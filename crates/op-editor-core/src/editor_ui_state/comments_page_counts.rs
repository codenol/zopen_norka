//! How many open conversations one page carries — the single count behind both
//! the toolbar badge and a page row's marker.
//!
//! ## Why one function returns two numbers
//!
//! A thread the daemon migrated from the old element-keyed format has no pin at
//! all: no page, no coordinate (see [`CommentThread::anchor`]). So it belongs to
//! no page — and it is listed on *every* page, because a conversation nobody can
//! find would be worse than one listed without a marker to jump to.
//!
//! Those two facts pull the counts apart, and the honest answer is to keep both
//! rather than to pick one and make the other surface lie:
//!
//! - [`PageCommentCounts::pinned`] is what a page's **marker** shows: the
//!   conversations a reviewer can see on that page, which is the question a page
//!   list asks. A migrated thread is not one of them, on any page, and it is not
//!   missing from the count — it was never on that page.
//! - [`PageCommentCounts::listed`] is what the toolbar **badge** shows: the same
//!   number plus the pin-less threads, because the badge counts what pressing it
//!   opens, and the rail lists those rows too.
//!
//! The two therefore differ by exactly the migrated threads, and only in a
//! document that has any. That is a consequence of the model, not a bug to file:
//! reach for [`CommentsUiState::page_comment_counts`] when one of the two numbers
//! is wanted, never for a third count written beside it.
//!
//! [`CommentThread::anchor`]: super::comments::CommentThread::anchor

use super::comments::CommentsUiState;

/// Open conversations on one page, split by whether that page can show them.
///
/// Both fields count **open** threads only: a resolved conversation is done, and
/// a marker that outlived its own resolution would be a permanent "there is
/// something here" about something that is not there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageCommentCounts {
    /// Open threads the page draws a pin for.
    pub pinned: usize,
    /// Open threads the page's list shows but the page cannot point at —
    /// the migrated, pin-less ones, plus any whose stored coordinate is outside
    /// the range a pin could be drawn at.
    pub unpinned: usize,
}

impl PageCommentCounts {
    /// Open threads the page's list holds — what the toolbar badge shows.
    pub fn listed(&self) -> usize {
        self.pinned + self.unpinned
    }
}

impl CommentsUiState {
    /// Open threads on `page_id`, split by whether the page can draw them.
    ///
    /// `unpinned` is the difference between the page's list and its pins rather
    /// than a second filter over the threads: the two lists are the state's own
    /// answer to "what belongs to this page" ([`Self::pinned_on_page`],
    /// [`Self::threads_on_page`]), and recomputing membership here would be a
    /// second opinion about which page a thread is on.
    pub fn page_comment_counts(&self, page_id: &str) -> PageCommentCounts {
        let pinned = self
            .pinned_on_page(page_id)
            .into_iter()
            .filter(|thread| !thread.resolved)
            .count();
        let listed = self
            .threads_on_page(page_id)
            .into_iter()
            .filter(|thread| !thread.resolved)
            .count();
        PageCommentCounts {
            pinned,
            // Saturating rather than plain: `pinned` is a subset of `listed` by
            // construction, and a future change that broke that would rather
            // show a count of zero than panic on a panel repaint.
            unpinned: listed.saturating_sub(pinned),
        }
    }
}

#[cfg(test)]
#[path = "comments_page_counts_tests.rs"]
mod tests;
