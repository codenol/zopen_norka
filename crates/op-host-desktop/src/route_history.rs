//! Back and Forward for the desktop window.
//!
//! The browser spells this "the address bar and the History API": selecting a
//! node rewrites the address, and Back returns to the previous one. The
//! desktop has neither, so the same trail is kept here — a stack of
//! `op_editor_core::route::DocumentRoute` values, which is the *same*
//! vocabulary the browser writes into its URL. What differs between the hosts
//! is only how a route is presented, never what a route is.
//!
//! Recording follows the browser's rule: changing document adds an entry,
//! while selecting a node or switching a page rewrites the current one. That
//! is what makes Back mean "the previous document, or the state I was in
//! before I started clicking around".

use op_editor_core::route::{self, DocumentRoute, RouteFile, RouteTarget};

use crate::DesktopApp;

/// How many steps back the window remembers.
///
/// Deep enough to retrace a session's worth of navigation, bounded so a long
/// day does not grow without limit.
const ROUTE_HISTORY_CAP: usize = 64;

impl DesktopApp {
    /// Record where the window is now, if it has moved.
    ///
    /// Called once per frame: cheap when nothing changed (a route compare) and
    /// the only place that knows both the document and the selection.
    pub(crate) fn note_route(&mut self) {
        if self.route_replaying {
            return;
        }
        let current = self.current_route();
        if let Some(last) = self.route_history.get(self.route_cursor) {
            if last == &current {
                return;
            }
            // Same document, different page or selection: rewrite the entry
            // rather than adding one, exactly as the browser's `replaceState`
            // does for a selection.
            if same_document(last, &current) {
                self.route_history[self.route_cursor] = current;
                return;
            }
        }
        self.route_history.truncate(self.route_cursor + 1);
        self.route_history.push(current);
        if self.route_history.len() > ROUTE_HISTORY_CAP {
            self.route_history.remove(0);
        }
        self.route_cursor = self.route_history.len().saturating_sub(1);
        self.refresh_window_title();
    }

    /// The route the window is showing.
    ///
    /// Built by the shared rule in `op_editor_core::route`, like the browser's
    /// address bar: the only thing the desktop contributes is the document's
    /// identity, which here is the file on disk rather than a server key.
    fn current_route(&self) -> DocumentRoute {
        let state = self.host.editor_state();
        let file = match self.current_path.as_ref() {
            Some(path) => RouteFile::Key(path.to_string_lossy().into_owned()),
            None => op_editor_core::route::file_from_key(state),
        };
        match route::state_route(state, file) {
            RouteTarget::Document(route) => route,
            // The window has no file-browser screen; the arm exists because
            // the rule is shared with the browser, which does.
            RouteTarget::Files => DocumentRoute::untitled(),
        }
    }

    /// Move `delta` entries through the trail and apply the route there.
    ///
    /// Returns whether anything moved, so the caller can repaint.
    pub(crate) fn navigate_route(&mut self, delta: isize) -> bool {
        if self.route_history.is_empty() {
            return false;
        }
        let target = self.route_cursor as isize + delta;
        if target < 0 || target as usize >= self.route_history.len() {
            return false;
        }
        self.route_cursor = target as usize;
        let route = self.route_history[self.route_cursor].clone();
        self.route_replaying = true;
        let applied = self.apply_route(&route);
        self.route_replaying = false;
        if applied {
            self.refresh_window_title();
        }
        applied
    }

    /// Show a route: switch page, select the node it names, and frame it.
    fn apply_route(&mut self, route: &DocumentRoute) -> bool {
        let (viewport_w, viewport_h) = (self.viewport_width, self.viewport_height);
        let mut changed = false;
        if let Some(page) = route.page {
            if self.host.editor_state_mut().set_active_page(page) {
                changed = true;
            }
        } else if self.host.editor_state().ui.active_page_index != 0 {
            if self.host.editor_state_mut().set_active_page(0) {
                changed = true;
            }
        }
        match route.node.clone() {
            Some(node) => {
                self.host.editor_state_mut().set_single_selection(node.clone());
                changed = true;
                // Camera only: retracing history must not land on undo.
                self.host
                    .reveal_node(node.as_str(), viewport_w, viewport_h);
            }
            None => {
                if !self.host.editor_state().selection.set.is_empty() {
                    self.host.editor_state_mut().clear_selection();
                    changed = true;
                }
            }
        }
        if changed {
            self.host.mark_editor_state_dirty();
        }
        changed
    }
}

