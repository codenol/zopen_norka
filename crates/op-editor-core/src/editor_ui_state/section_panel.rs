//! The Section block in the property panel: what a section was built from, and
//! what it says.
//!
//! ## Why the answer lives here and the question does not
//!
//! A section's properties are a row in the daemon's store, not a field of the
//! `.op` file — see `op_editor_core::section` on why — so the panel cannot read
//! them off the document it already has. What it can do is say what it was
//! TOLD, which is this state: the properties as they came back, the section
//! they came back for, and whether an answer arrived at all.
//!
//! That last part is the one worth being exact about. "Nobody has written
//! anything about this section" and "we have not asked yet" and "the daemon
//! could not be asked" are three different facts, and a panel that paints the
//! same empty block for all three tells a designer their summary is gone when
//! nobody touched it. So the state carries `read` and `failed` beside the
//! properties rather than inferring them from emptiness.
//!
//! ## Why the link states are computed elsewhere and kept here
//!
//! A link is in sync when the analytics it names still hashes to what the link
//! recorded AND the section's own screens do too. The first half lives in
//! another store and the second half in the document the caller is holding, so
//! neither this module nor the widget can work it out alone; the host that has
//! both answers it and puts the result here. What this module owns is that the
//! answer is stored per link, so paint is a lookup rather than a walk.

use jian_core::text_input::TextInputState;

use crate::section::{AnalyticsLink, LinkState, SectionProperties, SectionSummary};
use crate::NodeId;

/// One of the four questions the summary answers.
///
/// A named field rather than an index into the summary: paint, hit-test and the
/// commit all walk the same list, and an index is a place for the three to
/// disagree after somebody reorders the questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryField {
    WhatItIs,
    WhereToLook,
    UseCases,
    WhatToCheck,
}

impl SummaryField {
    /// Every question, in the order the panel paints them.
    pub const ALL: [Self; 4] = [
        Self::WhatItIs,
        Self::WhereToLook,
        Self::UseCases,
        Self::WhatToCheck,
    ];

    /// The i18n key of the question.
    pub const fn i18n_key(self) -> &'static str {
        match self {
            Self::WhatItIs => "section.summary.whatItIs",
            Self::WhereToLook => "section.summary.whereToLook",
            Self::UseCases => "section.summary.useCases",
            Self::WhatToCheck => "section.summary.whatToCheck",
        }
    }

    /// What the summary says about this question.
    pub fn read(self, summary: &SectionSummary) -> &str {
        match self {
            Self::WhatItIs => &summary.what_it_is,
            Self::WhereToLook => &summary.where_to_look,
            Self::UseCases => &summary.use_cases,
            Self::WhatToCheck => &summary.what_to_check,
        }
    }

    /// Write an answer into the summary.
    pub fn write(self, summary: &mut SectionSummary, value: String) {
        match self {
            Self::WhatItIs => summary.what_it_is = value,
            Self::WhereToLook => summary.where_to_look = value,
            Self::UseCases => summary.use_cases = value,
            Self::WhatToCheck => summary.what_to_check = value,
        }
    }
}

/// One analytics document attached to the section, with the state of the link.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionLink {
    /// The link as the section stores it: the asset's key, the name it had when
    /// the link was made, and the two fingerprints.
    pub link: AnalyticsLink,
    /// Whether it still points at what it was made from.
    pub state: LinkState,
}

