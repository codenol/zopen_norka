//! UI state for the guidelines panel's rules view.
//!
//! The structured rules themselves live in `jian_ops_schema`
//! (`DesignRule`) and are resolved by [`crate::design_rules`]. This
//! module owns only what the panel needs to *show* them: which view is
//! active, the filter chip, and the in-progress create/edit form.
//!
//! Same wasm32-clean discipline as the other `*_state` mirrors — no
//! widget types, no host types.

use jian_core::text_input::TextInputState;
use jian_ops_schema::{DesignMdSpec, DesignRule, DesignRuleKind, DesignRuleScope};

use crate::design_rules::{DesignRuleSource, EffectiveDesignRule};

/// The rules-view filter chips, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesignRulesFilter {
    /// Every effective rule.
    #[default]
    All,
    /// Rules scoped to the whole document.
    Global,
    /// Rules scoped to a component type or master.
    Components,
    /// Document-local rules and overrides (never library rules).
    Local,
}

impl DesignRulesFilter {
    /// All four chips in display order.
    pub const ALL: [DesignRulesFilter; 4] = [
        DesignRulesFilter::All,
        DesignRulesFilter::Global,
        DesignRulesFilter::Components,
        DesignRulesFilter::Local,
    ];

    /// `designMd` i18n key for the chip label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            DesignRulesFilter::All => "designMd.rules.filter.all",
            DesignRulesFilter::Global => "designMd.rules.filter.global",
            DesignRulesFilter::Components => "designMd.rules.filter.components",
            DesignRulesFilter::Local => "designMd.rules.filter.local",
        }
    }

    /// Position of the chip in the filter row.
    pub fn index(self) -> u8 {
        match self {
            DesignRulesFilter::All => 0,
            DesignRulesFilter::Global => 1,
            DesignRulesFilter::Components => 2,
            DesignRulesFilter::Local => 3,
        }
    }

    /// Inverse of [`Self::index`] — out-of-range falls back to `All`.
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => DesignRulesFilter::Global,
            2 => DesignRulesFilter::Components,
            3 => DesignRulesFilter::Local,
            _ => DesignRulesFilter::All,
        }
    }

    /// Whether an effective rule passes this filter.
    pub fn matches(self, entry: &EffectiveDesignRule) -> bool {
        match self {
            DesignRulesFilter::All => true,
            DesignRulesFilter::Global => matches!(entry.rule.scope, DesignRuleScope::Global),
            DesignRulesFilter::Components => matches!(
                entry.rule.scope,
                DesignRuleScope::ComponentType { .. } | DesignRuleScope::ComponentMaster { .. }
            ),
            DesignRulesFilter::Local => !matches!(entry.source, DesignRuleSource::Library { .. }),
        }
    }
}

/// Render one rule the way the editor's markdown field shows it.
///
/// The title is a markdown heading and everything after the first blank
/// line is the instruction body, so the field reads like the rule does in
/// a `design.md` projection while staying editable as plain text.
pub fn rule_markdown(title: &str, instruction: &str) -> String {
    format!("## {title}\n\n{instruction}")
}

/// Split the editor's markdown back into `(title, instruction)`.
///
/// `None` when either half is empty — the form treats that as invalid and
/// refuses to write a half-filled rule into the document.
pub fn parse_rule_markdown(markdown: &str) -> Option<(String, String)> {
    let trimmed = markdown.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut lines = trimmed.lines();
    let first = lines.next().unwrap_or_default().trim();
    let title = first.trim_start_matches('#').trim();
    if title.is_empty() {
        return None;
    }
    let instruction = lines.collect::<Vec<_>>().join("\n");
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return None;
    }
    Some((title.to_string(), instruction.to_string()))
}

/// Which editor field owns the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignRuleFocus {
    /// The rule's title (author rules only).
    Title,
    /// The document body.
    Body,
}