/// Whether two routes name the same document.
fn same_document(a: &DocumentRoute, b: &DocumentRoute) -> bool {
    matches!(
        (&a.file, &b.file),
        (RouteFile::Untitled, RouteFile::Untitled)
    ) || route::to_path(&RouteTarget::Document(a.clone()))
        .split('?')
        .next()
        == route::to_path(&RouteTarget::Document(b.clone()))
            .split('?')
            .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::NodeId;

    fn route(key: Option<&str>, page: Option<usize>, node: Option<&str>) -> DocumentRoute {
        DocumentRoute {
            file: match key {
                Some(key) => RouteFile::Key(key.to_string()),
                None => RouteFile::Untitled,
            },
            slug: None,
            page,
            node: node.map(|node| NodeId::new(node.to_string())),
            embed: None,
        }
    }

    #[test]
    fn one_document_with_a_different_page_is_the_same_document() {
        assert!(same_document(
            &route(Some("k"), None, None),
            &route(Some("k"), Some(3), None)
        ));
    }

    #[test]
    fn different_documents_are_not_the_same() {
        assert!(!same_document(
            &route(Some("a"), None, None),
            &route(Some("b"), None, None)
        ));
        assert!(!same_document(
            &route(None, None, None),
            &route(Some("a"), None, None)
        ));
    }
}

/// The origin a copied link points at, when the desktop has no better answer.
///
/// A desktop window does not know which daemon a link should open against —
/// that is a setting nobody has made yet, so the local daemon (the one this
/// app starts and talks to) is the honest default: it is the address that
/// actually works on this machine.
const DEFAULT_SHARE_BASE: &str = op_editor_core::DEFAULT_LOCAL_DAEMON_ORIGIN;

impl DesktopApp {
    /// Answer a "copy link" the widget layer asked for.
    ///
    /// The menu row and the shortcut both land on the state flag, because the
    /// widget layer has no clipboard and no origin; this is the layer that has
    /// both. Returns whether anything was copied, so the caller can repaint the
    /// toast.
    pub(crate) fn drain_copy_link_request(&mut self) -> bool {
        if !self.host.editor_state().editor_ui.copy_link_requested {
            return false;
        }
        self.host.editor_state_mut().editor_ui.copy_link_requested = false;
        let Some(link) =
            op_editor_core::route::selection_link(DEFAULT_SHARE_BASE, self.host.editor_state())
        else {
            return false;
        };
        crate::clipboard::set_text(&link);
        // The clock is read before the mutable borrow: `show_toast` needs the
        // state mutably and the states's own wall clock immutably.
        let now_ms = self.host.editor_state().editor_ui.now_unix_ms as u64;
        self.host.editor_state_mut().editor_ui.show_toast(
            "layerMenu.linkCopied",
            Vec::new(),
            op_editor_core::editor_toast::EditorToastLevel::Info,
            now_ms,
        );
        self.request_redraw(true);
        true
    }
}

impl DesktopApp {
    /// Apply `--node <id>` once the window knows its size.
    ///
    /// The desktop's answer to opening a link: the document is already loaded
    /// by the time a window exists, so this only has to select the node and
    /// frame it — the same two steps `navigate_route` performs.
    pub(crate) fn drain_pending_node(&mut self) -> bool {
        let Some(node) = self.pending_node.take() else {
            return false;
        };
        if self.host.reveal_node(
            &node,
            self.viewport_width,
            self.viewport_height,
        ) {
            self.host
                .editor_state_mut()
                .set_single_selection(op_editor_core::NodeId::new(node));
            self.host.mark_editor_state_dirty();
            self.request_redraw(true);
            return true;
        }
        false
    }
}
