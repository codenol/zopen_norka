//! Daemon-side proxy for verified collaboration participant avatars.
//!
//! A relay session's roster carries profile image URLs that may be signed CDN
//! links. They are registered into the process-local avatar registry by
//! `op-collab-host`'s roster projection and must never reach the browser, so
//! the wasm shell asks for one participant's bytes by the opaque
//! `participantKey` it already holds and the daemon does the fetching with its
//! public-only HTTPS client.
//!
//! The desktop pumps the same registry from its frame loop
//! (`op-host-desktop/src/collab_avatar_host.rs`, worker threads + `pump`); the
//! daemon has no frame loop, so the drain happens here — on the connection
//! thread, never while the editor-state mutex is held.

use std::fmt;

use op_editor_ui::collab_avatar_runtime::{
    collab_avatar_image, complete_collab_avatar_request, take_collab_avatar_requests,
    CollabAvatarFetchRequest,
};

use crate::web_canvas_server::WebReply;

/// Longest body the route accepts: one short opaque key and nothing else.
const MAX_BODY_BYTES: usize = 1024;
/// Upper bound on participant-key length, mirroring the registry's own cap.
const MAX_PARTICIPANT_KEY_BYTES: usize = 256;
/// Roster URLs fetched per request before the answer is assembled.
///
/// Each fetch is bounded by `profile_avatar_fetch`'s own 5 s timeout, and the
/// queue only ever holds one entry per participant, so this caps how long one
/// request can occupy its connection thread.
const MAX_FETCHES_PER_REQUEST: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollabAvatarProxyError {
    BodyTooLarge,
    MalformedRequest,
    KeyNotAllowed,
    NotAvailable,
}

impl fmt::Display for CollabAvatarProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BodyTooLarge => "body too large",
            Self::MalformedRequest => "malformed avatar request",
            Self::KeyNotAllowed => "participant key is not allowed",
            Self::NotAvailable => "no avatar is available for this participant",
        })
    }
}

impl std::error::Error for CollabAvatarProxyError {}

impl CollabAvatarProxyError {
    const fn status(self) -> &'static str {
        match self {
            Self::BodyTooLarge => "413 Payload Too Large",
            Self::MalformedRequest | Self::KeyNotAllowed => "400 Bad Request",
            Self::NotAvailable => "404 Not Found",
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::BodyTooLarge => "payload-too-large",
            Self::MalformedRequest => "malformed-avatar-request",
            Self::KeyNotAllowed => "invalid-participant-key",
            Self::NotAvailable => "avatar-unavailable",
        }
    }
}

/// One participant's proxied image, ready to serialize.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AvatarPayload {
    participant_key: String,
    /// Opaque, process-local identity for these bytes. It changes exactly when
    /// the source profile URL changes, which is what a client cache needs —
    /// and it reveals nothing about the URL itself.
    revision: String,
    encoded: Vec<u8>,
}

/// `POST /api/collab/avatar` — proxy one roster participant's profile image.
///
/// Runs on the connection thread (bounded public HTTPS I/O), never under the
/// editor-state mutex.
pub(crate) fn avatar(body: &str) -> WebReply {
    match resolve_avatar(body, fetch_for_request) {
        Ok(payload) => {
            use base64::Engine as _;
            WebReply {
                status: "200 OK",
                body: serde_json::json!({
                    "participantKey": payload.participant_key,
                    "revision": payload.revision,
                    "encoded": base64::engine::general_purpose::STANDARD.encode(payload.encoded),
                })
                .to_string(),
            }
        }
        Err(error) => WebReply {
            status: error.status(),
            body: serde_json::json!({
                "ok": false,
                "error": error.code(),
                "message": error.to_string(),
            })
            .to_string(),
        },
    }
}

/// Host fetch policy, mirroring the desktop worker: a remote roster URL always
/// takes the public-only client. Only the locally authenticated account may
/// traverse a fake-IP TUN, and remote participants live in a namespace that
/// cannot claim to be the account slot.
fn fetch_for_request(request: &CollabAvatarFetchRequest) -> Option<Vec<u8>> {
    if request.is_current_account() {
        crate::profile_avatar_fetch::fetch_account_avatar_blocking(request.url()).ok()
    } else {
        crate::profile_avatar_fetch::fetch_profile_avatar_blocking(request.url()).ok()
    }
}

fn resolve_avatar(
    body: &str,
    fetch: impl Fn(&CollabAvatarFetchRequest) -> Option<Vec<u8>>,
) -> Result<AvatarPayload, CollabAvatarProxyError> {
    let key = parse_participant_key(body)?;
    // Fast path: this participant's bytes already landed in the registry.
    if let Some(image) = collab_avatar_image(&key) {
        return Ok(payload(key, &image));
    }
    // Otherwise drain the registry's bounded pending queue. Every request that
    // is taken is also completed — including with `None` on failure — so a
    // queued participant is never silently dropped just because a different
    // participant was asked for.
    for pending in take_collab_avatar_requests(MAX_FETCHES_PER_REQUEST) {
        let bytes = fetch(&pending);
        let _ = complete_collab_avatar_request(&pending, bytes);
    }
    collab_avatar_image(&key)
        .map(|image| payload(key, &image))
        .ok_or(CollabAvatarProxyError::NotAvailable)
}

fn payload(
    participant_key: String,
    image: &op_editor_ui::collab_avatar_runtime::CollabAvatarImage,
) -> AvatarPayload {
    AvatarPayload {
        participant_key,
        revision: format!("{:016x}", image.image_id),
        encoded: image.encoded.to_vec(),
    }
}

fn parse_participant_key(body: &str) -> Result<String, CollabAvatarProxyError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(CollabAvatarProxyError::BodyTooLarge);
    }
    let request: AvatarRequestBody =
        serde_json::from_str(body).map_err(|_| CollabAvatarProxyError::MalformedRequest)?;
    let key = request.participant_key;
    if key.is_empty() || key.len() > MAX_PARTICIPANT_KEY_BYTES || key.chars().any(char::is_control)
    {
        return Err(CollabAvatarProxyError::KeyNotAllowed);
    }
    Ok(key)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvatarRequestBody {
    participant_key: String,
}

#[cfg(test)]
#[path = "collab_avatar_proxy_tests.rs"]
mod tests;
