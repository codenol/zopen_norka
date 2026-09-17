//! "Is this reply cut off?" — a pure text judgement, no model, no I/O.
//!
//! The two hand measurements of 2026-09-16 kept hitting the same thing: a reply
//! that stops mid-statement while the turn still reports `APPLIED` + `done`
//! (issue #205). `RESULTS.md` (gq2) quotes it as `389 { against 380 }` and a
//! tail character inside a string literal, and leaves it out of the verdict
//! where the reply carries no document payload at all.
//!
//! This module reproduces that judgement so a run does not need a human eye,
//! and it is deliberately explicit about what it CANNOT say: a prose reply
//! (`Done — 1 subtask(s) succeeded …`, the orchestrator route) and an empty
//! reply (the route applied the turn server-side and streamed no text) get a
//! reason instead of a verdict, because neither has a document payload whose
//! completeness could be judged.
//!
//! Signal set (structural, drives [`TruncationReport::truncated`]):
//! - `unbalanced-braces(a/b)` — raw `{` against `}` count. Raw, not
//!   string-literal-aware, so the number stays comparable with the hand-written
//!   reports; a brace inside a string literal would look like a signal here.
//! - `unbalanced-parens(a/b)`, `unbalanced-brackets(a/b)` — same, for `()`/`[]`.
//! - `unterminated-call(byte N)` — the scan stack still holds an open paren at
//!   the end of the text: the classic `I(b38, {"type":"text", …` cut.
//! - `unterminated-code-fence` — an odd number of ``` fences: a JSON blueprint
//!   that opened a block and never closed it.
//! - `tail-char='X'` — the last non-space character is not a terminator
//!   (`;` `}` `]` `)` `"` `'` `.` `!` `?` `…` `:` `*` `-`). Only meaningful for
//!   a document payload, which is why prose never reaches this rule.
//!
//! Marker set (reported, never folded into `truncated`): whether the reply
//! carried the `<!-- APPLIED -->` marker the DSL route emits once it applied
//! the subtree, and how many root-level statements it emitted.

use serde::Serialize;

/// What kind of payload the reply carried — decides which signals apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplyShape {
    /// No reply text at all.
    Empty,
    /// Prose: a status line or a summary, no document payload.
    Prose,
    /// Pencil DSL: at least one line-start `I(...)` statement.
    Dsl,
    /// A fenced JSON blueprint (```` ```json ```` + an object/array body).
    JsonBlueprint,
}

/// One reply's truncation judgement.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TruncationReport {
    /// Which signals apply to this reply's shape.
    pub(crate) shape: ReplyShape,
    /// Structural signals — a non-empty list on a structured reply means the
    /// text cannot be a complete submission.
    pub(crate) signals: Vec<String>,
    /// `true` when [`Self::signals`] is non-empty on a structured reply.
    pub(crate) truncated: bool,
    /// Set when the shape makes the question unanswerable, with the reason. A
    /// verdict is never guessed for these: the scorecard prints `n/a`.
    pub(crate) not_applicable: Option<String>,
    /// The DSL route's applied marker — reported, not a truncation signal.
    pub(crate) applied_marker: bool,
}

impl TruncationReport {
    /// One-word state for the scorecard column.
    pub(crate) fn label(&self) -> &'static str {
        if self.not_applicable.is_some() {
            "n/a"
        } else if self.truncated {
            "TRUNCATED"
        } else {
            "clean"
        }
    }

    /// Every signal, comma-joined, for a scorecard detail line.
    pub(crate) fn signal_line(&self) -> String {
        self.signals.join(", ")
    }
}

const APPLIED_MARKERS: [&str; 2] = ["<!-- APPLIED -->", "<!--APPLIED-->"];