/// The open document in the rules editor.
///
/// One markdown body per rule: a component document's body is the whole
/// content, and its title is the component's own name (never authored
/// here), while an author rule keeps its title alongside the body.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignRuleDraft {
    /// `Some(rule_id)` when editing a stored rule, `None` for a new one.
    pub editing_id: Option<String>,
    /// The id the saved rule is written under.
    pub id: String,
    /// The rule's title — the component name, or the author's own label.
    pub title_input: TextInputState,
    /// Whether the title may be edited (author rules only).
    pub title_editable: bool,
    /// Library rule this draft overrides, when editing one locally.
    pub overrides: Option<String>,
    /// The document body.
    pub body: TextInputState,
    pub kind: DesignRuleKind,
    pub scope: DesignRuleScope,
    /// Which field owns the keyboard.
    pub focus: DesignRuleFocus,
}

impl DesignRuleDraft {
    /// A blank author rule.
    pub fn new_local(id: String) -> Self {
        Self {
            editing_id: None,
            id,
            title_input: TextInputState::default(),
            title_editable: true,
            overrides: None,
            body: TextInputState::default(),
            kind: DesignRuleKind::Do,
            scope: DesignRuleScope::Global,
            focus: DesignRuleFocus::Body,
        }
    }

    /// The editor state for one panel row — a component document or an
    /// author rule.
    pub fn from_row(row: &PanelRow) -> Self {
        Self {
            editing_id: row.is_component.then(|| row.rule_id.clone()),
            id: row.rule_id.clone(),
            title_input: TextInputState::with_text(row.title.clone()),
            title_editable: !row.is_component && !row.is_recipe && !row.is_primary,
            overrides: None,
            body: TextInputState::with_text(row.body.clone()),
            kind: DesignRuleKind::Do,
            scope: row.scope.clone(),
            focus: DesignRuleFocus::Body,
        }
    }

    /// The document body, trimmed to `None` when blank.
    pub fn body_text(&self) -> Option<String> {
        let body = self.body.text().trim();
        (!body.is_empty()).then(|| body.to_string())
    }

    /// Whether the draft is complete enough to save: a component
    /// document needs a body, an author rule needs a title too.
    pub fn is_valid(&self) -> bool {
        self.body_text().is_some() && (!self.title_editable || !self.title_text().is_empty())
    }

    /// The rule's title, trimmed.
    pub fn title_text(&self) -> &str {
        self.title_input.text().trim()
    }

    /// The text input the keyboard is currently editing.
    pub fn focused_input(&mut self) -> &mut TextInputState {
        match self.focus {
            DesignRuleFocus::Title => &mut self.title_input,
            DesignRuleFocus::Body => &mut self.body,
        }
    }

    /// Build the rule this draft describes.
    ///
    /// Returns `None` while the form is still invalid, so the caller
    /// never writes a half-filled rule into the document.
    pub fn to_rule(&self) -> Option<DesignRule> {
        if !self.is_valid() {
            return None;
        }
        Some(DesignRule {
            id: self.id.clone(),
            title: self.title_text().to_string(),
            instruction: self.body_text()?,
            kind: self.kind,
            scope: self.scope.clone(),
            condition: None,
            priority: 0,
            enabled: true,
            overrides: self.overrides.clone(),
        })
    }
}

/// One component's rules document.
///
/// The product model is one document per component: the kit's `do` / `dont`
/// entries are its default body, and saving writes that body as a single
/// document-scoped rule. The name is the component's — it is not an
/// editable label, which is why the row carries the type id rather than a
/// free-form title.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDoc {
    pub type_id: String,
    /// Display name — the component's own name, never authored here.
    pub name: String,
    /// The rule id a save writes to.
    pub rule_id: String,
    /// Markdown body: the saved document, or the kit's rules when none was saved.
    pub body: String,
    /// Whether the body is already stored in the document.
    pub saved: bool,
    /// Whether the document's rule is switched on.
    pub enabled: bool,
}

/// Stable rule id of one recipe's rules document.
pub fn recipe_document_id(recipe_id: &str) -> String {
    format!("doc:recipe:{recipe_id}")
}

/// Stable rule id of one component's document.
pub fn component_document_id(kit_id: &str, type_id: &str) -> String {
    format!("doc:component:{kit_id}:{type_id}")
}

