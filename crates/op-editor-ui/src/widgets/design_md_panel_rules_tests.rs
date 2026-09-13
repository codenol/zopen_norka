//! Rules-panel tests: the document list, the markdown editor, and the
//! shared flow both hosts call.
//!
//! The model is one document per component: the kit's `do` / `dont`
//! entries are a component document's default body, the component's name
//! is its title, and it can be switched off but never renamed or deleted.

use crate::widgets::design_md_rules_flow::apply_design_rules_hit;
use crate::widgets::{DesignMdHit, DesignMdPanel};
use crate::{Point2D, Rect};
use op_editor_core::host_keyboard_transitions as shared;
use op_editor_core::{parse_design_md, DesignRuleScope, EditorState};

/// Index of the first component row — the AI instruction owns row 0.
const FIRST_COMPONENT: u16 = 1;

fn panel_rect() -> Rect {
    Rect::xywh(0.0, 0.0, 480.0, 560.0)
}

fn panel_state() -> EditorState {
    let mut state = EditorState::default();
    state.editor_ui.design_md_panel.open = true;
    state
}

fn find_hit(panel: &DesignMdPanel<'_>, rect: Rect, target: DesignMdHit) -> Option<Point2D> {
    let mut y = rect.origin.y;
    while y <= rect.origin.y + rect.size.y {
        let mut x = rect.origin.x;
        while x <= rect.origin.x + rect.size.x {
            let point = Point2D::new(x, y);
            if panel.hit_test(rect, point) == Some(target) {
                return Some(point);
            }
            x += 3.0;
            y += 0.0;
        }
        y += 3.0;
    }
    None
}

fn any_hit(panel: &DesignMdPanel<'_>, rect: Rect, matches: impl Fn(DesignMdHit) -> bool) -> bool {
    let mut y = rect.origin.y;
    while y <= rect.origin.y + rect.size.y {
        let mut x = rect.origin.x;
        while x <= rect.origin.x + rect.size.x {
            if let Some(hit) = panel.hit_test(rect, Point2D::new(x, y)) {
                if matches(hit) {
                    return true;
                }
            }
            x += 3.0;
        }
        y += 3.0;
    }
    false
}

#[test]
fn the_list_holds_one_document_per_component() {
    let state = panel_state();
    let panel = DesignMdPanel::for_editor(&state).expect("open");

    let types = op_editor_core::session_kit().types.len();
    assert!(types > 0, "the active kit ships component types");
    assert!(panel.rows[0].is_primary, "the AI instruction is first");
    let recipes = op_editor_core::session_kit().recipes.len();
    assert_eq!(
        panel.rows.len(),
        types + recipes + 1,
        "the instruction, one row per kit component, one per recipe"
    );
    assert!(panel
        .rows
        .iter()
        .skip(1)
        .take(types)
        .all(|row| row.is_component && !row.removable));
    assert!(panel
        .rows
        .iter()
        .skip(1 + types)
        .all(|row| row.is_recipe && !row.removable));
    assert_eq!(
        panel.rows[FIRST_COMPONENT as usize].body,
        op_editor_core::default_component_body(&op_editor_core::session_kit().types[0]),
        "an untouched component shows the kit's own rules as its document"
    );
}

#[test]
fn clicking_a_row_opens_that_components_document() {
    let mut state = panel_state();
    let rect = panel_rect();
    let point = {
        let panel = DesignMdPanel::for_editor(&state).expect("open");
        find_hit(&panel, rect, DesignMdHit::RuleEdit(FIRST_COMPONENT))
            .expect("row body is hittable")
    };

    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleEdit(FIRST_COMPONENT),
        true,
        0
    ));
    let _ = point;
    let draft = state
        .editor_ui
        .design_md_panel
        .rule_draft
        .as_ref()
        .expect("editor open");
    assert!(
        !draft.title_editable,
        "a component's name is not authored here"
    );
    assert!(!draft.body.text().is_empty());
    assert!(matches!(draft.scope, DesignRuleScope::ComponentType { .. }));
}

#[test]
fn a_component_document_offers_no_bin() {
    let state = panel_state();
    let panel = DesignMdPanel::for_editor(&state).expect("open");
    let rect = panel_rect();

    assert!(any_hit(&panel, rect, |hit| matches!(
        hit,
        DesignMdHit::RuleToggle(0)
    )));
    assert!(
        !any_hit(&panel, rect, |hit| matches!(
            hit,
            DesignMdHit::RuleDelete(0)
        )),
        "component documents may be switched off, never deleted"
    );
}

