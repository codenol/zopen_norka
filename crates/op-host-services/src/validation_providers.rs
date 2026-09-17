//! Real host-side providers for the orchestrator's Class-C vision
//! validation loop (`op_orchestrator::run_post_generation_validation`).
//!
//! These replace the `SkippedScreenshotProvider` / `SkippedVisionLlmClient`
//! stubs with implementations backed by the live render pipeline and a
//! real multimodal chat provider — but they are only injected when the
//! per-request `validation_enabled` flag is ON (see
//! [`design_session`](crate::design_session) +
//! [`web_chat_standard`](crate::web_chat_standard)). With the flag OFF
//! (the default) the stubs are used exactly as before, so the default
//! generation path is byte-for-byte unchanged.
//!
//! ## [`RealScreenshotProvider`]
//! Derives a fresh layout-resolved scene from the live (post-mutation)
//! `EditorState` via `op_pen_loader::editor_state_to_layout_scene`, then
//! renders the active page to a base64 PNG through the SAME raster
//! pipeline file exports + `debug_screenshot` use
//! (`export::screenshot::capture_scene`). No GL — the headless backend
//! links only raster skia. This is the Rust analog of the TS
//! `SkiaEngine.captureRegion` live readback.
//!
//! ## [`ChatVisionLlmClient`]
//! Wraps any `op_ai::chat_provider::ChatProvider` (the same trait the
//! chat panel + planning/sub-agent `ChatProviderLlmClient` ride) and
//! issues a single multimodal turn: the critique prompt as text + the
//! screenshot PNG as a `ChatAttachment` image. Builtin / HTTP providers
//! map the attachment onto their wire's inline image block (Anthropic
//! `type:"image"` with a base64 `source`, OpenAI `image_url` with a
//! `data:` URL — see `chat_builtin_http_wire::{anthropic,openai}_user_content`);
//! the blocking delta iterator is drained to a single `String` and returned
//! as `VisionResponse::Text` for the orchestrator's
//! `parse_validation_response` to consume. Reuses the existing request/SSE
//! plumbing — no hand-rolled HTTP client.
//!
//! Before it sends anything, the client asks the transport how it conveys
//! attachments ([`ChatProvider::attachment_transport`]). A transport that
//! drops them gets no call at all and the caller sees `Skipped`: a text-only
//! "vision" call answers about the attachment's file name as if it had seen
//! the picture, and every downstream consumer treats that answer as grounded
//! (issue #61 — an invented screenshot inventory reached the design planner
//! this way). `Skipped` is the honest outcome, and it is what makes the
//! conservative fallback brief engage.

use std::sync::Arc;

use op_ai::chat_provider::{
    ChatAttachment, ChatDelta, ChatProvider, ChatRequest, EffortLevel, ThinkingMode,
};
use op_editor_core::EditorState;
use op_orchestrator::{
    ScreenshotProvider, VisionCallRequest, VisionImage, VisionLlmClient, VisionResponse, VisionRole,
};

use crate::export::screenshot::{capture_scene, CaptureSpec};

/// Env flag that selects the REAL vision-validation providers over the no-op
/// stubs at the host run sites. Defaults ON (issue #62): the comparison
/// compares something now, and a reference is only worth attaching if the
/// result is checked against it.
///
/// `OPENPENCIL_VISION_VALIDATION=0` turns it off, which is the switch for the
/// cost rather than for the behaviour — see [`vision_validation_enabled`].
///
/// This is a SEPARATE switch from `DesignRequest.validation_enabled`
/// (which only gates whether the loop runs at all, with whatever providers
/// the host injected). Flipping `validation_enabled` alone keeps the stubs,
/// so it stays a no-op; the real loop only activates when BOTH are on. The
/// flag default is NOT changed — see `default_validation_enabled` in
/// `op_orchestrator::types`.
///
/// Read this as a COST switch, not a correctness one. It used to hide a
/// defect as well — the loop asked the model to compare the design against a
/// reference screenshot it never sent (issue #62) — so leaving it off kept a
/// lie off the wire. That comparison is real now (both pictures travel, and a
/// client that cannot deliver one returns `Skipped` instead of guessing), so
/// what the flag buys today is "no extra paid vision call and no
/// model-authored edit on an ordinary turn".
const VISION_VALIDATION_ENV: &str = "OPENPENCIL_VISION_VALIDATION";

