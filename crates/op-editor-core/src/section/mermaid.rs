//! Mermaid: how a flow is written down and read back.
//!
//! Structure is the source and mermaid is the notation — the operator's
//! decision — so this module has two directions and a strict grammar:
//!
//! - [`flow_to_mermaid`] prints a flow.
//! - [`flow_from_mermaid`] reads one back, refusing anything outside the subset
//!   below rather than guessing.
//!
//! ## Why a subset, and why it refuses instead of guessing
//!
//! Mermaid is a large language (subgraphs, styling, click handlers, several
//! arrows, entity codes, `&` fan-out) and almost none of it survives a
//! round-trip through a step graph. A parser that accepted the whole language
//! while keeping a fraction of it would be a parser that silently drops what a
//! person wrote — an agent asked for a flow would get back something plausible
//! and different. So the grammar is exactly what this product writes, plus the
//! shapes a person naturally types, and everything else is a typed error naming
//! the line.
//!
//! The subset:
//!
//! ```text
//! flowchart TD            header — `flowchart` or `graph`, with a direction
//! %% a comment            ignored, anywhere
//! A([Entry])              a start   — the stadium shape
//! B[Opens the cart]       a step    — a rectangle
//! C{Has an account?}      a decision— a rhombus
//! Z((Done))               an end     — a circle
//! A --> B                 an edge, one arrow
//! B -->|yes| C            an edge with a label
//! A --> B --> C           a chain, which is the same as two edges
//! A["A label with ] in it"]   quoted when the text needs it
//! ```
//!
//! ## What the notation does not carry
//!
//! Two things belong to the section rather than to the diagram, and neither is
//! invented as syntax here:
//!
//! - **The flow's own id and name.** A flow parsed from text is given its
//!   identity by the section it is saved into.
//! - **Which mockup a step is.** Mermaid has nowhere to say it, and a node id
//!   written into a diagram a person edits would be stale the moment anything
//!   was renamed. [`UxFlow::adopt_screens_from`] is how the links survive an
//!   edit: the steps that are still there keep the mockups they had.
//!
//! The **direction** token is accepted and dropped. The canvas lays a flow out
//! vertically, which is this product's layout and not mermaid's (issue #59);
//! storing the token would make a property of the drawing a fact about the
//! section.

use crate::section::flow::{FlowEdge, FlowStep, FlowStepId, FlowStepKind, UxFlow};

/// Arrow this notation uses. One kind: a UX flow is directed, and a second
/// spelling for the same thing is a second thing to get wrong.
const ARROW: &[char] = &['-', '-', '>'];

/// Why mermaid text could not be read, or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidError {
    /// 1-based line the problem is on. `1` for a header that is missing
    /// altogether, which is a statement about the text rather than a place in
    /// it.
    pub line: usize,
    pub kind: MermaidErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidErrorKind {
    /// The text does not open with `flowchart <direction>` or `graph
    /// <direction>`.
    MissingHeader,
    /// A node could not be read: a bad id, an unclosed or nested shape, or
    /// trailing text a statement cannot explain.
    MalformedNode { text: String },
    /// An arrow or an edge label could not be read, or a chain ended in the
    /// middle of one.
    MalformedEdge { text: String },
    /// Two declarations of one id disagree about what that step is. Merging them
    /// would be picking one at random.
    DuplicateStepId { id: String },
    /// A step id this notation cannot print — writing it down would produce text
    /// that does not read back as the same flow.
    UnsafeStepId { id: String },
    /// An edge naming a step the flow does not have. Mermaid has no way to say
    /// "an edge to nothing": printed, it would come back as a new step.
    DanglingEdge { from: String, to: String },
}

impl std::fmt::Display for MermaidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: ", self.line)?;
        match &self.kind {
            MermaidErrorKind::MissingHeader => {
                f.write_str("a flow must start with `flowchart TD` (or `graph TD`)")
            }
            MermaidErrorKind::MalformedNode { text } => {
                write!(f, "could not read a step from `{text}`")
            }
            MermaidErrorKind::MalformedEdge { text } => {
                write!(f, "could not read a connection from `{text}`")
            }
            MermaidErrorKind::DuplicateStepId { id } => {
                write!(f, "step `{id}` is declared twice, differently")
            }
            MermaidErrorKind::UnsafeStepId { id } => write!(
                f,
                "step id `{id}` cannot be written as mermaid (letters, digits, `_` and `-` only, not starting with `-`)"
            ),
            MermaidErrorKind::DanglingEdge { from, to } => {
                write!(f, "connection `{from} --> {to}` names a step that is not in the flow")
            }
        }
    }
}

