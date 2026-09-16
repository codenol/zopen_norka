//! The truncation detector, tested on the shapes the two hand passes actually
//! saw — no model, no daemon: the detector is pure text logic.

use super::truncation::{detect, ReplyShape};

/// The complete DSL statement a well-behaved reply ends with.
const COMPLETE_DSL: &str = "\
I(null, {id:\"n20\", type:\"frame\", name:\"Login screen\", children:[\n\
I(n20, {id:\"n21\", type:\"text\", name:\"Title\", content:\"Вход\"});\n\
] });\n\
<!-- APPLIED -->\n";

/// The cut mid-statement shape `RESULTS.md` (gq2) quotes for prompt 2: the
/// stream stops inside a string literal, before the applied marker.
const TRUNCATED_DSL: &str = "\
I(null, {id:\"n32\", type:\"frame\", name:\"Recipe/Ops servers screen\", children:[\n\
I(n32, {id:\"n40\", type:\"frame\", name:\"Table\", children:[\n\
I(n40, {id:\"n41\", type:\"text\", name:\"Row\", content:\"srv-stage-01\", fontFamily:\"Rob";

/// The prose reply the orchestrator route answers with (prompt 1 in gq2).
const PROSE: &str =
    "\n\nDone — 1 subtask(s) succeeded, 0 failed, 14 paintable node(s) (1 forest root(s)).\n";

#[test]
fn complete_dsl_reply_is_clean() {
    let report = detect(COMPLETE_DSL);
    assert_eq!(report.shape, ReplyShape::Dsl);
    assert!(!report.truncated, "signals: {:?}", report.signals);
    assert!(report.signals.is_empty());
    assert!(report.applied_marker);
    assert_eq!(report.label(), "clean");
    assert!(report.not_applicable.is_none());
}

#[test]
fn dsl_cut_mid_statement_is_truncated() {
    let report = detect(TRUNCATED_DSL);
    assert_eq!(report.shape, ReplyShape::Dsl);
    assert!(report.truncated);
    let signals = report.signal_line();
    assert!(signals.contains("unbalanced-braces"), "{signals}");
    assert!(signals.contains("unbalanced-parens"), "{signals}");
    assert!(signals.contains("unterminated-call"), "{signals}");
    assert!(signals.contains("tail-char"), "{signals}");
    // The applied marker is absent, which is reported but is not itself the
    // truncation verdict.
    assert!(!report.applied_marker);
    assert_eq!(report.label(), "TRUNCATED");
}

#[test]
fn dsl_without_applied_marker_but_balanced_is_not_truncated() {
    let reply = COMPLETE_DSL.replace("<!-- APPLIED -->", "");
    let report = detect(&reply);
    assert!(!report.truncated, "signals: {:?}", report.signals);
    assert!(!report.applied_marker);
}

#[test]
fn prose_reply_is_not_applicable_rather_than_clean_or_cut() {
    let report = detect(PROSE);
    assert_eq!(report.shape, ReplyShape::Prose);
    assert!(!report.truncated);
    assert!(report.signals.is_empty());
    assert_eq!(report.label(), "n/a");
    let reason = report.not_applicable.expect("a reason, not a guess");
    assert!(reason.contains("no document payload"), "{reason}");
}

#[test]
fn empty_reply_is_not_applicable() {
    let report = detect("   \n\n");
    assert_eq!(report.shape, ReplyShape::Empty);
    assert!(!report.truncated);
    assert_eq!(report.label(), "n/a");
    assert!(report
        .not_applicable
        .expect("a reason")
        .contains("no reply text"));
}

#[test]
fn truncated_json_blueprint_is_truncated() {
    // The prompt-4 shape: `<step>` prose, a fenced blueprint, cut inside an
    // object while the fence is never closed.
    let reply = "\
<step>Building the ops-console shell.</step>\n\n\
```json\n\
[\n  {\n    \"id\": \"page-settings\",\n    \"type\": \"frame\",\n    \"name\": \"Settings\",\n    \
\"children\": [\n      {\n        \"id\": \"nav\",\n        \"fill\": [{ \"type";
    let report = detect(reply);
    assert_eq!(report.shape, ReplyShape::JsonBlueprint);
    assert!(report.truncated);
    let signals = report.signal_line();
    assert!(signals.contains("unbalanced-braces"), "{signals}");
    assert!(signals.contains("unterminated-code-fence"), "{signals}");
    assert!(signals.contains("unbalanced-brackets"), "{signals}");
    assert!(signals.contains("tail-char"), "{signals}");
}

#[test]
fn complete_json_blueprint_is_clean() {
    let reply = "\
<step>Two cards.</step>\n\n\
```json\n\
[\n  { \"id\": \"card-a\", \"type\": \"frame\", \"name\": \"Card A\" },\n  \
{ \"id\": \"card-b\", \"type\": \"frame\", \"name\": \"Card B\" }\n]\n\
```\n";
    let report = detect(reply);
    assert_eq!(report.shape, ReplyShape::JsonBlueprint);
    assert!(!report.truncated, "signals: {:?}", report.signals);
    assert_eq!(report.label(), "clean");
}

#[test]
fn progress_tags_do_not_skew_the_counts() {
    // `<step …>` tags carry no braces, but a reply ending in a closing tag
    // would otherwise look like a `tail-char='>'` cut.
    let reply = format!("<step>One screen.</step>\n{COMPLETE_DSL}</step>");
    let report = detect(&reply);
    assert!(!report.truncated, "signals: {:?}", report.signals);
}
