//! Restoring a retried turn's reference pictures (issue #95).
//!
//! Split out of `design_session.rs` at the 800-line cap; the launch path calls
//! [`restore_turn_reference_attachments`] right after it decodes the stashed
//! request.

use op_host_native::WidgetHostNative;

/// Restore a retried request's reference pictures from the user message that
/// owns its turn.
///
/// `DesignRequest::reference_attachments` is `#[serde(skip)]`, so the JSON stash
/// the "Retry" button re-runs from cannot carry it. That is a deliberate
/// property of the field — it is in-process only, and serializing image bytes
/// into the chat stash would copy them once per worker bubble — not something to
/// fix by making the field travel. The transcript already holds the turn's
/// pictures once, on its user bubble (`ChatState::begin_send` copies the staged
/// images there), so the retry reads them from there instead of re-running blind
/// and regenerating a section that matches nothing around it.
///
/// A request that already carries attachments keeps them: this only fills a
/// hole.
pub(super) fn restore_turn_reference_attachments(
    host: &WidgetHostNative,
    msg_idx: usize,
    request: &mut op_orchestrator::DesignRequest,
) {
    if !request.reference_attachments.is_empty() {
        return;
    }
    let restored: Vec<op_orchestrator::ReferenceAttachment> = host
        .editor_state()
        .chat
        .owning_turn_images(msg_idx)
        .iter()
        .map(|image| op_orchestrator::ReferenceAttachment {
            name: image.name.clone(),
            media_type: image.media_type.clone(),
            data: image.data.clone(),
        })
        .collect();
    if !restored.is_empty() {
        request.reference_attachments = restored;
    }
}