impl std::error::Error for MermaidError {}

/// Print a flow as mermaid.
///
/// Refuses two things a flow can hold but the notation cannot say, so that
/// printed text always reads back as the same graph: a step id outside the
/// grammar, and an edge to a step the flow does not have.
pub fn flow_to_mermaid(flow: &UxFlow) -> Result<String, MermaidError> {
    for step in &flow.steps {
        if !id_is_safe(step.id.as_str()) {
            return Err(MermaidError {
                line: 1,
                kind: MermaidErrorKind::UnsafeStepId {
                    id: step.id.to_string(),
                },
            });
        }
    }
    for edge in &flow.edges {
        let known = |id: &FlowStepId| flow.steps.iter().any(|step| &step.id == id);
        if !known(&edge.from) || !known(&edge.to) {
            return Err(MermaidError {
                line: 1,
                kind: MermaidErrorKind::DanglingEdge {
                    from: edge.from.to_string(),
                    to: edge.to.to_string(),
                },
            });
        }
    }

    let mut out = String::from("flowchart TD\n");
    for step in &flow.steps {
        let label = print_label(&step.label);
        let shape = match step.kind {
            FlowStepKind::Start => format!("([{label}])"),
            FlowStepKind::Step => format!("[{label}]"),
            FlowStepKind::Decision => format!("{{{label}}}"),
            FlowStepKind::End => format!("(({label}))"),
        };
        out.push_str(&format!("  {} {shape}\n", step.id));
    }
    for edge in &flow.edges {
        match &edge.label {
            Some(label) => out.push_str(&format!(
                "  {} -->|{}| {}\n",
                edge.from,
                print_edge_label(label),
                edge.to
            )),
            None => out.push_str(&format!("  {} --> {}\n", edge.from, edge.to)),
        }
    }
    Ok(out)
}

/// Read a flow from mermaid text.
///
/// The flow comes back with an empty id and name and no mockups attached: see
/// the module docs for where those come from.
pub fn flow_from_mermaid(text: &str) -> Result<UxFlow, MermaidError> {
    let mut steps: Vec<StepEntry> = Vec::new();
    let mut edges: Vec<FlowEdge> = Vec::new();
    let mut header_seen = false;

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        for statement in split_statements(&strip_comment(raw)) {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            if !header_seen {
                if !is_header(statement) {
                    return Err(MermaidError {
                        line,
                        kind: MermaidErrorKind::MissingHeader,
                    });
                }
                header_seen = true;
                continue;
            }
            parse_statement(statement, line, &mut steps, &mut edges)?;
        }
    }

    if !header_seen {
        return Err(MermaidError {
            line: 1,
            kind: MermaidErrorKind::MissingHeader,
        });
    }

    Ok(UxFlow {
        id: String::new(),
        name: String::new(),
        steps: steps.into_iter().map(|entry| entry.step).collect(),
        edges,
    })
}

/// One step while parsing: the step, and whether the text declared it.
///
/// A bare id (`A --> B` with no shape anywhere) still makes a step — that is
/// what mermaid does — but it is a WEAKER statement than a declaration, so a
/// later `B[Opens the cart]` is an upgrade rather than a duplicate.
struct StepEntry {
    step: FlowStep,
    declared: bool,
}

impl StepEntry {
    fn declare(&mut self, kind: FlowStepKind, label: String) -> Result<(), MermaidError> {
        if self.declared {
            if self.step.kind == kind && self.step.label == label {
                // The same declaration twice is what a text written for a human
                // reader looks like; it says nothing new.
                return Ok(());
            }
            return Err(MermaidError {
                line: 0,
                kind: MermaidErrorKind::DuplicateStepId {
                    id: self.step.id.to_string(),
                },
            });
        }
        self.step.kind = kind;
        self.step.label = label;
        self.declared = true;
        Ok(())
    }
}

