//! The generation canon as data (design/design-rules-canon.md).
//!
//! Until this existed, the rules lived in two places that could not be kept in
//! step: 8.8 KB of prose in the document, and a handful of hard-coded strings
//! inside the detectors. The prompt took two `do` and one `don't` per kit type
//! and none of the canon; the detectors knew ids nobody else could name.
//!
//! One list, then, read from three sides:
//!
//! - the prompt, through [`prompt_block`] — the model is told the rules it will
//!   be judged by, by id;
//! - the detectors, which report the same ids in their findings;
//! - a person, through the markdown document, which a test keeps in step with
//!   this list.
//!
//! A rule that cannot be checked is not here: `checking` states how it is
//! decided, and `NotDeterministic` means a model judgement rather than a
//! detector.

/// How binding a rule is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLevel {
    /// The screen is wrong without it.
    Required,
    /// Doing this is a defect.
    Forbidden,
    /// A deviation is allowed but must be a choice.
    Preferred,
}

impl RuleLevel {
    /// The word the prompt and the findings use.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuleLevel::Required => "required",
            RuleLevel::Forbidden => "forbidden",
            RuleLevel::Preferred => "preferred",
        }
    }
}

/// How a rule is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCheck {
    /// A detector decides it from the document alone.
    Deterministic,
    /// A structural walk decides it (counts, emptiness, duplication).
    Structural,
    /// Only a model can judge it — the verifier's half.
    NotDeterministic,
}

impl RuleCheck {
    /// The note the markdown document carries for this rule.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuleCheck::Deterministic => "Д",
            RuleCheck::Structural => "С",
            RuleCheck::NotDeterministic => "С-LLM",
        }
    }
}

/// One rule of the canon.
#[derive(Debug, Clone, Copy)]
pub struct CanonRule {
    /// Stable id, used in the prompt, in findings and in the document.
    pub id: &'static str,
    pub level: RuleLevel,
    /// The rule, in one sentence.
    pub text: &'static str,
    pub checking: RuleCheck,
}

/// The canon. Order is the order the prompt prints.
pub const CANON: &[CanonRule] = &[
    CanonRule {
        id: "C-01",
        level: RuleLevel::Required,
        text: "the primary action is filled Bondi #2D98B4 ($button/filled/accent/…)",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "C-02",
        level: RuleLevel::Forbidden,
        text: "Java #00BEC8 as a button fill — Java is the identity mark (logo, switch-on) only",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "C-03",
        level: RuleLevel::Required,
        text: "the canvas is #EEF1F5 ($layout/background/default)",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "C-04",
        level: RuleLevel::Required,
        text: "body text is #3F4146 (on-surface)",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "C-05",
        level: RuleLevel::Required,
        text: "destructive/error colour is #E53334",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "T-01",
        level: RuleLevel::Required,
        text: "Roboto everywhere; Roboto Mono only for codes and tabular figures",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "T-02",
        level: RuleLevel::Required,
        text: "body 14/400, buttons 14/500, headlines 18/600 and 16/600",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "T-03",
        level: RuleLevel::Preferred,
        text: "dense, calm typography — no decorative sizes outside the scale",
        checking: RuleCheck::NotDeterministic,
    },
    CanonRule {
        id: "L-01",
        level: RuleLevel::Required,
        text: "every desktop screen is Layout/Default (tpl-layout-default); the body goes in Main container (tpl-layout-main)",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "L-02",
        level: RuleLevel::Forbidden,
        text: "a second Layout, Sidebar or topbar on the page",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "L-03",
        level: RuleLevel::Required,
        text: "sidebar 251px (not 240–280), radius 16, white",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "L-04",
        level: RuleLevel::Required,
        text: "gutters 20 top/left, 16 right/bottom",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "L-05",
        level: RuleLevel::Forbidden,
        text: "fill_container carried through the root padding",
        checking: RuleCheck::NotDeterministic,
    },
    CanonRule {
        id: "L-06",
        level: RuleLevel::Required,
        text: "start from the ops-shell recipe — do not invent a shell",
        checking: RuleCheck::NotDeterministic,
    },
    CanonRule {
        id: "K-01",
        level: RuleLevel::Required,
        text: "Button: Large (32) Filled Accent; ghost has no chrome; outline is a 1px centre stroke",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-02",
        level: RuleLevel::Forbidden,
        text: "a 38px input — inputs are 236×32, radius 8, 1px centre stroke",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-03",
        level: RuleLevel::Required,
        text: "MenuButton 40×40 rx8, MenuItem 168 wide hug, no nested pad, no stroke.align",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-04",
        level: RuleLevel::Required,
        text: "StatusIndicator (ring, 16/20) in tables and statuses; Status (dot) only for sidebar presence",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-05",
        level: RuleLevel::Required,
        text: "Divider is a hairline fill ($sidebar/border/default), not a stroke",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-06",
        level: RuleLevel::Required,
        text: "Table: header 52, body 44 zebra, checkbox column 56",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-07",
        level: RuleLevel::Required,
        text: "Chip hug×20 pad 4/8 x-circle; Badge pill 17 rx 8.5 on $badge/… tokens",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-08",
        level: RuleLevel::Required,
        text: "Checkbox 20×20 rx6 — not round",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-09",
        level: RuleLevel::Required,
        text: "PaginationItem 32×32 rx8; digits are live text, arrows are icons",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-10",
        level: RuleLevel::Required,
        text: "Breadcrumbs come from molecule-breadcrumbs, not buttons",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-11",
        level: RuleLevel::Required,
        text: "ContextMenu 320 rx8 pad 8, shadow 6/8, 4px below its trigger",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "K-12",
        level: RuleLevel::Required,
        text: "Avatar overrides its initials — the disc is never swapped for an ellipse",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "G-01",
        level: RuleLevel::Required,
        text: "kit masters are instantiated as type:\"ref\" with descendants — never rebuilt as frames",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "G-02",
        level: RuleLevel::Forbidden,
        text: "pulling in the generic design-system composition (sidebar 240–280, pad 32, $color-accent)",
        checking: RuleCheck::Deterministic,
    },
    CanonRule {
        id: "G-03",
        level: RuleLevel::Required,
        text: "no empty top-level frames; no duplicated panels or shells",
        checking: RuleCheck::Structural,
    },
    CanonRule {
        id: "G-04",
        level: RuleLevel::Required,
        text: "variants are switched on the existing instance, not rebuilt",
        checking: RuleCheck::NotDeterministic,
    },
    CanonRule {
        id: "G-05",
        level: RuleLevel::Preferred,
        text: "nothing extra on the canvas — anything unasked for is a finding",
        checking: RuleCheck::NotDeterministic,
    },
    CanonRule {
        id: "S-01",
        level: RuleLevel::Required,
        text: "one request, one screen — a second root only when the request names several",
        checking: RuleCheck::Deterministic,
    },
];

