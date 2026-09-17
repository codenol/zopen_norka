//! TS-compatible `get_design_prompt` MCP tool.

use std::collections::BTreeMap;

use op_editor_core::EditorState;

use super::{McpTool, ToolErrorCode, ToolOutcome};

const PROMPT_SECTIONS: &[&str] = &[
    "all",
    "schema",
    "layout",
    "roles",
    "text",
    "style",
    "icons",
    "examples",
    "guidelines",
    "quality",
    "mobile",
    "planning",
    "elements",
    "elements-cookbook",
    "design-md",
    "copywriting",
    "overflow",
    "cjk",
    "variables",
    "codegen-planning",
    "codegen-chunk",
    "codegen-assembly",
    "codegen-react",
    "codegen-vue",
    "codegen-svelte",
    "codegen-html",
    "codegen-flutter",
    "codegen-swiftui",
    "codegen-compose",
    "codegen-react-native",
];

const INTRO: &str = r#"You are working with OpenPencil, a vector design tool.

TOOL SELECTION - match the user's intent:
- READ/INSPECT the canvas: read_nodes, snapshot_layout, get_selection, get_node
- CREATE new designs: batch_design, design_skeleton, design_content, design_refine
- MODIFY existing nodes: update_node, replace_node, set_node_* tools
- DELETE/REMOVE elements: delete_node after inspecting the target id
- MOVE/COPY: move_node, copy_node, copy_selected

When the user asks to read or inspect existing content, use read tools first.
Each node must follow the PenNode schema and stay under the root frame."#;

const PLANNING_GUIDE: &str = r#"DESIGN PLANNING:
- Classify by purpose: marketing/informational pages use desktop 1200px wide scrollable roots; single-task screens use mobile 375x812; dashboards/admin workspaces use desktop layouts.
- Create a skeleton first, fill each section with content, then refine.
- Keep forms together with their primary action. Split only when one section would be too large.
- Default to light neutral styling unless the request explicitly asks for dark, cyber, neon, terminal, noir, night, or similar themes."#;

const AESTHETIC_QUALITY_GUIDE: &str = r#"AESTHETIC QUALITY BAR:
- Avoid crowded output. Prefer fewer, stronger modules with visible negative space over filling every pixel.
- Mobile screens should use one App Content wrapper for the main body. The wrapper owns horizontal padding (16-20px) and vertical gap (20-24px); inner sections should not each add competing gutters.
- Mobile top rhythm: keep the header/title group and first primary module close. On 375-430px screens, the gap from a greeting/title/header cluster to search, primary action, chart, or first card should usually be 20-32px. Do not leave a blank hero-sized band above the first useful control unless the request explicitly asks for an editorial hero.
- Keep one primary job per screen. Above the fold, show the title/context, the primary action or search, and at most 2-3 supporting modules.
- Use a clear type rhythm: one display/title size, one section heading size, one body/caption size. Avoid many near-identical bold text sizes.
- Reuse one card radius, one card padding, and one shadow treatment within a screen.
- Use at most two saturated colors. Let hierarchy come from spacing, contrast, and content scale, not decoration.
- Product card favorite/heart controls must stay fully inside its card or image with an 8-12px inset. Treat favorite/heart as a functional icon-button, not a floating badge; never use negative x/y, straddle the card edge, or let it overlap the section heading row.
- Mobile tab bars are part of the page flow; do not add fake bottom spacers or overlays.
- Pick a distinct visual concept for each new design and build around one signature moment. Do not repeat the same predictable mobile stack of search + categories + orange promo + two cards unless the prompt specifically calls for that pattern.
- Use the user's domain to vary composition, imagery, palette, rhythm, and interaction affordances; polish should feel intentional rather than template-derived."#;

const RUST_ELEMENT_TOOL_GUIDE: &str = r##"RUST MCP ELEMENT TOOL COMPATIBILITY:
- This Rust MCP server does not expose the TS `add_*_v1` element-tool family unless those exact tools appear in tools/list. Do not call `add_*` tools just because older prompt text or examples mention them.
- CREATE custom UI trees with `batch_design.script` (script-first). The sandboxed JavaScript program may call only `I(parent, nodeObject)` for custom PenNodes and `K("starter/<id>"|"shadcn/<id>"|"<kit>/<component>", parent, overrides?)` for UIKit components. Use loops, helpers, and data arrays inside the script; bindings are ordinary JavaScript variables whose returned node ids can be used by later edit batches.
- `parent` may be `null` for the active page root, a previous `I()`/`K()` result, or one real existing parent id. Node objects use canonical PenNode fields and may omit `id`, because Rust remaps inserted ids.
- Use `batch_design.operations` ONLY to edit existing nodes after inspection or verification. Supported edit/refine operations are one-at-a-time `U(nodeId, patchJson)`, `D(nodeId)`, `M(nodeId, parent, index?)`, `binding=C(sourceId, parent, overrides?)`, `binding=R(nodeId, nodeJson)`, and `binding=G(slotIdOrBinding, "search"|"generate", prompt[, "append"])`. Do not create fresh UI trees with `I()` in operations.
- The default 3-argument `G` is strict slot-fill: its target must exist and have zero children. The explicit fourth argument `"append"` is accepted only on a parent that declares layout `"horizontal"` or `"vertical"`; it never overlays a layout-none/omitted parent. Size the appended binding with `U()` in the same edit batch.
- `U` currently patches geometry/name/fill fields; use dedicated `set_node_*` tools for text, rotation, stroke, font, effects, and other specialized fields.

