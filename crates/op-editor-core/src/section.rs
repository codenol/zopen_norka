//! Sections — the node that says why a screen is the way it is (#59).
//!
//! A section groups the mockups of one feature, scenario or flow, and carries
//! the reasoning they were built from: a reference to the analytics document
//! the screens were built from, the summary a reader who was not in the
//! conversation needs (what this is, where to look, use cases, what to check),
//! and the UX flows drawn from it.
//!
//! This module is the MODEL and nothing else: no storage, no I/O, no widget
//! state. It is wasm32-clean, so the same answers are available in the browser
//! bundle, in the daemon and in a test.
//!
//! ## Why a marked frame, and not a new node type
//!
//! Measured, not preferred. `PenNode` is matched EXHAUSTIVELY — no `_` arm — in
//! 91 places across this workspace and `vendor/jian`, six of them in
//! `op-editor-ui` and more in `op-orchestrator` / `op-host-desktop` /
//! `op-host-web`: crates a parallel agent is working in. A new variant is a
//! union change in a *vendored* schema, and it does not compile until every one
//! of those matches has been taught about it — plus the loader, the taffy layout
//! pass, the scene builder and both renderers. That is not a change one stage
//! can land, and the price is not a rebuild, it is the collision.
//!
//! The schema already answers this shape of question with a marker.
//! `FrameNode::screen` turns an ordinary frame into "one screen of the app" with
//! one additive optional field, consumed by one pass and ignored everywhere
//! else. `PenNodeBase::role` is the same idea needing no schema change at all: a
//! free-form semantic tag a node already carries, which this codebase already
//! uses as a marker — `EditorState::promote_legacy_widgets` exists precisely to
//! rewrite `role: "input"` frames into first-class widget nodes once the node
//! type is there.
//!
//! So a section is a [`Frame`](jian_ops_schema::node::FrameNode) whose `role` is
//! exactly [`SECTION_ROLE`]. A `Group` carrying that role is deliberately NOT a
//! section: a section has bounds, a colour and a child list, which is what a
//! frame is. Promotion to a dedicated node type stays open and travels the same
//! road the widgets did — add the variant in a stage that may touch the whole
//! workspace, add a promotion command, rewrite the markers.
//!
//! ## Where a section's properties live, and why not in the frame
//!
//! Not in the `.op` file today, and that is a cost, not a preference: adding one
//! optional field to `FrameNode` breaks 51 struct literals in 8 crates, five of
//! them crates this stage may not touch. A JSON payload smuggled through a
//! free-form string field (`explain` is claimed by the HTML importer,
//! `PenNodeBase::theme` is read by theme resolution) would be worse than either.
//!
//! The properties are therefore read and written by
//! `op_host_services::section_store`, keyed by `(document key, node id)` — the
//! same shape, and the same reason, as a comment thread. What that costs is
//! stated plainly: the *grouping* survives copy-paste with the frame and its
//! children, the *metadata* does not, because a copy is a new node id. When the
//! schema edit is scheduled in a stage that owns the whole workspace, the row
//! becomes a field and nothing in this module changes except where a caller
//! reads it from.
//!
//! ## What is here
//!
//! - [`AnalyticsLink`] / [`LinkState`] — the binary link: in sync, or broken
//!   and which side moved, or gone, or not this reader's to open. See [`link`].
//! - [`UxFlow`] — steps and connections as a graph, validated by
//!   [`check_flow`]. Mermaid is a notation, not the source: see [`mermaid`].
//! - [`SectionDigest`] — the fingerprints a link remembers. See [`digest`].

use serde::{Deserialize, Serialize};

use jian_ops_schema::node::{FrameNode, PenNode};

use crate::node_id::NodeId;

pub mod digest;
pub mod flow;
pub mod link;
pub mod mermaid;

#[cfg(test)]
mod tests;

pub use digest::{analytics_fingerprint, mockup_fingerprint, SectionDigest};
pub use flow::{
    check_flow, FlowCheck, FlowEdge, FlowGap, FlowIssue, FlowStep, FlowStepId, FlowStepKind, UxFlow,
};
pub use link::{
    link_state, louder, read_outcome, refused_link_state, section_link_state, unchecked_link_state,
    AnalyticsLink, LinkState, MovedSide, ReadOutcome,
};
pub use mermaid::{flow_from_mermaid, flow_to_mermaid, MermaidError, MermaidErrorKind};

