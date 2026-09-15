//! Mermaid: the subset that is read, the text that is written, and the shapes
//! that are refused.

use super::*;
use crate::node_id::NodeId;
use crate::section::flow::{FlowStep, FlowStepId, FlowStepKind};

fn id(name: &str) -> FlowStepId {
    FlowStepId::new(name).expect("non-empty")
}

/// The flow every round-trip test uses: a start, a step, a decision, an end,
/// a labelled branch and a loop back.
fn checkout() -> UxFlow {
    UxFlow::new("f1", "Checkout")
        .with_step(FlowStep::new(
            id("A"),
            "Открывает корзину",
            FlowStepKind::Start,
        ))
        .with_step(FlowStep::new(
            id("B"),
            "Проверяет товары",
            FlowStepKind::Step,
        ))
        .with_step(FlowStep::new(
            id("C"),
            "Есть аккаунт?",
            FlowStepKind::Decision,
        ))
        .with_step(FlowStep::new(id("Z"), "Заказ оформлен", FlowStepKind::End))
        .with_edge(FlowEdge::new(id("A"), id("B")))
        .with_edge(FlowEdge::new(id("B"), id("C")))
        .with_edge(FlowEdge::new(id("C"), id("Z")).labelled("да"))
        .with_edge(FlowEdge::new(id("C"), id("B")).labelled("нет"))
}

/// Two graphs are the same when every step and every edge matches.
fn assert_same_graph(left: &UxFlow, right: &UxFlow) {
    assert_eq!(left.steps.len(), right.steps.len(), "step count");
    for (a, b) in left.steps.iter().zip(&right.steps) {
        assert_eq!(a.id, b.id, "step id");
        assert_eq!(a.label, b.label, "label of {}", a.id);
        assert_eq!(a.kind, b.kind, "kind of {}", a.id);
    }
    assert_eq!(left.edges.len(), right.edges.len(), "edge count");
    for (a, b) in left.edges.iter().zip(&right.edges) {
        assert_eq!(a.from, b.from, "edge from");
        assert_eq!(a.to, b.to, "edge to");
        assert_eq!(a.label, b.label, "edge label");
    }
}

#[test]
fn a_flow_prints_as_mermaid_and_reads_back() {
    let flow = checkout();
    let text = flow_to_mermaid(&flow).expect("printed");
    assert!(text.starts_with("flowchart TD\n"));
    assert!(text.contains("A ([Открывает корзину])"), "{text}");
    assert!(text.contains("B [Проверяет товары]"), "{text}");
    assert!(text.contains("C {Есть аккаунт?}"), "{text}");
    assert!(text.contains("Z ((Заказ оформлен))"), "{text}");
    assert!(text.contains("C -->|да| Z"), "{text}");

    let back = flow_from_mermaid(&text).expect("parsed");
    assert_same_graph(&flow, &back);
}

#[test]
fn the_four_shapes_are_the_four_kinds() {
    let text = "\
flowchart TD
  A([Entry])
  B[A step]
  C{A question?}
  Z((Done))
  A --> B
  B --> C
  C --> Z
";
    let flow = flow_from_mermaid(text).expect("parsed");
    let kind = |name: &str| {
        flow.steps
            .iter()
            .find(|step| step.id == id(name))
            .map(|step| step.kind)
            .expect("step")
    };
    assert_eq!(kind("A"), FlowStepKind::Start);
    assert_eq!(kind("B"), FlowStepKind::Step);
    assert_eq!(kind("C"), FlowStepKind::Decision);
    assert_eq!(kind("Z"), FlowStepKind::End);
}

#[test]
fn a_label_needing_quotes_survives_the_round_trip() {
    let mut flow = UxFlow::new("f1", "Tricky");
    flow.steps = vec![
        FlowStep::new(id("A"), "Button [Save] {now}", FlowStepKind::Start),
        FlowStep::new(id("B"), "  padded  ", FlowStepKind::Step),
        FlowStep::new(id("Z"), "ends\nhere", FlowStepKind::End),
    ];
    flow.edges = vec![FlowEdge::new(id("A"), id("B")).labelled("a | b")];
    let text = flow_to_mermaid(&flow).expect("printed");
    let back = flow_from_mermaid(&text).expect("parsed");
    assert_same_graph(&flow, &back);
}

#[test]
fn an_empty_label_is_carried_rather_than_refused() {
    let mut flow = UxFlow::new("f1", "Empty");
    flow.steps = vec![FlowStep::new(id("A"), "", FlowStepKind::Start)];
    let text = flow_to_mermaid(&flow).expect("printed");
    let back = flow_from_mermaid(&text).expect("parsed");
    assert_eq!(back.steps[0].label, "");
}

