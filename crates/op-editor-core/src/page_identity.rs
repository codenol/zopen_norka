//! Which page the editor is showing, and what it is called.
//!
//! A page-scoped projection — comment pins, the comment rail, anything that
//! belongs to one page rather than to the document — has to name the page it is
//! talking about, and the name has to be the same string everywhere: the render
//! scene builder calls [`EditorState::active_page_identity`] to label its pages
//! and the comment client sends what it returns, so a marker written under one
//! name and looked for under another is the failure this file exists to prevent.
//!
//! It lives beside the page mutators rather than inside `mutators.rs` for the
//! repository's line ceiling, and because the rule it states is about pages
//! rather than about the document's nodes.

use crate::state::EditorState;

impl EditorState {
    /// Id of the page the editor is showing.
    ///
    /// `None` for a document with no `pages` array — use
    /// [`Self::active_page_identity`] when a page has to be named: a
    /// single-page document has an identity, it just has no authored id for it.
    /// An out-of-range `active_page_index` reads the last page, the same
    /// fallback [`Self::active_children`] uses, so the id always names the page
    /// whose children are on screen.
    pub fn active_page_id(&self) -> Option<&str> {
        let pages = self.doc.pages.as_ref().filter(|pages| !pages.is_empty())?;
        let i = self.ui.active_page_index.min(pages.len() - 1);
        Some(pages[i].id.as_str())
    }

    /// The active page's id **and** display name, with the single-page fallback.
    ///
    /// A document with a `pages` array names its page by that entry. A document
    /// without one — the shape every legacy `.op` file and every freshly
    /// imported SVG has — has no authored page identity at all, so one is
    /// synthesized: `"n1"` while the document holds nothing, `"page-1"`
    /// otherwise, which is the id the render scene and the MCP page tools have
    /// always used for that case.
    pub fn active_page_identity(&self) -> (String, String) {
        if let Some(page) = self
            .doc
            .pages
            .as_ref()
            .filter(|pages| !pages.is_empty())
            .map(|pages| {
                let i = self.ui.active_page_index.min(pages.len() - 1);
                &pages[i]
            })
        {
            return (page.id.clone(), page.name.clone());
        }
        if self.doc.children.is_empty() {
            ("n1".to_string(), "Page 1".to_string())
        } else {
            (
                "page-1".to_string(),
                self.doc.name.as_deref().unwrap_or("Page 1").to_string(),
            )
        }
    }
}

#[cfg(test)]
#[path = "page_identity_tests.rs"]
mod tests;
