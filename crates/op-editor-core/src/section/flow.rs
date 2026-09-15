//! UX flows: the graph, and what makes one checkable.
//!
//! A flow is a list of steps and the connections between them, stored as a
//! structure rather than as mermaid text — the operator's decision, and the
//! reason is in what a graph can be asked that a picture of text cannot:
//!
//! - **A step has an identity**, so other things can point at it. That is what
//!   lets a step name the mockup it is (issue #59's "this is step 3 becomes a
//!   fact rather than a caption"), and what lets a screen the flow needs but
//!   nobody built show up as a gap in the flow instead of being noticed by eye.
//! - **The flow can be checked.** Mermaid is parsed into this shape on save, so
//!   an agent asked to write a flow produces something that can be judged:
//!   every step reachable, every branch terminated, no orphans.
//! - **The link question extends to it.** The flow and the screens live in the
//!   same section, so "has anything moved since?" covers a flow drawn for a
//!   screen that has since been deleted.
//!
//! Mermaid remains how a flow is written down and one of the ways it is shown —
//! see [`crate::section::mermaid`] — but it is not what is kept.
//!
//! ## What [`check_flow`] reports, and what it deliberately does not
//!
//! Two lists, because they are two different statements:
//!
//! - [`FlowIssue`]s make the graph wrong: a step nobody can get to, a branch
//!   that never ends, an edge to a step that does not exist.
//! - [`FlowGap`]s make it incomplete: a step no mockup is attached to, or one
//!   attached to a mockup the section does not contain. A gap is not a defect —
//!   it is the work that is left, and it is exactly the list a reviewer wants.
//!
//! A cycle is **not** an issue by itself. "Back to the cart" is a normal flow,
//! and a validator that refused it would be one people work around. What is
//! refused is a cycle with no way out: a loop whose steps cannot reach an end,
//! which is a flow that never finishes for anybody who enters it.

use serde::{Deserialize, Serialize};

use crate::node_id::NodeId;

/// A step's identity inside its flow.
///
/// A newtype rather than a bare `String` because it is not interchangeable with
/// the other string ids in this feature — a step id is never a node id, and a
/// signature that confused them would compile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FlowStepId(String);

impl FlowStepId {
    /// Build an id. Empty ids are refused at construction: a step with no
    /// identity cannot be pointed at, which is the whole point of having one.
    pub fn new(id: impl Into<String>) -> Option<Self> {
        let id = id.into();
        (!id.is_empty()).then_some(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FlowStepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of step this is.
///
/// Small and closed. `Start` and `End` are what reachability and termination are
/// computed from; `Decision` is a step with more than one way out, which is the
/// only kind a branch can be checked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowStepKind {
    /// Where the flow begins — a user's entry into it.
    Start,
    /// An ordinary step.
    Step,
    /// A step that asks a question and goes different ways.
    Decision,
    /// Where a path finishes.
    End,
}

/// One step of a flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowStep {
    pub id: FlowStepId,
    /// What a person reads: "Открывает корзину".
    #[serde(default)]
    pub label: String,
    pub kind: FlowStepKind,
    /// The mockup in this section that this step is, when one is attached.
    ///
    /// The reason the flow is stored as a graph: this reference makes "step 3 is
    /// this screen" a fact the section can answer, and its absence a gap the
    /// section can list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<NodeId>,
}

impl FlowStep {
    /// A step of the given kind, with no mockup attached.
    pub fn new(id: FlowStepId, label: impl Into<String>, kind: FlowStepKind) -> Self {
        Self {
            id,
            label: label.into(),
            kind,
            screen: None,
        }
    }

    /// A step that is a mockup of this section.
    pub fn with_screen(mut self, screen: NodeId) -> Self {
        self.screen = Some(screen);
        self
    }
}

/// One connection between two steps.
///
/// The label is what a branch says ("да" / "нет"), which is also what makes a
/// decision readable rather than a shape with two anonymous arrows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowEdge {
    pub from: FlowStepId,
    pub to: FlowStepId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl FlowEdge {
    pub fn new(from: FlowStepId, to: FlowStepId) -> Self {
        Self {
            from,
            to,
            label: None,
        }
    }

    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.label = (!label.is_empty()).then_some(label);
        self
    }
}

