//! Sub-agent tests — shared fixtures live here; the per-cluster cases are
//! mounted as child modules below.

use super::*;
use crate::plan::{OrchestratorPlan, Region, RootFrameSpec};
use crate::test_support::{ScriptResponse, ScriptedLlm, VecDocSink};
use crate::types::LlmError;
use futures::executor::block_on;
use jian_ops_schema::node::PenNode;

fn req() -> DesignRequest {
    DesignRequest {
        prompt: "a page".into(),
        model: None,
        provider: None,
        rules: Vec::new(),
        concurrency: 1,
        continuation_context: None,
        append_context: None,
        validation_enabled: true,

        visual_ref_enabled: false,
        pinned_style_guide: None,
        reference_attachments: Vec::new(),
        reference_brief: None,
    }
}

fn plan() -> OrchestratorPlan {
    OrchestratorPlan {
        root_frame: RootFrameSpec {
            id: "root".into(),
            name: "P".into(),
            width: 1200.0,
            height: 800.0,
            layout: None,
            gap: None,
            padding: None,
            fill: None,
        },
        subtasks: vec![],
        style_guide_name: None,
    }
}

fn subtask() -> Subtask {
    Subtask {
        id: "hero".into(),
        label: "Hero".into(),
        region: Region {
            width: 1200.0,
            height: 400.0,
        },
        id_prefix: "hero".into(),
        parent_frame_id: None,
        elements: None,
        screen: None,
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    }
}

// A single I(null, {...}) call whose node object nests its children inline
// (batch_design's insert accepts a whole subtree per call). Authored ids
// are dropped: the batch_design executor reassigns fresh ids to every
// inserted node regardless, so tests that use this constant must not assert
// on literal id strings.
const NODE_SCRIPT: &str = r#"I(null, {"type":"frame","name":"Card","x":0,"y":0,"width":1200,"height":200,"children":[{"type":"text","content":"Hero","fontSize":18}]});"#;

#[path = "subagent_coalesce_tests.rs"]
mod coalesce_tests;
#[path = "subagent_run_subtask_tests.rs"]
mod run_subtask_tests;

#[test]
fn a_section_whose_named_parent_is_gone_is_re_homed_not_refused() {
    // Issue #245, measured on the English pricing-page prompt: the turn ran
    // 110 s, spent 9 329 characters of reasoning, and ended with `pages[0]`
    // holding nothing because one `parent_id` the plan named was not on the
    // page. A section that lands a level too high is worth more than a section
    // that does not land at all.
    let state = EditorState::starter();
    let wanted = NodeId::new("n36");

    // The page's only container is what the sections hang under.
    let (parent, status) = super::resolve_subtask_parent(&state, &wanted);
    assert_eq!(status, Some("missing"));
    assert_eq!(
        parent.as_str(),
        state.active_children()[0].id_str(),
        "the section is re-homed into the page's only container"
    );

    // A page with no container at all takes the section at its root.
    let mut empty = EditorState::starter();
    empty.active_children_mut().clear();
    let (parent, status) = super::resolve_subtask_parent(&empty, &wanted);
    assert_eq!(status, Some("missing"));
    assert!(
        !parent.is_real(),
        "an empty page takes the section at the page root"
    );

    // A parent that IS on the page is left exactly alone.
    let live = NodeId::new(state.active_children()[0].id_str());
    let (parent, status) = super::resolve_subtask_parent(&state, &live);
    assert_eq!(status, None);
    assert_eq!(parent.as_str(), live.as_str());
}
