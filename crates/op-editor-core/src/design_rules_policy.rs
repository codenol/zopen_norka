//! Design-rules policy: the session's structured rules rendered as the
//! prompt block the AI reads.
//!
//! Rules are the only design-system input the AI gets. They are resolved
//! by the caller (library kit rules + document rules + overrides, with
//! disabled ones dropped), and this module turns them into the compact
//! prose block the prompts carry — global rules first, then the ones
//! scoped to a component type or master.

use jian_ops_schema::{DesignRule, DesignRuleKind, DesignRuleScope};

use crate::design_rules::EffectiveDesignRule;
use crate::design_rules_ui::AI_INSTRUCTION_RULE_ID;

/// One rule rendered as `- [Kind] Title: instruction (when …)`.
fn rule_line(rule: &DesignRule) -> String {
    let kind = match rule.kind {
        DesignRuleKind::Do => "Do",
        DesignRuleKind::Dont => "Don't",
        DesignRuleKind::Require => "Require",
        DesignRuleKind::Avoid => "Avoid",
    };
    let mut line = format!("- [{kind}] {}", rule.title.trim());
    let instruction = rule.instruction.trim();
    if !instruction.is_empty() {
        line.push_str(": ");
        line.push_str(instruction);
    }
    if let Some(condition) = rule.condition.as_deref().map(str::trim) {
        if !condition.is_empty() {
            line.push_str(&format!(" (when: {condition})"));
        }
    }
    line
}

/// How a rule is scoped, as the heading it belongs under.
fn scope_target(rule: &DesignRule) -> Option<String> {
    match &rule.scope {
        DesignRuleScope::Global => None,
        DesignRuleScope::ComponentType { type_id, .. } => Some(type_id.clone()),
        DesignRuleScope::ComponentMaster { component_id } => Some(component_id.clone()),
        DesignRuleScope::Recipe { recipe_id } => Some(recipe_id.clone()),
    }
}

/// Render every rule as one prompt block. Empty string when there are
/// none — callers then leave the rules section out entirely.
pub fn build_design_rules_policy(rules: &[DesignRule]) -> String {
    // The top instruction opens the block verbatim — it plays the AGENTS.md
    // role, so it is not folded into the bulleted rules below it. A document
    // that never saved one gets the shipped default from the resolver, so
    // this function stays a pure renderer.
    let instruction: Vec<&DesignRule> = rules
        .iter()
        .filter(|rule| rule.id == AI_INSTRUCTION_RULE_ID)
        .collect();
    let global: Vec<&DesignRule> = rules
        .iter()
        .filter(|rule| {
            matches!(rule.scope, DesignRuleScope::Global) && rule.id != AI_INSTRUCTION_RULE_ID
        })
        .collect();
    let recipes: Vec<&DesignRule> = rules
        .iter()
        .filter(|rule| matches!(rule.scope, DesignRuleScope::Recipe { .. }))
        .collect();
    let scoped: Vec<&DesignRule> = rules
        .iter()
        .filter(|rule| {
            !matches!(rule.scope, DesignRuleScope::Global)
                && !matches!(rule.scope, DesignRuleScope::Recipe { .. })
        })
        .collect();
    if instruction.is_empty() && global.is_empty() && recipes.is_empty() && scoped.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    if !instruction.is_empty() {
        out.push_str("WORKING AGREEMENT — follow this before anything else:\n");
        for rule in instruction {
            out.push_str(rule.instruction.trim());
            out.push('\n');
        }
    }
    if !global.is_empty() {
        out.push_str("GLOBAL RULES — apply to everything you produce:\n");
        for rule in global {
            out.push_str(&rule_line(rule));
            out.push('\n');
        }
    }
    if !recipes.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(
            "RECIPE RULES — a recipe is a ready-made screen: take it and adapt its \
             blocks instead of composing the screen from scratch.\n",
        );
        for rule in recipes {
            out.push_str(&rule_line(rule));
            out.push('\n');
        }
    }
    if !scoped.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("COMPONENT RULES — apply whenever you use the named component type:\n");
        for rule in scoped {
            match scope_target(rule) {
                Some(target) => out.push_str(&format!(
                    "{target} {}\n",
                    rule_line(rule).trim_start_matches("- ")
                )),
                None => out.push_str(&rule_line(rule)),
            }
        }
    }
    out.trim_end().to_string()
}

/// Whether the session carries any rule at all.
pub fn has_design_rules(rules: &[DesignRule]) -> bool {
    !rules.is_empty()
}

