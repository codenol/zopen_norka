//! Reading the address, and naming what the editor state says.
//!
//! The half of the address bar that parses rather than performs: what the tab's
//! address currently names, what the editor state's own route is, the document
//! part of an address for comparing two of them, the file list and its cap, the
//! page a node is on, whether an open answer said this caller may write, and the
//! slug rule. Nothing here touches history or the wire.

use op_editor_core::route::{self, RouteFile, RoutePath, RouteTarget};
use op_editor_core::NodeId;

use crate::widget_host::WidgetHost;
/// Read the tab's current route, if it names one.
pub(crate) fn current_location() -> Option<RouteTarget> {
    let window = web_sys::window()?;
    let location = window.location();
    let path = location.pathname().ok()?;
    let query = location.search().unwrap_or_default();
    match route::parse(&path, &query) {
        RoutePath::Known(target) => Some(target),
        RoutePath::NotARoute => None,
    }
}

/// The route the editor state currently describes, for the address bar.
///
/// Two clauses, on purpose. The document mapping is the shared
/// [`route::state_route`] — the same rule the desktop records for Back/Forward
/// — and the browser adds the one thing that rule cannot know: `/files` is a
/// screen rather than a place in a document. A copy of the document mapping
/// here is exactly the drift the shared rule exists to prevent, so there is
/// none.
pub(crate) fn state_route(state: &op_editor_core::EditorState) -> RouteTarget {
    if state.editor_ui.screen == op_editor_core::AppScreen::Files {
        return RouteTarget::Files;
    }
    route::state_route(state, route::file_from_key(state))
}

/// The document part of an address, for comparing two routes.
pub(super) fn route_file_of(path: &str) -> Option<String> {
    match route::parse(path, "") {
        RoutePath::Known(RouteTarget::Document(route)) => Some(match &route.file {
            RouteFile::Key(key) => key.clone(),
            RouteFile::Untitled => "/".to_string(),
        }),
        _ => None,
    }
}

pub(super) fn set_title(host: &WidgetHost, target: &RouteTarget) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let name = host.editor_state().editor_ui.file_name_display.clone();
    let base = op_editor_ui::PRODUCT_NAME;
    let title = match (&target, name) {
        (RouteTarget::Files, _) => format!("Files — {base}"),
        (RouteTarget::Document(_), Some(name)) => format!("{name} — {base}"),
        (RouteTarget::Document(_), None) => format!("Untitled — {base}"),
    };
    document.set_title(&title);
}
/// Read a `/api/files` response, newest first, capped for the screen.
pub(super) fn parse_file_list(response: &str) -> Result<Vec<op_editor_core::ServerFile>, String> {
    let value: serde_json::Value = serde_json::from_str(response)
        .map_err(|_| "The server sent an unreadable list".to_string())?;
    if value.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        return Err(value
            .get("error")
            .and_then(|error| error.as_str())
            .unwrap_or("The server refused the list")
            .to_string());
    }
    let files = value
        .get("files")
        .and_then(|files| files.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    Some(op_editor_core::ServerFile {
                        key: file.get("key")?.as_str()?.to_string(),
                        name: file
                            .get("name")
                            .and_then(|name| name.as_str())
                            .unwrap_or("Untitled")
                            .to_string(),
                        updated_at: file
                            .get("updatedAt")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(0),
                        size: file
                            .get("size")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(0),
                        has_thumbnail: file
                            .get("hasThumbnail")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut files = files;
    files.truncate(op_editor_core::SERVER_FILE_CAP);
    Ok(files)
}

/// The page index holding `node`, when the document has it.
pub(super) fn page_of(state: &op_editor_core::EditorState, node: &NodeId) -> Option<usize> {
    let pages = state.doc.pages.as_ref()?;
    pages.iter().position(|page| {
        page.children.iter().any(|root| {
            op_editor_core::walkers::find_node(std::slice::from_ref(root), node).is_some()
        })
    })
}

/// Keeps `route::slugify` reachable from the route tests without importing the
/// crate path there; also documents the only place the slug is produced.
#[cfg(test)]
pub(crate) fn slug_for(name: &str) -> String {
    route::slugify(name)
}

/// Whether an open answer says this caller may write the document.
///
/// `None` when the answer does not say — an older daemon, or a deployment with
/// no accounts. Silence is "no opinion", not "no": the caller keeps whatever it
/// knew rather than a working editor turning read-only because a field is
/// missing (issue #43).
pub(super) fn can_write_from_open(value: &serde_json::Value) -> Option<bool> {
    value.get("canWrite").and_then(|can| can.as_bool())
}
