//! Sub-agent prompt content tests: explicit design numbers, mobile-food
//! templates, language consistency and design-system dropping.

use super::*;
/// The rules a real turn carries: resolved, so the shipped working agreement
/// and the kit's component rules are part of the prompt under test.
fn resolved_rules() -> Vec<jian_ops_schema::DesignRule> {
    op_editor_core::effective_design_rules(None)
        .into_iter()
        .map(|entry| entry.rule)
        .collect()
}

#[test]
fn subagent_prompt_injects_exact_json_quoted_screen_route_inventory() {
    let subtask = subtask();
    let plan = plan();
    let req = req();
    let routes = vec![
        ("Home".to_string(), "/".to_string()),
        (
            "Movie \"Night\"\nDetail".to_string(),
            "/movie-detail".to_string(),
        ),
    ];

    let (call, with_routes_report) = build_subagent_prompt_with_screen_routes(
        &subtask,
        &plan,
        &req,
        AbortFlag::new(),
        false,
        false,
        &ComponentLibrary::default(),
        &routes,
    );
    let expected = r#"DOCUMENT SCREEN ROUTES (use these exact route values in schema-encoded navigation actions; never invent another route):
- "Home" -> "/"
- "Movie \"Night\"\nDetail" -> "/movie-detail"

CRITICAL LAYOUT CONSTRAINTS:"#;
    assert!(
        call.user_prompt.contains(expected),
        "route inventory must be exact and JSON-quoted:\n{}",
        call.user_prompt
    );

    let (_, empty_report) = build_subagent_prompt_with_screen_routes(
        &subtask,
        &plan,
        &req,
        AbortFlag::new(),
        false,
        false,
        &ComponentLibrary::default(),
        &[],
    );
    assert_eq!(
        with_routes_report, empty_report,
        "user-prompt route context must not perturb the skill budget"
    );
}

#[test]
fn empty_screen_route_inventory_matches_public_builder_byte_for_byte() {
    let subtask = subtask();
    let plan = plan();
    let req = req();
    let components = ComponentLibrary::default();

    let (compat_call, compat_report) = build_subagent_prompt(
        &subtask,
        &plan,
        &req,
        AbortFlag::new(),
        false,
        false,
        &components,
    );
    let (empty_call, empty_report) = build_subagent_prompt_with_screen_routes(
        &subtask,
        &plan,
        &req,
        AbortFlag::new(),
        false,
        false,
        &components,
        &[],
    );

    assert_eq!(compat_call.system_prompt, empty_call.system_prompt);
    assert_eq!(compat_call.user_prompt, empty_call.user_prompt);
    assert_eq!(compat_call.timeout, empty_call.timeout);
    assert_eq!(compat_call.no_text_timeout, empty_call.no_text_timeout);
    assert_eq!(
        compat_call.first_text_timeout,
        empty_call.first_text_timeout
    );
    assert_eq!(compat_report, empty_report);
    assert!(
        !empty_call.user_prompt.contains("DOCUMENT SCREEN ROUTES"),
        "empty inventory must not grow a route header"
    );
}

#[test]
fn subagent_prompt_honors_explicit_radius_and_spacing_numbers() {
    let mobile_req = DesignRequest {
        prompt: "设计一个美食移动端首页，圆角和间距要统一，圆角 8 px，间距 12 px".into(),
        model: Some("claude-haiku".into()),
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
    };
    let mut mobile_plan = plan();
    mobile_plan.root_frame.width = 402.0;
    mobile_plan.root_frame.height = 874.0;
    mobile_plan.style_guide_name = Some("warm-food-mobile-light".into());
    let subtask = Subtask {
        id: "content".into(),
        label: "Content".into(),
        region: Region {
            width: 402.0,
            height: 640.0,
        },
        id_prefix: "content".into(),
        parent_frame_id: Some("page".into()),
        elements: Some("search, categories, promo, restaurant cards".into()),
        screen: None,
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    };

    let (cr, _) = bsp(
        &subtask,
        &mobile_plan,
        &mobile_req,
        AbortFlag::new(),
        false,
        false,
    );
    assert!(
        cr.user_prompt
            .contains("EXPLICIT USER DESIGN TOKENS: cornerRadius must be 8px"),
        "missing explicit radius override:\n{}",
        cr.user_prompt
    );
    assert!(
        cr.user_prompt.contains("layout gap/spacing must be 12px"),
        "missing explicit spacing override:\n{}",
        cr.user_prompt
    );
    assert!(
        !cr.user_prompt.contains("cornerRadius 14-18"),
        "mobile search guidance must not contradict explicit 8px radius:\n{}",
        cr.user_prompt
    );
}

