use crate::plan::{OrchestratorPlan, Region, RootFrameSpec, Subtask};
use crate::prompt::{build_resolved_style_instruction, build_subagent_prompt};
use crate::types::{AbortFlag, DesignRequest};
use op_ai_skills::resolve_style::{resolve_style, Fonts, ResolveOutcome, StyleParams};
use op_editor_core::ComponentLibrary;
/// The rules a real turn carries: resolved, so the shipped working agreement
/// and the kit's component rules are part of the prompt under test.
fn resolved_rules() -> Vec<jian_ops_schema::DesignRule> {
    op_editor_core::effective_design_rules(None)
        .into_iter()
        .map(|entry| entry.rule)
        .collect()
}



fn atlas_params() -> StyleParams {
    StyleParams {
        color_palette: "Alloy Blue".to_string(),
        roundness: "medium".to_string(),
        elevation: "low".to_string(),
        fonts: Fonts {
            headings: "Inter".to_string(),
            body: "Inter".to_string(),
            captions: "Inter".to_string(),
            data: "IBM Plex Mono".to_string(),
        },
        decorative_imagery: Some("restrained product imagery".to_string()),
    }
}

fn req() -> DesignRequest {
    DesignRequest {
        prompt: "a dense analytics workspace".into(),
        model: Some("claude".into()),
        provider: None,
        rules: resolved_rules(),
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

fn subtask() -> Subtask {
    Subtask {
        id: "hero".into(),
        label: "Hero".into(),
        region: Region {
            width: 1200.0,
            height: 400.0,
        },
        id_prefix: "hero".into(),
        parent_frame_id: Some("root".into()),
        elements: Some("overview metrics and primary workspace controls".into()),
        screen: None,
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    }
}

fn plan_with_style(style_name: &str) -> OrchestratorPlan {
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
        subtasks: vec![subtask()],
        style_guide_name: Some(style_name.to_string()),
    }
}

#[test]
fn subagent_style_instruction_contains_our_tokens() {
    let params = atlas_params();
    let block = build_resolved_style_instruction("Atlas Grid", &params)
        .expect("known style and palette should format");
    let ResolveOutcome::Hit(guide) = resolve_style("Atlas Grid", &params) else {
        panic!("known style and palette should resolve");
    };

    let surface_primary = guide
        .tokens
        .surface
        .get("primary")
        .expect("surface.primary");
    let accent_primary = guide.tokens.accent.get("primary").expect("accent.primary");

    assert!(block.contains("RESOLVED STYLE REFERENCE (Atlas Grid / Alloy Blue)"));
    assert!(block.contains(&format!("surface.primary={surface_primary}")));
    assert!(block.contains(&format!("accent.primary={accent_primary}")));
    assert!(block.contains("rounded.md=8px"));
    assert!(block
        .contains("typography: headings=Inter, body=Inter, captions=Inter, data=IBM Plex Mono"));
    assert!(block.contains("on-surface.primary="));
    assert!(block.contains("Bake these reference values directly into node fills"));
    assert!(block.contains("Do NOT create document variables"));
    assert!(block.contains("Do NOT call set_variables"));
}

#[test]
fn subagent_resolved_style_emits_no_variable_commands() {
    let params = atlas_params();
    let block = build_resolved_style_instruction("Atlas Grid", &params)
        .expect("known style and palette should format");

    let block_type = std::any::type_name_of_val(&block);
    assert!(
        block_type.contains("String"),
        "resolved-style builder must return prompt text, got {block_type}"
    );

    let plan = plan_with_style("Atlas Grid");
    let (call, _) = build_subagent_prompt(
        &plan.subtasks[0],
        &plan,
        &req(),
        AbortFlag::new(),
        false,
        false,
        &ComponentLibrary::default(),
    );
    assert!(
        !call
            .system_prompt
            .contains("RESOLVED STYLE REFERENCE (Atlas Grid / Alloy Blue)"),
        "session kit owns style; catalog resolved-style must not append"
    );
    assert!(
        call.system_prompt.contains("COMPONENT RULES") || call.user_prompt.contains("COMPONENT RULES"),
        "the session's rules should reach the subagent prompt"
    );

    for text in [&block, &call.system_prompt] {
        assert!(!text.contains("EditorCommand"));
        assert!(!text.contains("SetVariable"));
        assert!(!text.contains("MergeThemePreset"));
        assert!(!text.contains("set_variables("));
        assert!(!text.contains("\"set_variables\""));
    }
}
