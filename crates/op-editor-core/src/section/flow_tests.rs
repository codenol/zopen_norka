//! The flow graph: what a checkable structure catches, and what counts as a gap.

use super::*;
use crate::node_id::NodeId;
use crate::section::SectionProperties;

fn id(name: &str) -> FlowStepId {
    FlowStepId::new(name).expect("non-empty")
}

fn start(name: &str) -> FlowStep {
    FlowStep::new(id(name), name, FlowStepKind::Start)
}

fn step(name: &str) -> FlowStep {
    FlowStep::new(id(name), name, FlowStepKind::Step)
}

fn decision(name: &str) -> FlowStep {
    FlowStep::new(id(name), name, FlowStepKind::Decision)
}

fn end(name: &str) -> FlowStep {
    FlowStep::new(id(name), name, FlowStepKind::End)
}

fn edge(from: &str, to: &str) -> FlowEdge {
    FlowEdge::new(id(from), id(to))
}

/// The flow every other test is a single change away from:
///
/// ```text
/// A(start) --> B --> C{?} --|yes|--> Z(end)
///                    |--|no|-----> B
/// ```
fn checkout() -> UxFlow {
    UxFlow::new("f1", "Checkout")
        .with_step(start("A"))
        .with_step(step("B"))
        .with_step(decision("C"))
        .with_step(end("Z"))
        .with_edge(edge("A", "B"))
        .with_edge(edge("B", "C"))
        .with_edge(edge("C", "Z").labelled("yes"))
        .with_edge(edge("C", "B").labelled("no"))
}

#[test]
fn a_sound_flow_reports_nothing() {
    let check = check_flow(&checkout(), &[]);
    assert!(check.is_valid(), "{:?}", check.issues);
    assert_eq!(check.issues, Vec::new());
}

#[test]
fn an_empty_flow_is_empty_rather_than_broken() {
    // A flow the moment it is created has no steps. Reporting "no start" would
    // put a mark on every new flow.
    let check = check_flow(&UxFlow::new("f1", "New"), &[]);
    assert!(check.is_valid());
    assert!(check.gaps.is_empty());
}

#[test]
fn a_flow_without_a_start_is_refused_before_reachability() {
    // Without a start, "unreachable" is true of every step and says nothing
    // about which of them is the mistake.
    let flow = UxFlow::new("f1", "No entry")
        .with_step(step("B"))
        .with_step(end("Z"))
        .with_edge(edge("B", "Z"));
    let check = check_flow(&flow, &[]);
    assert!(check.issues.contains(&FlowIssue::NoStart));
}

#[test]
fn a_step_nobody_can_reach_is_reported() {
    let mut flow = checkout();
    flow.steps.push(step("D"));
    flow.edges.push(edge("D", "Z"));
    let check = check_flow(&flow, &[]);
    assert!(check
        .issues
        .contains(&FlowIssue::UnreachableStep { step: id("D") }));
    // ...and it is reachable-ness, not termination, that is wrong with it.
    assert!(!check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("D") }));
}

#[test]
fn an_orphan_is_reported_as_an_orphan_and_not_twice() {
    let mut flow = checkout();
    flow.steps.push(step("D"));
    let check = check_flow(&flow, &[]);
    assert!(check
        .issues
        .contains(&FlowIssue::OrphanStep { step: id("D") }));
    // The more useful of the two true things: "nothing connects this" is a
    // different repair from "this is on a branch nobody can get to".
    assert!(!check
        .issues
        .contains(&FlowIssue::UnreachableStep { step: id("D") }));
    assert!(!check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("D") }));
}

#[test]
fn a_loop_with_a_way_out_is_a_flow_and_not_a_defect() {
    // "Back to the cart" is a normal flow; a validator that refused it would be
    // one people work around. `checkout` has exactly such a loop.
    let check = check_flow(&checkout(), &[]);
    assert!(!check
        .issues
        .iter()
        .any(|issue| matches!(issue, FlowIssue::NoWayOut { .. })));
}

#[test]
fn a_loop_with_no_way_out_is_named_as_a_place() {
    let mut flow = checkout();
    // The `no` branch now goes round B <-> C for ever: nothing reaches Z from
    // there any more.
    flow.edges
        .retain(|edge| edge.from != id("C") || edge.to != id("Z"));
    flow.edges.push(edge("B", "C"));
    let check = check_flow(&flow, &[]);
    assert!(check.issues.contains(&FlowIssue::NoWayOut {
        steps: vec![id("A"), id("B"), id("C")]
    }));
    assert!(check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("B") }));
    assert!(check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("C") }));
    // The start can reach... nothing that ends, so it is reported too — and it
    // is NOT part of the loop, which is the distinction the two findings make.
    assert!(check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("A") }));
}

#[test]
fn a_self_loop_alone_can_never_finish() {
    let flow = UxFlow::new("f1", "Retry for ever")
        .with_step(start("A"))
        .with_step(step("B"))
        .with_step(end("Z"))
        .with_edge(edge("A", "B"))
        .with_edge(edge("B", "B"));
    let check = check_flow(&flow, &[]);
    // A and B are both in it: A cannot stop either — its only way on is into
    // the loop — and the group is the place a flow is stuck, not just the cycle
    // inside it.
    assert!(check.issues.contains(&FlowIssue::NoWayOut {
        steps: vec![id("A"), id("B")]
    }));
    assert!(check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("B") }));
}

