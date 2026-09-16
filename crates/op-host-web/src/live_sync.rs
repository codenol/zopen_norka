//! Live-canvas sync IO for the Rust web shell — the pure `web_sys` plumbing
//! (interval timer, one-shot GET, one-shot POST) the live-sync glue drives to
//! talk to the web-canvas daemon (`op-host-desktop`'s `web_canvas_server`,
//! run via `--serve-web` / `op start --web`).
//!
//! This module is PURE `web_sys` IO (no native Skia/C toolchain and no
//! `op_editor_core`), so it compile-checks on the wasm32 web stub. The
//! highest-risk part of the live-sync glue (the `web_sys` XHR/interval calls)
//! is verified by `cargo check -p op-host-web --target wasm32-unknown-unknown`.
//! The protocol decisions (version gating, push baselines, apply + repaint)
//! live in `op_editor_core::web_sync` + `crate::live_sync_glue`, both
//! host-unit-tested.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Shared network budget for the web shell's XHR fetches (daemon status
/// polls, Iconify search) — one crate-level timeout instead of per-module
/// copies.
pub(crate) const DAEMON_FETCH_TIMEOUT_MS: u32 = 15_000;

thread_local! {
    /// Managed-daemon auth token supplied by the VS Code host over the
    /// postMessage bridge's `Init`. `None` for a direct-open browser tab (no
    /// token → no header). Lives here (always-compiled module) rather than in
    /// the `canvaskit`-gated `vscode_bridge` so [`attach_daemon_headers`] can
    /// read it from every request helper regardless of build config.
    static BRIDGE_TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Store the managed-daemon token (from the bridge `Init`). Idempotent.
pub fn set_bridge_token(token: String) {
    BRIDGE_TOKEN.with(|t| *t.borrow_mut() = Some(token));
}

/// The stored managed-daemon token, if any. `Some` only in a managed webview
/// after the host's `Init` landed.
pub fn bridge_token() -> Option<String> {
    BRIDGE_TOKEN.with(|t| t.borrow().clone())
}

/// The managed-daemon auth token to send for a request targeting `url`, if
/// any. `Some` only when BOTH a bridge token is stored AND `url` targets the
/// daemon (`daemon_base()` prefix) — public requests (e.g. the Iconify CDN)
/// MUST NEVER carry the token, so the prefix check is the leak guard: a URL
/// that is not the daemon origin never yields a token even when one is set.
/// This is the single policy every request helper (XHR-based and `fetch`-based
/// alike) must funnel through so the leak guard can't drift between call sites.
pub fn daemon_token_for(url: &str) -> Option<String> {
    let token = bridge_token()?;
    if url.starts_with(&crate::daemon_base::daemon_base()) {
        Some(token)
    } else {
        None
    }
}

/// Attach the managed-daemon auth header to `req` — but ONLY when `url` targets
/// the daemon (`daemon_base()` prefix). Public requests (e.g. the Iconify CDN)
/// MUST NEVER carry the token, so the prefix check is the leak guard: a URL that
/// is not the daemon origin never receives the header even when a token is set.
/// Call AFTER `open` and BEFORE `send` (request headers require an open XHR).
pub fn attach_daemon_headers(req: &web_sys::XmlHttpRequest, url: &str) {
    if let Some(token) = daemon_token_for(url) {
        let _ = req.set_request_header("X-OpenPencil-Token", &token);
    }
}

/// Run `tick` every `interval_ms` for the page lifetime (the interval owns
/// the closure — same `forget()` idiom the previous document poll used).
pub fn start_interval(interval_ms: i32, tick: Rc<dyn Fn()>) -> Result<(), JsValue> {
    let cb = Closure::<dyn FnMut()>::new(move || tick());
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("live-sync: window unavailable"))?
        .set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            interval_ms,
        )?;
    cb.forget(); // the interval owns the closure for the page lifetime
    Ok(())
}

