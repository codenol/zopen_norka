//! The turn-result guard: what stops a tab that is BEHIND from writing its
//! older copy over a screen the daemon's own tools just drew.
//!
//! Issues #247 and #248, measured. The failing sequence is:
//!
//! 1. a design turn runs; the daemon applies the agent's tool commits to its own
//!    document (measured: 230 nodes on `pages[0]`);
//! 2. the tab is behind — it never took that document (its read was cut off, or
//!    its push was refused by the duplicate-id gate of #226), and it holds an
//!    older copy (the starter, or 116 of the 230 nodes);
//! 3. the tab autosaves, which is `POST /api/files/<key>/autosave` with no
//!    `baseVersion` at all (measured: every autosave body of a live run carries
//!    only `document` and `activePageIndex`), and the daemon adopts it —
//!    replacing both its memory and the file with the older copy.
//!
//! The guard that was meant to prevent step 3 keyed on a boolean that any
//! `GET /api/mcp/document` cleared. Since EVERY reader clears it — the tab's own
//! sync poll, a second tab, an outside observer watching the run — the guard was
//! down almost all the time, and the loss came back.
//!
//! So the question this module answers is not "has anyone read the document"
//! but "does the copy being written actually contain what the daemon drew".
//! Reading is not taking: a tab has taken the result only when the document it
//! writes back carries the turn's own top-level nodes. Anything else is a tab
//! that never saw them, and its write is refused with `stale-autosave`.
//!
//! An explicit Save is deliberately not subject to this: "save" means "what I
//! see", and that is the operator's call (#169), not a rule to guess.

use std::collections::BTreeSet;

/// The top-level ids of the active page as the daemon's own tools last left it,
/// while no tab has taken them.
///
/// `None` means the daemon is not ahead of anyone: nothing to protect.
#[derive(Debug, Default)]
pub(crate) struct TurnResultGuard {
    roots: Option<BTreeSet<String>>,
}

impl TurnResultGuard {
    /// Whether the daemon holds a result no tab has taken yet.
    pub(crate) fn is_ahead(&self) -> bool {
        self.roots.is_some()
    }

    /// Record the ids the daemon's tools just committed. Replaces any earlier
    /// set rather than merging: the guard asks whether the tab holds the
    /// document as it stands NOW, and a node the turn itself removed is not
    /// something a tab has to carry.
    pub(crate) fn note_turn_result(&mut self, roots: impl IntoIterator<Item = String>) {
        self.roots = Some(roots.into_iter().collect());
    }

    /// Whether a write carrying `incoming` is a tab that never took the result.
    ///
    /// A tab that took the document has every one of these ids — ids are the
    /// document's own, and taking a document preserves them. A tab that did not
    /// has none of them (it never saw them) or only some (it saw an earlier
    /// turn), and either way its write is the older copy.
    pub(crate) fn refuses(&self, incoming: &BTreeSet<String>) -> bool {
        match &self.roots {
            None => false,
            Some(turn) => !turn.is_subset(incoming),
        }
    }

    /// The daemon and the tabs agree again — a write that carried the result
    /// was taken, the document was replaced wholesale, or the result is no
    /// longer the document's.
    pub(crate) fn clear(&mut self) {
        self.roots = None;
    }
}

/// Every page's top-level node ids in a document push body, or `None` when the
/// body does not parse.
///
/// Deliberately a shallow JSON walk rather than a full canonical load: it runs
/// on the save path, against a body that can be megabytes, and all it needs is
/// the ids of the nodes that sit directly on a page. Every page is collected,
/// not only the active one, so a tab whose active page differs still counts as
/// holding the result. The document's own top-level `children` is read too: a
/// canonical document carries the active page's nodes there as well, and both
/// shapes reach this daemon (see `SYNC_BODY` in the host tests).
pub(crate) fn top_level_ids_in_body(body: &str) -> Option<BTreeSet<String>> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let doc = value.get("document").unwrap_or(&value);
    let mut ids = BTreeSet::new();
    let mut saw_pages_or_children = false;
    if let Some(pages) = doc.get("pages").and_then(|p| p.as_array()) {
        saw_pages_or_children = true;
        for page in pages {
            collect_top_level(page, &mut ids);
        }
    }
    if doc.get("children").is_some() {
        saw_pages_or_children = true;
        collect_top_level(doc, &mut ids);
    }
    saw_pages_or_children.then_some(ids)
}

/// Add the ids of `container`'s direct children to `ids`.
fn collect_top_level(container: &serde_json::Value, ids: &mut BTreeSet<String>) {
    let Some(children) = container.get("children").and_then(|c| c.as_array()) else {
        return;
    };
    for child in children {
        if let Some(id) = child.get("id").and_then(|i| i.as_str()) {
            ids.insert(id.to_string());
        }
    }
}

#[cfg(test)]
#[path = "turn_result_guard_tests.rs"]
mod tests;