Example:
`batch_design({script: `
const root = I(null, {type:"frame",name:"Page",width:1200,height:800,layout:"vertical",gap:24,fill:"#ffffff"});
const hero = I(root, {type:"frame",name:"Hero",width:"fill_container",height:360,layout:"vertical",gap:16});
I(hero, {type:"text",name:"Headline",content:"Welcome Back",width:"fill_container",height:64,fontSize:48,fontWeight:700});
K("shadcn/btn-primary", hero, {label:"Get started"});
`})`

Light/dark handling:
- Inspect `get_variables` / `get_active_theme` first when the document already defines theme axes.
- Use variable refs such as `$color-bg`, `$color-text`, and `$color-surface` when the document provides them.
- If the user explicitly asks for a one-off dark or light design and no variables exist, use concrete high-contrast fills and text colors directly."##;

const DEFAULT_DESIGN_MD: &str = "No design rules are loaded in the current document.";

pub struct GetDesignPrompt {
    /// The session's resolved design rules, rendered for the prompt.
    rules_policy: Option<String>,
}

impl McpTool for GetDesignPrompt {
    fn name(&self) -> &str {
        "get_design_prompt"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        let section = args.get("section").map(String::as_str).unwrap_or("all");
        let available = match serde_json::to_string(PROMPT_SECTIONS) {
            Ok(json) => json,
            Err(e) => {
                return ToolOutcome::Err(
                    ToolErrorCode::Internal,
                    format!("serialize prompt sections failed: {e}"),
                );
            }
        };
        let mut out = BTreeMap::new();
        out.insert("section".into(), section.into());
        out.insert("availableSections".into(), available);
        out.insert(
            "designPrompt".into(),
            build_design_prompt(Some(section), self.rules_policy.as_deref()),
        );
        ToolOutcome::Ok(out)
    }
}

pub fn get_design_prompt_snapshot(state: &EditorState) -> GetDesignPrompt {
    GetDesignPrompt {
        // The AI reads the session's structured rules — never a markdown
        // brief, which the editor no longer maintains.
        rules_policy: {
            let rules = op_editor_core::design_rules_ui::visible_rules(
                op_editor_core::session_kit(),
                state.doc.design_md.as_ref(),
                op_editor_core::DesignRulesFilter::All,
            );
            let policy = op_editor_core::build_effective_rules_policy(&rules);
            (!policy.is_empty()).then_some(policy)
        },
    }
}

/// The section's text with the session's rules in front of it.
///
/// The rules are always in force — the editor offers no way to switch them
/// off — so every section carries them, not only the two that used to be
/// special-cased. An agent that asks for the full prompt (the common case)
/// used to receive none of them.
fn build_design_prompt(section: Option<&str>, rules_policy: Option<&str>) -> String {
    let body = match section {
        Some("design-md") => rules_policy
            .map(str::to_string)
            .unwrap_or_else(|| DEFAULT_DESIGN_MD.to_string()),
        Some(section) => section_content(section).unwrap_or_else(build_full_prompt),
        None => build_full_prompt(),
    };
    match rules_policy {
        Some(policy) if !policy.is_empty() => format!("{policy}\n\n{body}"),
        _ => body,
    }
}

fn section_content(section: &str) -> Option<String> {
    match section {
        "all" => Some(build_full_prompt()),
        "schema" => Some(skill_content("schema")),
        "layout" => Some(skill_content("layout")),
        "roles" => Some(skill_content("role-definitions")),
        "text" => Some(skill_content("text-rules")),
        "style" => Some(skill_content("style-defaults")),
        "icons" => Some(skill_content("icon-catalog")),
        "examples" => Some(skill_content("examples")),
        "guidelines" => Some(format!(
            "{}\n\n{}",
            skill_content("design-principles"),
            skill_content("product-principles")
        )),
        "quality" => Some(AESTHETIC_QUALITY_GUIDE.into()),
        "mobile" => Some(skill_content("mobile-app")),
        "planning" => Some(PLANNING_GUIDE.into()),
        "elements" => Some(RUST_ELEMENT_TOOL_GUIDE.into()),
        "elements-cookbook" => Some(RUST_ELEMENT_TOOL_GUIDE.into()),
        "design-md" => Some(DEFAULT_DESIGN_MD.into()),
        "copywriting" => Some(skill_content("copywriting")),
        "overflow" => Some(skill_content("overflow")),
        "cjk" => Some(skill_content("cjk-typography")),
        "variables" => Some(skill_content("variables")),
        "codegen-planning" => Some(skill_content("codegen-planning")),
        "codegen-chunk" => Some(skill_content("codegen-chunk")),
        "codegen-assembly" => Some(skill_content("codegen-assembly")),
        "codegen-react" => Some(skill_content("codegen-react")),
        "codegen-vue" => Some(skill_content("codegen-vue")),
        "codegen-svelte" => Some(skill_content("codegen-svelte")),
        "codegen-html" => Some(skill_content("codegen-html")),
        "codegen-flutter" => Some(skill_content("codegen-flutter")),
        "codegen-swiftui" => Some(skill_content("codegen-swiftui")),
        "codegen-compose" => Some(skill_content("codegen-compose")),
        "codegen-react-native" => Some(skill_content("codegen-react-native")),
        _ => None,
    }
}

fn build_full_prompt() -> String {
    [
        INTRO,
        &skill_content("schema"),
        &skill_content("style-defaults"),
        &skill_content("examples"),
        PLANNING_GUIDE,
        AESTHETIC_QUALITY_GUIDE,
        &skill_content("mobile-app"),
        &skill_content("role-definitions"),
        &skill_content("layout"),
        &skill_content("text-rules"),
        &skill_content("design-principles"),
        &skill_content("variables"),
        RUST_ELEMENT_TOOL_GUIDE,
    ]
    .join("\n\n")
}

fn skill_content(name: &str) -> String {
    op_ai_skills::get_skill_by_name(name)
        .map(|skill| skill.content.clone())
        .unwrap_or_default()
}