/// Find the step, or add one as an undeclared bare reference.
fn entry_for<'a>(steps: &'a mut Vec<StepEntry>, id: &str) -> &'a mut StepEntry {
    let position = match steps.iter().position(|entry| entry.step.id.as_str() == id) {
        Some(position) => position,
        None => {
            // `parse_spec` only produces ids that start with a letter, a digit
            // or `_`, so this cannot be empty.
            let id = FlowStepId::new(id).expect("a parsed id is never empty");
            steps.push(StepEntry {
                step: FlowStep::new(id, "", FlowStepKind::Step),
                declared: false,
            });
            steps.len() - 1
        }
    };
    &mut steps[position]
}

/// One node as the text wrote it: its id, and its shape when it had one.
#[derive(Clone)]
struct Spec {
    id: String,
    shape: Option<(FlowStepKind, String)>,
}

/// Read one statement: a declaration, an edge, or a chain of both.
fn parse_statement(
    statement: &str,
    line: usize,
    steps: &mut Vec<StepEntry>,
    edges: &mut Vec<FlowEdge>,
) -> Result<(), MermaidError> {
    let chars: Vec<char> = statement.chars().collect();
    let mut pos = 0usize;
    let first = parse_spec(&chars, &mut pos, statement, line)?;
    let mut previous = first.clone();
    let mut arrows = 0usize;

    loop {
        skip_spaces(&chars, &mut pos);
        // `:::class` is mermaid's styling hook: read and dropped, because a
        // class is a property of a drawing and this graph has no such property.
        if starts_with(&chars, pos, &[':', ':', ':']) {
            pos += 3;
            advance_while(&chars, &mut pos, |ch| {
                ch.is_alphanumeric() || ch == '_' || ch == '-'
            });
            skip_spaces(&chars, &mut pos);
        }
        if pos >= chars.len() {
            break;
        }
        if !starts_with(&chars, pos, ARROW) {
            return Err(if arrows == 0 {
                MermaidError {
                    line,
                    kind: MermaidErrorKind::MalformedNode {
                        text: statement.to_string(),
                    },
                }
            } else {
                MermaidError {
                    line,
                    kind: MermaidErrorKind::MalformedEdge {
                        text: statement.to_string(),
                    },
                }
            });
        }
        pos += ARROW.len();
        skip_spaces(&chars, &mut pos);

        let mut label = None;
        if chars.get(pos) == Some(&'|') {
            pos += 1;
            let raw = read_until_quote(&chars, &mut pos, statement, line)?;
            skip_spaces(&chars, &mut pos);
            if chars.get(pos) != Some(&'|') {
                return Err(MermaidError {
                    line,
                    kind: MermaidErrorKind::MalformedEdge {
                        text: statement.to_string(),
                    },
                });
            }
            pos += 1;
            skip_spaces(&chars, &mut pos);
            if !raw.trim().is_empty() {
                label = Some(raw.trim().to_string());
            }
        }

        let next = parse_spec(&chars, &mut pos, statement, line)?;
        register(steps, &previous, line)?;
        register(steps, &next, line)?;
        let from = FlowStepId::new(previous.id.clone()).expect("ids are non-empty");
        let to = FlowStepId::new(next.id.clone()).expect("ids are non-empty");
        let mut edge = FlowEdge::new(from, to);
        edge.label = label;
        edges.push(edge);
        arrows += 1;
        previous = next;
    }

    if arrows == 0 {
        register(steps, &first, line)?;
    }
    Ok(())
}

/// Put a spec into the step list, declaring or upgrading it as it says.
fn register(steps: &mut Vec<StepEntry>, spec: &Spec, line: usize) -> Result<(), MermaidError> {
    let entry = entry_for(steps, &spec.id);
    match &spec.shape {
        Some((kind, label)) => entry.declare(*kind, label.clone()).map_err(|mut error| {
            error.line = line;
            error
        }),
        // A bare id is a step whose label is its own id — mermaid's own rule,
        // and the reason `A --> B` is a flow and not an error.
        None => {
            if !entry.declared && entry.step.label.is_empty() {
                entry.step.label = entry.step.id.to_string();
            }
            Ok(())
        }
    }
}

