//! Fixtures behind `design_session_worker_tests`: the agent identity, the
//! persisted plan/request payloads and the chat-activity builder.
//!
//! The test cases stay in the parent module, so their names do not move.

use op_editor_core::{ChatActivity, ChatActivityStatus};
use op_orchestrator::agent_identity::AgentIdentity;

pub(super) fn identity(name: &str, color: &str) -> AgentIdentity {
    AgentIdentity {
        name: name.into(),
        color: color.into(),
    }
}

pub(super) fn persisted_subtask_json() -> String {
    serde_json::to_string(&op_orchestrator::plan::Subtask {
        id: "hero".into(),
        label: "Hero".into(),
        region: op_orchestrator::plan::Region {
            width: 1200.0,
            height: 400.0,
        },
        id_prefix: "hero".into(),
        parent_frame_id: None,
        elements: None,
        screen: Some("Profile".into()),
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    })
    .unwrap()
}

pub(super) fn persisted_request_json() -> String {
    serde_json::to_string(&op_orchestrator::DesignRequest {
        prompt: "design profile".into(),
        model: None,
        provider: None,
        rules: Vec::new(),
        concurrency: 2,
        continuation_context: None,
        append_context: None,
        validation_enabled: false,
        visual_ref_enabled: false,
        pinned_style_guide: None,
        reference_attachments: Vec::new(),
        reference_brief: None,
    })
    .unwrap()
}

pub(super) fn activity(id: &str, status: ChatActivityStatus) -> ChatActivity {
    ChatActivity {
        id: id.into(),
        title: id.into(),
        detail: None,
        status,
        content_offset: None,
    }
}