/// A flow: a named graph of steps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UxFlow {
    /// Stable id inside the section, so a flow can be replaced by id.
    pub id: String,
    /// What a person reads: "Оформление заказа".
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
    #[serde(default)]
    pub edges: Vec<FlowEdge>,
}

impl UxFlow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            steps: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn with_step(mut self, step: FlowStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn with_edge(mut self, edge: FlowEdge) -> Self {
        self.edges.push(edge);
        self
    }

    /// The step with this id, when the flow has one.
    pub fn step(&self, id: &FlowStepId) -> Option<&FlowStep> {
        self.steps.iter().find(|step| &step.id == id)
    }

    /// Keep the mockups the surviving steps were attached to.
    ///
    /// Mermaid has nowhere to say which screen a step is (see
    /// [`crate::section::mermaid`]), so a flow that was edited as text comes
    /// back with no attached screens. This is how they survive the edit: the
    /// steps that are still there — matched by the identity both the text and
    /// the structure carry — keep what they had, and a step the edit introduced
    /// has none, which is the honest state for a step nobody has linked yet.
    pub fn adopt_screens_from(&mut self, previous: &UxFlow) {
        for step in &mut self.steps {
            if step.screen.is_some() {
                continue;
            }
            if let Some(previous_step) = previous.step(&step.id) {
                step.screen = previous_step.screen.clone();
            }
        }
    }
}

/// Something that makes a flow wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowIssue {
    /// The flow has steps and none of them is a start, so there is nowhere to
    /// enter it and reachability has no meaning.
    NoStart,
    /// Two steps share an id. Refused rather than merged: a flow whose steps
    /// cannot be told apart cannot have edges that mean anything.
    DuplicateStepId { id: FlowStepId },
    /// An edge naming a step the flow does not have.
    DanglingEdge { from: FlowStepId, to: FlowStepId },
    /// A step no edge touches, in a flow that has more than one step.
    OrphanStep { step: FlowStepId },
    /// A step that cannot be reached from any start.
    UnreachableStep { step: FlowStepId },
    /// A step from which no end can be reached — a branch that never finishes.
    NeverEnds { step: FlowStepId },
    /// The steps left when every step that can still finish, or still stop, has
    /// been removed: they can neither end nor stand still, so whoever enters
    /// them goes round.
    ///
    /// The loop, plus everything whose only way on leads into it — named as a
    /// group because the repair is a place rather than a list, and one edge from
    /// any of these steps to an end resolves all of them. A step that leads into
    /// the loop while still having a way out of its own is not a member: it is
    /// only [`Self::NeverEnds`].
    NoWayOut { steps: Vec<FlowStepId> },
    /// An end step with edges out of it: the flow goes on after it finishes.
    EndWithOutgoingEdges { step: FlowStepId },
    /// A decision with fewer than two ways out, so nothing is being decided.
    DecisionWithOneBranch { step: FlowStepId },
}

/// Something a flow does not have yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowGap {
    /// A step no mockup is attached to: the screen this step is has not been
    /// built, or nobody has said which one it is.
    StepWithoutScreen { step: FlowStepId },
    /// A step attached to a mockup the section does not contain — a screen that
    /// was deleted, or a flow copied into a section that never had it.
    ScreenNotInSection { step: FlowStepId, screen: NodeId },
}

/// What [`check_flow`] found.
///
/// Two lists rather than one list with a severity, because the two are acted on
/// differently: an issue is something to fix before the flow means anything, a
/// gap is work that is still to be done and is worth showing as such.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowCheck {
    pub issues: Vec<FlowIssue>,
    pub gaps: Vec<FlowGap>,
}

impl FlowCheck {
    /// Whether the graph itself is sound. Gaps do not count — an incomplete flow
    /// that is well formed is exactly what a section looks like in progress.
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    /// Whether the flow is sound AND every step is attached to a mockup.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty() && self.gaps.is_empty()
    }
}

