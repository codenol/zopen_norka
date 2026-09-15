//! The subject matrix: what a section's parts may be changed by whom.
//!
//! The document matrix has its own tests (`request_access_tests`); these are
//! the answers that differ, and the two asymmetries that are the whole reason
//! this module exists: ownership does not answer the summary, and the agent
//! confers nothing.

use super::*;
use crate::mcp_serve::tool_profile::McpScopes;
use crate::web_canvas_server::online_policy::ServeMode;
use crate::web_canvas_server::request_access::{AccessRefusal, DocumentAction, RequestAccess};
use crate::web_canvas_server::tenant_auth::{IdentityVia, ResolvedIdentity};
use op_editor_core::access::RoleSet;
use op_editor_core::ShareLevel;

/// A verified account holding `roles`, spelled the way the hub spells them.
fn identity(user_id: &str, roles: &[&str]) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        roles: RoleSet::from_wire(roles),
        via: IdentityVia::ApiToken,
        scopes: McpScopes::FULL,
    }
}

/// What one caller may do, as a compact table.
fn allowed(access: &RequestAccess<'_>) -> Vec<(&'static str, bool)> {
    SubjectAction::ALL
        .into_iter()
        .map(|action| (action.as_str(), access.decide_subject(action).is_ok()))
        .collect()
}

#[test]
fn a_local_deployment_decides_nothing() {
    // One operator, their own files: the branch that lets the section work land
    // without touching local editing.
    for mode in [ServeMode::Local, ServeMode::Managed] {
        let access = RequestAccess::local_operator(mode);
        for action in SubjectAction::ALL {
            assert_eq!(access.decide_subject(action), Ok(()), "{action:?}");
        }
    }
}

#[test]
fn a_stranger_is_refused_whole() {
    // Reach first, so a caller with no business with the document learns
    // nothing about what it says — and a stranger gets the same answer for a
    // read as for a write.
    let outsider = identity("userB", &["admin"]);
    let access = RequestAccess::online("userA", &outsider, None);
    for action in SubjectAction::ALL {
        assert_eq!(
            access.decide_subject(action),
            Err(AccessRefusal::NotShared),
            "{action:?}"
        );
    }
}

#[test]
fn an_analyst_owns_the_analytics_and_the_summary_and_not_the_mockups() {
    let analyst = identity("userB", &["analyst"]);
    let access = RequestAccess::online("userA", &analyst, Some(ShareLevel::Editor));
    assert_eq!(
        allowed(&access),
        vec![
            ("analytics-read", true),
            ("analytics-write", true),
            ("summary-write", true),
            ("ux-flow-write", false),
            ("mockups-write", false),
        ]
    );
    // The refusal says which subject was refused rather than "no".
    assert_eq!(
        access.decide_subject(SubjectAction::UxFlowWrite),
        Err(AccessRefusal::ReadOnly)
    );
}

#[test]
fn a_designer_owns_the_flow_and_the_mockups_and_not_the_summary() {
    // The mirror image, and the reason the matrix is by subject: one section,
    // two authors, and neither can quietly rewrite the other's half.
    let designer = identity("userB", &["ux_ui"]);
    let access = RequestAccess::online("userA", &designer, Some(ShareLevel::Editor));
    assert_eq!(
        allowed(&access),
        vec![
            ("analytics-read", true),
            ("analytics-write", true),
            ("summary-write", false),
            ("ux-flow-write", true),
            ("mockups-write", true),
        ]
    );
}

#[test]
fn the_owner_of_the_document_may_still_not_rewrite_the_summary() {
    // The asymmetry the module docs argue for: a screen is the owner's
    // document, and a summary is a READING of somebody else's analytics. The
    // person who wrote down why should not have their reasoning quietly edited
    // by the person building from it.
    let owner = identity("userA", &["ux_ui"]);
    let access = RequestAccess::online("userA", &owner, None);
    assert_eq!(
        access.decide_subject(SubjectAction::SummaryWrite),
        Err(AccessRefusal::ReadOnly)
    );
    // ...while the screens, which are their own document, stay theirs.
    assert_eq!(access.decide_subject(SubjectAction::MockupsWrite), Ok(()));
    assert_eq!(access.decide(DocumentAction::Edit), Ok(()));
}

#[test]
fn the_owner_of_the_document_may_still_not_edit_analytics_without_the_role() {
    // Ownership of a document is not ownership of an asset: the markdown is
    // loaded once and referenced by any number of sections, so the right to
    // change it follows the work, not the file.
    let owner = identity("userA", &["frontend"]);
    let access = RequestAccess::online("userA", &owner, None);
    assert_eq!(
        access.decide_subject(SubjectAction::AnalyticsWrite),
        Err(AccessRefusal::ReadOnly)
    );
    assert_eq!(access.decide_subject(SubjectAction::AnalyticsRead), Ok(()));
}

