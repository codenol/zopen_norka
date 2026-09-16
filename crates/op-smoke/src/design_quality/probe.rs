//! The daemon side of the measurement: four calls per prompt, no model SDK.
//!
//! Exactly the four calls both hand passes made (`harness.py:1-18`), in the
//! same order, so a number produced here is comparable with a number produced
//! then:
//!
//! 1. `POST /api/file/new` — a fresh document (untitled starter + the kit).
//! 2. `GET  /api/mcp/document` — the before-state and its version.
//! 3. `POST /api/ai/standard` — the browser's own body, streamed as SSE. The
//!    stream is drained to EOF and kept verbatim on disk for review.
//! 4. `GET  /api/mcp/document` — what actually landed.
//!
//! `GET /api/mcp/document` already answers the document's `version`, so no
//! separate `/api/mcp/version` probe is made; the two samples it returns are
//! what "the turn moved the document" is decided on.
//!
//! The base URL is always supplied by the caller (`--url`); nothing here has a
//! default port, because a hard-coded port measures whatever happens to be
//! listening on it.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::corpus::{Routes, TurnBody};

/// How many times a read-back is retried before the prompt is reported failed.
const READ_ATTEMPTS: usize = 3;

/// What one `/api/mcp/document` read returned.
#[derive(Debug, Clone)]
pub(crate) struct DocumentRead {
    pub(crate) version: Option<u64>,
    pub(crate) payload: Value,
}

/// One drained `/api/ai/standard` response.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnStream {
    /// Raw SSE body, verbatim.
    pub(crate) raw: String,
    pub(crate) seconds: f64,
    pub(crate) thinking: String,
    pub(crate) delta: String,
    pub(crate) events: usize,
    /// `done`, `done-marker`, `error`, or `None` when the stream ended without
    /// a terminal event (a known defect — issue #203 — so it is reported, never
    /// treated as a failed measurement).
    pub(crate) terminal: Option<String>,
    pub(crate) errors: Vec<String>,
}

/// HTTP client for one daemon.
pub(crate) struct DaemonProbe {
    client: reqwest::Client,
    base: String,
    routes: Routes,
    timeout: Duration,
}

impl DaemonProbe {
    /// Builds a probe for `base`. `Err` carries the message to print.
    pub(crate) fn new(base: &str, routes: Routes, timeout: Duration) -> Result<Self, String> {
        let trimmed = base.trim().trim_end_matches('/');
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err(format!(
                "[QUALITY] --url {base:?} is not an http(s) URL, e.g. --url http://127.0.0.1:3199"
            ));
        }
        // The workspace pins its HTTP clients to rustls rather than inheriting
        // whatever TLS the platform SDK links (`tools/check-rustls-client-policy.sh`).
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(|e| format!("[QUALITY] http client: {e}"))?;
        Ok(Self {
            client,
            base: trimmed.to_string(),
            routes,
            timeout,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// `POST /api/file/new` — reset the daemon to a fresh document.
    pub(crate) async fn new_document(&self) -> Result<(), String> {
        let response = self
            .client
            .post(self.url(&self.routes.new_document))
            .header("Content-Type", "application/json")
            .body("{}")
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| {
                format!(
                    "[QUALITY] POST {}: {e}",
                    self.url(&self.routes.new_document)
                )
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "[QUALITY] POST {} -> {status}: {}",
                self.url(&self.routes.new_document),
                snippet(&body)
            ));
        }
        // The harness slept here so the reset is visible to the next read; the
        // daemon commits the new document before answering, but a read that
        // races it would silently measure the previous prompt's page.
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    /// `GET /api/mcp/document` — the document and its version.
    pub(crate) async fn read_document(&self) -> Result<DocumentRead, String> {
        let url = self.url(&self.routes.document);
        let mut last = String::new();
        for attempt in 1..=READ_ATTEMPTS {
            match self.client.get(&url).timeout(self.timeout).send().await {
                Ok(response) if response.status().is_success() => match response.text().await {
                    Ok(text) => match serde_json::from_str::<Value>(&text) {
                        Ok(payload) if payload.get("document").is_some() => {
                            return Ok(DocumentRead {
                                version: payload.get("version").and_then(Value::as_u64),
                                payload,
                            })
                        }
                        Ok(_) => last = "payload carries no `document`".to_string(),
                        Err(e) => last = format!("unparsable JSON: {e}"),
                    },
                    Err(e) => last = format!("body: {e}"),
                },
                Ok(response) => last = format!("status {}", response.status()),
                Err(e) => last = format!("{e}"),
            }
            if attempt < READ_ATTEMPTS {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
        Err(format!(
            "[QUALITY] GET {url} failed {READ_ATTEMPTS} time(s): {last}"
        ))
    }

    /// `POST /api/ai/standard` with the browser's own body, drained to EOF.
    pub(crate) async fn stream_turn(&self, body: &TurnBody) -> Result<TurnStream, String> {
        let url = self.url(&self.routes.turn);
        let started = Instant::now();
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(body)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| format!("[QUALITY] POST {url}: {e}"))?;
        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| format!("[QUALITY] POST {url}: reading the stream: {e}"))?;
        let seconds = started.elapsed().as_secs_f64();
        if !status.is_success() {
            return Err(format!(
                "[QUALITY] POST {url} -> {status}: {}",
                snippet(&raw)
            ));
        }
        let mut stream = parse_sse(&raw);
        stream.raw = raw;
        stream.seconds = seconds;
        Ok(stream)
    }

    /// Waits out the daemon's post-turn document commit before the read-back.
    pub(crate) async fn settle(&self) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Parses an SSE body into the pieces the scorecard reports. Pure.
pub(crate) fn parse_sse(raw: &str) -> TurnStream {
    let mut out = TurnStream::default();
    let mut thinking = Vec::new();
    let mut delta = Vec::new();
    for line in raw.lines() {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() {
            continue;
        }
        out.events += 1;
        if payload == "[DONE]" {
            out.terminal = Some("done-marker".to_string());
            continue;
        }
        let Ok(object) = serde_json::from_str::<Value>(payload) else {
            out.errors.push(format!("UNPARSED: {}", snippet(payload)));
            continue;
        };
        if let Some(text) = object.get("thinking").and_then(Value::as_str) {
            thinking.push(text.to_string());
        }
        if let Some(text) = object.get("delta").and_then(Value::as_str) {
            delta.push(text.to_string());
        }
        if let Some(error) = object.get("error") {
            out.errors.push(snippet(error.as_str().unwrap_or_default()));
            out.terminal = Some("error".to_string());
        }
        if object.get("done").and_then(Value::as_bool) == Some(true) {
            out.terminal = Some("done".to_string());
        }
    }
    out.thinking = thinking.concat();
    out.delta = delta.concat();
    out
}

/// First line of a response body, capped — for an error message, not a report.
fn snippet(body: &str) -> String {
    let first = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    first.chars().take(300).collect()
}
