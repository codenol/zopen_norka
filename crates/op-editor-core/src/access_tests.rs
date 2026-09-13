#![cfg(test)]

//! Tests for the product role / rights model.
//!
//! The point of these is the operator's matrix, not the bitmask: each test
//! names an authority level from it and asserts the level's rights exactly,
//! including the rights it must NOT have.

use crate::access::{
    rights_for, rights_for_roles, ProductRole, Right, Rights, RoleSet, RoleWireError,
};

#[test]
fn every_role_grants_exactly_the_operators_matrix() {
    // Admin — everything, including the account list.
    assert_eq!(rights_for(ProductRole::Admin), Rights::ADMIN);
    // UX/UI — edits everything except users.
    assert_eq!(rights_for(ProductRole::UxUi), Rights::EDITOR);
    assert!(rights_for(ProductRole::UxUi).can_edit());
    assert!(!rights_for(ProductRole::UxUi).can_manage_users());
    // The other five — view, comment, invite, and nothing more.
    for role in [
        ProductRole::Software,
        ProductRole::Analyst,
        ProductRole::Frontend,
        ProductRole::Backend,
        ProductRole::Qa,
    ] {
        let rights = rights_for(role);
        assert_eq!(rights, Rights::CONTRIBUTOR, "{role:?}");
        assert!(rights.can_view(), "{role:?}");
        assert!(rights.can_comment(), "{role:?}");
        assert!(rights.can_invite(), "{role:?}");
        // The two rights that decide whether the matrix is being honoured.
        assert!(!rights.can_edit(), "{role:?}");
        assert!(!rights.can_manage_users(), "{role:?}");
    }
}

#[test]
fn the_seven_roles_are_all_distinct_and_all_declared() {
    assert_eq!(ProductRole::ALL.len(), 7);
    for (index, role) in ProductRole::ALL.iter().enumerate() {
        assert!(
            !ProductRole::ALL[..index].contains(role),
            "duplicate {role:?}"
        );
    }
    // Only two roles differ from the contributor bucket — everything else is
    // the operator's own grouping, and this is where that is stated.
    let distinct: Vec<Rights> = ProductRole::ALL
        .iter()
        .map(|role| rights_for(*role))
        .collect();
    assert_eq!(distinct[0], Rights::ADMIN);
    assert_eq!(distinct[1], Rights::EDITOR);
    assert!(distinct[2..].iter().all(|r| *r == Rights::CONTRIBUTOR));
}

#[test]
fn admin_covers_every_other_role() {
    let admin = rights_for(ProductRole::Admin);
    for right in Right::ALL {
        assert!(admin.has(right), "{}", right.as_str());
    }
    for role in ProductRole::ALL {
        // Unioning admin with anything must not change admin.
        assert_eq!(admin.union(rights_for(role)), admin, "{role:?}");
    }
}

#[test]
fn an_account_holding_several_roles_gets_the_union_not_the_intersection() {
    // The case the operator's matrix is really about: QA who also designs.
    let qa_designer = RoleSet::from_wire(["qa", "ux_ui"]);
    assert!(qa_designer.contains(ProductRole::Qa));
    assert!(qa_designer.contains(ProductRole::UxUi));
    assert!(qa_designer.rights().can_edit(), "union, not intersection");
    assert!(qa_designer.rights().can_comment());
    assert!(!qa_designer.rights().can_manage_users());

    // Intersecting would have produced "only what both may do", which for
    // UX/UI and QA is the contributor bucket — an account restricted by
    // holding MORE roles. That is the bug this rule exists to prevent.
    let intersection = rights_for(ProductRole::Qa).bits() & rights_for(ProductRole::UxUi).bits();
    assert_ne!(qa_designer.rights().bits(), intersection);
}