/// Check a flow, and the mockups it claims to be part of.
///
/// `section_screens` is the set of mockups the section holds — the caller reads
/// them from the section frame, so this module stays free of the document.
/// Passing an empty slice is honest ("this flow is not in a section with any
/// screens") and makes every attached mockup a gap, which is correct.
pub fn check_flow(flow: &UxFlow, section_screens: &[NodeId]) -> FlowCheck {
    let mut check = FlowCheck::default();
    if flow.steps.is_empty() {
        // An empty flow is an empty list item, not a broken one. Reporting
        // "no start" here would put a mark on every flow the moment it is
        // created.
        return check;
    }

    // Ids first: everything below is stated in terms of them, and a duplicate
    // makes "the step with this id" ambiguous for every later question.
    let mut ids: Vec<&FlowStepId> = Vec::with_capacity(flow.steps.len());
    for step in &flow.steps {
        if ids.contains(&&step.id) {
            check.issues.push(FlowIssue::DuplicateStepId {
                id: step.id.clone(),
            });
        } else {
            ids.push(&step.id);
        }
    }

    // Then the edges, so the reachability work below can trust them.
    let mut dangling = false;
    for edge in &flow.edges {
        if !ids.contains(&&edge.from) || !ids.contains(&&edge.to) {
            dangling = true;
            check.issues.push(FlowIssue::DanglingEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
            });
        }
    }

    let edges_out = |id: &FlowStepId| -> Vec<&FlowEdge> {
        flow.edges
            .iter()
            .filter(|edge| &edge.from == id && ids.contains(&&edge.to))
            .collect()
    };
    let edges_in = |id: &FlowStepId| -> Vec<&FlowEdge> {
        flow.edges
            .iter()
            .filter(|edge| &edge.to == id && ids.contains(&&edge.from))
            .collect()
    };

    // An orphan is a step with no edges at all. Reported as an orphan INSTEAD of
    // as unreachable, because that is the more useful of the two true things:
    // "nothing connects this" is a different repair from "this is on a branch
    // nobody can get to".
    let multiple_steps = flow.steps.len() > 1;
    let mut orphans: Vec<FlowStepId> = Vec::new();
    for step in &flow.steps {
        if multiple_steps && edges_out(&step.id).is_empty() && edges_in(&step.id).is_empty() {
            orphans.push(step.id.clone());
            check.issues.push(FlowIssue::OrphanStep {
                step: step.id.clone(),
            });
        }
    }
    let connected = |id: &FlowStepId| !orphans.contains(id);

    // Ends and starts. A flow with steps and no declared start is refused before
    // reachability, because "unreachable" would then be true of every step and
    // would say nothing about which of them is the mistake.
    let ends: Vec<&FlowStepId> = flow
        .steps
        .iter()
        .filter(|step| step.kind == FlowStepKind::End && connected(&step.id))
        .map(|step| &step.id)
        .collect();
    let starts: Vec<&FlowStepId> = flow
        .steps
        .iter()
        .filter(|step| step.kind == FlowStepKind::Start)
        .map(|step| &step.id)
        .collect();
    if starts.is_empty() {
        check.issues.push(FlowIssue::NoStart);
    }

    // Reachability: forward from every start. Skipped when there is no start —
    // "unreachable" is then true of every step and says nothing about which of
    // them is the mistake, and `NoStart` above has already named it.
    if !starts.is_empty() {
        let mut reached: Vec<FlowStepId> = starts.iter().map(|id| (*id).clone()).collect();
        let mut frontier: Vec<FlowStepId> = reached.clone();
        while let Some(id) = frontier.pop() {
            for edge in edges_out(&id) {
                if !reached.contains(&edge.to) {
                    reached.push(edge.to.clone());
                    frontier.push(edge.to.clone());
                }
            }
        }
        for step in &flow.steps {
            if connected(&step.id) && !reached.contains(&step.id) {
                check.issues.push(FlowIssue::UnreachableStep {
                    step: step.id.clone(),
                });
            }
        }
    }

    // Termination: backward from every end. A step not in this set is on a
    // branch that never finishes — which is a cycle with no way out, a path that
    // simply stops, or both.
    let mut terminates: Vec<FlowStepId> = ends.iter().map(|id| (*id).clone()).collect();
    let mut frontier: Vec<FlowStepId> = terminates.clone();
    while let Some(id) = frontier.pop() {
        for edge in edges_in(&id) {
            if !terminates.contains(&edge.from) {
                terminates.push(edge.from.clone());
                frontier.push(edge.from.clone());
            }
        }
    }
    for step in &flow.steps {
        if connected(&step.id) && !terminates.contains(&step.id) {
            check.issues.push(FlowIssue::NeverEnds {
                step: step.id.clone(),
            });
        }
    }

    // The stuck region, named: it is the cause, and a list of symptoms is a
    // worse thing to hand somebody who has to fix it.
    let stuck: Vec<FlowStepId> = flow
        .steps
        .iter()
        .filter(|step| connected(&step.id) && !terminates.contains(&step.id))
        .map(|step| step.id.clone())
        .collect();
    let loop_steps = loop_without_exit(flow, &ids, &stuck);
    if !loop_steps.is_empty() {
        check.issues.push(FlowIssue::NoWayOut { steps: loop_steps });
    }

    // Shapes that misdescribe themselves.
    for step in &flow.steps {
        match step.kind {
            FlowStepKind::End if !edges_out(&step.id).is_empty() => {
                check.issues.push(FlowIssue::EndWithOutgoingEdges {
                    step: step.id.clone(),
                });
            }
            FlowStepKind::Decision if edges_out(&step.id).len() < 2 => {
                check.issues.push(FlowIssue::DecisionWithOneBranch {
                    step: step.id.clone(),
                });
            }
            _ => {}
        }
    }

    // Gaps last, in step order.
    for step in &flow.steps {
        match &step.screen {
            None => check.gaps.push(FlowGap::StepWithoutScreen {
                step: step.id.clone(),
            }),
            Some(screen) if !section_screens.contains(screen) => {
                check.gaps.push(FlowGap::ScreenNotInSection {
                    step: step.id.clone(),
                    screen: screen.clone(),
                })
            }
            Some(_) => {}
        }
    }

    // A dangling edge means the graph is not a graph; naming a loop inside it
    // would be reading structure that is not there.
    if dangling {
        check.issues.retain(|issue| {
            !matches!(
                issue,
                FlowIssue::NeverEnds { .. } | FlowIssue::NoWayOut { .. }
            )
        });
    }
    check
}

