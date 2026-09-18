//! Structured design-rule resolution and document-local overrides.

use std::collections::BTreeMap;

use jian_ops_schema::{DesignMdSpec, DesignRule, DesignRuleKind, DesignRuleScope};

use crate::{session_kit, KitManifest};

/// Where an effective rule came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignRuleSource {
    Library { kit_id: String },
    Document,
    DocumentOverride { overridden_rule_id: String },
}

/// A resolved rule paired with provenance for UI and MCP consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveDesignRule {
    pub rule: DesignRule,
    pub source: DesignRuleSource,
}

/// Convert the active kit's legacy `do` / `dont` entries into stable rules.
pub fn library_design_rules(kit: &KitManifest) -> Vec<EffectiveDesignRule> {
    let mut rules = Vec::new();
    for ty in &kit.types {
        for (index, instruction) in ty.do_rules.iter().enumerate() {
            rules.push(kit_rule(
                kit,
                &ty.id,
                &ty.name,
                DesignRuleKind::Do,
                "do",
                index,
                instruction,
            ));
        }
        for (index, instruction) in ty.dont_rules.iter().enumerate() {
            rules.push(kit_rule(
                kit,
                &ty.id,
                &ty.name,
                DesignRuleKind::Dont,
                "dont",
                index,
                instruction,
            ));
        }
    }
    rules
}

fn kit_rule(
    kit: &KitManifest,
    type_id: &str,
    type_name: &str,
    kind: DesignRuleKind,
    kind_id: &str,
    index: usize,
    instruction: &str,
) -> EffectiveDesignRule {
    EffectiveDesignRule {
        rule: DesignRule {
            id: format!("kit:{}:{type_id}:{kind_id}:{index}", kit.id),
            title: type_name.to_string(),
            instruction: instruction.to_string(),
            kind,
            scope: DesignRuleScope::ComponentType {
                kit_id: kit.id.clone(),
                type_id: type_id.to_string(),
            },
            condition: None,
            priority: 0,
            enabled: true,
            overrides: None,
        },
        source: DesignRuleSource::Library {
            kit_id: kit.id.clone(),
        },
    }
}

/// Resolve active-kit rules with document-local rules and overrides.
///
/// Disabled rules are dropped — this is the prompt / MCP view.
pub fn effective_design_rules(spec: Option<&DesignMdSpec>) -> Vec<EffectiveDesignRule> {
    effective_design_rules_for_kit(session_kit(), spec)
}

pub fn effective_design_rules_for_kit(
    kit: &KitManifest,
    spec: Option<&DesignMdSpec>,
) -> Vec<EffectiveDesignRule> {
    resolved_design_rules_for_kit(kit, spec, false)
}

/// Resolve active-kit rules for the given spec.
///
/// `include_disabled` keeps locally disabled rules in the result so the
/// guidelines panel can show — and re-enable — them. Prompt builders and
/// MCP consumers pass `false` via [`effective_design_rules`], which is
/// the same set they have always received.
pub fn resolved_design_rules(
    spec: Option<&DesignMdSpec>,
    include_disabled: bool,
) -> Vec<EffectiveDesignRule> {
    resolved_design_rules_for_kit(session_kit(), spec, include_disabled)
}