#[test]
fn an_admin_role_swallows_the_roles_it_is_held_with() {
    let admin_and_frontend = RoleSet::from_wire(["frontend", "admin"]);
    assert_eq!(admin_and_frontend.rights(), Rights::ADMIN);
    assert!(admin_and_frontend.rights().can_manage_users());
    assert!(admin_and_frontend.rights().can_edit());
}

#[test]
fn an_unknown_role_is_neither_admin_nor_editor() {
    assert_eq!(
        ProductRole::from_wire("superuser"),
        Err(RoleWireError::Unknown {
            raw: "superuser".into()
        })
    );

    let set = RoleSet::from_wire(["superuser"]);
    assert!(set.is_empty());
    assert!(set.roles().is_empty());
    assert_eq!(set.unrecognized(), &["superuser".to_string()]);
    let rights = set.rights();
    assert!(!rights.can_edit(), "fail closed");
    assert!(!rights.can_manage_users(), "fail closed");
    assert!(!rights.can_comment(), "fail closed");
    // ...but an authenticated account can still look at its own workspace.
    assert!(rights.can_view());
    assert_eq!(rights, Rights::VIEW_ONLY);
}

#[test]
fn one_unknown_role_does_not_take_rights_away_from_a_known_one() {
    let set = RoleSet::from_wire(["analyst", "chief-vibes-officer"]);
    assert_eq!(set.roles(), &[ProductRole::Analyst]);
    assert_eq!(set.unrecognized(), &["chief-vibes-officer".to_string()]);
    assert_eq!(set.rights(), Rights::CONTRIBUTOR);
}

#[test]
fn a_blank_role_string_is_recorded_rather_than_dropped() {
    for raw in ["", "   "] {
        let set = RoleSet::from_wire([raw]);
        assert!(set.is_empty(), "{raw:?}");
        assert_eq!(set.unrecognized().len(), 1, "{raw:?}");
        assert_eq!(ProductRole::from_wire(raw), Err(RoleWireError::Blank));
    }
}

#[test]
fn an_empty_role_list_means_no_roles_and_not_no_access() {
    // `StaticVerifier` (env table, no hub) is why this rule exists: it has no
    // roles to give, and a deployment that suddenly denied its own operator
    // read access would be a regression, not a tightening.
    let none = RoleSet::empty();
    assert!(none.is_empty());
    assert!(none.roles().is_empty());
    assert!(none.unrecognized().is_empty());
    assert_eq!(none.rights(), Rights::VIEW_ONLY);
    assert!(none.rights().can_view());
    assert!(!none.rights().can_edit());
    assert!(!none.rights().can_comment());

    // The pure statement about roles is still "nothing".
    assert_eq!(rights_for_roles(&[]), Rights::NONE);
    assert_eq!(rights_for_roles(&[]).bits(), 0);
}

#[test]
fn roles_are_matched_ignoring_case_and_separators() {
    for raw in ["UX/UI", "ux-ui", " ux ui ", "UX_UI", "UxUi", "ux.ui"] {
        assert_eq!(
            ProductRole::from_wire(raw),
            Ok(ProductRole::UxUi),
            "{raw:?}"
        );
    }
    assert_eq!(ProductRole::from_wire("Admin"), Ok(ProductRole::Admin));
    // The operator's list is Russian; a hub panel built from it may send it
    // as written.
    assert_eq!(ProductRole::from_wire("Админ"), Ok(ProductRole::Admin));
    assert_eq!(ProductRole::from_wire("Аналитик"), Ok(ProductRole::Analyst));
    assert_eq!(ProductRole::from_wire("Бэк"), Ok(ProductRole::Backend));
}

#[test]
fn every_role_round_trips_through_its_canonical_slug() {
    for role in ProductRole::ALL {
        assert_eq!(ProductRole::from_wire(role.as_wire()), Ok(role), "{role:?}");
    }
}

#[test]
fn repeated_roles_collapse_and_keep_their_first_appearance_order() {
    let set = RoleSet::from_wire(["qa", "QA", " qa ", "analyst", "qa"]);
    assert_eq!(set.roles(), &[ProductRole::Qa, ProductRole::Analyst]);
    assert!(set.unrecognized().is_empty());
}

