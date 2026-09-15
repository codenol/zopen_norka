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

use crate::section::{AnalyticsLink, LinkState, SectionProperties};
use crate::NodeId;

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
    fn a_section_with_no_summary_says_so() {
        let mut state = SectionPanelState::default();
        let node = NodeId::new("s1");
        state.select(Some(node.clone()));
        state.apply(&node, SectionProperties::empty(), Vec::new());

        assert!(state.read);
        assert!(!state.has_summary());
    }
}