/// Judges one reply. Pure: text in, report out.
pub(crate) fn detect(reply: &str) -> TruncationReport {
    let applied_marker = APPLIED_MARKERS.iter().any(|marker| reply.contains(marker));
    let trimmed = reply.trim();
    if trimmed.is_empty() {
        return TruncationReport {
            shape: ReplyShape::Empty,
            signals: Vec::new(),
            truncated: false,
            not_applicable: Some(
                "the turn streamed no reply text: the route applied it server-side, so there is \
                 no document payload whose completeness could be judged"
                    .to_string(),
            ),
            applied_marker,
        };
    }

    let shape = shape_of(reply);
    if shape == ReplyShape::Prose {
        return TruncationReport {
            shape,
            signals: Vec::new(),
            truncated: false,
            not_applicable: Some(
                "the reply is prose with no document payload (the orchestrator route applies the \
                 design server-side): truncation of a payload that was never sent cannot be judged"
                    .to_string(),
            ),
            applied_marker,
        };
    }

    let body = strip_markers(reply);
    let body = body.trim_end();
    let mut signals = Vec::new();

    if body.matches('{').count() != body.matches('}').count() {
        signals.push(format!(
            "unbalanced-braces({}/{})",
            body.matches('{').count(),
            body.matches('}').count()
        ));
    }
    if body.matches('(').count() != body.matches(')').count() {
        signals.push(format!(
            "unbalanced-parens({}/{})",
            body.matches('(').count(),
            body.matches(')').count()
        ));
    }
    if body.matches('[').count() != body.matches(']').count() {
        signals.push(format!(
            "unbalanced-brackets({}/{})",
            body.matches('[').count(),
            body.matches(']').count()
        ));
    }
    if let Some(byte) = unterminated_call_byte(body) {
        signals.push(format!("unterminated-call(byte {byte})"));
    }
    if body.matches("```").count() % 2 == 1 {
        signals.push("unterminated-code-fence".to_string());
    }
    if let Some(tail) = tail_char(body) {
        if !is_terminator(tail) {
            signals.push(format!("tail-char={tail:?}"));
        }
    }

    TruncationReport {
        shape,
        truncated: !signals.is_empty(),
        signals,
        not_applicable: None,
        applied_marker,
    }
}

/// Which payload shape the reply carries.
fn shape_of(reply: &str) -> ReplyShape {
    let has_statement = reply
        .lines()
        .any(|line| line.trim_start().starts_with("I("));
    if has_statement || APPLIED_MARKERS.iter().any(|m| reply.contains(m)) {
        return ReplyShape::Dsl;
    }
    if reply.contains("```json") && (reply.contains('{') || reply.contains('[')) {
        return ReplyShape::JsonBlueprint;
    }
    ReplyShape::Prose
}

/// Drops the applied markers and the `<step …>` / `</step>` progress tags so
/// they cannot skew the brace/paren counts or the tail character (the hand
/// prototypes stripped the same two things).
fn strip_markers(reply: &str) -> String {
    let mut out = reply.to_string();
    for marker in APPLIED_MARKERS {
        out = out.replace(marker, "");
    }
    let mut cleaned = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let name = after.strip_prefix('/').unwrap_or(after);
        if !name.starts_with("step") {
            // Not a progress tag: keep the `<` and carry on after it.
            cleaned.push_str(&rest[..start + 1]);
            rest = after;
            continue;
        }
        cleaned.push_str(&rest[..start]);
        match after.find('>') {
            Some(rel) => rest = &after[rel + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    cleaned.push_str(rest);
    cleaned
}

/// Byte offset of the first paren the scan stack leaves open at the end of the
/// text, when that paren opened a call (`I(...)`). `None` = none left open.
fn unterminated_call_byte(body: &str) -> Option<usize> {
    // A tiny scanner: `"` toggles string state, `\` escapes the next char, an
    // unclosed `(` is pushed with its byte offset.
    let bytes = body.as_bytes();
    let mut stack: Vec<usize> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'(' if !in_string => stack.push(index),
            b')' if !in_string => {
                stack.pop();
            }
            _ => {}
        }
    }
    let first = *stack.first()?;
    let preceded_by_call = body[..first]
        .trim_end()
        .chars()
        .next_back()
        .map(|c| c.is_alphabetic() || c == '_')
        .unwrap_or(false);
    preceded_by_call.then_some(first)
}

/// The last non-space character of the text.
fn tail_char(body: &str) -> Option<char> {
    body.chars().next_back()
}

/// Characters that can legitimately end a complete reply.
fn is_terminator(c: char) -> bool {
    matches!(
        c,
        ';' | '}' | ']' | ')' | '"' | '\'' | '.' | '!' | '?' | ':' | '…' | '*' | '-' | '`'
    )
}
