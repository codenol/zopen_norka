//! The preview of a stored document: rendering it, recording it, serving it.
//!
//! A sibling of `files_routes` rather than part of it because the preview is a
//! cluster of its own — a raster export, the row's flag, and one route that
//! hands the bytes back — and the spine had reached the repository's 800-line
//! cap. The routes that USE it (create, save, `GET /<key>/thumb`) stay in the
//! spine; what moved here is what they all call.

use super::files_routes::{ok_json, store_error_reply};
use super::*;
use crate::document_db::DocumentDb;
use crate::document_store::{self, DocumentStoreError};

/// Width a card's preview is rendered at.
///
/// A card paints roughly 250 px wide at 2× on a retina display, so 480 is the
/// smallest size that still looks sharp — and it keeps the file small enough
/// to send as base64 without a second route type.
const THUMB_WIDTH: f32 = 480.0;

/// Render and store a preview for a document, returning whether one exists.
///
/// Rendering happens through the same raster export the Export button uses, so
/// the preview is the document as the renderer sees it — not a second, simpler
/// painter that would drift from it.
fn render_thumbnail(state: &WebCanvasState, dir: &std::path::Path, key: &str) -> bool {
    let Ok(path) = document_store::thumb_path(dir, key) else {
        return false;
    };
    let scene = op_pen_loader::editor_state_to_active_page_layout_scene(&state.editor);
    // Scale to the target width rather than a fixed factor: documents are
    // authored at whatever size the designer chose, and a 20 000 px board at
    // scale 1 would be a several-megabyte preview.
    let scale = scene
        .active_page()
        .and_then(op_render_export::page_bounds)
        .map(|bounds| (THUMB_WIDTH / bounds.size.x.max(1.0)).min(1.0))
        .unwrap_or(1.0);
    match crate::export::export_raster(&scene, &path, crate::export::RasterFormat::Png, scale) {
        Ok(()) => true,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            false
        }
    }
}

/// Keep the stored preview in step with a document that was just written.
pub(super) fn refresh_thumbnail(state: &WebCanvasState, store: &DocumentDb, key: &str) {
    let has = render_thumbnail(state, store.dir(), key);
    let _ = document_store::note_thumbnail(store, key, has);
}

/// `GET /api/files/<key>/thumb` — the stored preview, base64 like the export
/// routes (the reply type carries text, and the export precedent is a JSON
/// envelope rather than a raw body).
pub(super) fn thumbnail(dir: &std::path::Path, key: &str) -> WebReply {
    let path = match document_store::thumb_path(dir, key) {
        Ok(path) => path,
        Err(error) => return store_error_reply(error),
    };
    match std::fs::read(&path) {
        Ok(bytes) => ok_json(serde_json::json!({
            "ok": true,
            "mime": "image/png",
            "dataBase64": base64::engine::general_purpose::STANDARD.encode(bytes),
        })),
        Err(_) => store_error_reply(DocumentStoreError::NotFound),
    }
}