/// The canon as the prompt carries it: one line per rule, id first.
///
/// One line each on purpose. The rules used to reach the model as prose it could
/// not be held to; a numbered list it can be — and a finding that names `C-02`
/// is a thing a person can look up.
pub fn prompt_block() -> String {
    let mut out = String::from(
        "DESIGN CANON — the rules this screen is judged by. Each finding names its id:\n",
    );
    for rule in CANON {
        out.push_str(&format!(
            "- {} {}: {}\n",
            rule.id,
            rule.level.as_str(),
            rule.text
        ));
    }
    out.trim_end().to_string()
}

/// The rules a detector is expected to answer (`Д`), for the coverage report.
pub fn deterministic_ids() -> Vec<&'static str> {
    CANON
        .iter()
        .filter(|rule| rule.checking == RuleCheck::Deterministic)
        .map(|rule| rule.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_is_named_once_and_says_how_it_is_checked() {
        let mut seen = std::collections::BTreeSet::new();
        for rule in CANON {
            assert!(seen.insert(rule.id), "duplicate rule id {}", rule.id);
            assert!(rule.id.len() >= 4, "{} has no usable id", rule.id);
            assert!(!rule.text.trim().is_empty(), "{} has no rule text", rule.id);
        }
        assert!(CANON.len() >= 28, "the canon lost rules: {}", CANON.len());
        assert!(
            !deterministic_ids().is_empty(),
            "a canon nobody can check is prose"
        );
    }

    #[test]
    fn the_markdown_document_stays_in_step_with_the_list() {
        // The document is what a person reads; this list is what the prompt and
        // the detectors use. A rule that exists in one and not the other is a
        // rule nobody can trust.
        let doc = include_str!("../../../design/design-rules-canon.md");
        for rule in CANON {
            assert!(
                doc.contains(rule.id),
                "{} is in the canon list but not in design/design-rules-canon.md",
                rule.id
            );
        }
    }

    #[test]
    fn a_rule_cannot_claim_a_detector_it_does_not_have() {
        // Both directions: every id the detector answers is a canon rule, and
        // every id it claims is one it also reports somewhere in its reasons.
        let source = include_str!("detectors/design_rules.rs");
        for id in crate::detectors::IMPLEMENTED_RULE_IDS {
            assert!(
                CANON.iter().any(|rule| rule.id == *id),
                "{id} is answered by a detector but is not in the canon"
            );
            assert!(
                source.contains(&format!("{id}:")),
                "{id} is listed as implemented but no finding carries it"
            );
            let rule = CANON.iter().find(|rule| rule.id == *id).expect("checked");
            // A detector may answer a "Д" rule or a structural one ("С"); what
            // it cannot do is answer a rule the canon leaves to judgement.
            assert!(
                rule.checking != RuleCheck::NotDeterministic,
                "{id} is answered by a detector, so the canon cannot leave it to the model"
            );
        }
    }

    #[test]
    fn the_prompt_block_names_every_rule_with_its_level() {
        let block = prompt_block();
        for rule in CANON {
            assert!(
                block.contains(&format!("{} {}:", rule.id, rule.level.as_str())),
                "{} is missing from the prompt block",
                rule.id
            );
        }
        assert!(
            block.contains("C-02 forbidden"),
            "a forbidden rule must read as forbidden: {block}"
        );
    }
}