/// Read `id[shape]`, `id([shape])`, `id((shape))`, `id{shape}` or a bare `id`.
fn parse_spec(
    chars: &[char],
    pos: &mut usize,
    statement: &str,
    line: usize,
) -> Result<Spec, MermaidError> {
    let malformed = || MermaidError {
        line,
        kind: MermaidErrorKind::MalformedNode {
            text: statement.to_string(),
        },
    };
    let start = *pos;
    let first = *chars.get(*pos).ok_or_else(malformed)?;
    if !(first.is_alphanumeric() || first == '_') {
        return Err(malformed());
    }
    *pos += 1;
    advance_while(chars, pos, |ch| {
        ch.is_alphanumeric() || ch == '_' || ch == '-'
    });
    let id: String = chars[start..*pos].iter().collect();
    skip_spaces(chars, pos);

    let shape = if starts_with(chars, *pos, &['(', '[']) {
        *pos += 2;
        let label = read_label(chars, pos, &[']', ')'], statement, line)?;
        (FlowStepKind::Start, label)
    } else if starts_with(chars, *pos, &['(', '(']) {
        *pos += 2;
        let label = read_label(chars, pos, &[')', ')'], statement, line)?;
        (FlowStepKind::End, label)
    } else if chars.get(*pos) == Some(&'(') {
        // A single rounded rectangle: a step a person drew that way. It is a
        // shape and not a kind — printing turns it into the plain rectangle.
        *pos += 1;
        let label = read_label(chars, pos, &[')'], statement, line)?;
        (FlowStepKind::Step, label)
    } else if chars.get(*pos) == Some(&'[') {
        *pos += 1;
        let label = read_label(chars, pos, &[']'], statement, line)?;
        (FlowStepKind::Step, label)
    } else if chars.get(*pos) == Some(&'{') {
        *pos += 1;
        let label = read_label(chars, pos, &['}'], statement, line)?;
        (FlowStepKind::Decision, label)
    } else {
        return Ok(Spec { id, shape: None });
    };
    skip_spaces(chars, pos);
    Ok(Spec {
        id,
        shape: Some(shape),
    })
}

/// Read a shape's label: quoted with escapes, or raw up to the closing
/// delimiter. Delimiters do not nest — mermaid's do not either.
fn read_label(
    chars: &[char],
    pos: &mut usize,
    closing: &[char],
    statement: &str,
    line: usize,
) -> Result<String, MermaidError> {
    let malformed = || MermaidError {
        line,
        kind: MermaidErrorKind::MalformedNode {
            text: statement.to_string(),
        },
    };
    skip_spaces(chars, pos);
    if chars.get(*pos) == Some(&'"') {
        let raw = read_quoted(chars, pos, statement, line)?;
        skip_spaces(chars, pos);
        if !starts_with(chars, *pos, closing) {
            return Err(malformed());
        }
        *pos += closing.len();
        return Ok(raw);
    }
    let start = *pos;
    while *pos < chars.len() && !starts_with(chars, *pos, closing) {
        if chars[*pos] == '"' {
            return Err(malformed());
        }
        *pos += 1;
    }
    if *pos >= chars.len() {
        return Err(malformed());
    }
    let raw: String = chars[start..*pos].iter().collect();
    *pos += closing.len();
    Ok(raw.trim().to_string())
}