#[test]
fn a_role_may_come_from_the_hub_already_typed() {
    let set = RoleSet::from_iter([ProductRole::Frontend, ProductRole::Frontend]);
    assert_eq!(set.roles(), &[ProductRole::Frontend]);
    assert_eq!(set.rights(), Rights::CONTRIBUTOR);
    assert_eq!(
        RoleSet::from_role(ProductRole::Admin).rights(),
        Rights::ADMIN
    );
}

#[test]
fn every_right_has_its_own_bit_and_the_levels_stack() {
    let mut seen = 0u8;
    for right in Right::ALL {
        // `bit` is private to the module; a child module may read it, and a
        // public accessor would invite callers to build `Rights` by hand.
        let bit = right.bit();
        assert_eq!(bit.count_ones(), 1, "{}", right.as_str());
        assert_eq!(seen & bit, 0, "duplicate bit for {}", right.as_str());
        seen |= bit;
    }
    assert_eq!(seen, Rights::ADMIN.bits());
    assert_eq!(Rights::default(), Rights::NONE);
    assert!(Rights::NONE.is_empty());

    // The levels are nested, which is what makes "admin covers everything"
    // true by construction rather than by four coincidences.
    assert_eq!(
        Rights::CONTRIBUTOR.bits() & Rights::VIEW_ONLY.bits(),
        Rights::VIEW_ONLY.bits()
    );
    assert_eq!(
        Rights::EDITOR.bits() & Rights::CONTRIBUTOR.bits(),
        Rights::CONTRIBUTOR.bits()
    );
    assert_eq!(
        Rights::ADMIN.bits() & Rights::EDITOR.bits(),
        Rights::EDITOR.bits()
    );
}

#[test]
fn rights_print_themselves_for_diagnostics() {
    assert_eq!(Rights::NONE.to_string(), "none");
    assert_eq!(Rights::VIEW_ONLY.to_string(), "view");
    assert_eq!(Rights::CONTRIBUTOR.to_string(), "view+comment+invite");
    assert_eq!(
        Rights::ADMIN.to_string(),
        "view+comment+invite+edit+manage-users"
    );
}

#[test]
fn a_parse_failure_says_which_role_it_could_not_read() {
    assert_eq!(
        ProductRole::from_wire("").unwrap_err().to_string(),
        "blank role name"
    );
    assert_eq!(
        ProductRole::from_wire("wat").unwrap_err().to_string(),
        "unknown product role: wat"
    );
}

#[test]
fn every_role_has_its_own_colour() {
    let mut seen: Vec<(&str, ProductRole)> = Vec::new();
    for role in ProductRole::ALL {
        let colour = role.colour().hex;
        assert!(
            colour.starts_with('#') && colour.len() == 7,
            "{role:?} has a malformed colour: {colour}"
        );
        if let Some((other, other_role)) = seen.iter().find(|(seen, _)| *seen == colour) {
            panic!("{role:?} and {other_role:?} share the colour {other}");
        }
        seen.push((colour, role));
    }
    assert_eq!(seen.len(), 7, "all seven roles must be covered");
}

#[test]
fn the_two_warm_roles_stay_apart() {
    // Admin's gold and the Analyst's yellow are the pair most at risk of
    // reading as one colour; keeping them apart is the reason the values were
    // chosen rather than picked from a palette in order.
    let gold = ProductRole::Admin.colour().hex;
    let yellow = ProductRole::Analyst.colour().hex;
    let channel = |hex: &str, index: usize| {
        u8::from_str_radix(&hex[1 + index * 2..3 + index * 2], 16).expect("hex")
    };
    // Gold is markedly darker and less blue than the analyst's light yellow.
    assert!(
        channel(gold, 2) + 60 < channel(yellow, 2),
        "gold {gold} and yellow {yellow} are too close"
    );
}
