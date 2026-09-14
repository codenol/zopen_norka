//! The attachment half of the `op-ai` chat wire.
//!
//! Split out of `chat_provider.rs` (pure code motion + the
//! `AttachmentTransport` declaration that arrived with issue #61) so the
//! spine stays under the 800-line cap. `chat_provider` re-exports both items,
//! so every existing `op_ai::chat_provider::{ChatAttachment,
//! AttachmentTransport}` path keeps working.
//!
//! Why the transport declaration lives beside the attachment type: the two
//! answer one question together — "can the model actually see what the user
//! attached?" — and a caller that needs real pixels must ask it instead of
//! assuming `ChatProvider::send` carried them.

/// A file the user attached to a chat turn — typically a pasted or
/// picked image. Mirrors TS `ChatAttachment` (`apps/web/.../ai`),
/// minus the UI-only `id` / `size` fields. `data` is the raw decoded
/// bytes; providers base64-encode (Claude image blocks) or spill to a
/// temp file (CLI subprocesses) as their wire format demands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAttachment {
    /// Original file name, e.g. `screenshot.png`.
    pub name: String,
    /// MIME type, e.g. `image/png`.
    pub media_type: String,
    /// Raw file bytes (not base64).
    pub data: Vec<u8>,
}

impl ChatAttachment {
    /// True when this attachment is an image — the only kind every
    /// provider can ingest (as an image content block).
    pub fn is_image(&self) -> bool {
        self.media_type.starts_with("image/")
    }
}

/// How a transport conveys [`super::ChatRequest::attachments`] to the model.
///
/// The distinction is load-bearing for every vision caller. A screenshot
/// handed to a transport that silently drops it does not fail: the model
/// answers about the file *name* it can see, confidently and at length. That
/// is how an invented screenshot inventory reached the design planner
/// (issue #61), so a caller that needs real pixels must ask this question
/// instead of assuming `send` carried them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentTransport {
    /// Attachment bytes ride the request body as inline image content —
    /// Anthropic `{"type":"image","source":{…base64…}}` blocks, OpenAI
    /// `{"type":"image_url","image_url":{"url":"data:…"}}` parts.
    InlineImage,
    /// The model is handed a filesystem path and reads the file with its own
    /// tooling (Claude Code's guided Read flow, the Copilot SDK's `File`
    /// attachment, an ACP / CLI coding agent with a file-read tool). Delivery
    /// is real; the model still has to take the reading step.
    ReadablePath,
    /// Attachments never reach the model. This is the DEFAULT on purpose: a
    /// transport that has not declared how it carries attachments must not be
    /// trusted with one, because the failure mode of guessing wrong is a
    /// fabricated description rather than an error.
    Dropped,
}

impl AttachmentTransport {
    /// True when the model can actually consume `super::ChatRequest::attachments`
    /// through this transport.
    ///
    /// [`Self::ReadablePath`] counts as delivered: the model is given the
    /// bytes' location and its own tools, which is a real route to the pixels
    /// (unlike a [`Self::Dropped`] transport, where the path names a file the
    /// model has no way to open).
    pub fn delivers_attachments(self) -> bool {
        matches!(
            self,
            AttachmentTransport::InlineImage | AttachmentTransport::ReadablePath
        )
    }
}