/// What the panel knows about the selected section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionPanelState {
    /// The section these properties belong to. `None` when the selection is not
    /// a section, which is also when the block does not paint.
    pub node: Option<NodeId>,
    /// What the daemon answered. The empty shape while an answer is on its way,
    /// which is why [`Self::read`] exists.
    pub properties: SectionProperties,
    /// Whether the answer for [`Self::node`] has arrived.
    pub read: bool,
    /// Whether the read failed. The panel says so rather than showing an empty
    /// section it cannot vouch for.
    pub failed: bool,
    /// The attached analytics, each with the state of its link.
    pub links: Vec<SectionLink>,
    /// The question being answered, when one is.
    pub focus: Option<SummaryField>,
    /// What is being typed into it. Seeded from the stored answer when the
    /// field takes focus, so a person edits what is there rather than an empty
    /// box where their sentence used to be.
    pub draft: TextInputState,
    /// Whether the draft differs from what is stored.
    pub dirty: bool,
    /// A save is on its way to the daemon.
    pub saving: bool,
    /// The last save failed, and the draft is still here to retry.
    pub save_failed: bool,
    /// Properties waiting for the host to send. The widget layer has no HTTP,
    /// so a commit queues the write and the host drains it — the same
    /// arrangement the Share dialog uses for its own requests.
    pub pending_save: Option<SectionProperties>,
}

impl SectionPanelState {
    /// Point the panel at a section, forgetting the previous section's answer.
    ///
    /// Called when the selection changes. The properties are dropped rather
    /// than kept: a block that showed the last section's summary while the next
    /// one is being read would be a lie told for exactly as long as the request
    /// takes.
    pub fn select(&mut self, node: Option<NodeId>) {
        if self.node == node {
            return;
        }
        self.node = node;
        self.properties = SectionProperties::empty();
        self.links.clear();
        self.read = false;
        self.failed = false;
        self.focus = None;
        self.draft.set_text("");
        self.dirty = false;
        self.saving = false;
        self.save_failed = false;
        self.pending_save = None;
    }

    /// Record the daemon's answer for `node`.
    ///
    /// Ignored when the answer is for a section the panel has moved away from:
    /// requests race, and the slower one must not overwrite what the newer
    /// selection is showing.
    pub fn apply(&mut self, node: &NodeId, properties: SectionProperties, links: Vec<SectionLink>) {
        if self.node.as_ref() != Some(node) {
            return;
        }
        self.properties = properties;
        self.links = links;
        self.read = true;
        self.failed = false;
        // A draft somebody is typing is theirs: a read that lands while a field
        // has focus refreshes what is stored without overwriting the sentence
        // being written over it.
        if let Some(field) = self.focus {
            self.dirty = field.read(&self.properties.summary) != self.draft.text();
        }
    }

    /// Record that the read failed for `node`.
    pub fn fail(&mut self, node: &NodeId) {
        if self.node.as_ref() != Some(node) {
            return;
        }
        self.properties = SectionProperties::empty();
        self.links.clear();
        self.read = false;
        self.failed = true;
    }

    /// Whether this block paints at all — a section is selected.
    pub fn is_visible(&self) -> bool {
        self.node.is_some()
    }

    /// Take focus on one question, seeding the draft from what is stored.
    ///
    /// Refused while a save is in flight: the answer on its way back would
    /// otherwise land on top of a draft somebody had already started.
    pub fn focus_field(&mut self, field: SummaryField, now_ms: u64) -> bool {
        if self.saving {
            return false;
        }
        self.draft
            .set_text(field.read(&self.properties.summary).to_string());
        let end = self.draft.text().len();
        self.draft.set_caret(end, now_ms);
        self.draft.touch(now_ms);
        self.focus = Some(field);
        self.dirty = false;
        self.save_failed = false;
        true
    }

    /// Give the keyboard back, dropping whatever was typed.
    pub fn blur(&mut self) {
        self.focus = None;
        self.dirty = false;
        self.draft.set_text("");
    }

    /// Type into the focused question. `false` when nothing has focus.
    pub fn edit_text(&mut self, character: char, now_ms: u64) -> Option<bool> {
        let field = self.focus?;
        if character.is_control() {
            return Some(false);
        }
        let mut buffer = [0_u8; 4];
        self.draft
            .insert_str(character.encode_utf8(&mut buffer), now_ms);
        self.dirty = field.read(&self.properties.summary) != self.draft.text();
        Some(true)
    }

    /// Backspace in the focused question.
    pub fn edit_backspace(&mut self, now_ms: u64) -> Option<bool> {
        let field = self.focus?;
        self.draft.backspace(now_ms);
        self.dirty = field.read(&self.properties.summary) != self.draft.text();
        Some(true)
    }