#[test]
fn mobile_food_prompt_avoids_fixed_food_template() {
    let mobile_req = DesignRequest {
        prompt: "设计一个美食应用移动端首页，希望好看点".into(),
        model: Some("claude-haiku".into()),
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
    };
    let mut mobile_plan = plan();
    mobile_plan.root_frame.width = 402.0;
    mobile_plan.root_frame.height = 874.0;
    // Test the GENERAL mobile path (no specific style guide). A domain style
    // guide is a separate, opt-in source of layout rules — this test verifies
    // the general pipeline no longer hardcodes the fixed food template.
    mobile_plan.style_guide_name = None;
    let subtask = Subtask {
        id: "content".into(),
        label: "Content".into(),
        region: Region {
            width: 402.0,
            height: 640.0,
        },
        id_prefix: "content".into(),
        parent_frame_id: Some("page".into()),
        elements: Some("header, search, categories, featured dish, popular list".into()),
        screen: None,
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    };

    let (cr, _) = bsp(
        &subtask,
        &mobile_plan,
        &mobile_req,
        AbortFlag::new(),
        false,
        false,
    );
    // Mobile UI guardrails now load via the `mobile-ui` skill (system prompt),
    // so check the combined prompt.
    let combined = format!("{}\n{}", cr.system_prompt, cr.user_prompt);
    // The hardcoded food-template anatomy is REMOVED — it locked every food app
    // into the same structure (user direction 2026-06-23). It must appear in
    // NEITHER prompt.
    assert!(
        !combined.contains("MOBILE FOOD POLISH"),
        "the fixed food-template rule must not be injected:\n{combined}"
    );
    assert!(
        !combined.contains("never use space_between or space_around for category chips"),
        "category-rail distribution must NOT be hardcoded:\n{combined}"
    );
    assert!(
        !combined.contains("Product card rows use two equal fill_container cards"),
        "product-card layout must NOT be hardcoded:\n{combined}"
    );
    // The variation guidance + general (non-food-specific) alignment rules stay.
    assert!(
        combined.contains("NO FIXED FOOD TEMPLATE"),
        "variation guidance must remain:\n{combined}"
    );
    assert!(
        combined.contains("MOBILE GRID ALIGNMENT"),
        "general grid-alignment guidance must remain:\n{combined}"
    );
    // Every nav tab keeps its label (the cart-tab-loses-label fix).
    assert!(
        combined.contains("never emit a tab (e.g. cart) with an icon but no label"),
        "nav tabs must all keep a label:\n{combined}"
    );
    // The filter button is a neutral surface, not an accent-filled dark-icon button.
    assert!(
        combined.contains("Do NOT make it an accent-filled"),
        "filter button must be neutral, not orange-on-dark:\n{combined}"
    );
}

#[test]
fn chinese_mobile_food_prompt_carries_language_consistency_rule() {
    let mobile_req = DesignRequest {
        prompt: "设计一个美食应用移动端首页，包含配送地址、搜索、分类和主题推荐".into(),
        model: Some("claude-haiku".into()),
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
    };
    let mut mobile_plan = plan();
    mobile_plan.root_frame.width = 390.0;
    mobile_plan.root_frame.height = 844.0;
    let subtask = Subtask {
        id: "content".into(),
        label: "首页内容".into(),
        region: Region {
            width: 390.0,
            height: 640.0,
        },
        id_prefix: "content".into(),
        parent_frame_id: Some("page".into()),
        elements: Some("配送地址、搜索框、美食分类、主题推荐卡片".into()),
        screen: None,
        generated_root_id: None,
        existing_section_labels: None,
        retry_feedback: None,
    };

    let (cr, report) = bsp(
        &subtask,
        &mobile_plan,
        &mobile_req,
        AbortFlag::new(),
        false,
        false,
    );

    assert!(
        report
            .included
            .iter()
            .any(|entry| entry.name == "cjk-typography"),
        "Chinese prompts must keep cjk-typography guidance; report={report:?}"
    );
    assert!(
        cr.system_prompt.contains("LANGUAGE CONSISTENCY"),
        "Chinese prompts must forbid mixed English boilerplate:\n{}",
        cr.system_prompt
    );
}

/// design-system is dropped whenever another styling source covers it:
/// session kit (its rules), or style-defaults. The markdown brief is gone.
#[test]
fn subagent_prompt_drops_design_system_when_styling_covered() {
    const DESIGN_SYSTEM_ONLY: &str = "design system architect";
    const STYLE_DEFAULTS_ONLY: &str = "VISUAL STYLE POLICY";

    // No style guide named, no design.md → session kit owns style.
    let (covered, _) = bsp(&subtask(), &plan(), &req(), AbortFlag::new(), false, false);
    let kit = op_editor_core::session_kit();
    assert!(
        covered.system_prompt.contains(&kit.name),
        "session kit must cover styling when no catalog guide is named"
    );
    assert!(
        covered.system_prompt.contains("COMPONENT RULES")
            || covered.user_prompt.contains("COMPONENT RULES"),
        "the kit's own rules cover styling when no catalog guide is named"
    );
    assert!(
        !covered.system_prompt.contains(DESIGN_SYSTEM_ONLY),
        "design-system dropped when the session kit covers styling"
    );
    assert!(
        !covered.system_prompt.contains(STYLE_DEFAULTS_ONLY),
        "catalog style-defaults must not load alongside the session kit"
    );

    // A catalog styleGuideName on the plan is ignored while the kit owns style.
    let mut sg_plan = plan();
    sg_plan.style_guide_name = Some("saas-clean-light".into());
    let (with_guide, _) = bsp(&subtask(), &sg_plan, &req(), AbortFlag::new(), false, false);
    assert!(
        with_guide.system_prompt.contains(&kit.name),
        "named catalog guide must not replace the session kit"
    );
    assert!(
        !with_guide.system_prompt.contains(DESIGN_SYSTEM_ONLY),
        "design-system dropped when the session kit covers styling"
    );
    assert!(
        !with_guide.system_prompt.contains(STYLE_DEFAULTS_ONLY),
        "catalog style-defaults must not load alongside the session kit"
    );
}