#[test]
fn a_chain_is_the_same_as_two_edges() {
    let chained = flow_from_mermaid("flowchart TD\n  A([Start]) --> B[Middle] --> Z((End))\n")
        .expect("parsed");
    let split = flow_from_mermaid(
        "flowchart TD\n  A([Start])\n  B[Middle]\n  Z((End))\n  A --> B\n  B --> Z\n",
    )
    .expect("parsed");
    assert_same_graph(&chained, &split);
}

#[test]
fn statements_may_share_a_line_and_comments_are_ignored() {
    let text = "\
%% a flow for the checkout
flowchart TD
  A([Start]); B[Middle]; Z((End))
  A --> B ; B --> Z
  %% trailing note
";
    let flow = flow_from_mermaid(text).expect("parsed");
    assert_eq!(flow.steps.len(), 3);
    assert_eq!(flow.edges.len(), 2);
}

#[test]
fn a_bare_id_is_a_step_labelled_by_its_own_id() {
    // Mermaid's own rule, and the reason `A --> B` is a flow and not an error.
    let flow = flow_from_mermaid("flowchart TD\n  A --> B\n").expect("parsed");
    assert_eq!(flow.steps.len(), 2);
    assert_eq!(flow.steps[0].label, "A");
    assert_eq!(flow.steps[0].kind, FlowStepKind::Step);
    assert_eq!(flow.edges.len(), 1);
}

#[test]
fn a_flow_without_the_header_is_refused_with_the_line() {
    let error = flow_from_mermaid("A --> B\n").expect_err("refused");
    assert_eq!(error.kind, MermaidErrorKind::MissingHeader);
    assert_eq!(error.line, 1);
    assert!(error.to_string().starts_with("line 1:"));
}

#[test]
fn an_empty_text_is_refused_as_a_missing_header() {
    assert_eq!(
        flow_from_mermaid("   \n\n%% nothing\n")
            .expect_err("refused")
            .kind,
        MermaidErrorKind::MissingHeader
    );
}

#[test]
fn a_bad_header_is_refused() {
    for text in [
        "sequenceDiagram\n",
        "flowchart sideways\n",
        "flowchart TD extra\n",
    ] {
        assert_eq!(
            flow_from_mermaid(text).expect_err("refused").kind,
            MermaidErrorKind::MissingHeader,
            "{text}"
        );
    }
    // A missing direction is legal mermaid and means top-down.
    assert!(flow_from_mermaid("flowchart\n  A([Start])\n").is_ok());
}

#[test]
fn an_unclosed_shape_is_refused_with_its_line() {
    let error = flow_from_mermaid("flowchart TD\n  A([Start\n  B[Middle]\n").expect_err("refused");
    assert!(matches!(error.kind, MermaidErrorKind::MalformedNode { .. }));
    assert_eq!(error.line, 2);
}

#[test]
fn trailing_garbage_is_refused_as_a_node_or_an_edge() {
    let node = flow_from_mermaid("flowchart TD\n  A[Start] ??\n").expect_err("refused");
    assert!(matches!(node.kind, MermaidErrorKind::MalformedNode { .. }));
    let edge = flow_from_mermaid("flowchart TD\n  A --> B ??\n").expect_err("refused");
    assert!(matches!(edge.kind, MermaidErrorKind::MalformedEdge { .. }));
}

#[test]
fn two_declarations_that_disagree_are_refused() {
    let error = flow_from_mermaid("flowchart TD\n  A[Start]\n  A([Entry])\n").expect_err("refused");
    assert_eq!(
        error.kind,
        MermaidErrorKind::DuplicateStepId {
            id: "A".to_string()
        }
    );
    assert_eq!(error.line, 3, "the line of the second declaration");
}

#[test]
fn declaring_the_same_thing_twice_is_a_no_op() {
    let flow =
        flow_from_mermaid("flowchart TD\n  A([Entry])\n  A([Entry])\n  A --> A\n").expect("parsed");
    assert_eq!(flow.steps.len(), 1);
}

#[test]
fn a_bare_id_is_upgraded_by_a_later_declaration() {
    let flow = flow_from_mermaid("flowchart TD\n  A --> B\n  B[Middle]\n").expect("parsed");
    let b = flow
        .steps
        .iter()
        .find(|step| step.id == id("B"))
        .expect("B");
    assert_eq!(b.label, "Middle");
    assert_eq!(b.kind, FlowStepKind::Step);
}

#[test]
fn a_class_annotation_is_read_and_dropped() {
    // `:::class` is a styling hook for a drawing; this graph has no such
    // property, and silently keeping it would be a property nothing reads.
    let flow =
        flow_from_mermaid("flowchart TD\n  A[Start]:::primary\n  A --> A\n").expect("parsed");
    assert_eq!(flow.steps.len(), 1);
    assert_eq!(flow.steps[0].label, "Start");
}