/// Issue one async `GET` and pass the response body to `on_response` when it
/// completes. Returns `false` when the request could not even start (the
/// callback will then never fire — callers must not park on it). `onloadend`
/// fires on completion regardless of HTTP status, so a non-2xx body still
/// reaches the callback (the protocol parsers reject it there).
pub fn get(url: &str, on_response: Rc<dyn Fn(String)>) -> bool {
    let Ok(xhr) = web_sys::XmlHttpRequest::new() else {
        return false;
    };
    if xhr
        .open_with_async("GET", &crate::daemon_base::with_tenant_param(url), true)
        .is_err()
    {
        return false;
    }
    attach_daemon_headers(&xhr, url);
    let xhr_for_load = xhr.clone();
    // `once_into_js` self-cleans after a single firing (no leaked closure per
    // request, unlike `forget()`).
    let onloadend = Closure::<dyn FnMut()>::once_into_js(move || {
        let text = xhr_for_load
            .response_text()
            .ok()
            .flatten()
            .unwrap_or_default();
        on_response(text);
    });
    xhr.set_onloadend(Some(onloadend.unchecked_ref()));
    xhr.send().is_ok()
}

/// Issue one async `GET` and report both HTTP status and response body.
/// Callers that use a response as an authorization decision must use this
/// variant so an error body cannot be mistaken for a successful response.
pub fn get_with_status(url: &str, on_response: Rc<dyn Fn(u16, String)>) -> bool {
    let Ok(xhr) = web_sys::XmlHttpRequest::new() else {
        return false;
    };
    if xhr
        .open_with_async("GET", &crate::daemon_base::with_tenant_param(url), true)
        .is_err()
    {
        return false;
    }
    attach_daemon_headers(&xhr, url);
    xhr.set_timeout(DAEMON_FETCH_TIMEOUT_MS);
    let xhr_for_load = xhr.clone();
    let onloadend = Closure::<dyn FnMut()>::once_into_js(move || {
        let status = xhr_for_load.status().unwrap_or(0);
        let text = xhr_for_load
            .response_text()
            .ok()
            .flatten()
            .unwrap_or_default();
        on_response(status, text);
    });
    xhr.set_onloadend(Some(onloadend.unchecked_ref()));
    xhr.send().is_ok()
}

/// Issue one async JSON `POST`. `on_response` (when given) receives the
/// response body on completion — including error/empty bodies, so an
/// in-flight latch held by the caller is always released. Returns `false`
/// when the request could not start (the callback will then never fire).
/// Issue one async `DELETE` and hand the response body to the caller.
///
/// The file screen deletes stored documents; the shape mirrors `post_json`, so
/// callers do not have to care which verb the daemon expects.
pub fn delete_json(url: &str, on_response: Option<Rc<dyn Fn(String)>>) -> bool {
    let Ok(xhr) = web_sys::XmlHttpRequest::new() else {
        return false;
    };
    if xhr
        .open_with_async("DELETE", &crate::daemon_base::with_tenant_param(url), true)
        .is_err()
    {
        return false;
    }
    attach_daemon_headers(&xhr, url);
    if let Some(on_response) = on_response {
        let xhr_for_load = xhr.clone();
        let onloadend = Closure::<dyn FnMut()>::once_into_js(move || {
            let text = xhr_for_load
                .response_text()
                .ok()
                .flatten()
                .unwrap_or_default();
            on_response(text);
        });
        xhr.set_onloadend(Some(onloadend.unchecked_ref()));
    }
    xhr.send().is_ok()
}

pub fn post_json(url: &str, body: &str, on_response: Option<Rc<dyn Fn(String)>>) -> bool {
    let Ok(xhr) = web_sys::XmlHttpRequest::new() else {
        return false;
    };
    if xhr
        .open_with_async("POST", &crate::daemon_base::with_tenant_param(url), true)
        .is_err()
    {
        return false;
    }
    attach_daemon_headers(&xhr, url);
    let _ = xhr.set_request_header("Content-Type", "application/json");
    if let Some(on_response) = on_response {
        let xhr_for_load = xhr.clone();
        let onloadend = Closure::<dyn FnMut()>::once_into_js(move || {
            let text = xhr_for_load
                .response_text()
                .ok()
                .flatten()
                .unwrap_or_default();
            on_response(text);
        });
        xhr.set_onloadend(Some(onloadend.unchecked_ref()));
    }
    xhr.send_with_opt_str(Some(body)).is_ok()
}