/// Render resolved rules — the shape the editor and MCP hold — as the
/// prompt block. Disabled rules are already dropped by the resolver.
pub fn build_effective_rules_policy(rules: &[EffectiveDesignRule]) -> String {
    let rules: Vec<DesignRule> = rules.iter().map(|entry| entry.rule.clone()).collect();
    build_design_rules_policy(&rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(
        id: &str,
        kind: DesignRuleKind,
        scope: DesignRuleScope,
        instruction: &str,
    ) -> DesignRule {
        DesignRule {
            id: id.into(),
            title: "Title".into(),
            instruction: instruction.into(),
            kind,
            scope,
            condition: None,
            priority: 0,
            enabled: true,
            overrides: None,
        }
    }

    #[test]
    fn empty_rules_produce_no_block() {
        assert_eq!(build_design_rules_policy(&[]), "");
        assert!(!has_design_rules(&[]));
    }

    #[test]
    fn global_and_component_rules_are_grouped_and_labelled() {
        let rules = vec![
            rule(
                "global:1",
                DesignRuleKind::Require,
                DesignRuleScope::Global,
                "Use 8px steps",
            ),
            rule(
                "kit:1",
                DesignRuleKind::Dont,
                DesignRuleScope::ComponentType {
                    kit_id: "skala-spectrum".into(),
                    type_id: "button".into(),
                },
                "No inline buttons",
            ),
        ];

        let policy = build_design_rules_policy(&rules);

        assert!(policy.contains("GLOBAL RULES"));
        assert!(policy.contains("[Require] Title: Use 8px steps"));
        assert!(policy.contains("COMPONENT RULES"));
        assert!(policy.contains("button [Don't] Title: No inline buttons"));
        // The instruction must never be re-targeted: the component section
        // labels the rule and the global section does not.
        assert!(policy.contains("GLOBAL RULES"));
        assert!(!policy.contains("Title [Require]"));
    }

    #[test]
    fn a_condition_is_carried_into_the_prompt() {
        let mut conditional = rule(
            "g",
            DesignRuleKind::Avoid,
            DesignRuleScope::Global,
            "No shadows",
        );
        conditional.condition = Some("dark theme".into());
        let policy = build_design_rules_policy(&[conditional]);
        assert!(policy.contains("(when: dark theme)"));
    }
}

/// Rules to send when the user pointed at a reference picture.
///
/// The recipe rules say "take this recipe for an ops list", and the model
/// follows them — which is how a "как на картинке" request still came back as
/// the ops screen even after the recipe stopped being placed. A reference turn
/// drops those rules; everything else (working agreement, component rules)
/// still applies.
///
/// `evidence` is the caller's knowledge of this turn's attachment (issue #65).
/// A caller that holds the attachment list passes the fact, and then `prompt`
/// is not read at all; `prompt` is consulted only for
/// [`ReferenceEvidence::Unknown`], where no attachment list exists to read and
/// the words are the only evidence left. Every kit recipe ships its own rule
/// (`effective_design_rules`), so `rules` is never free of recipe rules in
/// practice — this filter is load-bearing on every design turn, not a corner.
pub fn rules_without_recipes_for_reference(
    rules: &[DesignRule],
    prompt: &str,
    evidence: crate::design_rules::ReferenceEvidence,
) -> Vec<DesignRule> {
    let is_reference = crate::design_rules::is_reference_turn(prompt, evidence);
    if !is_reference {
        return rules.to_vec();
    }
    rules
        .iter()
        .filter(|rule| !matches!(rule.scope, DesignRuleScope::Recipe { .. }))
        .cloned()
        .collect()
}

#[cfg(test)]
mod reference_rule_tests {
    use super::*;

    #[test]
    fn a_reference_turn_drops_recipe_rules_only() {
        let rules = vec![
            DesignRule {
                id: "doc:ai-instructions".into(),
                title: "AI".into(),
                instruction: "keep".into(),
                kind: DesignRuleKind::Do,
                scope: DesignRuleScope::Global,
                condition: None,
                priority: 0,
                enabled: true,
                overrides: None,
            },
            DesignRule {
                id: "doc:recipe:x".into(),
                title: "Recipe".into(),
                instruction: "take it".into(),
                kind: DesignRuleKind::Require,
                scope: DesignRuleScope::Recipe {
                    recipe_id: "x".into(),
                },
                condition: None,
                priority: 0,
                enabled: true,
                overrides: None,
            },
        ];
        let kept = rules_without_recipes_for_reference(
            &rules,
            "сделай как на картинке",
            crate::design_rules::ReferenceEvidence::Unknown,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "doc:ai-instructions");
        assert_eq!(
            rules_without_recipes_for_reference(
                &rules,
                "список серверов",
                crate::design_rules::ReferenceEvidence::Unknown
            )
            .len(),
            2
        );
    }

    /// The fact, not the words: an attached picture drops the recipe rules
    /// even when the prompt never mentions a picture, and a turn whose
    /// attachment list is known to hold no picture keeps them even when the
    /// prompt does mention one (issue #65).
    #[test]
    fn the_attachment_list_decides_which_rules_travel() {
        let recipe_rule = DesignRule {
            id: "lib:recipe:ops".into(),
            title: "Ops".into(),
            instruction: "take the ops screen".into(),
            kind: DesignRuleKind::Require,
            scope: DesignRuleScope::Recipe {
                recipe_id: "ops-servers-screen".into(),
            },
            condition: None,
            priority: 0,
            enabled: true,
            overrides: None,
        };
        let rules = vec![recipe_rule];

        assert!(rules_without_recipes_for_reference(
            &rules,
            "сделай список серверов",
            crate::design_rules::ReferenceEvidence::Attached
        )
        .is_empty());
        assert_eq!(
            rules_without_recipes_for_reference(
                &rules,
                "сделай как на картинке",
                crate::design_rules::ReferenceEvidence::NoImage
            )
            .len(),
            1
        );
    }
}