/// Read a quoted string, resolving `\\`, `\"` and `\n`.
fn read_quoted(
    chars: &[char],
    pos: &mut usize,
    statement: &str,
    line: usize,
) -> Result<String, MermaidError> {
    let malformed = || MermaidError {
        line,
        kind: MermaidErrorKind::MalformedNode {
            text: statement.to_string(),
        },
    };
    *pos += 1; // opening quote
    let mut out = String::new();
    while *pos < chars.len() {
        match chars[*pos] {
            '"' => {
                *pos += 1;
                return Ok(out);
            }
            '\\' => {
                *pos += 1;
                match chars.get(*pos) {
                    Some('n') => out.push('\n'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    _ => return Err(malformed()),
                }
                *pos += 1;
            }
            ch => {
                out.push(ch);
                *pos += 1;
            }
        }
    }
    Err(malformed())
}

/// Read up to the next unescaped closing quote, for `-->|label|`.
fn read_until_quote(
    chars: &[char],
    pos: &mut usize,
    statement: &str,
    line: usize,
) -> Result<String, MermaidError> {
    let malformed = || MermaidError {
        line,
        kind: MermaidErrorKind::MalformedEdge {
            text: statement.to_string(),
        },
    };
    skip_spaces(chars, pos);
    if chars.get(*pos) == Some(&'"') {
        return read_quoted(chars, pos, statement, line).map_err(|_| malformed());
    }
    let start = *pos;
    while *pos < chars.len() && chars[*pos] != '|' {
        if chars[*pos] == '"' {
            return Err(malformed());
        }
        *pos += 1;
    }
    if *pos >= chars.len() {
        return Err(malformed());
    }
    Ok(chars[start..*pos].iter().collect())
}

/// `flowchart TD` / `graph LR` and nothing else.
fn is_header(statement: &str) -> bool {
    let mut words = statement.split_whitespace();
    let Some(kind) = words.next() else {
        return false;
    };
    if kind != "flowchart" && kind != "graph" {
        return false;
    }
    match words.next() {
        Some(direction) => {
            matches!(direction, "TD" | "TB" | "BT" | "LR" | "RL") && words.next().is_none()
        }
        // A missing direction is legal mermaid and means top-down.
        None => true,
    }
}

/// Drop a `%%` comment, ignoring `%%` inside a quoted label.
fn strip_comment(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut pos = 0usize;
    let mut in_quotes = false;
    while pos < chars.len() {
        match chars[pos] {
            '\\' if in_quotes => pos += 2,
            '"' => {
                in_quotes = !in_quotes;
                pos += 1;
            }
            '%' if !in_quotes && starts_with(&chars, pos, &['%', '%']) => {
                return chars[..pos].iter().collect();
            }
            _ => pos += 1,
        }
    }
    line.to_string()
}

/// Split a line into statements on `;`, outside quotes and outside shapes.
fn split_statements(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut pos = 0usize;
    let mut in_quotes = false;
    let mut depth = 0i32;
    while pos < chars.len() {
        let ch = chars[pos];
        match ch {
            '\\' if in_quotes => {
                current.push(ch);
                if let Some(next) = chars.get(pos + 1) {
                    current.push(*next);
                    pos += 2;
                    continue;
                }
                pos += 1;
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
                pos += 1;
            }
            '[' | '{' | '(' if !in_quotes => {
                depth += 1;
                current.push(ch);
                pos += 1;
            }
            ']' | '}' | ')' if !in_quotes => {
                depth -= 1;
                current.push(ch);
                pos += 1;
            }
            ';' if !in_quotes && depth <= 0 => {
                out.push(std::mem::take(&mut current));
                pos += 1;
            }
            _ => {
                current.push(ch);
                pos += 1;
            }
        }
    }
    out.push(current);
    out
}

/// Whether an id can be printed and read back unchanged.
fn id_is_safe(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_alphanumeric() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
}

/// Print a label, quoting it when the unquoted form would not read back.
fn print_label(label: &str) -> String {
    let needs_quotes = label.is_empty()
        || label.trim() != label
        || label.chars().any(|ch| {
            matches!(
                ch,
                '[' | ']' | '{' | '}' | '(' | ')' | '"' | '|' | '\n' | '%' | ';'
            )
        })
        || label.contains("-->");
    if !needs_quotes {
        return label.to_string();
    }
    let mut out = String::from("\"");
    for ch in label.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// An edge label needs no quoting of its own — `|` cannot appear inside one
/// unquoted — but a `|`, a newline or a stray quote still has to be spelled.
fn print_edge_label(label: &str) -> String {
    if label.contains('|') || label.contains('"') || label.contains('\n') || label.trim() != label {
        return print_label(label);
    }
    label.to_string()
}

fn skip_spaces(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn starts_with(chars: &[char], pos: usize, needle: &[char]) -> bool {
    pos + needle.len() <= chars.len() && chars[pos..pos + needle.len()] == *needle
}

fn advance_while(chars: &[char], pos: &mut usize, keep: impl Fn(char) -> bool) {
    while *pos < chars.len() && keep(chars[*pos]) {
        *pos += 1;
    }
}

#[cfg(test)]
#[path = "mermaid_tests.rs"]
mod tests;
