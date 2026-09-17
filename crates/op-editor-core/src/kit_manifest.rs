//! Skala Spectrum kit manifest — types, slots, do/don't, recipes.
//!
//! The reusable masters stay in `design/skala-spectrum.lib.op` and are
//! merged at session start. This JSON is the *index* the agent prompt, the
//! Assets rail, and the compact `design.md` policy all share. It is small
//! enough to `include_str!` on wasm32.

use std::sync::OnceLock;

use jian_ops_schema::DesignMdSpec;
use serde::Deserialize;

use crate::design_md::parse_design_md;
use crate::state::EditorState;

const KIT_JSON: &str = include_str!("../../../design/skala-spectrum.kit.json");

/// Frost layer a kit type belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KitLayer {
    Atom,
    Molecule,
    Organism,
    Template,
    Recipe,
}

/// A descendant slot the agent may override via `descendants`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KitSlot {
    /// Suffix on the master id (`-label`, `-icon`, `-initials`, `-main`).
    pub suffix: String,
    /// PenNode field to write (`content`, `iconFontName`, `children`).
    pub field: String,
}

/// One component *type* (Button, Sidebar), not every variant frame.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitType {
    pub id: String,
    pub name: String,
    pub layer: KitLayer,
    pub default_master_id: String,
    pub name_pattern: String,
    pub variant_count: u32,
    #[serde(default)]
    pub slots: Vec<KitSlot>,
    #[serde(default, rename = "do")]
    pub do_rules: Vec<String>,
    #[serde(default, rename = "dont")]
    pub dont_rules: Vec<String>,
}

/// A writing system a text is written in.
///
/// Used on both sides of one comparison: the script a recipe's own copy is
/// written in, and the script detected in a request. Two scripts are enough
/// for the question being asked — "is this the same writing system" — and a
/// script is a fact a prompt carries, unlike a locale code it never states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KitScript {
    Latin,
    Cyrillic,
}

/// The shape of screen a recipe's master is authored at.
///
/// A recipe is a fixed-size composed screen, so it serves one shape. Nothing
/// used to record which, and a prompt that asked for a phone screen was
/// answered with a 1440-wide table (issue #181); the declaration is what the
/// selection reads to refuse that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KitFormFactor {
    Mobile,
    Desktop,
}

impl KitFormFactor {
    /// The shape an artboard width implies: a phone-shaped screen is a few
    /// hundred points across, a desktop screen a thousand or more. Anything
    /// between is neither, and claims nothing.
    pub fn of_width(width: f64) -> Option<Self> {
        if width <= MOBILE_MAX_WIDTH {
            Some(Self::Mobile)
        } else if width >= DESKTOP_MIN_WIDTH {
            Some(Self::Desktop)
        } else {
            None
        }
    }
}

/// Widest artboard still read as a phone screen.
pub const MOBILE_MAX_WIDTH: f64 = 600.0;
/// Narrowest artboard read as a desktop screen.
pub const DESKTOP_MIN_WIDTH: f64 = 1000.0;

/// Named screen composition that picks a template and describes its slots.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KitRecipe {
    pub id: String,
    pub name: String,
    pub template: String,
    pub notes: String,
    /// Words that mark a request as this recipe's job ("коммутатор",
    /// "switch", "inventory"). The product selects on them, so the choice of
    /// recipe is deterministic instead of depending on the model's mood.
    ///
    /// This is the recipe's *subject* vocabulary: the nouns that name what
    /// the screen is about. At least one of them has to appear in a request
    /// before the recipe may be placed — a list-shaped screen is not this
    /// screen, and words that describe any admin screen belong in
    /// [`Self::supporting`], where they support a match without making one
    /// (issue #181).
    #[serde(default)]
    pub matches: Vec<String>,
    /// Words that support the choice but cannot make it.
    ///
    /// "list", "список", "user", "пользовател" appear in a request for almost
    /// any screen with rows, so on their own they are evidence of nothing —
    /// two of them once scored 17 and answered a mobile profile prompt with
    /// the ops equipment table. They still count as evidence next to a
    /// subject word.
    #[serde(default)]
    pub supporting: Vec<String>,
    /// Shape this recipe's master is authored at. `None` falls back to the
    /// kit canvas, which is the artboard every master in the kit is drawn at.
    #[serde(default, rename = "formFactor")]
    pub form_factor: Option<KitFormFactor>,
    /// Script the copy inside this recipe's master is written in ("cyrillic"
    /// for the Skala ops screen). A request written in another script is not
    /// this recipe's job: placing it imports its copy and the answer comes
    /// back in the recipe's language (issue #187).
    #[serde(default, rename = "copyScript")]
    pub copy_script: Option<KitScript>,
    /// Blocks this recipe carries but does not require. A request that says
    /// it does not want one ("no filter") hides it before the model runs —
    /// the same reasoning as the recipe choice: removing what the user named
    /// is a decision the product can make, not a favour to ask for.
    #[serde(default)]
    pub optional: Vec<KitRecipeOptional>,
}

