//! The link between a section and the analytics it was built from.
//!
//! Binary on purpose: a link is either in sync or broken, and when it is broken
//! the section says which side moved. Two fingerprints are recorded when the
//! link is made ([`AnalyticsLink::digest`] for the analytics document,
//! [`AnalyticsLink::mockups`] for the section's screens), and the state is the
//! comparison of those with what is there now. There are no versions, no
//! history and no merge — that is issue #60's subject, and pretending to have it
//! here would be a claim the storage cannot back.
//!
//! ## Why "broken" is not "wrong"
//!
//! A change on either side is news, not an error: the analytics may have been
//! corrected, or the designer may have improved the screen. What the section
//! must not do is go on claiming that the screens were built from *this*
//! analytics when they were built from the version before it. So the state is
//! reported and the repair is one gesture — [`restore`] writes both fingerprints
//! again, which is the designer saying "yes, I have read the new analytics and
//! the screens are what they should be".
//!
//! ## Why the state is a pure function of three values
//!
//! No store, no clock, no document: the caller resolves the current analytics
//! digest (it has the file) and the current mockup digest (it has the document),
//! and this module answers. That is what makes every case below a test rather
//! than a scenario, and it keeps the same answer available to the daemon and to
//! the browser bundle.

use serde::{Deserialize, Serialize};

use crate::section::{SectionDigest, SectionProperties};

/// One analytics document attached to a section, with what the link remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsLink {
    /// The asset's short key — how the markdown document is addressed in the
    /// store. Not a title: the name below is what a person reads.
    pub key: String,
    /// The asset's name when the link was made.
    ///
    /// A snapshot rather than a join, for the reason a comment keeps its
    /// author's name: a reader of the section must be able to see what this
    /// section was built from without depending on the asset still being called
    /// that, or on being able to open it at all.
    pub name: String,
    /// The analytics document's digest when the link was made.
    pub digest: SectionDigest,
    /// The section's mockup digest when the link was made.
    pub mockups: SectionDigest,
    /// Seconds since the epoch, when the link was made.
    pub linked_at: u64,
    /// The account that made it; `None` for the local operator, who has none —
    /// the same meaning this carries on a comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_by: Option<String>,
}

impl AnalyticsLink {
    /// Attach an analytics document, recording both fingerprints now.
    pub fn new(
        key: impl Into<String>,
        name: impl Into<String>,
        digest: SectionDigest,
        mockups: SectionDigest,
        linked_at: u64,
        linked_by: Option<&str>,
    ) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            digest,
            mockups,
            linked_at,
            linked_by: linked_by.map(str::to_string),
        }
    }

    /// Whether this link's two fingerprints are the current ones.
    pub fn is_in_sync(&self, digest: &SectionDigest, mockups: &SectionDigest) -> bool {
        &self.digest == digest && &self.mockups == mockups
    }
}

/// Which side moved since the link was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovedSide {
    /// The analytics document is not the one the section was built from.
    Analytics,
    /// The section's screens are not the ones the analytics were read against.
    Mockups,
    /// Both. Reported rather than resolved: picking one would make the section
    /// state a fact about the other that is not true.
    Both,
}

impl MovedSide {
    /// Stable name, for logs and for a mark that has to say what happened.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Analytics => "analytics",
            Self::Mockups => "mockups",
            Self::Both => "both",
        }
    }
}

/// The state of one link: the four cases the operator named, plus the two the
/// world adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// The section references no analytics document.
    ///
    /// Not a fault: a section that is being drawn before anybody wrote anything
    /// down is a real and common state, and the canvas shows it as the section's
    /// own colour rather than as something to fix.
    NoAnalytics,
    /// Attached, and both fingerprints are current.
    InSync,
    /// Attached, and something moved since. [`MovedSide`] says what.
    Broken { side: MovedSide },
    /// Attached, and the asset the link names is not in the store.
    ///
    /// Its own case rather than a spelling of [`LinkState::Broken`], because it
    /// is a different fact with a different repair: nothing "changed", the
    /// document was deleted or never arrived, and restoring the link would
    /// silently re-point the section at whatever is there now. The operator's
    /// four states do not cover it, and flattening it into "the analytics
    /// changed" would tell a reader something untrue.
    AssetMissing,
}

impl LinkState {
    /// Whether the link is intact — the only state the canvas leaves unmarked.
    pub const fn is_in_sync(self) -> bool {
        matches!(self, Self::InSync)
    }

    /// Whether the section is attached to something.
    pub const fn has_analytics(self) -> bool {
        !matches!(self, Self::NoAnalytics)
    }

    /// How loudly this state has to be reported, when a section has several
    /// links and one mark has to stand for all of them.
    ///
    /// Ordered deliberately: a document that is gone outranks a document that
    /// changed (a reader cannot compare against something that is not there, and
    /// the repair is different), and a change on both sides outranks a change on
    /// one (it is strictly more to say). Everything broken outranks everything
    /// in sync.
    const fn severity(self) -> u8 {
        match self {
            Self::InSync => 0,
            Self::NoAnalytics => 1,
            Self::Broken {
                side: MovedSide::Analytics,
            } => 2,
            Self::Broken {
                side: MovedSide::Mockups,
            } => 3,
            Self::Broken {
                side: MovedSide::Both,
            } => 4,
            Self::AssetMissing => 5,
        }
    }
}

/// The state of one link, from what is there now.
///
/// `current` is the analytics document's digest as resolved by the caller, or
/// `None` when the asset is not in the store any more.
pub fn link_state(
    link: Option<&AnalyticsLink>,
    current: Option<&SectionDigest>,
    mockups: &SectionDigest,
) -> LinkState {
    let Some(link) = link else {
        return LinkState::NoAnalytics;
    };
    let Some(current) = current else {
        return LinkState::AssetMissing;
    };
    let analytics_moved = link.digest != *current;
    let mockups_moved = link.mockups != *mockups;
    match (analytics_moved, mockups_moved) {
        (false, false) => LinkState::InSync,
        (true, false) => LinkState::Broken {
            side: MovedSide::Analytics,
        },
        (false, true) => LinkState::Broken {
            side: MovedSide::Mockups,
        },
        (true, true) => LinkState::Broken {
            side: MovedSide::Both,
        },
    }
}

/// The state of a whole section, which the canvas paints one mark for.
///
/// `current` answers for one analytics key: the document's digest now, or `None`
/// when the store does not have it. The mockup digest is the same for every link
/// — the section has one set of screens — so it is taken once.
pub fn section_link_state<F>(
    properties: &SectionProperties,
    current: F,
    mockups: &SectionDigest,
) -> LinkState
where
    F: Fn(&str) -> Option<SectionDigest>,
{
    if properties.analytics.is_empty() {
        return LinkState::NoAnalytics;
    }
    let mut worst = LinkState::InSync;
    for link in &properties.analytics {
        let resolved = current(&link.key);
        let state = link_state(Some(link), resolved.as_ref(), mockups);
        if state.severity() > worst.severity() {
            worst = state;
        }
    }
    worst
}

/// Restore a link: accept what is there now and record both fingerprints again.
///
/// The gesture the operator named. It writes the analytics digest **and** the
/// mockup digest, because a link is one claim about two things and half of it
/// restored would make the section report the other half as moved the moment it
/// was looked at.
pub fn restore(
    link: &mut AnalyticsLink,
    digest: &SectionDigest,
    mockups: &SectionDigest,
    at: u64,
    by: Option<&str>,
) {
    link.digest = digest.clone();
    link.mockups = mockups.clone();
    link.linked_at = at;
    link.linked_by = by.map(str::to_string);
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