/// The `role` value that makes an ordinary frame a section.
///
/// Lower-case and unqualified, matching the vocabulary this field already
/// carries (`"button"`, `"input"`, `"overlay"`). Nothing else in the workspace
/// reads `role` for behaviour except the legacy-widget promotion, which looks
/// for widget role names, so an unknown value here is inert everywhere else.
pub const SECTION_ROLE: &str = "section";

/// Whether this node is a section: a frame, explicitly marked as one.
///
/// A frame without the marker is an ordinary frame, and a `Group` with the
/// marker is a group — see the module docs for why a section has to be a frame.
pub fn section_frame(node: &PenNode) -> Option<&FrameNode> {
    let PenNode::Frame(frame) = node else {
        return None;
    };
    (frame.base.role.as_deref() == Some(SECTION_ROLE)).then_some(frame)
}

/// Whether this node is a section.
pub fn is_section(node: &PenNode) -> bool {
    section_frame(node).is_some()
}

/// Mark a frame as a section. `false` when the node is not a frame.
///
/// What the section tool does to the frame it has just drawn, and what a
/// "make this a section" press would do to one that already exists. A group is
/// refused rather than promoted: a section groups SCREENS, and a group is not a
/// container the canvas lets somebody drop a screen into.
pub fn mark_as_section(node: &mut PenNode) -> bool {
    let PenNode::Frame(frame) = node else {
        return false;
    };
    frame.base.role = Some(SECTION_ROLE.to_string());
    true
}

/// Remove the marker. `true` when the node was a section.
///
/// The frame and its children stay: unmarking says "this is an ordinary frame
/// again", not "delete what was grouped". What happens to the properties row
/// that hung off the id is the caller's decision — a store keeps the row, and a
/// document that no longer has the section simply stops reading it.
pub fn unmark_section(node: &mut PenNode) -> bool {
    let PenNode::Frame(frame) = node else {
        return false;
    };
    if frame.base.role.as_deref() != Some(SECTION_ROLE) {
        return false;
    }
    frame.base.role = None;
    true
}

/// The id of a section node, when this node is one.
///
/// The id is the section's identity across a save: it is what the properties
/// row is keyed by and what a step of a flow points at, so it is read from the
/// frame rather than minted here.
pub fn section_id(node: &PenNode) -> Option<NodeId> {
    section_frame(node).map(|frame| NodeId::new(frame.base.id.clone()))
}

/// The mockups a section owns: its direct children, in document order.
///
/// Direct children, not the whole subtree: a section's mockups are the screens
/// it groups, and a screen's own descendants are part of that screen. Order is
/// document order because the fingerprint over them is order-sensitive — moving
/// a screen to the front of a section is a change somebody made.
///
/// Empty for anything that is not a section, so a caller can ask without
/// checking first.
pub fn section_mockups(node: &PenNode) -> &[PenNode] {
    match section_frame(node).and_then(|frame| frame.children.as_deref()) {
        Some(children) => children,
        None => &[],
    }
}

/// What a section says, in the four questions the operator named.
///
/// Four named fields rather than one body of prose, because they are four
/// different questions asked by four different readers: what this is (what the
/// feature is for), where to look (which screen to open first), use cases (what
/// somebody does here), what to check (what a reviewer must verify). Prose that
/// mixes them is prose a reader has to take apart again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionSummary {
    #[serde(default)]
    pub what_it_is: String,
    #[serde(default)]
    pub where_to_look: String,
    #[serde(default)]
    pub use_cases: String,
    #[serde(default)]
    pub what_to_check: String,
}

impl SectionSummary {
    /// Whether all four questions are still unanswered.
    pub fn is_empty(&self) -> bool {
        self.what_it_is.trim().is_empty()
            && self.where_to_look.trim().is_empty()
            && self.use_cases.trim().is_empty()
            && self.what_to_check.trim().is_empty()
    }
}

