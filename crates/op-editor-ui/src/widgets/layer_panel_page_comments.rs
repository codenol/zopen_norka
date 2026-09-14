//! Which page rows show a comment marker, and how many.
//!
//! The count is the same one the toolbar badge shows for the active page, read
//! out of the same state through the same function
//! (`CommentsUiState::page_comment_counts`): a page row answers "how much of this
//! review is on that page", the badge answers "how much is on the page I am
//! looking at", and when the row is the active one they are the same question.
//!
//! ## Why this is computed per build, not cached with the rows
//!
//! The layer panel's row model is cached against the document revision, the
//! active page, the collapsed set and the rename draft — nothing a comment
//! touches. A thread arriving or being resolved does not move the document (the
//! daemon deliberately keeps comments out of the document version, see
//! `op_editor_core::editor_ui_state::comments`), so a count baked into the
//! cached rows would keep saying "three" long after the third conversation was
//! closed. It is therefore a live overlay on the panel, like selection and
//! hover, and costs one pass over the document's pages per built panel.

use std::rc::Rc;

use op_editor_core::EditorState;

/// Open threads pinned on each document page, indexed by page index.
///
/// Indexed the way `PageItem::page_index` is — the document's own page index for
/// every row that names a page — so a row looks its count up by the position it
/// already carries, and no second mapping from row to page can drift.
///
/// The number is the **pinned** one: open threads the page draws a marker for.
/// A thread the daemon migrated from the old element-keyed format has no page
/// (see `CommentThread::anchor`), so it is counted on none of them, however many
/// pages the document has. It is not lost — the toolbar badge and the rail still
/// list it — and a reader who adds up a document's page markers and finds one
/// thread missing has found exactly that, not a bug.
pub(crate) fn page_comment_counts(state: &EditorState) -> Rc<Vec<usize>> {
    // A document without a `pages` array still shows one page row, and its id is
    // synthesized, so "one page" rather than "no pages" is the length here.
    let pages = state
        .doc
        .pages
        .as_ref()
        .filter(|pages| !pages.is_empty())
        .map_or(1, |pages| pages.len());
    Rc::new(
        (0..pages)
            .map(|index| {
                let (page_id, _) = state.page_identity_at(index);
                state
                    .editor_ui
                    .comments
                    .page_comment_counts(&page_id)
                    .pinned
            })
            .collect(),
    )
}