pub fn resolved_design_rules_for_kit(
    kit: &KitManifest,
    spec: Option<&DesignMdSpec>,
    include_disabled: bool,
) -> Vec<EffectiveDesignRule> {
    let mut resolved: BTreeMap<String, EffectiveDesignRule> = library_design_rules(kit)
        .into_iter()
        .map(|entry| (entry.rule.id.clone(), entry))
        .collect();

    // Every shipped recipe carries its own rule: when to take it and what a
    // screen built from it may change. It is a library rule like the
    // component ones, so the AI reads it without the author having to write
    // anything, and a saved document rule of the same id wins below.
    for recipe in &kit.recipes {
        let id = crate::design_rules_ui::recipe_document_id(&recipe.id);
        let entry = EffectiveDesignRule {
            rule: DesignRule {
                id: id.clone(),
                title: recipe.name.clone(),
                instruction: recipe.notes.clone(),
                kind: DesignRuleKind::Require,
                scope: jian_ops_schema::DesignRuleScope::Recipe {
                    recipe_id: recipe.id.clone(),
                },
                condition: None,
                priority: 0,
                enabled: true,
                overrides: None,
            },
            source: DesignRuleSource::Library {
                kit_id: kit.id.clone(),
            },
        };
        resolved.entry(id).or_insert(entry);
    }

    if let Some(spec) = spec {
        for rule in &spec.rules {
            let source = if let Some(overridden_rule_id) = &rule.overrides {
                resolved.remove(overridden_rule_id);
                DesignRuleSource::DocumentOverride {
                    overridden_rule_id: overridden_rule_id.clone(),
                }
            } else {
                DesignRuleSource::Document
            };
            resolved.insert(
                rule.id.clone(),
                EffectiveDesignRule {
                    rule: rule.clone(),
                    source,
                },
            );
        }
    }

    // A document that has never saved the top instruction still gets the
    // shipped one: the rules panel shows it from the start, and the prompt
    // must agree with the panel. A document that saved it — enabled or not —
    // keeps whatever the author decided, so switching it off still works.
    let instruction_saved = spec.is_some_and(|spec| {
        spec.rules
            .iter()
            .any(|rule| rule.id == crate::design_rules_ui::AI_INSTRUCTION_RULE_ID)
    });
    if !instruction_saved {
        let rule = crate::design_rules_ui::default_ai_instruction_rule();
        resolved
            .entry(rule.id.clone())
            .or_insert(EffectiveDesignRule {
                rule,
                source: DesignRuleSource::Library {
                    kit_id: kit.id.clone(),
                },
            });
    }

    let mut rules: Vec<_> = resolved
        .into_values()
        .filter(|entry| include_disabled || entry.rule.enabled)
        .collect();
    // Deterministic order: priority desc, then the rule's *base* id.
    // Sorting by the stored id would make a row jump elsewhere the moment
    // it is toggled — a library override carries a fresh `override:…` id
    // while still describing the very same rule.
    let base_id = |rule: &DesignRule| rule.overrides.clone().unwrap_or_else(|| rule.id.clone());
    rules.sort_by(|left, right| {
        right
            .rule
            .priority
            .cmp(&left.rule.priority)
            .then_with(|| base_id(&left.rule).cmp(&base_id(&right.rule)))
    });
    rules
}

/// Insert a new local rule or replace an existing local rule with the same id.
pub fn upsert_document_rule(spec: &mut DesignMdSpec, rule: DesignRule) {
    if let Some(existing) = spec.rules.iter_mut().find(|item| item.id == rule.id) {
        *existing = rule;
    } else {
        spec.rules.push(rule);
    }
}

/// Toggle a local rule, or create a document override for a library rule.
pub fn set_rule_enabled(spec: &mut DesignMdSpec, rule_id: &str, enabled: bool) -> bool {
    if let Some(rule) = spec
        .rules
        .iter_mut()
        .find(|item| item.id == rule_id || item.overrides.as_deref() == Some(rule_id))
    {
        if rule.enabled == enabled {
            return false;
        }
        rule.enabled = enabled;
        return true;
    }
    let Some(base) = library_design_rules(session_kit())
        .into_iter()
        .find(|entry| entry.rule.id == rule_id)
    else {
        return false;
    };
    if base.rule.enabled == enabled {
        return false;
    }
    let mut rule = base.rule;
    rule.id = override_id(rule_id);
    rule.enabled = enabled;
    rule.overrides = Some(rule_id.to_string());
    spec.rules.push(rule);
    true
}

/// Delete a local rule, or locally disable a library rule.
pub fn delete_rule(spec: &mut DesignMdSpec, rule_id: &str) -> bool {
    if let Some(index) = spec.rules.iter().position(|item| item.id == rule_id) {
        spec.rules.remove(index);
        return true;
    }
    set_rule_enabled(spec, rule_id, false)
}