/// The default body of a component document — the kit's own rules.
pub fn default_component_body(ty: &crate::KitType) -> String {
    let mut body = String::new();
    if !ty.do_rules.is_empty() {
        body.push_str("Do:\n");
        for rule in &ty.do_rules {
            body.push_str(&format!("- {rule}\n"));
        }
    }
    if !ty.dont_rules.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str("Avoid:\n");
        for rule in &ty.dont_rules {
            body.push_str(&format!("- {rule}\n"));
        }
    }
    body.trim_end().to_string()
}

/// Every component the active kit ships, as one document each.
pub fn component_documents(
    kit: &crate::KitManifest,
    spec: Option<&DesignMdSpec>,
) -> Vec<ComponentDoc> {
    kit.types
        .iter()
        .map(|ty| {
            let rule_id = component_document_id(&kit.id, &ty.id);
            let saved_rule = spec.and_then(|spec| {
                spec.rules.iter().find(|rule| {
                    rule.id == rule_id
                        || matches!(
                            &rule.scope,
                            DesignRuleScope::ComponentType { kit_id, type_id }
                                if *kit_id == kit.id && *type_id == ty.id
                        )
                })
            });
            match saved_rule {
                Some(rule) => ComponentDoc {
                    type_id: ty.id.clone(),
                    name: ty.name.clone(),
                    rule_id: rule.id.clone(),
                    body: rule.instruction.clone(),
                    saved: true,
                    enabled: rule.enabled,
                },
                None => ComponentDoc {
                    type_id: ty.id.clone(),
                    name: ty.name.clone(),
                    rule_id,
                    body: default_component_body(ty),
                    saved: false,
                    enabled: true,
                },
            }
        })
        .collect()
}

/// Rules that are not component documents — the ones the author created.
pub fn author_rules(spec: Option<&DesignMdSpec>) -> Vec<DesignRule> {
    spec.map(|spec| {
        spec.rules
            .iter()
            .filter(|rule| {
                !matches!(rule.scope, DesignRuleScope::ComponentType { .. })
                    && !rule.id.starts_with("doc:component:")
                    && !rule.id.starts_with("doc:recipe:")
                    && rule.id != AI_INSTRUCTION_RULE_ID
            })
            .cloned()
            .collect()
    })
    .unwrap_or_default()
}

/// Stable rule id of the top "how to work here" document — the rule that
/// plays the AGENTS.md role for the AI.
pub const AI_INSTRUCTION_RULE_ID: &str = "doc:ai-instructions";

/// Default body of that document, used until the author saves their own.
///
/// Deliberately short and imperative: it is the first thing the model
/// reads, and it exists to set the order of work — instruction first,
/// component rules next, existing recipes before designing from scratch.
pub const DEFAULT_AI_INSTRUCTIONS: &str = "How to work with this design system:\n\
- Follow this document first; it outranks every other rule below it.\n\
- Read the rules of a component before you use it, and obey them.\n\
- Prefer components from the session kit; do not invent a component the kit already ships.\n\
- When a saved recipe matches the request, start from that recipe and adapt its blocks instead of designing the screen from scratch.\n\
- Keep one visual idea per screen; do not repeat the same generic stack of sections.";

/// The shipped default as a real rule, for the prompt paths.
///
/// The panel shows this body until the author saves their own; the prompt has
/// to do the same, or a brand-new document would reach the model with the
/// component rules but no working agreement above them.
pub fn default_ai_instruction_rule() -> DesignRule {
    DesignRule {
        id: AI_INSTRUCTION_RULE_ID.to_string(),
        title: "AI instructions".to_string(),
        instruction: DEFAULT_AI_INSTRUCTIONS.to_string(),
        kind: DesignRuleKind::Do,
        scope: DesignRuleScope::Global,
        condition: None,
        priority: i32::MIN,
        enabled: true,
        overrides: None,
    }
}

