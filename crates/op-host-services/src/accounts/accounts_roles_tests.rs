//! What a role list an OPERATOR wrote is accepted as, and what it is refused
//! as.
//!
//! The rule under test is not "which roles exist" — `op_editor_core::access`
//! owns that and proves it — but the consequence this module exists for: a
//! typo must be refused rather than stored as a role that silently grants
//! nothing.

use super::*;

fn roles(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|role| role.to_string()).collect()
}

#[test]
fn the_seven_roles_are_stored_under_their_canonical_wire_spellings() {
    let all: Vec<String> = ProductRole::ALL
        .iter()
        .map(|role| role.as_wire().to_string())
        .collect();
    assert_eq!(
        canonical_roles(&all).expect("every role this build has"),
        all
    );
}

#[test]
fn aliases_and_separators_fold_onto_one_role() {
    // Four spellings of one role, all of them what somebody would actually
    // type: the hub's aliases (`ux_designer`), the human form (`UX/UI`), and
    // the operator's Russian name. Storing any of them verbatim would put
    // four values in one column that mean one thing.
    for raw in ["UX/UI", "ux-ui", "ux ui", "ux_ui", "designer", "Дизайнер"] {
        assert_eq!(
            canonical_roles(&roles(&[raw])).expect(raw),
            vec!["ux_ui".to_string()],
            "{raw}"
        );
    }
    // The Russian names the operator's own list uses.
    assert_eq!(
        canonical_roles(&roles(&["Админ", "Аналитик", "Фронт", "Бэк", "QA"])).expect("russian"),
        vec!["admin", "analyst", "frontend", "backend", "qa"]
    );
}

#[test]
fn a_role_this_build_does_not_have_is_refused_with_its_own_spelling() {
    let error = canonical_roles(&roles(&["superuser"])).expect_err("not a role here");
    assert_eq!(
        error.raw, "superuser",
        "the caller has to see what they typed, not a normalised form of it"
    );
    let text = error.to_string();
    assert!(text.contains("superuser"), "{text}");
    // And what a refusal prints is the whole vocabulary, so the caller can fix
    // the input without going to look it up.
    for role in ProductRole::ALL {
        assert!(text.contains(role.as_wire()), "{text} missing {role:?}");
    }
}

#[test]
fn a_typo_among_good_roles_still_refuses_the_whole_list() {
    // Partially applied input is the shape that leaves an account looking
    // configured and behaving as if it were not, so nothing is written unless
    // every entry is understood.
    assert!(canonical_roles(&roles(&["qa", "superuser"])).is_err());
}

#[test]
fn no_roles_is_a_list_and_not_an_error() {
    // A guest invitation is a real thing to hand out, and an account with no
    // roles is what `RoleSet::rights` floors at view-only.
    assert_eq!(canonical_roles(&[]).expect("empty"), Vec::<String>::new());
}

#[test]
fn a_repeated_role_is_one_grant() {
    assert_eq!(
        canonical_roles(&roles(&["qa", "QA", "qa"])).expect("deduped"),
        vec!["qa".to_string()],
        "the column is a tag list; writing the same grant twice says nothing extra"
    );
}

#[test]
fn the_order_the_operator_wrote_is_the_order_that_is_stored() {
    assert_eq!(
        canonical_roles(&roles(&["qa", "admin", "backend"])).expect("ordered"),
        vec!["qa", "admin", "backend"]
    );
}

#[test]
fn a_comma_separated_command_line_argument_becomes_its_roles() {
    assert_eq!(split_roles("qa, ux_ui"), vec!["qa", "ux_ui"]);
    // A trailing separator is a typing habit, not a role with no name.
    assert_eq!(split_roles("qa,"), vec!["qa"]);
    assert_eq!(split_roles("  "), Vec::<String>::new());
    assert_eq!(split_roles(""), Vec::<String>::new());
}