fn override_id(rule_id: &str) -> String {
    format!("override:{rule_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_spec() -> DesignMdSpec {
        crate::parse_design_md("")
    }

    #[test]
    fn kit_rules_have_deterministic_ids_and_component_scope() {
        let first = library_design_rules(session_kit());
        let second = library_design_rules(session_kit());
        assert!(!first.is_empty());
        assert_eq!(first, second);
        assert!(first[0].rule.id.starts_with("kit:skala-spectrum:"));
        assert!(matches!(
            first[0].rule.scope,
            DesignRuleScope::ComponentType { .. }
        ));
    }

    #[test]
    fn document_override_replaces_library_rule() {
        let base = library_design_rules(session_kit())
            .into_iter()
            .next()
            .unwrap();
        let mut spec = empty_spec();
        let mut replacement = base.rule.clone();
        replacement.id = "local:replacement".into();
        replacement.instruction = "Replacement".into();
        replacement.overrides = Some(base.rule.id.clone());
        upsert_document_rule(&mut spec, replacement.clone());

        let effective = effective_design_rules(Some(&spec));
        assert!(!effective.iter().any(|entry| entry.rule.id == base.rule.id));
        assert!(effective.iter().any(|entry| entry.rule == replacement));
    }

    #[test]
    fn deleting_library_rule_creates_disabled_override() {
        let base = library_design_rules(session_kit())
            .into_iter()
            .next()
            .unwrap();
        let mut spec = empty_spec();
        assert!(delete_rule(&mut spec, &base.rule.id));
        assert_eq!(spec.rules.len(), 1);
        assert!(!spec.rules[0].enabled);
        assert_eq!(
            spec.rules[0].overrides.as_deref(),
            Some(base.rule.id.as_str())
        );
        assert!(!effective_design_rules(Some(&spec))
            .iter()
            .any(|entry| entry.rule.id == base.rule.id));
    }

    #[test]
    fn toggling_a_library_rule_keeps_its_position_in_the_list() {
        let mut spec = empty_spec();
        let before = resolved_design_rules(Some(&spec), true);
        let target = before[3].rule.id.clone();
        let before_index = before
            .iter()
            .position(|entry| entry.rule.id == target)
            .expect("the rule is listed");

        assert!(set_rule_enabled(&mut spec, &target, false));

        let after = resolved_design_rules(Some(&spec), true);
        let after_index = after
            .iter()
            .position(|entry| entry.rule.overrides.as_deref() == Some(target.as_str()))
            .expect("the disabled override replaces it");
        assert_eq!(
            before_index, after_index,
            "switching a rule off must not move its row"
        );
    }

    #[test]
    fn editor_commands_mutate_rules_and_join_history() {
        let mut state = crate::EditorState::new();
        state.doc.design_md = Some(empty_spec());
        let rule = DesignRule {
            id: "local:button-usage".into(),
            title: "Button usage".into(),
            instruction: "Use the shared button".into(),
            kind: DesignRuleKind::Require,
            scope: DesignRuleScope::Global,
            condition: None,
            priority: 10,
            enabled: true,
            overrides: None,
        };
        state.commit_history();
        assert!(state.apply(crate::EditorCommand::UpsertDesignRule {
            rule: Box::new(rule.clone()),
        }));
        assert_eq!(state.doc.design_md.as_ref().unwrap().rules, vec![rule]);
        assert!(state.history.can_undo());

        assert!(state.apply(crate::EditorCommand::SetDesignRuleEnabled {
            rule_id: "local:button-usage".into(),
            enabled: false,
        }));
        assert!(!state.doc.design_md.as_ref().unwrap().rules[0].enabled);

        assert!(state.apply(crate::EditorCommand::DeleteDesignRule {
            rule_id: "local:button-usage".into(),
        }));
        assert!(state.doc.design_md.as_ref().unwrap().rules.is_empty());
    }
}

/// The recipe a request is asking for, by the words the kit declares.
///
/// Selection lives here, not in the prompt, for the same reason the rules do:
/// "the model should pick the recipe" is a hope, while a keyword match is a
/// decision. It is a decision the product has to be able to defend, though —
/// a placed recipe becomes the page and the turn becomes a rewrite of it — so
/// the matching itself lives in [`crate::design_recipe_match`], where a needle
/// has to land on a word the request actually uses, at least one needle has to
/// name the recipe's own subject, and the request may not contradict the
/// recipe's shape or the script its copy is written in (issues #181, #187).
/// A request that names nothing keeps the plain, model-drawn route.
pub fn select_recipe<'a>(
    prompt: &str,
    kit: &'a crate::KitManifest,
) -> Option<&'a crate::kit_manifest::KitRecipe> {
    crate::design_recipe_match::recipe_decision(prompt, kit).recipe()
}