/// One row of the rules panel: the top instruction, a component document,
/// or an author rule.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelRow {
    /// What the row shows — a component name, or an author rule's title.
    pub label: String,
    /// The rule id a save writes to.
    pub rule_id: String,
    /// Rule title stored on save (the component name, or the author's title).
    pub title: String,
    /// The document body the editor opens.
    pub body: String,
    /// Scope a save writes.
    pub scope: DesignRuleScope,
    /// Whether the row can be switched off / on.
    pub enabled: bool,
    /// Whether the row can be deleted. Component documents never can.
    pub removable: bool,
    /// Whether the row is one of the kit's component documents.
    pub is_component: bool,
    /// Whether the row is a shipped recipe's rules document.
    pub is_recipe: bool,
    /// Whether the body is already stored in the document.
    pub saved: bool,
    /// Whether the row is the top AI instruction — always first, never
    /// renamed or deleted.
    pub is_primary: bool,
}

/// The panel's rows: the AI instruction first, then one document per
/// component, then the author's rules.
pub fn panel_rows(kit: &crate::KitManifest, spec: Option<&DesignMdSpec>) -> Vec<PanelRow> {
    let stored_instruction = spec.and_then(|spec| {
        spec.rules
            .iter()
            .find(|rule| rule.id == AI_INSTRUCTION_RULE_ID)
    });
    let mut rows: Vec<PanelRow> = vec![PanelRow {
        label: "AI instructions".to_string(),
        rule_id: AI_INSTRUCTION_RULE_ID.to_string(),
        title: "AI instructions".to_string(),
        body: stored_instruction
            .map(|rule| rule.instruction.clone())
            .unwrap_or_else(|| DEFAULT_AI_INSTRUCTIONS.to_string()),
        scope: DesignRuleScope::Global,
        enabled: stored_instruction.is_none_or(|rule| rule.enabled),
        removable: false,
        is_component: false,
        is_recipe: false,
        saved: stored_instruction.is_some(),
        is_primary: true,
    }];
    rows.extend(component_documents(kit, spec)
        .into_iter()
        .map(|doc| PanelRow {
            label: doc.name.clone(),
            rule_id: doc.rule_id,
            title: doc.name,
            body: doc.body,
            scope: DesignRuleScope::ComponentType {
                kit_id: kit.id.clone(),
                type_id: doc.type_id,
            },
            enabled: doc.enabled,
            removable: false,
            is_component: true,
            is_recipe: false,
            saved: doc.saved,
            is_primary: false,
        })
        .collect::<Vec<PanelRow>>());
    rows.extend(kit.recipes.iter().map(|recipe| {
        let rule_id = recipe_document_id(&recipe.id);
        let stored = spec.and_then(|spec| spec.rules.iter().find(|rule| rule.id == rule_id));
        PanelRow {
            label: recipe.name.clone(),
            rule_id,
            title: recipe.name.clone(),
            body: stored
                .map(|rule| rule.instruction.clone())
                .unwrap_or_else(|| recipe.notes.clone()),
            scope: DesignRuleScope::Recipe {
                recipe_id: recipe.id.clone(),
            },
            enabled: stored.is_none_or(|rule| rule.enabled),
            removable: false,
            is_component: false,
            is_recipe: true,
            saved: stored.is_some(),
            is_primary: false,
        }
    }));
    rows.extend(author_rules(spec).into_iter().map(|rule| PanelRow {
        label: rule.title.clone(),
        rule_id: rule.id.clone(),
        title: rule.title.clone(),
        body: rule.instruction.clone(),
        scope: rule.scope.clone(),
        enabled: rule.enabled,
        removable: true,
        is_component: false,
        is_recipe: false,
        saved: true,
        is_primary: false,
    }));
    rows
}

/// Whether `entry` matches `filter` — free-function form for callers
/// that hold no filter state.
pub fn rule_matches_filter(filter: DesignRulesFilter, entry: &EffectiveDesignRule) -> bool {
    filter.matches(entry)
}

