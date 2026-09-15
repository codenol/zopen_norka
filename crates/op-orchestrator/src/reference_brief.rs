//! Vision grounding for user-attached reference screenshots.
//!
//! Before the planner invents a dashboard, one multimodal call inventories
//! what is actually on the reference image. The resulting text brief is
//! injected into planning + sub-agent prompts. When vision is unavailable,
//! a conservative fallback still forbids inventing KPI/analytics chrome.

use crate::types::{ReferenceAttachment, VisionCallRequest, VisionLlmClient, VisionResponse};
use base64::Engine as _;
use std::time::Duration;

const BRIEF_TIMEOUT: Duration = Duration::from_secs(45);

const REFERENCE_BRIEF_SYSTEM: &str = "\
You inventory a UI screenshot for a design agent. Output SHORT structured markdown only.\n\
\n\
## Visible regions\n\
List every major region that is clearly present (sidebar, breadcrumbs, toolbar, \
search, filters, table, pagination, tabs, form fields, modals, etc.).\n\
\n\
## Table / list detail (when present)\n\
- Column headers (exact labels if readable)\n\
- Row density / sample cell themes\n\
- Language of UI copy (e.g. Russian, English)\n\
\n\
## Absent modules (required)\n\
Explicitly list modules that are NOT visible. Prefer: no KPI cards, no charts, \
no analytics metrics, no credit/exposure/drawdown tiles — unless they appear.\n\
\n\
## Agent rules\n\
- Subtasks must map 1:1 to visible body regions only.\n\
- Do not invent finance/analytics widgets absent from the image.\n\
- Prefer toolbar + table + pagination style body when those are visible.\n\
Keep the whole reply under ~350 words. No JSON. No prose preamble.";

const REFERENCE_BRIEF_USER: &str = "\
Describe this reference UI for planning. Inventory visible regions and absent \
modules. Focus on matching the screenshot — not a generic dashboard.";

/// Conservative text when vision fails or is stubbed.
pub fn fallback_reference_brief(image_count: usize) -> String {
    format!(
        "## Reference attachments\n\
         - {image_count} image(s) attached by the user.\n\
         \n\
         ## Agent rules (fallback — vision brief unavailable)\n\
         - Follow the user prompt and SESSION DESIGN SYSTEM kit types only.\n\
         - Do NOT invent KPI cards, charts, analytics metrics, credits, \
         exposure, drawdown, or win-rate tiles unless the prompt explicitly asks.\n\
         - Prefer content-area body modules (toolbar / search / table / pagination) \
         when the prompt or product context is an ops or list screen.\n\
         - Subtasks must not add a second app shell (sidebar/topbar/header)."
    )
}

/// Encode attachment bytes as standard base64 for [`VisionCallRequest`].
pub fn attachment_base64(att: &ReferenceAttachment) -> String {
    base64::engine::general_purpose::STANDARD.encode(&att.data)
}

/// First image attachment as base64 PNG/JPEG payload, if any.
pub fn first_reference_image_base64(attachments: &[ReferenceAttachment]) -> Option<String> {
    attachments
        .iter()
        .find(|a| a.is_image() && !a.data.is_empty())
        .map(attachment_base64)
}

/// Resolve a reference brief via vision, or the fallback string.
///
/// Any non-empty `VisionResponse::Text` is accepted as the inventory. That
/// trust is deliberate and it rests on the vision client's contract (see
/// [`VisionLlmClient::validate`]): a client returns `Text` only when the
/// image actually reached the model, and returns `Skipped` when it could not.
/// The brief cannot verify grounding from the text itself — a model handed
/// only a file name writes a perfectly structured inventory of a picture it
/// never saw (issue #61) — so the honesty has to live at the transport
/// boundary, and a client that breaks that contract reinstates the bug.
pub fn resolve_reference_brief(
    attachments: &[ReferenceAttachment],
    vision: &dyn VisionLlmClient,
    model: Option<&str>,
    provider: Option<&str>,
) -> Option<String> {
    let images: Vec<&ReferenceAttachment> = attachments
        .iter()
        .filter(|a| a.is_image() && !a.data.is_empty())
        .collect();
    if images.is_empty() {
        return None;
    }
    let image_base64 = attachment_base64(images[0]);
    let req = VisionCallRequest {
        system: REFERENCE_BRIEF_SYSTEM.to_string(),
        message: REFERENCE_BRIEF_USER.to_string(),
        image_base64,
        model: model.map(|s| s.to_string()),
        provider: provider.map(|s| s.to_string()),
        timeout: BRIEF_TIMEOUT,
    };
    let response = vision.validate(req);
    match &response {
        VisionResponse::Text(text) if !text.trim().is_empty() => {
            Some(format!("## Reference screen brief\n\n{}", text.trim()))
        }
        VisionResponse::Text(_) | VisionResponse::Skipped { .. } => {
            // Both outcomes produce the same conservative brief, so without
            // the reason on the record "the image never reached the model" and
            // "the model answered nothing" are indistinguishable to whoever
            // reads the log — and they need different fixes.
            if let VisionResponse::Skipped { reason } = &response {
                eprintln!(
                    "[reference-brief] no screenshot inventory: {}",
                    reason
                        .as_deref()
                        .unwrap_or("vision unavailable (no reason given)")
                );
            } else {
                eprintln!("[reference-brief] no screenshot inventory: vision returned empty text");
            }
            Some(fallback_reference_brief(images.len()))
        }
    }
}