/// The recipe blocks a request asks to be left out.
///
/// The kit declares them (`KitRecipe::optional`); this only matches the
/// user's words against each block's phrases. Empty means the request asked
/// for nothing specific, and the recipe lands whole.
pub fn requested_hidden_blocks(
    prompt: &str,
    recipe: &crate::kit_manifest::KitRecipe,
) -> Vec<String> {
    let haystack = prompt.to_lowercase();
    let chars: Vec<char> = haystack.chars().collect();
    recipe
        .optional
        .iter()
        .filter(|optional| {
            optional.subjects.iter().any(|subject| {
                let subject: Vec<char> = subject.to_lowercase().chars().collect();
                if subject.is_empty() || subject.len() > chars.len() {
                    return false;
                }
                // Every place the subject appears; a negation near any of them
                // means this block is the one being dismissed. All indexing
                // stays in CHARS: a byte offset into `haystack` is not an
                // index into `chars`, and in a Cyrillic request the two
                // diverge two-to-one — mixing them panicked the recipe route
                // on any prompt long enough (measured: «поиск» at byte 781
                // of a 486-char request, range end out of bounds).
                let mut from = 0usize;
                while from + subject.len() <= chars.len() {
                    let Some(at) = find_char_subslice(&chars, &subject, from) else {
                        break;
                    };
                    from = at + subject.len();
                    let start = at.saturating_sub(NEGATION_WINDOW);
                    let end = (at + subject.len() + NEGATION_WINDOW).min(chars.len());
                    let window: String = chars[start..end].iter().collect();
                    if optional
                        .negations
                        .iter()
                        .any(|negation| window.contains(&negation.to_lowercase()))
                    {
                        return true;
                    }
                }
                false
            })
        })
        .map(|optional| optional.block.clone())
        .collect()
}