/// Issue one async JSON `POST` and report both HTTP status and body. This is
/// used when a caller must distinguish an acknowledged write from a completed
/// but rejected request.
pub fn post_json_with_status(url: &str, body: &str, on_response: Rc<dyn Fn(u16, String)>) -> bool {
    post_json_with_status_and_retry(
        url,
        body,
        Rc::new(move |status, body, _retry_after_secs| on_response(status, body)),
    )
}

/// Issue one async JSON `POST` and report the status, the body, AND the wait the
/// answer asks for.
///
/// The third value is the response's `Retry-After`, in seconds, and this variant
/// exists because exactly one answer in this product carries one: the daemon's
/// throttled sign-in (`429 too-many-attempts`), which deliberately keeps the
/// wait out of its body so that the body cannot become an oracle for whether an
/// account exists (issue #77) — and which therefore cannot be acted on by a
/// caller that reads only the body. Issue #151: the shell read only the status
/// and the body, so a locked-out person was told the sign-in was unavailable.
///
/// [`parse_retry_after_secs`] decides what counts as a usable header. `None`
/// means the caller is told the answer asked for a wait without being told how
/// long — never that there was no such answer.
pub fn post_json_with_status_and_retry(
    url: &str,
    body: &str,
    on_response: Rc<dyn Fn(u16, String, Option<u64>)>,
) -> bool {
    let Ok(xhr) = web_sys::XmlHttpRequest::new() else {
        return false;
    };
    if xhr
        .open_with_async("POST", &crate::daemon_base::with_tenant_param(url), true)
        .is_err()
    {
        return false;
    }
    attach_daemon_headers(&xhr, url);
    xhr.set_timeout(DAEMON_FETCH_TIMEOUT_MS);
    let _ = xhr.set_request_header("Content-Type", "application/json");
    let xhr_for_load = xhr.clone();
    let onloadend = Closure::<dyn FnMut()>::once_into_js(move || {
        let status = xhr_for_load.status().unwrap_or(0);
        let text = xhr_for_load
            .response_text()
            .ok()
            .flatten()
            .unwrap_or_default();
        // Read here rather than by the caller: after `onloadend` returns, the
        // XHR object is the only thing that still knows the header, and a
        // cross-origin read depends on the response exposing it
        // (`Access-Control-Expose-Headers`, applied daemon-side).
        let retry_after = xhr_for_load
            .get_response_header("Retry-After")
            .ok()
            .flatten();
        on_response(status, text, parse_retry_after_secs(retry_after.as_deref()));
    });
    xhr.set_onloadend(Some(onloadend.unchecked_ref()));
    xhr.send_with_opt_str(Some(body)).is_ok()
}

/// The wait a `Retry-After` header states, in seconds.
///
/// Only the delta-seconds form is honoured. The HTTP-date form is legal and an
/// intermediary may send it, but turning a date into a wait needs a clock the
/// two ends can disagree about, and a wrong number is worse than none here: the
/// caller then says "too many attempts" with no figure instead of naming a wait
/// that has already passed. Anything else — absent, empty, a date, a negative
/// number — is `None` for the same reason.
pub fn parse_retry_after_secs(raw: Option<&str>) -> Option<u64> {
    raw?.trim().parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_seconds_are_read_from_the_daemon_s_own_header() {
        // The daemon sends `max(1)` seconds — see `AccountReply::throttled`.
        assert_eq!(parse_retry_after_secs(Some("900")), Some(900));
        assert_eq!(parse_retry_after_secs(Some(" 60 ")), Some(60));
        assert_eq!(parse_retry_after_secs(Some("1")), Some(1));
    }

    #[test]
    fn an_unreadable_retry_after_is_not_a_number_and_not_a_zero() {
        for raw in [
            None,
            Some(""),
            Some("   "),
            Some("Wed, 21 Oct 2026 07:28:00 GMT"),
            Some("-30"),
            Some("soon"),
        ] {
            assert_eq!(
                parse_retry_after_secs(raw),
                None,
                "`{raw:?}` is not a wait this shell can state"
            );
        }
    }
}
