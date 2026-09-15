//! Host/widget handoff for document previews (`/api/files/<key>/thumb`).
//!
//! The file screen paints a card's preview from bytes the daemon serves as a
//! base64 JSON envelope. The widget layer cannot make that request itself, so
//! this is the same shape as `collab_avatar_runtime`: paint asks here, the host
//! drains the queue, fetches, decodes and installs the bytes, and paint draws
//! them on a later frame.
//!
//! Bytes land in the shared image cache (`canvas_viewport_image`), so decode,
//! scale-to-crispness and eviction are the ones the canvas already uses — a
//! preview is not a second image pipeline.
//!
//! Deliberately not `web_assets`: that registry installs what the server sends
//! verbatim (this route answers JSON), and it leaks each entry for the process
//! lifetime — acceptable for a shipped asset catalogue, wrong for documents a
//! user creates and deletes.

use std::sync::{Arc, Mutex, OnceLock};

/// Where a preview stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbState {
    /// Never asked for.
    Absent,
    /// Asked for; the host has not answered yet.
    Pending,
    /// Bytes are in the image cache.
    Ready,
    /// The request failed, or the document has no preview. Retryable: a later
    /// save changes `updated_at`, which changes the id.
    Failed,
}

/// Longest the pending list may grow before new requests are refused.
///
/// A screen shows at most 60 cards; a list longer than this means something is
/// enqueueing without draining, and dropping requests is better than growing
/// without bound.
const MAX_PENDING: usize = 64;

struct Registry {
    pending: Vec<String>,
    /// Key → (state, the document revision that state is about). The revision
    /// is kept so a failure is remembered for *that* version of the document
    /// only: saving again is a new preview, and asking again is right.
    states: std::collections::HashMap<String, (ThumbState, u64)>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            pending: Vec::new(),
            states: std::collections::HashMap::new(),
        })
    })
}

/// The paint id for a document's preview.
///
/// It carries the document's `updated_at` on purpose. The renderer caches a
/// failed decode against the id and never asks again for that id
/// (`mark_decode_failed`), so a fixed id would burn a card for the rest of the
/// session the first time a decode failed — and would also keep showing a
/// stale picture after the document was saved again.
pub fn thumb_image_id(key: &str, updated_at: u64) -> u64 {
    jian_ops_schema::node::image_src::paint_image_id(&format!(
        "/api/files/{key}/thumb@{updated_at}"
    ))
}

/// Ask for a document's preview. Safe to call from paint every frame: a
/// document already asked for (or already answered) is not enqueued again.
pub fn request_thumb(key: &str, updated_at: u64) {
    let Ok(mut registry) = registry().lock() else {
        return;
    };
    match registry.states.get(key) {
        // Already asked for, already have it, or already failed for this very
        // revision (asking again would spin a request per frame).
        Some((ThumbState::Pending | ThumbState::Ready, _)) => return,
        Some((ThumbState::Failed, revision)) if *revision == updated_at => return,
        _ => {}
    }
    if registry.pending.len() >= MAX_PENDING {
        return;
    }
    registry
        .states
        .insert(key.to_string(), (ThumbState::Pending, updated_at));
    registry.pending.push(key.to_string());
}

/// Take up to `max` keys for the host to fetch.
pub fn take_thumb_requests(max: usize) -> Vec<String> {
    let Ok(mut registry) = registry().lock() else {
        return Vec::new();
    };
    let take = max.min(registry.pending.len());
    registry.pending.drain(..take).collect()
}

/// Install fetched bytes for a document's preview.
///
/// Returns whether the bytes were accepted; empty bytes count as a failure,
/// because a zero-byte image would decode to nothing and leave the card
/// waiting forever.
pub fn install_thumb_bytes(key: &str, bytes: Vec<u8>) -> bool {
    if bytes.is_empty() {
        mark_thumb_failed(key);
        return false;
    }
    // The id comes from the revision the request was made for, so the host
    // does not have to carry `updated_at` through the fetch and the bytes
    // cannot land under a stale id.
    let revision = registry()
        .lock()
        .ok()
        .and_then(|registry| registry.states.get(key).map(|(_, revision)| *revision))
        .unwrap_or(0);
    let image_id = thumb_image_id(key, revision);
    crate::widgets::canvas_viewport_image::store_remote_image_bytes(image_id, bytes);
    set_ready(key);
    true
}