    /// Replace the focused question's text — a paste.
    pub fn edit_paste(&mut self, text: &str, now_ms: u64) -> Option<bool> {
        let field = self.focus?;
        let sanitized: String = text.chars().filter(|c| !c.is_control()).collect();
        self.draft.set_text(sanitized);
        self.draft.touch(now_ms);
        self.dirty = field.read(&self.properties.summary) != self.draft.text();
        Some(true)
    }

    /// What to save, or `None` when there is nothing to save.
    ///
    /// The whole properties object goes to the daemon, not the one field: the
    /// route compares what it is handed with what it holds to decide which
    /// SUBJECT a write touches, so a body carrying only the summary would be a
    /// claim that everything else is being cleared.
    pub fn request_save(&mut self) -> bool {
        if self.saving || !self.dirty || self.pending_save.is_some() {
            return false;
        }
        let Some(field) = self.focus else {
            return false;
        };
        let mut properties = self.properties.clone();
        SummaryField::write(
            field,
            &mut properties.summary,
            self.draft.text().to_string(),
        );
        self.pending_save = Some(properties);
        self.saving = true;
        self.save_failed = false;
        true
    }

    /// Take the queued write. The host calls this from its own tick.
    pub fn take_pending_save(&mut self) -> Option<SectionProperties> {
        self.pending_save.take()
    }

    /// Record that a save landed: what was stored is now what the panel shows.
    pub fn saved(&mut self, properties: SectionProperties) {
        self.properties = properties;
        self.saving = false;
        self.dirty = false;
        self.save_failed = false;
        if let Some(field) = self.focus {
            self.draft
                .set_text(field.read(&self.properties.summary).to_string());
        }
    }

    /// Record that a save did not land. The draft stays, so the retry is a
    /// keystroke rather than retyping the sentence.
    pub fn save_refused(&mut self) {
        self.saving = false;
        self.save_failed = true;
    }

    /// The state of the link to one analytics document, when it is attached.
    pub fn link_state(&self, key: &str) -> Option<LinkState> {
        self.links
            .iter()
            .find(|attached| attached.link.key == key)
            .map(|attached| attached.state)
    }

    /// Whether anything is written down about this section.
    ///
    /// Read by the panel to choose between the summary fields and the sentence
    /// that says nobody has written any — which is a state the operator named,
    /// not an error.
    pub fn has_summary(&self) -> bool {
        !self.properties.summary.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section::SectionSummary;

    fn summary() -> SectionProperties {
        SectionProperties {
            summary: SectionSummary {
                what_it_is: "Checkout".to_string(),
                ..SectionSummary::default()
            },
            ..SectionProperties::empty()
        }
    }

    #[test]
    fn a_panel_starts_with_nothing_to_show() {
        let state = SectionPanelState::default();

        assert!(!state.is_visible());
        assert!(!state.read);
        assert!(state.properties.is_empty());
    }

    #[test]
    fn selecting_a_section_forgets_the_previous_answer() {
        let mut state = SectionPanelState::default();
        state.select(Some(NodeId::new("s1")));
        state.apply(&NodeId::new("s1"), summary(), Vec::new());
        assert!(state.read);

        state.select(Some(NodeId::new("s2")));

        assert!(
            state.properties.is_empty(),
            "the previous section's summary must not stand in for the next one while it loads"
        );
        assert!(!state.read);
    }

    #[test]
    fn an_answer_for_a_section_the_panel_left_is_ignored() {
        // Requests race: the slower answer must not overwrite what the newer
        // selection is showing.
        let mut state = SectionPanelState::default();
        state.select(Some(NodeId::new("s1")));

        state.apply(&NodeId::new("s2"), summary(), Vec::new());

        assert!(state.properties.is_empty());
        assert!(!state.read);
    }

    #[test]
    fn a_failed_read_is_not_an_empty_section() {
        let mut state = SectionPanelState::default();
        state.select(Some(NodeId::new("s1")));

        state.fail(&NodeId::new("s1"));

        assert!(state.failed);
        assert!(!state.read);
        assert!(state.properties.is_empty());
    }

    #[test]
    fn an_attached_link_reports_the_state_it_was_given() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        let link = AnalyticsLink::new(
            "abc",
            "Checkout analytics",
            crate::section::SectionDigest::of_text("then"),
            crate::section::SectionDigest::of_text("screens"),
            1,
            None,
        );
        state.apply(
            &node,
            SectionProperties::empty(),
            vec![SectionLink {
                link,
                state: LinkState::Broken {
                    side: crate::section::MovedSide::Mockups,
                },
            }],
        );

        assert_eq!(
            state.link_state("abc"),
            Some(LinkState::Broken {
                side: crate::section::MovedSide::Mockups
            })
        );
        assert_eq!(state.link_state("missing"), None);
    }