/// The rules the panel shows for `spec` under `filter`.
///
/// Unlike [`crate::effective_design_rules`] this keeps locally disabled
/// rules (so they can be switched back on) and drops nothing to the
/// prompt path — the panel is the only caller.
pub fn visible_rules(
    kit: &crate::KitManifest,
    spec: Option<&jian_ops_schema::DesignMdSpec>,
    filter: DesignRulesFilter,
) -> Vec<EffectiveDesignRule> {
    crate::design_rules::resolved_design_rules_for_kit(kit, spec, true)
        .into_iter()
        .filter(|entry| filter.matches(entry))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_rules::library_design_rules;

    fn local_entry(id: &str, scope: DesignRuleScope) -> EffectiveDesignRule {
        EffectiveDesignRule {
            rule: DesignRule {
                id: id.into(),
                title: "Local".into(),
                instruction: "Instruction".into(),
                kind: DesignRuleKind::Require,
                scope,
                condition: None,
                priority: 0,
                enabled: true,
                overrides: None,
            },
            source: DesignRuleSource::Document,
        }
    }

    #[test]
    fn filter_chips_partition_global_and_component_rules() {
        let global = local_entry("local:a", DesignRuleScope::Global);
        let component = local_entry(
            "local:b",
            DesignRuleScope::ComponentType {
                kit_id: "skala-spectrum".into(),
                type_id: "button".into(),
            },
        );
        assert!(DesignRulesFilter::All.matches(&global));
        assert!(DesignRulesFilter::All.matches(&component));
        assert!(DesignRulesFilter::Global.matches(&global));
        assert!(!DesignRulesFilter::Global.matches(&component));
        assert!(DesignRulesFilter::Components.matches(&component));
        assert!(!DesignRulesFilter::Components.matches(&global));
        assert!(DesignRulesFilter::Local.matches(&global));
    }

    #[test]
    fn local_filter_excludes_library_rules() {
        let library = library_design_rules(crate::session_kit())
            .into_iter()
            .next()
            .unwrap();
        assert!(DesignRulesFilter::All.matches(&library));
        assert!(!DesignRulesFilter::Local.matches(&library));
        assert!(DesignRulesFilter::Components.matches(&library));
    }

    #[test]
    fn opening_a_component_document_keeps_its_name_fixed() {
        let kit = crate::session_kit();
        let rows = panel_rows(kit, None);
        let component = rows
            .iter()
            .find(|row| row.is_component)
            .expect("the kit ships components");
        let draft = DesignRuleDraft::from_row(component);

        assert!(!draft.title_editable, "a component's name is not authored");
        assert_eq!(draft.title_text(), component.label);
        assert!(!draft.body.text().is_empty());
        assert!(draft.is_valid());
        let rule = draft.to_rule().unwrap();
        assert_eq!(rule.title, component.label);
        assert_eq!(rule.instruction, component.body);
    }

    #[test]
    fn blank_documents_block_saving() {
        let mut draft = DesignRuleDraft::new_local("local:new".into());
        assert!(!draft.is_valid(), "an author rule needs a title and a body");
        assert!(draft.to_rule().is_none());
        draft.title_input.set_text("  Spacing  ");
        draft.body.set_text("Use 8px steps");
        assert!(draft.is_valid());
        let rule = draft.to_rule().unwrap();
        assert_eq!(rule.title, "Spacing");
        assert_eq!(rule.instruction, "Use 8px steps");

        // A component document has a fixed title, so only its body gates it.
        let kit = crate::session_kit();
        let rows = panel_rows(kit, None);
        let component = rows.iter().find(|row| row.is_component).unwrap();
        let mut component_draft = DesignRuleDraft::from_row(component);
        component_draft.body.set_text("   ");
        assert!(!component_draft.is_valid());
    }

    #[test]
    fn markdown_round_trips_title_and_instruction() {
        let markdown = rule_markdown("Button usage", "Use the shared button\nNever inline one");
        assert_eq!(
            parse_rule_markdown(&markdown),
            Some((
                "Button usage".to_string(),
                "Use the shared button\nNever inline one".to_string()
            ))
        );
        // A bare title-less body still parses: the first line is the title.
        assert_eq!(
            parse_rule_markdown("Spacing\n\nUse 8px steps"),
            Some(("Spacing".to_string(), "Use 8px steps".to_string()))
        );
        assert_eq!(parse_rule_markdown("   "), None);
        assert_eq!(parse_rule_markdown("## Only a title"), None);
    }

    #[test]
    fn filter_indices_round_trip() {
        for filter in DesignRulesFilter::ALL {
            assert_eq!(DesignRulesFilter::from_index(filter.index()), filter);
        }
    }
}
