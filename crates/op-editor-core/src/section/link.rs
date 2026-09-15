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
//!
//! ## The one case the caller decides, and why it is here all the same
//!
//! `None` for the current digest means the store does not have the asset: GONE,
//! and [`LinkState::AssetMissing`] is the answer. A store that HAS the asset and
//! will not hand it to this reader leaves the caller with no digest either, and
//! this function cannot tell the two apart — only the caller sees a status code.
//! So a refusal is [`refused_link_state`], and the rule that a refusal must
//! never be spelled as a deletion lives in this module rather than at each call
//! site, because that conflation is exactly the lie issue #110 was filed for.

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
    /// Attached, and this READER may not fetch the asset.
    ///
    /// Its own case rather than a spelling of [`LinkState::AssetMissing`], and
    /// the distinction is the point of it: "gone" is a fact about the store —
    /// the document was deleted or never arrived — and this is a fact about
    /// whoever is looking, who may open the section and not the document it
    /// names. A panel that answered "the analytics document is gone" to a
    /// reader who was simply refused would state something untrue about
    /// somebody else's file and send them to the wrong repair: nothing is
    /// broken, and no restore would help them; they need access, or they need
    /// to be told to ask for it. Issue #110 is where this case was found.
    NotReadable,
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
    /// Ordered deliberately. A document that is gone outranks a document this
    /// reader may not open, because a fault of the store is the one somebody
    /// has to repair for everybody, where a refusal is a fact about the reader
    /// alone. Both outrank a document that merely changed: neither can be
    /// compared against at all, and "the screens may be out of date" is the
    /// smaller thing to say. A change on both sides outranks a change on one
    /// (it is strictly more to say), and everything marked outranks everything
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
            Self::NotReadable => 5,
            Self::AssetMissing => 6,
        }
    }
}

/// The louder of two states, for a section that carries several links.
///
/// Here rather than at each caller because the ORDER is a rule, not a detail:
/// the panel, the canvas mark and [`section_link_state`] all have to pick the
/// same one of a section's links to speak for the section, and three copies of
/// that comparison is how they would come to disagree.
pub fn louder(seen: Option<LinkState>, next: LinkState) -> LinkState {
    match seen {
        Some(seen) if seen.severity() > next.severity() => seen,
        _ => next,
    }
}

/// The state of one link the store REFUSED this reader.
///
/// [`link_state`] cannot answer this, and the reason is the whole point of the
/// case: its second argument is "the digest as it is now, or `None` when the
/// store does not have the asset", so `None` means GONE and nothing else.
/// Collapsing a refusal into it is the lie issue #110 names — a visitor told
/// "the analytics document is gone" for an asset they simply may not fetch. The
/// caller is the only side that sees a status code, so the caller says which of
/// the two happened, and this is the constructor for the refusal — here rather
/// than spelled out at the call sites so that the rule stays with the rest of
/// the state machine.
pub fn refused_link_state(link: Option<&AnalyticsLink>) -> LinkState {
    match link {
        Some(_) => LinkState::NotReadable,
        // A refusal about nothing attached is still nothing attached: there is
        // no link to be unable to read.
        None => LinkState::NoAnalytics,
    }
}

/// The state of one link, from what is there now.
///
/// `current` is the analytics document's digest as resolved by the caller, or
/// `None` when the asset is not in the store any more. `None` means GONE and
/// never "not for you": a caller the store REFUSED has no digest either, and
/// passing `None` for that would make this function say the asset was deleted.
/// That case is [`refused_link_state`].
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
        worst = louder(Some(worst), state);
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