#[test]
fn the_switch_stores_the_component_document_switched_off() {
    let mut state = panel_state();
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleToggle(FIRST_COMPONENT),
        true,
        0
    ));

    let rules = &state.doc.design_md.as_ref().expect("spec").rules;
    assert_eq!(rules.len(), 1, "the switch stores the document once");
    assert!(!rules[0].enabled);
    assert!(matches!(
        rules[0].scope,
        DesignRuleScope::ComponentType { .. }
    ));
    assert!(state.history.can_undo());

    // Deleting it is a no-op: component documents cannot be removed.
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleDelete(FIRST_COMPONENT),
        true,
        0
    ));
    assert_eq!(state.doc.design_md.as_ref().expect("spec").rules.len(), 1);
}

#[test]
fn editing_and_saving_a_component_document_persists_the_body() {
    let mut state = panel_state();
    apply_design_rules_hit(&mut state, DesignMdHit::RuleEdit(FIRST_COMPONENT), true, 0);
    {
        let draft = state
            .editor_ui
            .design_md_panel
            .rule_draft
            .as_mut()
            .expect("editor");
        draft
            .body
            .set_text("Always align the avatar to the text baseline.");
    }

    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleSave,
        true,
        0
    ));

    let saved = &state.doc.design_md.as_ref().expect("spec").rules;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].instruction,
        "Always align the avatar to the text baseline."
    );

    // The list shows the saved body from now on.
    let panel = DesignMdPanel::for_editor(&state).expect("open");
    assert!(panel.rows[FIRST_COMPONENT as usize]
        .body
        .starts_with("Always align"));
    assert!(panel.rows[FIRST_COMPONENT as usize].saved);
}

#[test]
fn an_author_rule_can_be_created_then_deleted() {
    let mut state = panel_state();
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::NewRule,
        true,
        0
    ));
    {
        let draft = state
            .editor_ui
            .design_md_panel
            .rule_draft
            .as_mut()
            .expect("editor");
        assert!(draft.title_editable, "an author rule has its own name");
        draft.title_input.set_text("Spacing");
        draft.body.set_text("Use 8px steps everywhere.");
    }
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleSave,
        true,
        0
    ));

    // A blank body blocks a save.
    apply_design_rules_hit(&mut state, DesignMdHit::NewRule, true, 0);
    {
        let draft = state
            .editor_ui
            .design_md_panel
            .rule_draft
            .as_mut()
            .expect("editor");
        draft.title_input.set_text("Empty");
    }
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleSave,
        true,
        0
    ));
    assert_eq!(
        state.doc.design_md.as_ref().expect("spec").rules.len(),
        1,
        "an empty document is never stored"
    );
    apply_design_rules_hit(&mut state, DesignMdHit::RuleCancel, true, 0);

    let panel = DesignMdPanel::for_editor(&state).expect("open");
    let author_index = panel
        .rows
        .iter()
        .position(|row| !row.is_component && !row.is_primary && !row.is_recipe)
        .expect("the author rule is listed") as u16;
    assert!(panel.rows[author_index as usize].removable);
    drop(panel);

    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleDelete(author_index),
        true,
        0
    ));
    let remaining = &state.doc.design_md.as_ref().expect("spec").rules;
    assert!(
        remaining.iter().all(|rule| rule.title != "Spacing"),
        "an author rule can be deleted"
    );
}

#[test]
fn clicking_the_body_lands_the_caret_and_typing_reaches_the_document() {
    let mut state = panel_state();
    apply_design_rules_hit(&mut state, DesignMdHit::RuleEdit(FIRST_COMPONENT), true, 0);
    let panel = DesignMdPanel::for_editor(&state).expect("open");
    let rect = panel_rect();
    let editor = panel.editor_layout(rect).expect("editor open");
    let point = Point2D::new(editor.body.origin.x + 12.0, editor.body.origin.y + 12.0);

    let hit = panel.hit_test(rect, point).expect("inside the panel");
    assert!(
        matches!(hit, DesignMdHit::RuleBodyCaret(_)),
        "a click in the body resolves to a caret offset, got {hit:?}"
    );
    assert!(apply_design_rules_hit(&mut state, hit, true, 0));

    // Typing and Enter reach the open document.
    assert_eq!(shared::design_rule_text(&mut state, 'x', 0), Some(true));
    assert_eq!(shared::design_rule_newline(&mut state, 0), Some(true));
    // Arrows are swallowed so they never nudge the canvas.
    assert!(shared::design_rule_caret_step(&mut state, 0.0, 1.0, false, 0).is_some());

    state.editor_ui.design_md_panel.rule_draft = None;
    assert_eq!(shared::design_rule_text(&mut state, 'x', 0), None);
    assert_eq!(shared::design_rule_newline(&mut state, 0), None);
    assert_eq!(
        shared::design_rule_caret_step(&mut state, 0.0, 1.0, false, 0),
        None
    );
}

