//! Rules-panel click dispatch, shared by the native and web hosts.
//!
//! The panel shows one document per component plus the author's own
//! rules. Every write goes through [`EditorCommand`] and is preceded by
//! [`EditorState::commit_history`], so editing a document joins undo/redo
//! exactly like any other document change. The single genuine difference
//! between the two hosts is the collaboration gate on document mutations,
//! so that arrives as the `allow_mutation` flag.

use op_editor_core::{
    parse_design_md, DesignRuleDraft, DesignRuleFocus, EditorCommand, EditorState, PanelRow,
};

use crate::widgets::design_md_panel::DesignMdHit;

/// Apply a panel click. Returns `true` when the hit was handled (the host
/// then marks the frame dirty and swallows the event).
pub fn apply_design_rules_hit(
    state: &mut EditorState,
    hit: DesignMdHit,
    allow_mutation: bool,
    now_ms: u64,
) -> bool {
    use DesignMdHit as H;
    match hit {
        H::NewRule => {
            let id = next_author_rule_id(state);
            state.editor_ui.design_md_panel.rule_draft = Some(DesignRuleDraft::new_local(id));
            state.editor_ui.design_md_panel.rules_scroll.offset = 0.0;
            true
        }
        H::RuleEdit(index) => {
            let Some(row) = panel_row(state, index) else {
                return true;
            };
            state.editor_ui.design_md_panel.rule_draft = Some(DesignRuleDraft::from_row(&row));
            true
        }
        H::RuleToggle(index) => {
            let Some(row) = panel_row(state, index) else {
                return true;
            };
            if !allow_mutation {
                return true;
            }
            let enabled = !row.enabled;
            let stored = stored_rule_id(state, &row);
            ensure_design_md_spec(state);
            state.commit_history();
            match stored {
                Some(rule_id) => {
                    state.apply(EditorCommand::SetDesignRuleEnabled { rule_id, enabled });
                }
                None => {
                    // A component document that was never edited still has
                    // to exist before it can be switched off.
                    let mut rule = row_to_rule(&row);
                    rule.enabled = enabled;
                    state.apply(EditorCommand::UpsertDesignRule {
                        rule: Box::new(rule),
                    });
                }
            }
            true
        }
        H::RuleDelete(index) => {
            let Some(row) = panel_row(state, index) else {
                return true;
            };
            // Component documents cannot be deleted — only switched off.
            if !allow_mutation || !row.removable {
                return true;
            }
            let Some(rule_id) = stored_rule_id(state, &row) else {
                return true;
            };
            state.commit_history();
            state.apply(EditorCommand::DeleteDesignRule { rule_id });
            true
        }
        H::RuleBodyCaret(offset) => {
            if let Some(draft) = state.editor_ui.design_md_panel.rule_draft.as_mut() {
                draft.focus = DesignRuleFocus::Body;
                draft.body.set_caret(offset as usize, now_ms);
            }
            true
        }
        H::RuleTitleCaret(offset) => {
            if let Some(draft) = state.editor_ui.design_md_panel.rule_draft.as_mut() {
                draft.focus = DesignRuleFocus::Title;
                draft.title_input.set_caret(offset as usize, now_ms);
            }
            true
        }
        H::RuleSave => {
            if !allow_mutation {
                return true;
            }
            let Some(rule) = state
                .editor_ui
                .design_md_panel
                .rule_draft
                .as_ref()
                .and_then(|draft| draft.to_rule())
            else {
                // Invalid document — the save button paints disabled, so
                // this only guards a programmatic press.
                return true;
            };
            ensure_design_md_spec(state);
            state.commit_history();
            state.apply(EditorCommand::UpsertDesignRule {
                rule: Box::new(rule),
            });
            state.editor_ui.design_md_panel.rule_draft = None;
            true
        }
        H::RuleCancel => {
            state.editor_ui.design_md_panel.rule_draft = None;
            true
        }
        H::RuleUndo => {
            state.undo();
            true
        }
        H::RuleRedo => {
            state.redo();
            true
        }
        _ => false,
    }
}

/// The row `index` currently shows, resolved exactly the way the panel
/// resolved it — a click can never act on a different row than the one
/// painted at that position.
fn panel_row(state: &EditorState, index: u16) -> Option<PanelRow> {
    op_editor_core::panel_rows(op_editor_core::session_kit(), state.doc.design_md.as_ref())
        .into_iter()
        .nth(index as usize)
}

/// The rule id the row is already stored under, if any.
fn stored_rule_id(state: &EditorState, row: &PanelRow) -> Option<String> {
    state
        .doc
        .design_md
        .as_ref()?
        .rules
        .iter()
        .find(|rule| rule.id == row.rule_id)
        .map(|rule| rule.id.clone())
}

fn row_to_rule(row: &PanelRow) -> jian_ops_schema::DesignRule {
    jian_ops_schema::DesignRule {
        id: row.rule_id.clone(),
        title: row.title.clone(),
        instruction: row.body.clone(),
        kind: jian_ops_schema::DesignRuleKind::Do,
        scope: row.scope.clone(),
        condition: None,
        priority: 0,
        enabled: true,
        overrides: None,
    }
}

/// Rules need a spec to live in; an empty one is the documented shape for
/// "document carries rules but no markdown brief yet".
fn ensure_design_md_spec(state: &mut EditorState) {
    if state.doc.design_md.is_none() {
        state.doc.design_md = Some(parse_design_md(""));
    }
}

/// A fresh id for an author rule.
fn next_author_rule_id(state: &EditorState) -> String {
    let used: Vec<&str> = state
        .doc
        .design_md
        .as_ref()
        .map(|spec| spec.rules.iter().map(|rule| rule.id.as_str()).collect())
        .unwrap_or_default();
    for index in 1..1000 {
        let candidate = format!("local:rule-{index}");
        if !used.contains(&candidate.as_str()) {
            return candidate;
        }
    }
    format!("local:rule-{}", used.len() + 1)
}

/// Whether a rules-list scroll offset changed — shared by both hosts'
/// wheel handlers.
pub fn scroll_rules(state: &mut EditorState, delta_y: f32, max: f32) -> bool {
    let scroll = &mut state.editor_ui.design_md_panel.rules_scroll;
    let before = scroll.offset;
    scroll.offset = (scroll.offset + delta_y).clamp(0.0, max.max(0.0));
    (scroll.offset - before).abs() > f32::EPSILON
}
