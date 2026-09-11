//! Available-component manifest from the session kit type index
//! (not harvested variant frames).

use super::*;
use op_editor_core::{session_kit, KitLayer, KitManifest};

/// Loud magenta for widgets the current kit has no type for.
pub(crate) const KIT_GAP_FILL: &str = "#E6007A";

fn layer_heading(layer: KitLayer) -> &'static str {
    match layer {
        KitLayer::Atom => "Atoms",
        KitLayer::Molecule => "Molecules",
        KitLayer::Organism => "Organisms",
        KitLayer::Template => "Templates",
        KitLayer::Recipe => "Recipes",
    }
}

/// Build the AVAILABLE COMPONENTS manifest from the session kit type
/// index — one row per type, not the first 60 Button variants.
///
/// Returns `None` when the live document has no reusable masters: advertising
/// kit ids without a merged library makes the model emit `ref`s that paint as
/// empty (dropped by `resolve_refs_for_canvas`).
pub(super) fn available_components_manifest(
    components: &ComponentLibrary,
    script_on: bool,
) -> Option<String> {
    if components.is_empty() {
        return None;
    }
    Some(session_kit_components_manifest(script_on))
}

/// Compact token index from the session kit.
pub(super) fn session_variable_index_block() -> String {
    let kit = session_kit();
    session_variable_index_block_for(kit)
}

pub(super) fn session_variable_index_block_for(kit: &KitManifest) -> String {
    let mut lines = vec![format!(
        "SESSION DESIGN SYSTEM TOKENS ({} — authoritative; do not use generic \
         `$color-accent` / `$color-surface` / sidebar 240–280):",
        kit.name
    )];
    for entry in &kit.variable_index {
        lines.push(format!("  - {entry}"));
    }
    lines.join("\n")
}

/// Recipe block for planning + generation.
pub(super) fn session_recipe_block() -> String {
    session_recipe_block_for(session_kit())
}

pub(super) fn session_recipe_block_for(kit: &KitManifest) -> String {
    let mut lines = vec![format!("SESSION DESIGN SYSTEM RECIPES ({}):", kit.name)];
    for recipe in &kit.recipes {
        lines.push(format!(
            "- `{}` ({}) → template id `{}`. {}",
            recipe.id, recipe.name, recipe.template, recipe.notes
        ));
    }
    lines.join("\n")
}

/// Chassis + type-index rules for compact/rich planning.
pub(super) fn session_kit_planning_block(desktop: bool) -> String {
    let kit = session_kit();
    let mut lines = vec![format!(
        "SESSION DESIGN SYSTEM: {} (id `{}`). Follow this kit, not a catalog style guide.",
        kit.name, kit.id
    )];
    if desktop {
        lines.push(kit.content_area_brief());
        if let Some(canvas) = &kit.canvas {
            lines.push(format!(
                "Layout/Default artboard is {}x{} with fill {}.",
                canvas.width as i32, canvas.height as i32, canvas.fill
            ));
        }
        lines.push(
            "Do NOT plan sidebar, topbar, header, or breadcrumbs as separate subtasks — \
             they already live in Layout/Default. Plan 1-3 subtasks that only fill the \
             content area. Retitle existing chrome for this product; do not invent \
             widgets that are not in the type index."
                .to_string(),
        );
    }
    lines.push(session_recipe_block_for(kit));
    lines.join("\n")
}

/// Recency override: kit types or loud KitGap, never invented chrome.
pub(super) fn kit_gap_rules_block() -> String {
    format!(
        "KIT GAP RULES:\n\
         - Chrome and controls MUST be `type:\"ref\"` to a default master from AVAILABLE COMPONENTS.\n\
         - Layout/Default is the ready screen chassis. Put product UI in its content area \
(`Main container`). Do NOT emit another Layout, Sidebar, or topbar.\n\
         - Do NOT draw a second app shell, top nav, search field, bar chart, or KPI card unless that type is listed.\n\
         - If no type fits, emit ONE frame named `KitGap / <needed widget>`, fill `{KIT_GAP_FILL}`, \
white 12px label with that name. Do not fake it with lookalike cards.\n\
         - Colors and radii only from SESSION DESIGN SYSTEM TOKENS (and design.md if present).\n\
         - Format-example cards/rows above apply ONLY when a listed type matches."
    )
}

fn session_kit_components_manifest(script_on: bool) -> String {
    let kit = session_kit();
    let total: u32 = kit.types.iter().map(|t| t.variant_count).sum();
    let example = kit
        .types
        .iter()
        .find(|t| t.id == "button")
        .map(|t| t.default_master_id.as_str())
        .unwrap_or(kit.sentinel_master_id.as_str());
    let mut lines = vec![format!(
        "AVAILABLE COMPONENTS ({} — {} types, {total} reusable masters; \
         PREFER instantiating these with a `ref` node over building from scratch. \
         Desktop screens already use Layout/Default — fill its content area, do not emit another shell):",
        kit.name,
        kit.types.len()
    )];
    for layer in [
        KitLayer::Template,
        KitLayer::Organism,
        KitLayer::Molecule,
        KitLayer::Atom,
    ] {
        let entries: Vec<_> = kit.types_in_layer(layer).collect();
        if entries.is_empty() {
            continue;
        }
        lines.push(format!("{}:", layer_heading(layer)));
        for ty in entries {
            let slots = if ty.slots.is_empty() {
                String::new()
            } else {
                let listed: Vec<String> = ty
                    .slots
                    .iter()
                    .map(|s| format!("{} ({})", s.suffix, s.field))
                    .collect();
                format!(" slots {}", listed.join(", "))
            };
            lines.push(format!(
                "  - {} ({}, {} variants, pattern `{}`){slots}",
                ty.default_master_id, ty.name, ty.variant_count, ty.name_pattern
            ));
            for rule in ty.do_rules.iter().take(2) {
                lines.push(format!("      do: {rule}"));
            }
            for rule in ty.dont_rules.iter().take(1) {
                lines.push(format!("      don't: {rule}"));
            }
        }
    }
    lines.push(session_recipe_block_for(kit));
    let slot = kit.content_slot_name();
    let instruction = if script_on {
        format!(
            "To use one, call I with a single ref node — no children needed; override its text/fill \
             via `descendants`. Example:\n  \
             const cta = I(<containerBinding>, {{\"type\":\"ref\",\"ref\":\"{example}\",\"descendants\":{{\"{example}-label\":{{\"content\":\"Get started\"}}}}}});\n\
             Layout/Default is already the page. Do NOT emit another Layout or Sidebar. \
             Fill the content area `{slot}` (id `{}`) with kit refs. \
             Only build an element by hand when no type above fits — then KitGap `{KIT_GAP_FILL}`.",
            kit.content_slot_id()
        )
    } else {
        format!(
            "To use one, emit a single node — `type:\"ref\"`, the component id, its `_parent`, and \
             override its text/fill via `descendants` (it needs no `children`). Example:\n  \
             {{\"_parent\":\"<container-id>\",\"id\":\"<your-id>\",\"type\":\"ref\",\"ref\":\"{example}\",\"descendants\":{{\"{example}-label\":{{\"content\":\"Get started\"}}}}}}\n\
             Layout/Default is already the page. Do NOT emit another Layout or Sidebar. \
             Fill the content area `{slot}` (id `{}`) with kit refs. \
             Only build an element by hand when no type above fits — then KitGap `{KIT_GAP_FILL}`.",
            kit.content_slot_id()
        )
    };
    lines.push(instruction);
    lines.join("\n")
}