/// Whether the host should inject the REAL vision providers. Defaults `true`.
///
/// The flag used to default off because the loop asked the model to compare the
/// design against a reference screenshot it never sent (issue #62), and leaving
/// it off kept that lie off the wire. The comparison is real now — both pictures
/// travel and a client that cannot deliver one returns `Skipped` rather than
/// guessing — so the default is on: a reference is only worth attaching if the
/// result is checked against it.
///
/// `OPENPENCIL_VISION_VALIDATION=0` (or `false` / `no` / `off`) turns it off.
/// That is a COST switch: every validated turn spends an extra vision call and
/// may take up to `MAX_VALIDATION_ROUNDS` model-authored edits to the document.
pub fn vision_validation_enabled() -> bool {
    vision_validation_requested(std::env::var(VISION_VALIDATION_ENV).ok().as_deref())
}

/// The decision behind [`vision_validation_enabled`], with the environment
/// passed in — so the default can be tested without a process-global variable
/// that another test could race.
pub fn vision_validation_requested(value: Option<&str>) -> bool {
    match value {
        Some(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        None => true,
    }
}

/// Resolve the vision LLM's system prompt from the `validation` phase of
/// `op-ai-skills` (mirrors the TS `resolveSkills('validation', '')` seam
/// the `ValidationProviders.system_prompt` field documents). Concatenates
/// the matched validation-phase skill bodies; falls back to a terse
/// built-in rubric if the phase resolves empty so the vision call always
/// has a usable critique instruction.
pub fn validation_system_prompt() -> String {
    let options = op_ai_skills::ResolveOptions::default();
    let ctx = op_ai_skills::resolve_skills(op_ai_skills::Phase::Validation, "", &options);
    let knowledge = ctx
        .skills
        .iter()
        .map(|s| s.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    if knowledge.trim().is_empty() {
        DEFAULT_VALIDATION_SYSTEM_PROMPT.to_string()
    } else {
        knowledge
    }
}

/// Terse built-in critique rubric used when the `validation` phase
/// resolves no skill content. Constrains the model to the JSON verdict
/// shape `parse_validation_response` consumes (issues / fixes /
/// structuralFixes / qualityScore).
const DEFAULT_VALIDATION_SYSTEM_PROMPT: &str = "You are a UI design validator. \
You are given a screenshot of a generated design and its node-tree structure. \
Identify concrete visual issues (overlap, clipping, low contrast, broken spacing, \
misaligned or missing elements) and return ONLY a JSON object of the shape \
{\"issues\":[string],\"fixes\":[{\"nodeId\":string,\"property\":string,\"value\":any}],\
\"structuralFixes\":[{\"action\":\"removeNode\"|\"addChild\",...}],\"qualityScore\":number} \
where qualityScore is 1-10. Use the real node IDs from the provided tree. \
Treat layout=horizontal clip=true as an intentional horizontal scroller; preserve its \
fixed item widths and partial right-edge item instead of changing layout geometry. \
Do not add borders to chart bars, bar tracks, columns, or other data marks. \
Review images for rendering integrity only: visible single-image slot, bounds, crop/fit, \
clipping, radius, and overlay order. Never judge or replace image content, relevance, \
aesthetics, perceived quality, resolution, tone, stock choice, search, or generation quality. \
Return an empty fixes array and a high qualityScore when the design looks good.";

/// Real `ScreenshotProvider` — renders the live document's active page
/// to a base64 PNG via the headless raster pipeline.
///
/// The validation loop calls `capture_root_frame(sink.state())` AFTER
/// each round's fixes are applied, so the captured pixels reflect the
/// current mutated document (not a stale pre-loop snapshot).
pub struct RealScreenshotProvider;

impl ScreenshotProvider for RealScreenshotProvider {
    fn capture_root_frame(&self, state: &EditorState) -> Option<String> {
        // Same derive the canvas painter + debug_screenshot use, so the
        // pixels match what the user sees at zoom 1.
        let scene = op_pen_loader::editor_state_to_active_page_layout_scene(state);
        let spec = CaptureSpec {
            // `None` = whole active-page content (the "root frame" the
            // validation loop wants).
            node_id: None,
            padding: 0.0,
            scale: 1.0,
        };
        // A render failure (empty page, oversized bounds) downgrades to
        // `None` — the loop then skips the vision round, exactly like the
        // stub. Capture errors must never abort the design turn.
        capture_scene(&scene, &spec)
            .ok()
            .map(|shot| shot.png_base64)
    }
}

/// Real `VisionLlmClient` — issues a single multimodal turn through a
/// `ChatProvider`, sending the screenshot as an image attachment + the
/// critique prompt as text, and returns the model's reply text.
///
/// `Arc<dyn ChatProvider>` so the host can share one provider instance
/// (the same one driving the design turn's `ChatProviderLlmClient`).
pub struct ChatVisionLlmClient {
    provider: Arc<dyn ChatProvider>,
    /// Vision-capable model id (forwarded into the `ChatRequest`); `None`
    /// keeps the provider's own default.
    model: Option<String>,
}

impl ChatVisionLlmClient {
    pub fn new(provider: Arc<dyn ChatProvider>) -> Self {
        Self {
            provider,
            model: None,
        }
    }

    /// Attach the vision model id to every request this client issues.
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = Self::transport_model(model);
        self
    }

    /// Decode one picture's base64 into a `ChatAttachment`, labelled with the
    /// role it plays in the prompt.
    ///
    /// `None` when the payload is not a raster image at all. The bytes decide
    /// the media type, not the caller: `VisionImage` carries no type field, and
    /// labelling every payload `image/png` (as this used to) makes a JPEG
    /// reference 400 the whole call on the Anthropic wire, which validates the
    /// base64 against its declared `media_type`.
    fn image_attachment(image: &VisionImage) -> Option<ChatAttachment> {
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD
            .decode(&image.base64)
            .ok()?;
        let media_type = crate::chat_attachment::sniff_image_media_type(&data)?;
        // The role names the file, and the prompt counts pictures by position:
        // a path transport (Claude Code's Read flow, the Copilot SDK) spills
        // these names to disk and the model reads them back, so "image 1 is the
        // design, image 2 is the reference" has to survive in the name too.
        let stem = match image.role {
            VisionRole::Design => "design-screenshot",
            VisionRole::Reference => "reference-design",
        };
        Some(ChatAttachment {
            // The extension has to match the bytes: a path transport spills
            // this name to a temp file and the model's Read step (or its SDK
            // file attachment) infers the type from it.
            name: format!("{stem}.{}", media_type.trim_start_matches("image/")),
            media_type: media_type.to_string(),
            data,
        })
    }

    /// Every picture the prompt names, or `None` naming the role that cannot be
    /// delivered.
    ///
    /// All-or-nothing on purpose: the prompt says "image 1 is the design,
    /// image 2 is the reference", so half a picture list turns the comparison
    /// it asks for into a question about a picture the model never got (issue
    /// #62). A caller that cannot deliver the reference gets `Skipped`, which
    /// is the honest answer.
    fn attachments_for(req: &VisionCallRequest) -> Result<Vec<ChatAttachment>, VisionRole> {
        let mut attachments = Vec::with_capacity(req.images.len());
        for image in &req.images {
            match Self::image_attachment(image) {
                Some(attachment) => attachments.push(attachment),
                None => return Err(image.role),
            }
        }
        Ok(attachments)
    }

    /// Keep ACP catalog identities inside orchestrator capability policy.
    /// They name an agent, not a provider model, and therefore must collapse
    /// to the provider default at this transport boundary.
    fn transport_model(model: Option<String>) -> Option<String> {
        model.filter(|id| !op_orchestrator::is_acp_capability_marker(id))
    }
}

impl VisionLlmClient for ChatVisionLlmClient {
    fn validate(&self, req: VisionCallRequest) -> VisionResponse {
        // Ask the transport how it conveys attachments BEFORE spending a
        // call. This is the only honest way to know whether the reply the
        // model produces was written about pixels or about a file name: a
        // transport that drops attachments still returns fluent, confident
        // text, and every consumer of `VisionResponse::Text` treats that text
        // as grounded (issue #61).
        let transport = self.provider.attachment_transport();
        if !transport.delivers_attachments() {
            return VisionResponse::Skipped {
                reason: Some(format!(
                    "transport '{}' does not deliver image attachments ({transport:?}); \
                     refusing a text-only vision call",
                    self.provider.provider_label()
                )),
            };
        }

        // A malformed / empty screenshot string can't drive a vision
        // call — skip rather than send a text-only critique that would
        // hallucinate against no image. The bytes also decide the media
        // type: a payload no vision wire accepts (SVG, an unknown format)
        // is not something a model can look at.
        let attachments = match Self::attachments_for(&req) {
            Ok(attachments) => attachments,
            Err(role) => {
                return VisionResponse::Skipped {
                    reason: Some(format!(
                        "the {:?} image payload is not base64 png/jpeg/gif/webp — a vision model \
                         has no way to look at it, and the prompt names it by position, so the \
                         call would ask about a picture the model does not have",
                        role
                    )),
                };
            }
        };

        // Inline the vision system prompt into the user message: the
        // CLI-backed `ChatProvider` impls ignore `ChatRequest.system_prompt`
        // (no per-turn system slot), so putting it in the field would
        // silently drop the critique rubric. Same prepend the planning /
        // sub-agent `ChatProviderLlmClient` uses.
        let user_message = if req.system.is_empty() {
            req.message.clone()
        } else {
            format!("{}\n\n---\n\n{}", req.system, req.message)
        };

        let chat_req = ChatRequest {
            system_prompt: String::new(),
            user_message,
            history: Vec::new(),
            // Validation replies are compact JSON critiques — a modest
            // budget is plenty and keeps the call cheap.
            max_output_tokens: 4096,
            // The validation rubric wants a deterministic JSON verdict,
            // not extended reasoning.
            thinking: ThinkingMode::Disabled,
            effort: EffortLevel::Low,
            attachments,
            model: Self::transport_model(self.model.clone())
                .or_else(|| Self::transport_model(req.model.clone())),
        };

        // `provider.send` is a blocking delta iterator (the same shape the
        // chat session + `ChatProviderLlmClient` drain). Accumulate the
        // text deltas into one reply string. An `Error` delta aborts to
        // `Skipped` so a transport failure can't crash the design turn.
        let mut text = String::new();
        for delta in self.provider.send(chat_req) {
            match delta {
                ChatDelta::TextDelta(s) => text.push_str(&s),
                // Thinking tokens are not part of the JSON verdict.
                ChatDelta::Thinking(_) => {}
                ChatDelta::Error(msg) => {
                    return VisionResponse::Skipped {
                        reason: Some(format!("vision provider error: {msg}")),
                    };
                }
                // `Done` closes the turn; tool-use is not expected here.
                ChatDelta::Done { .. } => break,
                ChatDelta::ToolUse { .. } => {}
            }
        }

        if text.trim().is_empty() {
            VisionResponse::Skipped {
                reason: Some("vision provider returned no text".to_string()),
            }
        } else {
            VisionResponse::Text(text)
        }
    }
}

#[cfg(test)]
#[path = "validation_providers_tests.rs"]
mod tests;
