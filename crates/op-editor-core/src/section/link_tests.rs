//! The link: four states, both sides, and the gesture that restores it.

use super::*;
use crate::section::SectionDigest;

/// A digest of a text, for links that only need a value.
fn digest(text: &str) -> SectionDigest {
    SectionDigest::of_text(text)
}

/// A link made when the analytics was `analytics` and the mockups were
/// `mockups`.
fn link(analytics: &str, mockups: &str) -> AnalyticsLink {
    AnalyticsLink::new(
        "k1",
        "Checkout analytics",
        digest(analytics),
        digest(mockups),
        1_700_000_000,
        Some("userA"),
    )
}

#[test]
fn no_link_is_no_analytics() {
    assert_eq!(
        link_state(None, Some(&digest("a")), &digest("m")),
        LinkState::NoAnalytics
    );
    assert!(!LinkState::NoAnalytics.has_analytics());
    assert!(!LinkState::NoAnalytics.is_in_sync());
}

#[test]
fn an_untouched_pair_is_in_sync() {
    let link = link("analytics", "mockups");
    let state = link_state(Some(&link), Some(&digest("analytics")), &digest("mockups"));
    assert_eq!(state, LinkState::InSync);
    assert!(state.is_in_sync());
    assert!(state.has_analytics());
}

#[test]
fn analytics_moving_breaks_the_link_and_names_the_analytics() {
    let link = link("analytics", "mockups");
    assert_eq!(
        link_state(
            Some(&link),
            Some(&digest("analytics v2")),
            &digest("mockups")
        ),
        LinkState::Broken {
            side: MovedSide::Analytics
        }
    );
}

#[test]
fn a_screen_moving_breaks_the_link_and_names_the_mockups() {
    let link = link("analytics", "mockups");
    assert_eq!(
        link_state(
            Some(&link),
            Some(&digest("analytics")),
            &digest("mockups v2")
        ),
        LinkState::Broken {
            side: MovedSide::Mockups
        }
    );
}

#[test]
fn both_sides_moving_says_both() {
    // The operator's four states do not cover the fifth case the world has, and
    // naming one side would state something untrue about the other.
    let link = link("analytics", "mockups");
    assert_eq!(
        link_state(Some(&link), Some(&digest("a2")), &digest("m2")),
        LinkState::Broken {
            side: MovedSide::Both
        }
    );
}

#[test]
fn an_asset_that_is_gone_is_its_own_state() {
    let link = link("analytics", "mockups");
    let state = link_state(Some(&link), None, &digest("mockups"));
    assert_eq!(state, LinkState::AssetMissing);
    // Not "the analytics changed": nothing changed, the document is not there,
    // and the repair is different.
    assert_ne!(
        state,
        LinkState::Broken {
            side: MovedSide::Analytics
        }
    );
    assert!(state.has_analytics());
}

#[test]
fn a_refused_reader_is_not_told_the_asset_is_gone() {
    // Issue #110. Both cases leave the reader without a digest, and they are
    // not the same fact: one is about the asset, the other about the reader. A
    // panel that answered "gone" to a refused visitor would state something
    // untrue about somebody else's file and send them to a repair that cannot
    // help them.
    let link = link("analytics", "mockups");
    assert_eq!(
        refused_link_state(Some(&link)),
        LinkState::NotReadable,
        "a refusal is its own state, not a spelling of `AssetMissing`"
    );
    assert_ne!(refused_link_state(Some(&link)), LinkState::AssetMissing);
    // The refusal is not "nothing attached" either: the section names a
    // document, and says so.
    assert!(refused_link_state(Some(&link)).has_analytics());
    assert_eq!(refused_link_state(None), LinkState::NoAnalytics);
}

#[test]
fn a_mark_that_cannot_be_compared_outranks_one_that_merely_moved() {
    // The ordering a single canvas mark has to stand for, when a section
    // carries several links.
    let broken = LinkState::Broken {
        side: MovedSide::Both,
    };
    assert_eq!(
        louder(Some(broken), LinkState::NotReadable),
        LinkState::NotReadable
    );
    assert_eq!(
        louder(Some(LinkState::NotReadable), broken),
        LinkState::NotReadable
    );
    assert_eq!(louder(Some(broken), LinkState::InSync), broken);
    // A document that is gone is a fault of the store, and the one somebody
    // has to repair for everybody — where a refusal is a fact about the reader.
    assert_eq!(
        louder(Some(LinkState::NotReadable), LinkState::AssetMissing),
        LinkState::AssetMissing
    );
    assert_eq!(
        louder(None, LinkState::NotReadable),
        LinkState::NotReadable,
        "the first link seen is the one to beat"
    );
}