impl KitRecipe {
    /// The shape this recipe can serve.
    ///
    /// Its own declaration wins; a recipe that makes none inherits the kit
    /// canvas, which is the artboard its masters are drawn at. The override
    /// exists for the exception — a phone screen in a desktop kit — and is
    /// the reason a recipe that is deliberately the other shape must say so.
    pub fn form_factor(&self, kit: &KitManifest) -> Option<KitFormFactor> {
        self.form_factor.or_else(|| {
            kit.canvas
                .as_ref()
                .and_then(|canvas| KitFormFactor::of_width(canvas.width))
        })
    }
}

/// One optional block of a recipe.
///
/// A removal request reads as a subject plus a negation — "фильтр … не
/// нужны", "without pagination" — and they are rarely adjacent, so the
/// matcher looks for the negation near the subject rather than encoding
/// every phrasing as its own phrase.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KitRecipeOptional {
    /// Node name inside the recipe's subtree.
    pub block: String,
    /// Words that name the block ("фильтр", "filter").
    #[serde(default)]
    pub subjects: Vec<String>,
    /// Words that ask for its removal ("не нуж", "without").
    #[serde(default)]
    pub negations: Vec<String>,
}

/// Default artboard for screens assembled from this kit's sentinel template.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KitCanvas {
    pub width: f64,
    pub height: f64,
    pub fill: String,
}

/// On-disk kit policy. One file per design system.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitManifest {
    pub id: String,
    pub name: String,
    pub library: String,
    pub sentinel_master_id: String,
    #[serde(default)]
    pub canvas: Option<KitCanvas>,
    /// Name of the nested frame sub-agents fill (remapped ids after instantiate).
    #[serde(default)]
    pub main_slot_name: Option<String>,
    /// Stable master id of that content-area frame (`tpl-layout-main`).
    #[serde(default)]
    pub main_slot_id: Option<String>,
    #[serde(default)]
    pub variable_index: Vec<String>,
    pub types: Vec<KitType>,
    #[serde(default)]
    pub recipes: Vec<KitRecipe>,
}

/// The product ships a single design system.
pub const SKALA_KIT_ID: &str = "skala-spectrum";

/// Parsed Skala Spectrum kit. Panics only if the committed JSON is invalid,
/// which is a build-time fixture bug.
pub fn skala_kit() -> &'static KitManifest {
    static KIT: OnceLock<KitManifest> = OnceLock::new();
    KIT.get_or_init(|| {
        serde_json::from_str(KIT_JSON).expect("design/skala-spectrum.kit.json must parse")
    })
}

/// Design system attached to this editor session.
///
/// Today the shipped kit is Skala Spectrum; callers must read fields from
/// the manifest instead of hardcoding that name or its tokens.
pub fn session_kit() -> &'static KitManifest {
    skala_kit()
}

impl KitManifest {
    pub fn type_by_id(&self, id: &str) -> Option<&KitType> {
        self.types.iter().find(|t| t.id == id)
    }

    pub fn type_by_master(&self, master_id: &str) -> Option<&KitType> {
        self.types.iter().find(|t| t.default_master_id == master_id)
    }

    pub fn types_in_layer(&self, layer: KitLayer) -> impl Iterator<Item = &KitType> {
        self.types.iter().filter(move |t| t.layer == layer)
    }

    pub fn recipe(&self, id: &str) -> Option<&KitRecipe> {
        self.recipes.iter().find(|r| r.id == id)
    }

    /// Nested frame sub-agents fill after the sentinel template is instantiated.
    pub fn content_slot_name(&self) -> &str {
        self.main_slot_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or("Main container")
    }

    /// Master id of the content-area frame inside `Layout/Default`.
    pub fn content_slot_id(&self) -> &str {
        self.main_slot_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .unwrap_or("tpl-layout-main")
    }