#[test]
fn the_notation_carries_the_graph_and_not_the_sections_bookkeeping() {
    // The flow's own id and name belong to the section; a parsed flow gets them
    // from the section it is saved into.
    let flow = flow_from_mermaid("flowchart TD\n  A([Start])\n").expect("parsed");
    assert_eq!(flow.id, "");
    assert_eq!(flow.name, "");
    assert_eq!(flow.steps[0].screen, None);
}

#[test]
fn the_mockups_a_step_was_linked_to_survive_the_round_trip() {
    // Mermaid cannot say which screen a step is, so the links are restored from
    // the structure the text was printed from — matched by step identity.
    let mut before = checkout();
    before.steps[0].screen = Some(NodeId::new("n1"));
    before.steps[2].screen = Some(NodeId::new("n2"));

    let text = flow_to_mermaid(&before).expect("printed");
    let mut after = flow_from_mermaid(&text).expect("parsed");
    after.adopt_screens_from(&before);

    let screen_of = |name: &str| {
        after
            .steps
            .iter()
            .find(|step| step.id == id(name))
            .and_then(|step| step.screen.clone())
    };
    assert_eq!(screen_of("A"), Some(NodeId::new("n1")));
    assert_eq!(screen_of("C"), Some(NodeId::new("n2")));
    assert_eq!(screen_of("B"), None);
}

#[test]
fn an_id_this_notation_cannot_print_is_refused_rather_than_printed() {
    // A renderer that emitted it would produce text that reads back as a
    // different flow, which is the one thing the round trip must not do.
    let mut flow = UxFlow::new("f1", "Bad ids");
    flow.steps = vec![FlowStep::new(id("A B"), "Space in id", FlowStepKind::Start)];
    let error = flow_to_mermaid(&flow).expect_err("refused");
    assert_eq!(
        error.kind,
        MermaidErrorKind::UnsafeStepId {
            id: "A B".to_string()
        }
    );
    assert!(error.to_string().contains("cannot be written as mermaid"));
}

#[test]
fn an_edge_to_nothing_is_refused_rather_than_invented() {
    let mut flow = UxFlow::new("f1", "Dangling");
    flow.steps = vec![FlowStep::new(id("A"), "Start", FlowStepKind::Start)];
    flow.edges = vec![FlowEdge::new(id("A"), id("missing"))];
    let error = flow_to_mermaid(&flow).expect_err("refused");
    assert_eq!(
        error.kind,
        MermaidErrorKind::DanglingEdge {
            from: "A".to_string(),
            to: "missing".to_string(),
        }
    );
}

#[test]
fn the_direction_is_accepted_and_dropped() {
    // The canvas lays a flow out vertically, which is this product's layout and
    // not mermaid's; storing the token would make a property of the drawing a
    // fact about the section.
    for direction in ["TD", "TB", "BT", "LR", "RL"] {
        let text = format!("flowchart {direction}\n  A([Start])\n");
        let flow = flow_from_mermaid(&text).expect("parsed");
        assert_eq!(flow.steps.len(), 1, "{direction}");
    }
}

#[test]
fn an_en_dash_in_a_label_is_kept() {
    // The label is text a person wrote, in whatever script they write in.
    let mut flow = UxFlow::new("f1", "Text");
    flow.steps = vec![FlowStep::new(
        id("A"),
        "Нажимает «Оформить» — сразу",
        FlowStepKind::Start,
    )];
    let text = flow_to_mermaid(&flow).expect("printed");
    let back = flow_from_mermaid(&text).expect("parsed");
    assert_eq!(back.steps[0].label, "Нажимает «Оформить» — сразу");
}

#[test]
fn every_error_says_what_is_wrong_and_where() {
    let cases: Vec<(MermaidErrorKind, &str)> = vec![
        (MermaidErrorKind::MissingHeader, "a flow must start with"),
        (
            MermaidErrorKind::MalformedNode {
                text: "A[".to_string(),
            },
            "could not read a step",
        ),
        (
            MermaidErrorKind::MalformedEdge {
                text: "A --> ?".to_string(),
            },
            "could not read a connection",
        ),
        (
            MermaidErrorKind::DuplicateStepId {
                id: "A".to_string(),
            },
            "declared twice",
        ),
        (
            MermaidErrorKind::UnsafeStepId {
                id: "A B".to_string(),
            },
            "cannot be written as mermaid",
        ),
        (
            MermaidErrorKind::DanglingEdge {
                from: "A".to_string(),
                to: "B".to_string(),
            },
            "names a step that is not in the flow",
        ),
    ];
    for (kind, expected) in cases {
        let message = MermaidError { line: 4, kind }.to_string();
        assert!(message.starts_with("line 4: "), "{message}");
        assert!(message.contains(expected), "{message}");
    }
}