    #[test]
    fn focusing_a_question_seeds_the_draft_from_what_is_stored() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, summary(), Vec::new());

        assert!(state.focus_field(SummaryField::WhatItIs, 0));

        assert_eq!(state.focus, Some(SummaryField::WhatItIs));
        assert_eq!(
            state.draft.text(),
            "Checkout",
            "a person edits their sentence, not an empty box where it used to be"
        );
        assert!(!state.dirty);
    }

    #[test]
    fn typing_marks_the_field_dirty_and_queues_nothing_until_enter() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, summary(), Vec::new());
        state.focus_field(SummaryField::WhatItIs, 0);

        assert_eq!(state.edit_text('!', 0), Some(true));

        assert!(state.dirty);
        assert!(state.pending_save.is_none(), "typing is not saving");
        assert!(!state.request_save() == false);
        let queued = state.pending_save.clone().expect("a queued write");
        assert_eq!(queued.summary.what_it_is, "Checkout!");
        assert!(state.saving);
    }

    #[test]
    fn an_unchanged_field_queues_nothing() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, summary(), Vec::new());
        state.focus_field(SummaryField::WhatItIs, 0);

        assert!(!state.request_save());
        assert!(state.pending_save.is_none());
    }

    #[test]
    fn a_saved_write_replaces_what_the_panel_shows() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, summary(), Vec::new());
        state.focus_field(SummaryField::WhatItIs, 0);
        state.edit_text('!', 0);
        state.request_save();

        let mut stored = summary();
        stored.summary.what_it_is = "Checkout!".to_string();
        state.saved(stored);

        assert!(!state.saving);
        assert!(!state.dirty);
        assert_eq!(state.properties.summary.what_it_is, "Checkout!");
        assert_eq!(state.draft.text(), "Checkout!");
    }

    #[test]
    fn a_refused_write_keeps_the_sentence_to_retry() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, summary(), Vec::new());
        state.focus_field(SummaryField::WhatItIs, 0);
        state.edit_text('!', 0);
        state.request_save();

        state.save_refused();

        assert!(state.save_failed);
        assert_eq!(
            state.draft.text(),
            "Checkout!",
            "retyping the sentence is not the price of a failed save"
        );
        assert!(state.dirty, "so the retry is a keystroke");
    }

    #[test]
    fn a_read_that_lands_while_somebody_is_typing_does_not_overwrite_them() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, summary(), Vec::new());
        state.focus_field(SummaryField::WhatItIs, 0);
        state.edit_text('!', 0);

        // The daemon answers about the same section while the draft is open.
        let mut stored = summary();
        stored.summary.where_to_look = "Elsewhere".to_string();
        state.apply(&node, stored, Vec::new());

        assert_eq!(state.draft.text(), "Checkout!");
        assert!(state.dirty, "the draft is still unsaved and still theirs");
    }

    #[test]
    fn a_section_with_no_summary_says_so() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, SectionProperties::empty(), Vec::new());

        assert!(state.read);
        assert!(!state.has_summary());
    }
}