    /// Planner + generator contract: reuse the ready layout, fill the content area.
    pub fn content_area_brief(&self) -> String {
        format!(
            "Use the ready `Layout/Default` chassis (id `{}`). Do not invent a new app \
             shell. The content area is the white card `{}` (id `{}`) in the `Content` \
             column — put this screen's body only there. Adapt the existing layout to \
             the task (logo, sidebar labels, breadcrumbs) via descendants on those kit \
             refs; never nest a second Layout, Sidebar, or topbar.",
            self.sentinel_master_id,
            self.content_slot_name(),
            self.content_slot_id()
        )
    }
}

/// Compact `design.md` the agent and the Design-MD panel share — not the
/// 400-line human spec in `DESIGN.md`.
pub fn skala_compact_design_md() -> String {
    let kit = skala_kit();
    let mut md = String::from("# Skala Spectrum\n\n");
    md.push_str("## Theme\n\n");
    md.push_str(
        "Enterprise operations UI: dense, calm, engineered. Light theme default. \
         Bondi `#2D98B4` is work (buttons, focus, table accent). Java `#00BEC8` is \
         identity (logo, switch-on) — never the primary button fill. Canvas \
         `$layout/background/default`. Body 14px Roboto. Controls from kit atoms \
         (Button Large 32), not a generic 38px / 32px-padded dashboard.\n\n",
    );
    md.push_str("## Colors\n\n");
    md.push_str("- **Bondi** (#2D98B4) — primary actions, `$button/filled/accent/…`\n");
    md.push_str("- **Java** (#00BEC8) — logo mark only\n");
    md.push_str("- **Canvas** (#EEF1F5) — `$layout/background/default`\n");
    md.push_str("- **On-surface** (#3F4146) — body text\n");
    md.push_str("- **Error** (#E53334) — destructive\n\n");
    md.push_str("## Typography\n\n");
    md.push_str(
        "Font family: Roboto. Body 14/400. Buttons 14/500. Headlines 18/600 and 16/600. \
         Roboto Mono only for codes and tabular figures.\n\n",
    );
    md.push_str("## Layout\n\n");
    md.push_str(
        "Every desktop screen starts from recipe `ops-shell`. \
         Gutters 20 top/left, 16 right/bottom. Sidebar 251px, not 240–280. ",
    );
    md.push_str(&kit.content_area_brief());
    md.push_str("\n\n");
    md.push_str("## Components\n\n");
    for ty in &kit.types {
        md.push_str(&format!(
            "**{}** (`{}`, {} variants). Default master `{}`. Pattern `{}`.\n",
            ty.name, ty.default_master_id, ty.variant_count, ty.default_master_id, ty.name_pattern
        ));
        for rule in &ty.do_rules {
            md.push_str(&format!("- Do: {rule}\n"));
        }
        for rule in &ty.dont_rules {
            md.push_str(&format!("- Don't: {rule}\n"));
        }
        if !ty.slots.is_empty() {
            let slots: Vec<String> = ty
                .slots
                .iter()
                .map(|s| format!("`{}` → {}", s.suffix, s.field))
                .collect();
            md.push_str(&format!("- Slots: {}\n", slots.join(", ")));
        }
        md.push('\n');
    }
    md.push_str("## Generation notes\n\n");
    md.push_str(
        "The page already is `Layout/Default` — fill the content area; do not emit \
         another chassis. Always instantiate kit masters with `type:\"ref\"` and \
         `descendants` on the slot ids above. Do not load generic \
         design-system-composition (sidebar 240–280, pad 32, `$color-accent`) — it \
         conflicts with this kit. Swap variants on an existing instance; do not \
         rebuild the control as a frame.\n",
    );
    md
}

/// Structured compact policy. Shared by `EditorState::starter` and hosts
/// that merge the library later.
pub fn skala_compact_design_md_spec() -> DesignMdSpec {
    parse_design_md(&skala_compact_design_md())
}

/// Attach the compact Skala policy when the document has none.
/// Existing custom `design_md` is left alone.
pub fn apply_skala_kit_policy(state: &mut EditorState) {
    if state.doc.design_md.is_none() {
        state.doc.design_md = Some(skala_compact_design_md_spec());
    }
}

/// Whether the document already carries the session kit sentinel.
pub fn document_has_kit_sentinel(state: &EditorState) -> bool {
    let sentinel = session_kit().sentinel_master_id.as_str();
    state
        .components
        .find_by_id(&crate::node_id::NodeId::new(sentinel))
        .is_some()
}