#[test]
fn the_other_product_roles_may_read_and_change_nothing() {
    // QA, frontend, backend and software: the readers the whole feature exists
    // for. They reach the reasoning and may not rewrite it.
    for role in ["qa", "frontend", "backend", "software"] {
        let caller = identity("userB", &[role]);
        let access = RequestAccess::online("userA", &caller, Some(ShareLevel::Editor));
        assert_eq!(
            allowed(&access),
            vec![
                ("analytics-read", true),
                ("analytics-write", false),
                ("summary-write", false),
                ("ux-flow-write", false),
                ("mockups-write", false),
            ],
            "{role}"
        );
    }
}

#[test]
fn an_admin_may_change_everything_a_section_carries() {
    let admin = identity("userB", &["admin"]);
    let access = RequestAccess::online("userA", &admin, Some(ShareLevel::Editor));
    for action in SubjectAction::ALL {
        assert_eq!(access.decide_subject(action), Ok(()), "{action:?}");
    }
}

#[test]
fn holding_two_roles_grants_the_union_of_their_subjects() {
    // The analyst who also covers QA is the ordinary case, not a
    // misconfiguration — the same rule the document matrix follows.
    let both = identity("userB", &["analyst", "qa"]);
    let access = RequestAccess::online("userA", &both, Some(ShareLevel::Editor));
    assert_eq!(access.decide_subject(SubjectAction::SummaryWrite), Ok(()));
    assert_eq!(
        access.decide_subject(SubjectAction::UxFlowWrite),
        Err(AccessRefusal::ReadOnly)
    );

    let analyst_and_designer = identity("userC", &["analyst", "ux_ui"]);
    let access = RequestAccess::online("userA", &analyst_and_designer, Some(ShareLevel::Editor));
    assert_eq!(access.decide_subject(SubjectAction::SummaryWrite), Ok(()));
    assert_eq!(access.decide_subject(SubjectAction::UxFlowWrite), Ok(()));
}

#[test]
fn an_unrecognised_role_grants_nothing_beyond_reading() {
    // Fail closed: a role this build does not know is not an analyst and not a
    // designer.
    let unknown = identity("userB", &["chief_vibe_officer"]);
    let access = RequestAccess::online("userA", &unknown, Some(ShareLevel::Editor));
    assert_eq!(
        access.decide_subject(SubjectAction::AnalyticsWrite),
        Err(AccessRefusal::ReadOnly)
    );
    assert_eq!(
        access.decide_subject(SubjectAction::SummaryWrite),
        Err(AccessRefusal::ReadOnly)
    );
    assert_eq!(access.decide_subject(SubjectAction::AnalyticsRead), Ok(()));
}

#[test]
fn a_caller_with_no_roles_at_all_reads_and_writes_nothing() {
    let bare = identity("userB", &[]);
    let access = RequestAccess::online("userA", &bare, Some(ShareLevel::Editor));
    assert_eq!(access.decide_subject(SubjectAction::AnalyticsRead), Ok(()));
    for action in SubjectAction::ALL.into_iter().filter(|a| a.is_write()) {
        assert_eq!(
            access.decide_subject(action),
            Err(AccessRefusal::ReadOnly),
            "{action:?}"
        );
    }
}

#[test]
fn an_analyst_may_not_obtain_screen_editing_through_an_agent() {
    // The hole #33 had to close for the AI turn. Building screens is a document
    // write, so a request to build them — phrased by a person or by the agent
    // acting for them — is refused with the same right the matrix refuses a
    // direct write with. There is no service identity: the agent's turn carries
    // the caller's access, which is this value.
    let analyst = identity("userB", &["analyst"]);
    let access = RequestAccess::online("userA", &analyst, Some(ShareLevel::Editor));

    assert_eq!(
        access.decide_subject(SubjectAction::MockupsWrite),
        Err(AccessRefusal::ReadOnly)
    );
    assert_eq!(
        access.decide(DocumentAction::Edit),
        Err(AccessRefusal::ReadOnly)
    );
    // What the analyst may do through the agent is what they may do by hand.
    assert_eq!(access.decide_subject(SubjectAction::SummaryWrite), Ok(()));
}

#[test]
fn every_action_has_a_stable_name_and_a_stated_side() {
    assert_eq!(SubjectAction::ALL.len(), 5);
    assert_eq!(SubjectAction::AnalyticsRead.as_str(), "analytics-read");
    assert_eq!(SubjectAction::AnalyticsWrite.as_str(), "analytics-write");
    assert_eq!(SubjectAction::SummaryWrite.as_str(), "summary-write");
    assert_eq!(SubjectAction::UxFlowWrite.as_str(), "ux-flow-write");
    assert_eq!(SubjectAction::MockupsWrite.as_str(), "mockups-write");
    assert!(!SubjectAction::AnalyticsRead.is_write());
    assert!(SubjectAction::ALL
        .into_iter()
        .filter(|action| *action != SubjectAction::AnalyticsRead)
        .all(|action| action.is_write()));
}