/// Record that a preview could not be fetched or decoded.
///
/// A preview is decoration: a failure costs the picture and nothing else, so
/// this only keeps the screen from asking again on every frame.
pub fn mark_thumb_failed(key: &str) {
    if let Ok(mut registry) = registry().lock() {
        let revision = registry
            .states
            .get(key)
            .map(|(_, revision)| *revision)
            .unwrap_or(0);
        registry
            .states
            .insert(key.to_string(), (ThumbState::Failed, revision));
    }
}

/// Where a document's preview stands.
pub fn thumb_state(key: &str) -> ThumbState {
    registry()
        .lock()
        .ok()
        .and_then(|registry| registry.states.get(key).map(|(state, _)| *state))
        .unwrap_or(ThumbState::Absent)
}

/// The preview's bytes, once installed.
pub fn thumb_bytes(image_id: u64) -> Option<Arc<[u8]>> {
    crate::widgets::canvas_viewport_image::cached_bytes_for(image_id)
}

/// Forget a document's preview — called when the screen reloads its list, so a
/// document deleted and recreated under the same key is not shown as its old
/// self.
pub fn forget_thumb(key: &str) {
    if let Ok(mut registry) = registry().lock() {
        registry.states.remove(key);
        registry.pending.retain(|pending| pending != key);
    }
}

/// Mark ready, keeping whatever revision the entry was recorded with.
fn set_ready(key: &str) {
    if let Ok(mut registry) = registry().lock() {
        let revision = registry
            .states
            .get(key)
            .map(|(_, revision)| *revision)
            .unwrap_or(0);
        registry
            .states
            .insert(key.to_string(), (ThumbState::Ready, revision));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys are per-test: the registry is process-global, as the host's other
    /// runtime registries are.
    fn key(tag: &str) -> String {
        format!("test-thumb-{tag}")
    }

    /// The registry is process-global and the pending list is shared, so tests
    /// that drain it take turns — otherwise one test's `take` steals another's
    /// request and both fail for the wrong reason.
    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Drain everything so a test starts from a known queue.
    fn drain_all() {
        while !take_thumb_requests(MAX_PENDING).is_empty() {}
    }

    #[test]
    fn a_request_is_enqueued_once() {
        let _guard = test_guard();
        drain_all();
        let key = key("once");
        request_thumb(&key, 7);
        request_thumb(&key, 7);
        assert_eq!(thumb_state(&key), ThumbState::Pending);
        let taken = take_thumb_requests(MAX_PENDING);
        assert_eq!(taken.iter().filter(|taken| *taken == &key).count(), 1);
    }

    #[test]
    fn installing_bytes_moves_the_state_to_ready() {
        let _guard = test_guard();
        drain_all();
        let key = key("install");
        request_thumb(&key, 7);
        let _ = take_thumb_requests(MAX_PENDING);
        assert!(install_thumb_bytes(&key, vec![1, 2, 3]));
        assert_eq!(thumb_state(&key), ThumbState::Ready);
        // The bytes are keyed by the revision that was asked for.
        assert!(thumb_bytes(thumb_image_id(&key, 7)).is_some());
    }

    #[test]
    fn empty_bytes_are_a_failure_not_a_ready_preview() {
        let _guard = test_guard();
        drain_all();
        let key = key("empty");
        request_thumb(&key, 7);
        let _ = take_thumb_requests(MAX_PENDING);
        assert!(!install_thumb_bytes(&key, Vec::new()));
        assert_eq!(thumb_state(&key), ThumbState::Failed);
    }

    #[test]
    fn a_failed_preview_is_not_asked_for_again() {
        let _guard = test_guard();
        drain_all();
        let key = key("failed");
        request_thumb(&key, 7);
        let _ = take_thumb_requests(MAX_PENDING);
        mark_thumb_failed(&key);
        request_thumb(&key, 7);
        assert!(
            take_thumb_requests(MAX_PENDING)
                .iter()
                .all(|taken| taken != &key),
            "a failed revision is not asked for again"
        );
        // Saving the document makes a new revision, and a new preview is fair
        // to ask for.
        request_thumb(&key, 8);
        assert!(take_thumb_requests(MAX_PENDING)
            .iter()
            .any(|taken| taken == &key));
    }

    #[test]
    fn the_image_id_follows_the_document_revision() {
        let _guard = test_guard();
        let key = key("revision");
        assert_ne!(thumb_image_id(&key, 1), thumb_image_id(&key, 2));
        assert_eq!(thumb_image_id(&key, 3), thumb_image_id(&key, 3));
    }
}