/// Everything a section carries besides its mockups.
///
/// The section's properties as one value, so a caller reads or writes them in
/// one go and a store keeps them in one row. A section may reference several
/// analytics documents (the operator's decision — analytics is an asset, and an
/// asset can be referenced by more than one section and by more than one
/// section's worth of work).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionProperties {
    /// The analytics documents this section was built from. Empty is the
    /// ordinary "nobody wrote down why" section, which the canvas is meant to
    /// make visible rather than hide.
    #[serde(default)]
    pub analytics: Vec<AnalyticsLink>,
    #[serde(default)]
    pub summary: SectionSummary,
    /// The UX flows drawn from the analytics. A list: a section can describe
    /// more than one path through the feature.
    #[serde(default)]
    pub flows: Vec<UxFlow>,
}

impl SectionProperties {
    /// Properties with nothing in them — what a freshly marked frame has.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether a section says nothing at all yet.
    pub fn is_empty(&self) -> bool {
        self.analytics.is_empty() && self.summary.is_empty() && self.flows.is_empty()
    }

    /// Attach an analytics document, replacing an earlier link to the same one.
    ///
    /// Replacing rather than appending: two links to one document would be two
    /// fingerprints of the same thing, and the section would report whichever
    /// the reader happened to look at.
    pub fn link(&mut self, link: AnalyticsLink) {
        match self
            .analytics
            .iter_mut()
            .find(|existing| existing.key == link.key)
        {
            Some(existing) => *existing = link,
            None => self.analytics.push(link),
        }
    }

    /// Drop the link to `key`, returning it when there was one.
    pub fn unlink(&mut self, key: &str) -> Option<AnalyticsLink> {
        let index = self.analytics.iter().position(|link| link.key == key)?;
        Some(self.analytics.remove(index))
    }

    /// The link to `key`, when this section references that document.
    pub fn link_for(&self, key: &str) -> Option<&AnalyticsLink> {
        self.analytics.iter().find(|link| link.key == key)
    }

    /// The flow with this id, when the section has one.
    pub fn flow(&self, id: &str) -> Option<&UxFlow> {
        self.flows.iter().find(|flow| flow.id == id)
    }
}

/// The format version of the serialized [`SectionProperties`].
///
/// Bumped when the shape changes in a way an older reader would misread. It is
/// stored beside the properties rather than inferred, so "this build cannot read
/// that" is a typed refusal and not a section that quietly comes back empty —
/// the flows and the summary are authored work, and losing them silently is the
/// one failure this file must not have.
pub const SECTION_PROPERTIES_FORMAT: u32 = 1;

/// A section's properties as they are stored, with the format they were written
/// in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSectionProperties {
    pub format: u32,
    pub properties: SectionProperties,
}

impl StoredSectionProperties {
    /// Wrap the current shape at the current format.
    pub fn current(properties: SectionProperties) -> Self {
        Self {
            format: SECTION_PROPERTIES_FORMAT,
            properties,
        }
    }

    /// The stored form: compact JSON, one row's worth.
    pub fn encode(&self) -> String {
        // Serialization of a plain owned tree cannot fail; a panic here would
        // take the daemon down for a value the type system already bounds.
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// Read the stored form back.
    ///
    /// Two typed refusals and no third: text that is not this shape at all, and
    /// a format this build does not know. A newer format is refused rather than
    /// read best-effort — an older reader that guesses at a newer shape is how
    /// authored work disappears without anybody deciding to delete it.
    pub fn decode(text: &str) -> Result<Self, SectionFormatError> {
        let stored: Self =
            serde_json::from_str(text).map_err(|error| SectionFormatError::Malformed {
                detail: error.to_string(),
            })?;
        if stored.format != SECTION_PROPERTIES_FORMAT {
            return Err(SectionFormatError::UnsupportedFormat {
                format: stored.format,
            });
        }
        Ok(stored)
    }
}

/// Why stored section properties could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionFormatError {
    /// The text is not this shape — a hand-repaired row, a truncated write.
    Malformed { detail: String },
    /// Written by a build with a different format number.
    UnsupportedFormat { format: u32 },
}

impl std::fmt::Display for SectionFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed { detail } => {
                write!(f, "section properties are not readable: {detail}")
            }
            Self::UnsupportedFormat { format } => {
                write!(f, "section properties format {format} is not supported")
            }
        }
    }
}

impl std::error::Error for SectionFormatError {}
