//! Fetch document previews for the file screen.
//!
//! The widget layer asks for a preview by key (see
//! `op_editor_ui::files_thumb_runtime`); this drains that queue, reads the
//! daemon's answer and installs the bytes. The route answers a base64 JSON
//! envelope rather than a body — the daemon's reply type carries text, and the
//! export routes set the same precedent — so the unwrapping belongs here, in
//! the host, and never in paint.
//!
//! Modelled on `collab_avatar_fetch`: the same three-verb contract (install on
//! success, mark failed otherwise) so a preview that cannot be fetched stops
//! being asked for instead of being retried every frame.

use std::rc::Rc;

use op_editor_ui::files_thumb_runtime;

use base64::Engine as _;

/// How many previews one frame may start.
///
/// The screen shows a grid, not a wall: a handful per frame fills it quickly
/// without opening thirty sockets at once.
const MAX_IN_FLIGHT_PER_DRAIN: usize = 6;

/// Why a preview could not be installed.
///
/// A preview is decoration, so every variant means the same thing to the user
/// — the card keeps its placeholder — but naming them keeps a transport
/// failure distinguishable from a document that simply has no preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ThumbFetchError {
    /// The request never left (no XHR, bad URL).
    RequestFailed,
    /// The daemon answered with a non-200 status.
    Http(u16),
    /// The body was not the JSON envelope.
    Malformed,
    /// The envelope said `ok: false`.
    Refused,
    /// The envelope had no usable `dataBase64`.
    MissingData,
    /// The payload was not valid base64.
    Base64,
    /// The payload decoded to nothing.
    Empty,
}

impl std::fmt::Display for ThumbFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestFailed => write!(f, "the preview request could not start"),
            Self::Http(status) => write!(f, "the preview request answered {status}"),
            Self::Malformed => write!(f, "the preview response was not readable"),
            Self::Refused => write!(f, "the server refused the preview"),
            Self::MissingData => write!(f, "the preview response carried no image"),
            Self::Base64 => write!(f, "the preview payload was not decodable"),
            Self::Empty => write!(f, "the preview was empty"),
        }
    }
}

/// Start fetching previews the widget layer has asked for.
pub(crate) fn drain_pending() {
    for key in files_thumb_runtime::take_thumb_requests(MAX_IN_FLIGHT_PER_DRAIN) {
        fetch_thumb(key);
    }
}

fn fetch_thumb(key: String) {
    let url = crate::daemon_base::daemon_url(&format!("/api/files/{key}/thumb"));
    let key_for_response = key.clone();
    let on_response: Rc<dyn Fn(u16, String)> = Rc::new(move |status, body| {
        match decode_thumb(status, &body) {
            Ok(bytes) => {
                files_thumb_runtime::install_thumb_bytes(&key_for_response, bytes);
                // A response is not an input event: without this the installed
                // bytes would wait for the next click to be painted.
                crate::repaint_coalescer::request();
            }
            Err(_) => files_thumb_runtime::mark_thumb_failed(&key_for_response),
        }
    });
    if !crate::live_sync::get_with_status(&url, on_response) {
        files_thumb_runtime::mark_thumb_failed(&key);
    }
}

/// Read the daemon's envelope into image bytes. Pure, so it is testable
/// without a browser.
fn decode_thumb(status: u16, body: &str) -> Result<Vec<u8>, ThumbFetchError> {
    if status != 200 {
        return Err(ThumbFetchError::Http(status));
    }
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ThumbFetchError::Malformed)?;
    if parsed.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        return Err(ThumbFetchError::Refused);
    }
    let encoded = parsed
        .get("dataBase64")
        .and_then(|data| data.as_str())
        .ok_or(ThumbFetchError::MissingData)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| ThumbFetchError::Base64)?;
    if bytes.is_empty() {
        return Err(ThumbFetchError::Empty);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_envelope_yields_the_image_bytes() {
        let png = vec![0x89, b'P', b'N', b'G'];
        let body = serde_json::json!({
            "ok": true,
            "mime": "image/png",
            "dataBase64": base64::engine::general_purpose::STANDARD.encode(&png),
        })
        .to_string();
        assert_eq!(decode_thumb(200, &body), Ok(png));
    }

    #[test]
    fn each_failure_is_named() {
        assert_eq!(decode_thumb(404, "{}"), Err(ThumbFetchError::Http(404)));
        assert_eq!(
            decode_thumb(200, "not json"),
            Err(ThumbFetchError::Malformed)
        );
        assert_eq!(
            decode_thumb(200, r#"{"ok":false}"#),
            Err(ThumbFetchError::Refused)
        );
        assert_eq!(
            decode_thumb(200, r#"{"ok":true}"#),
            Err(ThumbFetchError::MissingData)
        );
        assert_eq!(
            decode_thumb(200, r#"{"ok":true,"dataBase64":"!!!"}"#),
            Err(ThumbFetchError::Base64)
        );
        assert_eq!(
            decode_thumb(200, r#"{"ok":true,"dataBase64":""}"#),
            Err(ThumbFetchError::Empty)
        );
    }
}