/// First position at or after `from` where `needle` occurs in `haystack`,
/// both already lowercase char vectors.
fn find_char_subslice(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
    (from..=haystack.len().saturating_sub(needle.len()))
        .find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// How far from a subject word a negation still counts as "this one".
/// Wide enough for "фильтр и пагинация не нужны" (the negation trails both
/// subjects) and narrow enough that an unrelated "no" elsewhere in a long
/// sentence does not reach it.
const NEGATION_WINDOW: usize = 40;

/// Hide every node named in `blocks` inside `root`'s subtree.
///
/// Returns how many nodes were hidden. Hidden rather than deleted on purpose:
/// the recipe's blocks are the author's, the request only said it does not
/// want them now, and a hidden block is one click from coming back.
pub fn hide_blocks_in_subtree(
    state: &mut crate::EditorState,
    root: &crate::NodeId,
    blocks: &[String],
) -> usize {
    if blocks.is_empty() {
        return 0;
    }
    let ids: Vec<crate::NodeId> = {
        let Some(node) = crate::walkers::find_node(state.active_children(), root) else {
            return 0;
        };
        let mut found = Vec::new();
        collect_named(node, blocks, &mut found);
        found
    };
    let mut hidden = 0;
    for id in ids {
        if state.toggle_node_hidden_if_visible(&id) {
            hidden += 1;
        }
    }
    if hidden > 0 {
        state.mark_document_changed();
    }
    hidden
}

fn collect_named(
    node: &jian_ops_schema::node::PenNode,
    blocks: &[String],
    out: &mut Vec<crate::NodeId>,
) {
    use crate::PenNodeExt as _;
    if node
        .base()
        .name
        .as_deref()
        .is_some_and(|name| blocks.iter().any(|block| block == name))
    {
        out.push(crate::NodeId::new(node.id_str().to_string()));
    }
    for child in node.children().into_iter().flatten() {
        collect_named(child, blocks, out);
    }
}

#[cfg(test)]
mod optional_block_tests {
    use super::*;

    #[test]
    fn a_request_without_a_filter_names_the_toolbar() {
        let kit = crate::session_kit();
        let recipe = kit
            .recipes
            .iter()
            .find(|r| r.id == "ops-servers-screen")
            .expect("the ops recipe");
        assert_eq!(
            requested_hidden_blocks("список коммутаторов, фильтр не нужен", recipe),
            vec!["Toolbar".to_string()]
        );
        assert_eq!(
            requested_hidden_blocks("switches table without pagination", recipe),
            vec!["Footer".to_string()]
        );
        // The phrasing the live run used: one negation trailing two subjects.
        assert_eq!(
            requested_hidden_blocks(
                "Собери экран: список коммутаторов. Фильтр и пагинация не нужны — только таблица",
                recipe
            ),
            vec!["Toolbar".to_string(), "Footer".to_string()]
        );
        assert_eq!(
            requested_hidden_blocks("убери пагинацию", recipe),
            vec!["Footer".to_string()]
        );
    }

    #[test]
    fn a_plain_request_hides_nothing() {
        let kit = crate::session_kit();
        let recipe = kit
            .recipes
            .iter()
            .find(|r| r.id == "ops-servers-screen")
            .expect("the ops recipe");
        assert!(requested_hidden_blocks("список коммутаторов", recipe).is_empty());
    }

    /// Measured 2026-09-18: the first real long Cyrillic request through the
    /// recipe route PANICKED the connection thread — the subject's byte
    /// offset («поиск» at byte 781) was used to index the 486-char vector.
    /// A subject sitting in the back half of any Cyrillic prompt reproduces
    /// it; the window arithmetic must stay in chars end to end.
    #[test]
    fn a_long_cyrillic_request_does_not_panic_the_search() {
        let kit = crate::session_kit();
        let recipe = kit
            .recipes
            .iter()
            .find(|r| r.id == "ops-servers-screen")
            .expect("the ops recipe");
        // The baseline prompt, verbatim: «поиск» sits past the char length
        // in bytes, with no negation — nothing is hidden, nobody crashes.
        let baseline = "Экран «Установка ОС» для системы ГЕНОМ: слева сайдбар с навигацией \
                        (Сценарии, Сессии сценариев, Обновления, Командная строка), сверху \
                        хлебные крошки «Управление • Сценарии • s3m-03b02-msk44 • Установка ОС», \
                        строка поиска, справа кнопки «Отмена» и «Продолжить». Информационная \
                        плашка: «Выберите узлы, на которых будет устанавливаться ОС (не более \
                        10). Выбрано: 10». Таблица узлов с чекбоксами: колонки «Имя узла» и \
                        «IP узла», 10 строк, часть строк отмечена. Внизу пагинация 1 2 3 4 … 45.";
        assert!(requested_hidden_blocks(baseline, recipe).is_empty());
        // The same length, but the negation reaches «пагинация» — Footer goes.
        let mut negated = baseline.to_string();
        negated.push_str(" Пагинация не нужна.");
        assert_eq!(
            requested_hidden_blocks(&negated, recipe),
            vec!["Footer".to_string()]
        );
    }
}

/// Phrases that mean "follow the reference I am showing you".
///
/// A reference outranks the automatic recipe: "сделай экран как на картинке"
/// is a request to reproduce that picture, and quietly routing it into a
/// recipe rewrite answers a different question. The same applies to an image
/// attached to the message.
const REFERENCE_PHRASES: &[&str] = &[
    "как на картинк",
    "как на скриншот",
    "по картинк",
    "по скриншот",
    "по референс",
    "как на макете",
    "как на изображени",
    "с картинк",
    "as in the image",
    "as in the picture",
    "like the image",
    "like the picture",
    "like the screenshot",
    "from the screenshot",
    "based on the image",
    "based on the picture",
    "по образцу",
];

/// Whether a request points at a picture instead of at a recipe.
///
/// This is a guess, and it is only ever allowed to be one. A route that holds
/// the request's attachment list knows the answer exactly and reports it as
/// [`ReferenceEvidence::Attached`] / [`ReferenceEvidence::NoImage`] — this
/// function is for the routes that hold no list at all
/// ([`ReferenceEvidence::Unknown`]), where the words are the only evidence
/// there is. Nothing else may call it: a phrase standing in for a knowable
/// fact is the defect issue #65 is about.
pub fn refers_to_a_reference(prompt: &str) -> bool {
    let haystack = prompt.to_lowercase();
    REFERENCE_PHRASES
        .iter()
        .any(|phrase| haystack.contains(phrase))
}

/// What the calling route knows about this turn's reference image.
///
/// The fact of an attachment is knowable exactly, and guessing it from the
/// prompt's words was the worse half of issue #65: a person who attached a
/// picture and said nothing about it was read as having no reference, and one
/// who wrote "like on the screenshot" with nothing attached as having one.
/// A route that holds the request's attachment list reports the fact and the
/// fact decides; the words only get to speak for a route that has no list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceEvidence {
    /// The route holds this turn's attachment list, and an image is in it.
    Attached,
    /// The route holds this turn's attachment list, and no entry is an image.
    NoImage,
    /// The route has no attachment list to read — attachments are not plumbed
    /// through this path — so no fact is available here and the prompt's own
    /// words are the only evidence there is.
    ///
    /// This is the standing reason [`refers_to_a_reference`] still exists. A
    /// request that crossed a JSON hop lands here: `DesignRequest`'s
    /// `reference_attachments` is `serde(skip)`, so a deserialized request
    /// looks exactly like one sent without a picture, and a reader of that
    /// request cannot tell the two apart. Reporting `NoImage` for it would
    /// assert knowledge the type does not carry.
    Unknown,
}