/// The steps that can neither finish nor stop: the loop, and what leads into
/// it.
///
/// Peeling, not cycle detection. Start from the steps that cannot reach an end,
/// repeatedly drop any step with no edge left inside the set, and what remains
/// has nowhere to go but round. The steps that merely LEAD into such a loop are
/// peeled off in the first rounds, which is what keeps them reported as
/// [`FlowIssue::NeverEnds`] rather than as members of the loop — and it is why
/// this is a loop over a shrinking set rather than an SCC pass: the answer
/// wanted is "where is the place that has no way out", not "list every cycle".
fn loop_without_exit(flow: &UxFlow, ids: &[&FlowStepId], stuck: &[FlowStepId]) -> Vec<FlowStepId> {
    let mut remaining: Vec<FlowStepId> = ids
        .iter()
        .copied()
        .filter(|id| stuck.contains(id))
        .cloned()
        .collect();
    loop {
        // Collected before it replaces the set: the test reads the set it is
        // filtering, which `retain` would not allow.
        let keep: Vec<FlowStepId> = remaining
            .iter()
            .filter(|step| {
                flow.edges.iter().any(|edge| {
                    &edge.from == *step && ids.contains(&&edge.to) && remaining.contains(&edge.to)
                })
            })
            .cloned()
            .collect();
        if keep.len() == remaining.len() {
            return keep;
        }
        remaining = keep;
    }
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