#[test]
fn a_branch_that_simply_stops_is_reported() {
    let mut flow = checkout();
    flow.steps.push(step("D"));
    flow.edges.push(edge("C", "D"));
    let check = check_flow(&flow, &[]);
    assert!(check
        .issues
        .contains(&FlowIssue::NeverEnds { step: id("D") }));
    // Not a loop: a path that stops is not a cycle, and saying it was would send
    // somebody looking for an edge that does not exist.
    assert!(!check
        .issues
        .iter()
        .any(|issue| matches!(issue, FlowIssue::NoWayOut { .. })));
}

#[test]
fn an_edge_to_nowhere_is_refused() {
    let mut flow = checkout();
    flow.edges.push(edge("C", "nope"));
    let check = check_flow(&flow, &[]);
    assert!(check.issues.contains(&FlowIssue::DanglingEdge {
        from: id("C"),
        to: id("nope"),
    }));
    // A dangling edge means the graph is not a graph: naming a loop inside it
    // would be reading structure that is not there.
    assert!(!check
        .issues
        .iter()
        .any(|issue| matches!(issue, FlowIssue::NoWayOut { .. })));
}

#[test]
fn two_steps_with_one_id_are_refused() {
    let mut flow = checkout();
    flow.steps.push(step("B"));
    let check = check_flow(&flow, &[]);
    assert!(check
        .issues
        .contains(&FlowIssue::DuplicateStepId { id: id("B") }));
}

#[test]
fn a_decision_with_one_way_out_is_not_deciding_anything() {
    let mut flow = checkout();
    flow.edges
        .retain(|edge| !(edge.from == id("C") && edge.to == id("B")));
    let check = check_flow(&flow, &[]);
    assert!(check
        .issues
        .contains(&FlowIssue::DecisionWithOneBranch { step: id("C") }));
}

#[test]
fn an_end_with_a_way_on_is_refused() {
    let mut flow = checkout();
    flow.steps.push(step("D"));
    flow.edges.push(edge("Z", "D"));
    let check = check_flow(&flow, &[]);
    assert!(check
        .issues
        .contains(&FlowIssue::EndWithOutgoingEdges { step: id("Z") }));
}

#[test]
fn a_step_with_no_mockup_is_a_gap_and_not_an_issue() {
    let check = check_flow(&checkout(), &[]);
    assert!(check.is_valid());
    assert_eq!(check.gaps.len(), 4);
    assert!(check
        .gaps
        .contains(&FlowGap::StepWithoutScreen { step: id("B") }));
    assert!(!check.is_complete());
}

#[test]
fn a_step_attached_to_a_screen_the_section_has_is_complete() {
    let screens = vec![NodeId::new("n1"), NodeId::new("n2")];
    let mut flow = checkout();
    for (step, screen) in flow.steps.iter_mut().zip(["n1", "n1", "n2", "n2"]) {
        step.screen = Some(NodeId::new(screen));
    }
    let check = check_flow(&flow, &screens);
    assert!(check.is_complete(), "{:?}", check.gaps);
}

#[test]
fn a_step_attached_to_a_screen_the_section_lost_is_a_gap() {
    // The quiet lie the section exists to prevent: a flow drawn for a screen
    // that has since been deleted.
    let mut flow = checkout();
    flow.steps[1].screen = Some(NodeId::new("n_gone"));
    let check = check_flow(&flow, &[NodeId::new("n1")]);
    assert!(check.gaps.contains(&FlowGap::ScreenNotInSection {
        step: id("B"),
        screen: NodeId::new("n_gone"),
    }));
    assert!(!check.is_complete());
}

#[test]
fn a_screen_link_survives_an_edit_that_keeps_the_step() {
    // How mermaid text keeps what the notation cannot say.
    let mut previous = checkout();
    previous.steps[0].screen = Some(NodeId::new("n1"));
    previous.steps[3].screen = Some(NodeId::new("n9"));

    let mut edited = UxFlow::new("f1", "Checkout");
    edited.steps = vec![start("A"), step("B"), end("Z")];
    edited.adopt_screens_from(&previous);

    assert_eq!(edited.steps[0].screen, Some(NodeId::new("n1")));
    // A step the edit introduced has none — the honest state for a step nobody
    // has linked yet.
    assert_eq!(edited.steps[1].screen, None);
    // The end kept its link even though the decision is gone.
    assert_eq!(edited.steps[2].screen, Some(NodeId::new("n9")));
}

#[test]
fn a_step_id_cannot_be_empty() {
    assert!(FlowStepId::new("").is_none());
    assert_eq!(id("A").as_str(), "A");
    assert_eq!(id("A").to_string(), "A");
}

#[test]
fn a_flow_can_be_looked_up_by_id() {
    let mut properties = SectionProperties::empty();
    properties.flows.push(checkout());
    assert_eq!(
        properties.flow("f1").map(|flow| flow.name.as_str()),
        Some("Checkout")
    );
    assert!(properties.flow("missing").is_none());
}
