//! A pinned style guide must reach the plan on every planning path.
//!
//! Shrinking the planning menu to one entry only *asks* the model to echo the
//! pinned name back. Measured on a real Full-tier run it did not: the plan
//! came back naming no guide, the `forced_style_guide_name` backfill is only
//! populated on the Compact path, and the whole design generated in an
//! unrelated palette. These drive `planning_loop` — the one place every
//! production plan lands — rather than the helper, so a future path that
//! forgets to enforce the pin fails here.

use super::*;
use crate::test_support::{ScriptResponse, ScriptedLlm};

const PINNED: &str = "zen-paper-light";

fn pinned_req(pinned: Option<&str>) -> DesignRequest {
    DesignRequest {
        prompt: "a travel booking app screen".into(),
        // Full tier → Rich planning, which is the mode that was broken.
        model: Some("claude-opus".into()),
        provider: None,
        rules: Vec::new(),
        concurrency: 1,
        continuation_context: None,
        append_context: None,
        validation_enabled: true,
        visual_ref_enabled: false,
        pinned_style_guide: pinned.map(str::to_string),
        reference_attachments: Vec::new(),
        reference_brief: None,
    }
}

/// A plan body naming `guide`, or naming none when `guide` is `None`.
fn plan_json(guide: Option<&str>) -> String {
    let name = match guide {
        Some(guide) => format!("\"styleGuideName\": \"{guide}\","),
        None => String::new(),
    };
    format!(
        r##"{{
  "rootFrame": {{ "id": "root", "name": "Page", "width": 1200, "height": 800,
                 "layout": "vertical", "gap": 0,
                 "fill": [{{ "type": "solid", "color": "#FFFFFF" }}] }},
  {name}
  "subtasks": [
    {{ "id": "hero", "label": "Hero", "region": {{ "width": 1200, "height": 400 }} }},
    {{ "id": "feat", "label": "Features", "region": {{ "width": 1200, "height": 400 }} }}
  ]
}}"##
    )
}

fn plan_for(pinned: Option<&str>, model_choice: Option<&str>) -> OrchestratorPlan {
    let llm = ScriptedLlm::new(vec![ScriptResponse::Text(plan_json(model_choice))]);
    let (plan, _norm) = futures::executor::block_on(planning_loop(
        &pinned_req(pinned),
        &llm,
        &AbortFlag::default(),
    ))
    .expect("planning succeeds");
    plan
}

/// A catalog pin must not replace the session kit.
#[test]
fn a_rich_plan_does_not_take_a_catalog_pin() {
    let plan = plan_for(Some(PINNED), None);
    assert_eq!(plan.style_guide_name, None);
}

#[test]
fn a_pin_does_not_override_the_session_kit() {
    let plan = plan_for(Some(PINNED), Some("developer-terminal-dark"));
    assert_eq!(plan.style_guide_name, None);
}

#[test]
fn the_fallback_plan_does_not_carry_a_catalog_pin() {
    let llm = ScriptedLlm::new(vec![
        ScriptResponse::Text("not json at all".into()),
        ScriptResponse::Text("still not json".into()),
    ]);
    let (plan, _norm) = futures::executor::block_on(planning_loop(
        &pinned_req(Some(PINNED)),
        &llm,
        &AbortFlag::default(),
    ))
    .expect("the fallback plan is not an error");
    assert_eq!(plan.style_guide_name, None);
}

/// An imported guide is pinned by its `user:` id, and that id is what has to
/// survive to the plan — it is the string the sub-agent prompt resolves back
/// to markdown.
#[test]
fn an_imported_guides_id_survives_to_the_plan() {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    op_ai_skills::style_guide::set_user_style_guides(Vec::new());
    let imported = op_ai_skills::style_guide::import_design_md(
        "---\nname: Dimension\n---\n\n# Dimension\n\nNear-monochrome, one violet accent.\n",
        "dimension.md",
    )
    .expect("imports");

    let plan = plan_for(Some(&imported.id), None);
    assert_eq!(plan.style_guide_name, None);
    op_ai_skills::style_guide::set_user_style_guides(Vec::new());
}

/// A pin naming a guide that is in neither half of the catalogue — an import
/// deleted since it was pinned — degrades to the model's own choice rather
/// than forcing a name nothing can resolve.
#[test]
fn a_stale_pin_leaves_the_models_choice_alone() {
    let plan = plan_for(
        Some("user:deleted-last-week"),
        Some("developer-terminal-dark"),
    );
    assert_eq!(plan.style_guide_name, None);
}

/// Without a pin the session kit still owns style — catalog names are dropped.
#[test]
fn no_pin_still_drops_catalog_guide_names() {
    let plan = plan_for(None, Some("developer-terminal-dark"));
    assert_eq!(plan.style_guide_name, None);
}