#[test]
fn the_editor_undo_and_redo_buttons_walk_the_document_history() {
    let mut state = panel_state();
    apply_design_rules_hit(&mut state, DesignMdHit::RuleEdit(FIRST_COMPONENT), true, 0);
    {
        let draft = state
            .editor_ui
            .design_md_panel
            .rule_draft
            .as_mut()
            .expect("editor");
        draft.body.set_text("Edited once.");
    }
    apply_design_rules_hit(&mut state, DesignMdHit::RuleSave, true, 0);

    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleUndo,
        true,
        0
    ));
    assert!(
        state
            .doc
            .design_md
            .as_ref()
            .is_none_or(|spec| spec.rules.is_empty()),
        "undo drops the stored document"
    );

    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleRedo,
        true,
        0
    ));
    assert_eq!(state.doc.design_md.as_ref().expect("spec").rules.len(), 1);
}

#[test]
fn a_collab_gated_host_opens_documents_but_writes_nothing() {
    let mut state = panel_state();
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleEdit(FIRST_COMPONENT),
        false,
        0
    ));
    assert!(state.editor_ui.design_md_panel.rule_draft.is_some());

    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleToggle(FIRST_COMPONENT),
        false,
        0
    ));
    assert!(
        state
            .doc
            .design_md
            .as_ref()
            .is_none_or(|spec| spec.rules.is_empty()),
        "a collab-gated host must not write"
    );
}

#[test]
fn every_component_document_reaches_the_prompt_block() {
    let mut state = panel_state();
    apply_design_rules_hit(&mut state, DesignMdHit::RuleEdit(FIRST_COMPONENT), true, 0);
    {
        let draft = state
            .editor_ui
            .design_md_panel
            .rule_draft
            .as_mut()
            .expect("editor");
        draft.body.set_text("Keep the avatar circular.");
    }
    apply_design_rules_hit(&mut state, DesignMdHit::RuleSave, true, 0);

    let policy = op_editor_core::build_effective_rules_policy(
        &op_editor_core::effective_design_rules(state.doc.design_md.as_ref()),
    );
    assert!(
        policy.contains("Keep the avatar circular."),
        "the saved document must reach the AI: {policy}"
    );
    assert!(policy.contains("COMPONENT RULES"));
}

#[test]
fn the_ai_instruction_opens_the_prompt_block() {
    let mut state = panel_state();
    apply_design_rules_hit(&mut state, DesignMdHit::RuleEdit(0), true, 0);
    {
        let draft = state
            .editor_ui
            .design_md_panel
            .rule_draft
            .as_mut()
            .expect("editor");
        assert!(!draft.title_editable, "the instruction is never renamed");
        draft.body.set_text("Always answer in the user's language.");
    }
    apply_design_rules_hit(&mut state, DesignMdHit::RuleSave, true, 0);

    let policy = op_editor_core::build_effective_rules_policy(
        &op_editor_core::effective_design_rules(state.doc.design_md.as_ref()),
    );
    assert!(
        policy.starts_with("WORKING AGREEMENT"),
        "the instruction is the first thing the AI reads: {policy}"
    );
    assert!(policy.contains("Always answer in the user's language."));
    // Component rules still follow it.
    assert!(policy.contains("COMPONENT RULES"));
}

#[test]
fn the_instruction_is_switched_off_like_any_other_document() {
    let mut state = panel_state();
    assert!(apply_design_rules_hit(
        &mut state,
        DesignMdHit::RuleToggle(0),
        true,
        0
    ));
    let rules = &state.doc.design_md.as_ref().expect("spec").rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, op_editor_core::AI_INSTRUCTION_RULE_ID);
    assert!(!rules[0].enabled);

    let policy = op_editor_core::build_effective_rules_policy(
        &op_editor_core::effective_design_rules(state.doc.design_md.as_ref()),
    );
    assert!(
        !policy.contains("WORKING AGREEMENT"),
        "a switched-off instruction leaves the prompt"
    );
}

#[test]
fn an_empty_document_list_state_is_reachable() {
    // A document with no rules at all still lists the kit's documents, so
    // the panel never opens on an empty screen.
    let mut state = EditorState::default();
    state.editor_ui.design_md_panel.open = true;
    state.doc.design_md = Some(parse_design_md(""));
    let panel = DesignMdPanel::for_editor(&state).expect("open");
    assert!(!panel.rows.is_empty());
}