/// Whether the document already carries Skala masters (dedup sentinel).
pub fn document_has_skala_masters(state: &EditorState) -> bool {
    document_has_kit_sentinel(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skala_kit_json_parses_with_expected_types() {
        let kit = skala_kit();
        assert_eq!(kit.id, SKALA_KIT_ID);
        assert!(kit.type_by_id("button").is_some());
        assert!(kit.type_by_id("input").is_some());
        assert_eq!(
            kit.type_by_id("input").unwrap().default_master_id,
            "atom-input-default"
        );
        assert!(kit.type_by_id("input-icon").is_some());
        assert_eq!(
            kit.type_by_id("input-icon").unwrap().default_master_id,
            "atom-input-icon-trailing-default"
        );
        assert_eq!(kit.type_by_id("input-icon").unwrap().variant_count, 14);
        assert!(kit.type_by_id("pagination").is_some());
        assert_eq!(
            kit.type_by_id("pagination").unwrap().default_master_id,
            "molecule-pagination-default"
        );
        assert!(kit.type_by_id("pagination-item").is_some());
        assert!(kit.type_by_id("context-menu").is_some());
        assert_eq!(
            kit.type_by_id("context-menu").unwrap().default_master_id,
            "molecule-context-menu-base"
        );
        assert!(kit.type_by_id("context-menu-item").is_some());
        assert!(kit.type_by_id("chip").is_some());
        assert_eq!(
            kit.type_by_id("chip").unwrap().default_master_id,
            "atom-chip-default"
        );
        assert!(kit.type_by_id("badge-basic").is_some());
        assert_eq!(
            kit.type_by_id("badge-basic").unwrap().default_master_id,
            "atom-badge-basic-base"
        );
        assert!(kit.type_by_id("badge").is_some());
        assert_eq!(kit.type_by_id("badge").unwrap().variant_count, 90);
        assert!(kit.type_by_id("button-with-badge").is_some());
        assert_eq!(
            kit.type_by_id("button-with-badge")
                .unwrap()
                .default_master_id,
            "molecule-button-with-badge"
        );
        // The table type instantiates the container now: the cells and
        // headers it used to point at are its parts, not the whole thing.
        let table = kit.type_by_id("table").expect("table type");
        assert_eq!(table.default_master_id, "organism-table-base");
        assert_eq!(table.name_pattern, "Table/Default");
        assert_eq!(table.variant_count, 3);
        assert_eq!(
            table
                .slots
                .iter()
                .map(|slot| slot.suffix.as_str())
                .collect::<Vec<_>>(),
            vec!["-header", "-rows"]
        );
        assert!(kit.type_by_id("checkbox").is_some());
        assert_eq!(
            kit.type_by_id("checkbox").unwrap().default_master_id,
            "atom-checkbox-unchecked-default-text"
        );
        assert_eq!(kit.type_by_id("checkbox").unwrap().variant_count, 45);
        assert!(kit.type_by_id("layout").is_some());
        assert_eq!(kit.recipes[0].id, "ops-shell");
        assert_eq!(kit.content_slot_name(), "Main container");
        assert_eq!(kit.content_slot_id(), "tpl-layout-main");
        let area = kit.content_area_brief();
        assert!(area.contains("content area"));
        assert!(area.contains("Main container"));
        assert!(area.contains(&kit.sentinel_master_id));
        assert_eq!(kit.canvas.as_ref().map(|c| c.width), Some(1440.0));
        assert_eq!(
            kit.type_by_id("button").unwrap().default_master_id,
            "atom-button-filled-large-accent-default-text"
        );
    }

    #[test]
    fn compact_design_md_is_much_smaller_than_the_human_spec() {
        let md = skala_compact_design_md();
        assert!(md.contains("Skala Spectrum"));
        assert!(md.contains("ops-shell"));
        assert!(md.contains("tpl-layout-default"));
        assert!(md.contains("content area"));
        assert!(md.contains("Main container"));
        assert!(md.len() < 9_000, "compact policy drifted toward DESIGN.md");
    }

    #[test]
    fn applying_policy_is_idempotent_and_fills_empty_docs() {
        let mut state = EditorState::new();
        assert!(state.doc.design_md.is_none());
        apply_skala_kit_policy(&mut state);
        assert!(state.doc.design_md.is_some());
        let first = state.doc.design_md.clone();
        apply_skala_kit_policy(&mut state);
        assert_eq!(state.doc.design_md, first);
    }
}