/// Fill [`crate::types::DesignRequest::reference_brief`] when attachments are present.
pub fn enrich_request_with_reference_brief(
    request: &mut crate::types::DesignRequest,
    vision: &dyn VisionLlmClient,
) {
    if request.reference_brief.is_some() {
        return;
    }
    if let Some(brief) = resolve_reference_brief(
        &request.reference_attachments,
        vision,
        request.model.as_deref(),
        request.provider.as_deref(),
    ) {
        request.reference_brief = Some(brief);
    }
}

/// Planning / sub-agent user-prompt appendix when a brief is present.
pub fn reference_brief_prompt_block(brief: Option<&str>) -> String {
    let Some(brief) = brief.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    format!(
        "\n\nREFERENCE SCREEN BRIEF (authoritative layout inventory from the user's \
         attached screenshot — plan and generate ONLY modules listed as visible; \
         never invent modules listed as absent):\n{brief}\n\
         If this brief is present, subtasks must map 1:1 to brief regions. Ban \
         kpi / signals / credits / charts unless the brief lists them."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub_providers::SkippedVisionLlmClient;

    #[test]
    fn fallback_mentions_image_count_and_forbids_kpi() {
        let text = fallback_reference_brief(2);
        assert!(text.contains("2 image"));
        assert!(text.to_lowercase().contains("kpi"));
    }

    #[test]
    fn skipped_vision_yields_fallback_brief() {
        let att = ReferenceAttachment {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            data: vec![1, 2, 3, 4],
        };
        let brief =
            resolve_reference_brief(&[att], &SkippedVisionLlmClient, None, None).expect("brief");
        assert!(brief.contains("fallback") || brief.contains("KPI") || brief.contains("kpi"));
    }

    #[test]
    fn enrich_sets_brief_from_attachments() {
        let mut req = crate::types::DesignRequest {
            prompt: "как на картинке".into(),
            reference_attachments: vec![ReferenceAttachment {
                name: "a.png".into(),
                media_type: "image/png".into(),
                data: vec![9, 9, 9],
            }],
            ..Default::default()
        };
        enrich_request_with_reference_brief(&mut req, &SkippedVisionLlmClient);
        assert!(req.reference_brief.as_ref().is_some_and(|b| !b.is_empty()));
    }

    #[test]
    fn no_attachments_leaves_brief_none() {
        let mut req = crate::types::DesignRequest::default();
        enrich_request_with_reference_brief(&mut req, &SkippedVisionLlmClient);
        assert!(req.reference_brief.is_none());
    }

    /// The conservative brief exists so an ungrounded turn cannot hand the
    /// planner an inventory. It must therefore never read like one: no
    /// `## Reference screen brief` heading (which marks a model-written
    /// inventory of the actual image), no `## Visible regions` section, and —
    /// because the text is injected into planning prompts as authoritative —
    /// an explicit statement that vision was unavailable.
    #[test]
    fn fallback_brief_is_not_mistakable_for_an_image_inventory() {
        let text = fallback_reference_brief(1);
        assert!(!text.contains("## Reference screen brief"));
        assert!(!text.contains("## Visible regions"));
        assert!(text.contains("fallback"));
        assert!(text.contains("vision brief unavailable"));
    }

    /// A skip reason travels with the fallback rather than being swallowed:
    /// "the transport never delivered the image" and "the model answered
    /// nothing" need different fixes, and they are indistinguishable from the
    /// brief text alone.
    #[test]
    fn skipped_vision_reason_does_not_change_the_conservative_brief() {
        let att = ReferenceAttachment {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            data: vec![1, 2, 3, 4],
        };
        let brief = resolve_reference_brief(
            &[att],
            &crate::stub_providers::SkippedVisionLlmClient,
            None,
            None,
        )
        .expect("brief");
        assert_eq!(brief, fallback_reference_brief(1));
    }

    /// Text from a client that honoured its contract is wrapped as the
    /// authoritative inventory the planner reads.
    #[test]
    fn grounded_vision_text_becomes_the_reference_screen_brief() {
        struct GroundedClient;
        impl VisionLlmClient for GroundedClient {
            fn validate(&self, _req: VisionCallRequest) -> VisionResponse {
                VisionResponse::Text("## Visible regions\n- toolbar\n- table".into())
            }
        }
        let att = ReferenceAttachment {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            data: vec![1, 2, 3, 4],
        };
        let brief = resolve_reference_brief(&[att], &GroundedClient, None, None).expect("brief");
        assert!(brief.starts_with("## Reference screen brief"));
        assert!(brief.contains("- toolbar"));
        assert!(!brief.contains("fallback"));
    }
}