impl ReferenceEvidence {
    /// The evidence of an attachment list the caller actually holds.
    /// `has_image` is the exact answer to "does this turn carry a picture",
    /// so this constructor is where a fact replaces a guess.
    pub const fn of_attachments(has_image: bool) -> Self {
        if has_image {
            Self::Attached
        } else {
            Self::NoImage
        }
    }
}

/// Whether this turn is grounded on a reference picture.
///
/// One place answers that question, so the answer cannot drift between the
/// three decisions that depend on it: whether a recipe is placed, whether the
/// recipe rules travel, and whether the planner is offered the recipe index.
/// The attachment fact wins wherever a route holds the list — an attached
/// picture is a reference even when the prompt never says so, and no phrase may
/// claim one the list says is not there. The words decide for
/// [`ReferenceEvidence::Unknown`] alone, and that is the documented, narrow
/// reason [`refers_to_a_reference`] is still reachable at all.
pub fn is_reference_turn(prompt: &str, evidence: ReferenceEvidence) -> bool {
    match evidence {
        ReferenceEvidence::Attached => true,
        ReferenceEvidence::NoImage => false,
        ReferenceEvidence::Unknown => refers_to_a_reference(prompt),
    }
}

/// The recipe to place for this turn, or `None` when one must not be placed.
///
/// Placement is skipped for a reference turn: the picture decides the layout,
/// and the recipe would only compete with it — the `doc:recipe-base` rule that
/// comes with a placement then tells the model to keep it, so a picture
/// attached without a word about it used to lose to the ops screen.
pub fn recipe_to_place<'a>(
    prompt: &str,
    evidence: ReferenceEvidence,
    kit: &'a crate::KitManifest,
) -> Option<&'a crate::kit_manifest::KitRecipe> {
    if is_reference_turn(prompt, evidence) {
        return None;
    }
    // No picture: the words still get to ask for a recipe ("список
    // коммутаторов с таблицей").
    select_recipe(prompt, kit)
}

#[cfg(test)]
mod reference_turn_tests {
    use super::*;

    #[test]
    fn a_picture_request_does_not_place_a_recipe() {
        let kit = crate::session_kit();
        assert!(recipe_to_place(
            "Сделай экран как на картинке",
            ReferenceEvidence::Unknown,
            kit
        )
        .is_none());
        // An attached picture needs no words at all: the fact is the whole
        // signal, which is what a person who attaches a screenshot and writes
        // nothing depends on.
        assert!(
            recipe_to_place("сделай список серверов", ReferenceEvidence::Attached, kit).is_none()
        );
    }

    #[test]
    fn a_plain_admin_list_still_places_one() {
        let kit = crate::session_kit();
        let placed = recipe_to_place(
            "список коммутаторов с таблицей",
            ReferenceEvidence::NoImage,
            kit,
        )
        .expect("the ops recipe");
        assert_eq!(placed.id, "ops-servers-screen");
    }

    /// Issue #65, the guessing half. The route holds the request's attachment
    /// list, so "this turn has no picture" is a fact — and a fact outranks the
    /// prompt's own words. Before this, a request that merely *said* "как на
    /// картинке" with nothing attached was handled as a reference turn.
    #[test]
    fn no_attached_image_leaves_the_words_without_a_vote() {
        let kit = crate::session_kit();
        let placed = recipe_to_place(
            "список коммутаторов как на картинке",
            ReferenceEvidence::NoImage,
            kit,
        );
        assert!(
            placed.is_some(),
            "the attachment list says there is no picture — the phrase must not claim one"
        );
    }

    /// The route's own constructor for the fact: an attachment list it holds,
    /// asked one question — is there a picture in it.
    #[test]
    fn the_fact_is_read_from_the_attachment_list() {
        assert_eq!(
            ReferenceEvidence::of_attachments(false),
            ReferenceEvidence::NoImage
        );
        assert_eq!(
            ReferenceEvidence::of_attachments(true),
            ReferenceEvidence::Attached
        );
        // Only the variant with no attachment list to read lets the words
        // decide — everything else is the fact, in both directions.
        assert!(is_reference_turn(
            "сделай список серверов",
            ReferenceEvidence::Attached
        ));
        assert!(!is_reference_turn(
            "сделай как на картинке",
            ReferenceEvidence::NoImage
        ));
        assert!(is_reference_turn(
            "сделай как на картинке",
            ReferenceEvidence::Unknown
        ));
    }
}