#[test]
fn restoring_rewrites_both_fingerprints() {
    let mut link = link("analytics", "mockups");
    let analytics = digest("analytics v2");
    let mockups = digest("mockups v2");
    assert_eq!(
        link_state(Some(&link), Some(&analytics), &mockups),
        LinkState::Broken {
            side: MovedSide::Both
        }
    );

    restore(
        &mut link,
        &analytics,
        &mockups,
        1_700_000_500,
        Some("userB"),
    );

    assert_eq!(
        link_state(Some(&link), Some(&analytics), &mockups),
        LinkState::InSync
    );
    assert_eq!(link.linked_at, 1_700_000_500);
    assert_eq!(link.linked_by.as_deref(), Some("userB"));
}

#[test]
fn restoring_half_a_link_would_leave_it_broken() {
    // The reason the gesture writes both: a restore that only accepted the
    // analytics would report the mockups as moved the moment it was looked at.
    let mut link = link("analytics", "mockups");
    let analytics = digest("analytics v2");
    let mockups = digest("mockups v2");
    restore(&mut link, &analytics, &mockups, 1, None);
    assert!(link.is_in_sync(&analytics, &mockups));
    assert!(!link.is_in_sync(&analytics, &digest("mockups")));
}

#[test]
fn a_section_with_several_documents_reports_the_loudest_link() {
    let mut properties = SectionProperties::empty();
    properties.link(AnalyticsLink::new(
        "k1",
        "First",
        digest("a1"),
        digest("m1"),
        1,
        None,
    ));
    properties.link(AnalyticsLink::new(
        "k2",
        "Second",
        digest("a2"),
        digest("m1"),
        2,
        None,
    ));
    properties.link(AnalyticsLink::new(
        "k3",
        "Third",
        digest("a3"),
        digest("m1"),
        3,
        None,
    ));
    let mockups = digest("m1");

    // Every document matches: in sync.
    let resolved = |key: &str| match key {
        "k1" => Some(digest("a1")),
        "k2" => Some(digest("a2")),
        _ => Some(digest("a3")),
    };
    assert_eq!(
        section_link_state(&properties, resolved, &mockups),
        LinkState::InSync
    );

    // One of them changed: the section is broken, and says which side.
    let one_changed = |key: &str| match key {
        "k2" => Some(digest("a2 edited")),
        "k1" => Some(digest("a1")),
        _ => Some(digest("a3")),
    };
    assert_eq!(
        section_link_state(&properties, one_changed, &mockups),
        LinkState::Broken {
            side: MovedSide::Analytics
        }
    );

    // One of them is gone: the loudest of the three, because a reader cannot
    // compare against a document that is not there.
    let one_missing = |key: &str| match key {
        "k2" => None,
        "k1" => Some(digest("a1")),
        _ => Some(digest("a3")),
    };
    assert_eq!(
        section_link_state(&properties, one_missing, &mockups),
        LinkState::AssetMissing
    );
}

#[test]
fn a_section_with_no_documents_says_so_rather_than_being_in_sync() {
    let properties = SectionProperties::empty();
    assert_eq!(
        section_link_state(&properties, |_| None, &digest("m")),
        LinkState::NoAnalytics
    );
}

#[test]
fn moving_the_screens_breaks_every_link_at_once() {
    // The mockups belong to the section, not to a document: one screen changed
    // is one screen changed for every analytics it was read against.
    let mut properties = SectionProperties::empty();
    properties.link(AnalyticsLink::new(
        "k1",
        "A",
        digest("a1"),
        digest("m1"),
        1,
        None,
    ));
    properties.link(AnalyticsLink::new(
        "k2",
        "B",
        digest("a2"),
        digest("m1"),
        2,
        None,
    ));
    let resolved = |key: &str| {
        Some(if key == "k1" {
            digest("a1")
        } else {
            digest("a2")
        })
    };
    assert_eq!(
        section_link_state(&properties, resolved, &digest("m2")),
        LinkState::Broken {
            side: MovedSide::Mockups
        }
    );
}

#[test]
fn the_sides_have_stable_names() {
    assert_eq!(MovedSide::Analytics.as_str(), "analytics");
    assert_eq!(MovedSide::Mockups.as_str(), "mockups");
    assert_eq!(MovedSide::Both.as_str(), "both");
}
