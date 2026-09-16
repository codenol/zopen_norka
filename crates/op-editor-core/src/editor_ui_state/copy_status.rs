//! The copy of the document this browser is holding, and whether the canvas is
//! painting the daemon's current one (issues #171 / #191).
//!
//! ## Why this state exists
//!
//! Issue #191 was measured in the ordinary flow, with no offline anywhere: a
//! page reload while a recoverable draft exists, then a chat turn. The turn's
//! result is applied to the daemon's document and the transcript says
//! `<!-- APPLIED -->`, while the canvas keeps painting a different copy — so the
//! person reads "the AI did nothing", and nothing on screen contradicts that.
//! Issue #171 states the same gap from the offline side: the browser keeps no
//! document of its own, so it cannot even say what it is showing.
//!
//! The state here is deliberately *values, not a verdict*. It records the two
//! versions, whether this tab has unpushed edits, whether a conflict latched,
//! and the identity of the copy the browser's own store holds; [`standing`]
//! turns those into one honest answer about the screen.
//!
//! ## What it does not decide
//!
//! Which copy wins. That is #169 and it is the operator's decision; wave 1
//! exists so the divergence is *visible* first. Nothing in this module
//! resolves, merges, or overwrites anything — it reports.
//!
//! ## Nothing here is a guess
//!
//! Every field is written from something the host actually observed: an applied
//! version, a probe answer, the sync gate's own `needs_push`, the gate's own
//! conflict latch. The place this matters most is the one field that could
//! easily be invented — a copy this tab wrote itself carries **no** daemon
//! version, because inventing one would make the tab claim an agreement with
//! the daemon that it does not have.

use serde::{Deserialize, Serialize};

/// FNV-1a 64, the hash `crate::web_sync` already uses for its push baseline.
///
/// Not cryptographic: the fingerprint answers "is this the same document I
/// already stored?", never "did somebody tamper with it?".
pub fn fingerprint(bytes: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes.as_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Which side of the sync a stored copy came from.
///
/// The distinction is the reason the record has an origin at all: a copy the
/// daemon handed this tab was the daemon's document at a known version, while a
/// copy this tab wrote itself may never have reached the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopyOrigin {
    /// Applied from a `GET /api/mcp/document` answer, at `daemon_version`.
    Daemon,
    /// Written by this tab's own edits, unconfirmed by the daemon.
    Local,
}

/// Everything needed to recognise a copy later, without holding its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyIdentity {
    /// The daemon's key for this document, when it has one (`fileKey`). A
    /// key-less document is a draft and is stored under the draft slot's own
    /// record key, so the two can never be read as each other (issue #92).
    pub doc_key: Option<String>,
    /// The daemon version this copy came from, or `None` for a copy this tab
    /// made and the daemon has not confirmed.
    pub daemon_version: Option<u64>,
    /// Content fingerprint of the stored bytes.
    pub fingerprint: u64,
    /// Wall clock (unix ms) the copy was written to the store.
    pub saved_at_ms: u64,
    /// Which side wrote it.
    pub origin: CopyOrigin,
}

/// One persisted copy: the document's bytes and the identity beside them.
///
/// This is the record the browser's store holds — the shape #171 asks for
/// (*key, name, timestamps, and the bytes*) plus the two fields this wave needs
/// on top of it: the version the copy came from, and which side wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCopy {
    /// The daemon's key for this document, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_key: Option<String>,
    /// The document's name as this tab knew it, so the record is readable
    /// without parsing the bytes.
    #[serde(default)]
    pub name: String,
    /// Which side wrote this copy.
    pub origin: CopyOrigin,
    /// The daemon version this copy came from, when it came from the daemon.
    #[serde(default)]
    pub daemon_version: Option<u64>,
    /// Content fingerprint of `document`.
    pub fingerprint: u64,
    /// Unix ms the copy was written.
    pub saved_at_ms: u64,
    /// The serialized document — the same bytes the sync path moves
    /// (`serialize_sync_document`), which is #171's point that a stored copy is
    /// the artifact the daemon holds and not a new format.
    pub document: String,
}

/// The store key for one (account, document) pair.
///
/// One record per document, last writer wins: the *mirror* #171 describes —
/// bounded, one writer, no identity question — and not a store in its own
/// right, which would need naming, listing, deletion and an upload path.
pub fn record_key(subject: &str, doc_key: Option<&str>) -> String {
    match doc_key {
        // The `doc::` namespace is not decoration. A stored document whose own
        // key happens to BE the word "draft" would otherwise share a record
        // with the draft slot, and a key-less draft would come back as that
        // document — the same class of mix-up issue #92 paid for.
        Some(key) if !key.trim().is_empty() => format!("{subject}::doc::{key}"),
        _ => format!("{subject}::draft"),
    }
}

impl StoredCopy {
    /// The identity alone — what a staleness comparison reads, with the bytes
    /// left out.
    pub fn identity(&self) -> CopyIdentity {
        CopyIdentity {
            doc_key: self.doc_key.clone(),
            daemon_version: self.daemon_version,
            fingerprint: self.fingerprint,
            saved_at_ms: self.saved_at_ms,
            origin: self.origin,
        }
    }

    /// Build a record from a document's serialized bytes.
    pub fn new(
        doc_key: Option<String>,
        name: String,
        origin: CopyOrigin,
        daemon_version: Option<u64>,
        document: String,
        saved_at_ms: u64,
    ) -> Self {
        let fingerprint = fingerprint(&document);
        Self {
            doc_key,
            name,
            origin,
            daemon_version,
            fingerprint,
            saved_at_ms,
            document,
        }
    }

    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// Read a record back.
    ///
    /// `None` for anything unreadable, and for a record whose fingerprint does
    /// not match its own bytes: a stored copy that cannot be trusted to be what
    /// it says it is must read as "no copy", never as a copy that is quietly
    /// wrong. Unknown fields are ignored, so a record written by a later build
    /// still loads.
    pub fn parse(json: &str) -> Option<Self> {
        let stored: Self = serde_json::from_str(json).ok()?;
        if stored.fingerprint != fingerprint(&stored.document) {
            return None;
        }
        Some(stored)
    }
}

/// How the canvas's copy stands against the daemon's.
///
/// Ordered by precedence in [`DocumentCopyStatus::standing`]: the first
/// condition that holds is the answer, because a later one is either a
/// consequence of it or a weaker statement about the same screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyStanding {
    /// Nothing has been pulled yet: there is no copy to describe.
    Unpulled,
    /// The canvas holds exactly the version the daemon last handed this tab,
    /// with no unpushed edit. The only standing in which the canvas is the
    /// daemon's copy — and therefore the only one that paints nothing.
    InStep,
    /// The daemon has moved on (an AI turn, an MCP client, another tab) and
    /// this tab has not painted that version. **This is #191**: the edit exists,
    /// the screen does not show it.
    Behind { shown: u64, daemon: u64 },
    /// The daemon did not answer the last version probe, so this tab cannot say
    /// whether it is current. Not the same as "in step".
    DaemonSilent { shown: u64 },
    /// The canvas holds edits the daemon has never confirmed, at the version
    /// this tab last agreed with.
    LocalEdits { shown: u64 },
    /// A conflict latched: the tab no longer follows the daemon at all. Kept
    /// separate from [`Self::Behind`] because it does not heal on its own — a
    /// plain browser tab has no way to clear the latch.
    ConflictLatched { shown: u64, daemon: u64 },
}

impl CopyStanding {
    /// Whether the canvas is painting the daemon's current copy.
    pub const fn canvas_is_daemons_copy(self) -> bool {
        matches!(self, Self::InStep)
    }

    /// Whether there is a divergence to state at all.
    ///
    /// Distinct from `!canvas_is_daemons_copy()`, and the difference is the
    /// whole reason this predicate exists: [`Self::Unpulled`] is also "not the
    /// daemon's copy", but it is the state of a tab that has observed nothing —
    /// a fresh page, or the desktop host, which never fills this state at all.
    /// A status surface gated on the negation would paint a permanent warning
    /// about a divergence nobody observed, which is how a warning becomes
    /// noise. The widget asks THIS question.
    pub const fn is_divergence(self) -> bool {
        matches!(
            self,
            Self::Behind { .. }
                | Self::DaemonSilent { .. }
                | Self::LocalEdits { .. }
                | Self::ConflictLatched { .. }
        )
    }
}

/// What the browser knows about the two copies, as values.
///
/// Written by the browser host (see `op_host_web::web_copy_status`) from
/// observations it actually made; read by the canvas's status strip. Default is
/// "nothing known", which paints nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentCopyStatus {
    /// The daemon version this tab last applied and repainted (`None` = never).
    pub shown_version: Option<u64>,
    /// The daemon's current version, as the last version probe answered it
    /// (`None` = the probe came back without an answer, i.e. silent).
    pub daemon_version: Option<u64>,
    /// This tab holds content edits the daemon has not confirmed.
    pub local_edits: bool,
    /// The daemon's version at the moment a push conflict latched.
    pub conflict_version: Option<u64>,
    /// The copy this browser keeps for this document, as its store last read or
    /// wrote it. `None` until the store has either loaded or written one.
    pub stored: Option<CopyIdentity>,
}

impl DocumentCopyStatus {
    /// Compare the two copies. Pure, so the rule is testable without a wire.
    pub fn standing(&self) -> CopyStanding {
        // Nothing pulled yet, so the question has no subject. Reported before
        // the conflict check because a conflict cannot latch before a first
        // pull.
        let Some(shown) = self.shown_version else {
            return CopyStanding::Unpulled;
        };
        // A latched conflict outranks everything: the pull gate is closed, so a
        // newer daemon version could not be applied even if this tab wanted it,
        // and reporting "behind" alone would hide the reason it stays that way.
        if let Some(daemon) = self.conflict_version {
            return CopyStanding::ConflictLatched { shown, daemon };
        }
        match self.daemon_version {
            // The daemon is ahead: the newest content exists and is not on
            // screen. This is the #191 case, named exactly.
            Some(daemon) if daemon > shown => CopyStanding::Behind { shown, daemon },
            // The daemon answered and agrees on the version; whatever is left
            // is this tab's own unpushed work.
            Some(_) if self.local_edits => CopyStanding::LocalEdits { shown },
            Some(_) => CopyStanding::InStep,
            // Silent: unknown, which is not the same as current.
            None => CopyStanding::DaemonSilent { shown },
        }
    }

    /// Whether the canvas is the daemon's copy — the one question the status
    /// surface asks.
    pub fn canvas_is_daemons_copy(&self) -> bool {
        self.standing().canvas_is_daemons_copy()
    }

    /// The daemon version the copy on this browser's disk came from, when that
    /// copy came from the daemon. The store's answer to "what is the last thing
    /// I kept, and whose was it".
    pub fn stored_daemon_version(&self) -> Option<u64> {
        self.stored.as_ref().and_then(|copy| copy.daemon_version)
    }

    /// Record the daemon version a version probe answered.
    ///
    /// Called with `None` when the probe came back without an answer (an
    /// unreachable daemon, a non-JSON body): "the daemon did not say" is a
    /// distinct fact from "the daemon said an older version", and collapsing
    /// the two is how a tab talks itself into believing it is current.
    pub fn note_daemon_version(&mut self, version: Option<u64>) {
        self.daemon_version = version;
    }

    /// Record which daemon version this tab's content corresponds to.
    ///
    /// Advanced by BOTH directions, and that is why the host reads it from the
    /// sync client rather than from the pull's success path: a pull that
    /// applied version N and a push the daemon acknowledged with version N both
    /// mean "the canvas holds daemon version N". Deriving it from the pull
    /// alone would leave it one version behind after every successful push, and
    /// the strip would then report a divergence that does not exist.
    pub fn note_shown_version(&mut self, version: Option<u64>) {
        self.shown_version = version;
    }

    /// Record whether this tab holds edits the daemon has not confirmed.
    pub fn note_local_edits(&mut self, unpushed: bool) {
        self.local_edits = unpushed;
    }

    /// Record the daemon version a latched push conflict named (`None` clears
    /// the latch).
    pub fn note_conflict(&mut self, daemon_version: Option<u64>) {
        self.conflict_version = daemon_version;
    }

    /// Record what the browser's own store holds for this document.
    pub fn note_stored_copy(&mut self, copy: Option<CopyIdentity>) {
        self.stored = copy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tab that has applied the daemon's version and agrees with it.
    fn in_step(version: u64) -> DocumentCopyStatus {
        let mut status = DocumentCopyStatus::default();
        status.note_shown_version(Some(version));
        status.note_daemon_version(Some(version));
        status
    }

    #[test]
    fn an_ai_turn_that_reached_the_daemon_leaves_the_canvas_behind() {
        // Issue #191 as the two versions: this tab applied 12, an AI turn took
        // the daemon to 14, and no local edit is holding the pull back. Calling
        // this "in step" is the bug — it is what makes a screen that did not
        // change look like a turn that did nothing.
        let mut status = in_step(12);
        status.note_daemon_version(Some(14));

        assert_eq!(
            status.standing(),
            CopyStanding::Behind {
                shown: 12,
                daemon: 14
            }
        );
        assert!(!status.canvas_is_daemons_copy());
    }

    #[test]
    fn a_latched_conflict_names_the_reason_it_is() {
        // The pull gate is closed, so nothing newer can arrive at all.
        // Reporting only "behind" would hide the one fact that explains why it
        // stays that way.
        let mut status = in_step(12);
        status.note_daemon_version(Some(14));
        status.note_local_edits(true);
        status.note_conflict(Some(14));

        assert_eq!(
            status.standing(),
            CopyStanding::ConflictLatched {
                shown: 12,
                daemon: 14
            }
        );
    }

    #[test]
    fn clearing_the_conflict_returns_the_daemon_as_the_answer() {
        let mut status = in_step(12);
        status.note_conflict(Some(14));
        status.note_conflict(None);
        status.note_daemon_version(Some(12));

        assert_eq!(status.standing(), CopyStanding::InStep);
    }

    #[test]
    fn unpushed_local_edits_are_not_the_daemons_copy() {
        // Same version on both sides, but the canvas holds edits the daemon has
        // never seen: the person is still looking at a copy of their own.
        let mut status = in_step(12);
        status.note_local_edits(true);

        assert_eq!(status.standing(), CopyStanding::LocalEdits { shown: 12 });
        assert!(!status.canvas_is_daemons_copy());
    }

    #[test]
    fn a_silent_daemon_is_not_reported_as_in_step() {
        // A probe that came back without an answer says nothing about what the
        // daemon holds. Treating silence as agreement is the same failure as
        // treating a newer daemon version as agreement.
        let mut status = in_step(12);
        status.note_daemon_version(None);

        assert_eq!(status.standing(), CopyStanding::DaemonSilent { shown: 12 });
        assert!(!status.canvas_is_daemons_copy());
    }

    #[test]
    fn agreement_is_the_only_in_step_standing() {
        assert_eq!(in_step(12).standing(), CopyStanding::InStep);
        assert!(in_step(12).canvas_is_daemons_copy());
    }

    #[test]
    fn nothing_observed_is_not_a_divergence_to_report() {
        // A tab that has pulled nothing has seen no divergence, and the desktop
        // host never fills this state at all. Painting on "not in step" would
        // put a permanent warning on a screen where nothing is wrong.
        let mut status = in_step(12);
        status.note_daemon_version(Some(14));
        assert!(status.standing().is_divergence());

        for standing in [CopyStanding::Unpulled, CopyStanding::InStep] {
            assert!(!standing.is_divergence(), "{standing:?}");
        }
        for standing in [
            CopyStanding::Behind {
                shown: 1,
                daemon: 2,
            },
            CopyStanding::DaemonSilent { shown: 1 },
            CopyStanding::LocalEdits { shown: 1 },
            CopyStanding::ConflictLatched {
                shown: 1,
                daemon: 2,
            },
        ] {
            assert!(standing.is_divergence(), "{standing:?}");
        }
    }

    #[test]
    fn a_fresh_tab_paints_no_status() {
        // Nothing known is not a claim about the screen: a fresh tab must not
        // announce a divergence it has not observed.
        assert_eq!(
            DocumentCopyStatus::default().standing(),
            CopyStanding::Unpulled
        );
        assert!(!DocumentCopyStatus::default().canvas_is_daemons_copy());
    }

    #[test]
    fn the_daemon_being_at_the_shown_version_is_what_agreement_means() {
        // The two halves are recorded separately, because they are observed
        // separately: the shown version advances on a pull AND on an
        // acknowledged push, while the daemon's version comes from the probe.
        let mut status = in_step(9);
        status.note_shown_version(Some(10));

        assert_eq!(
            status.standing(),
            CopyStanding::InStep,
            "a probe that agreed with the new shown version leaves nothing to report"
        );
    }

    #[test]
    fn the_kept_copy_reports_the_version_it_came_from() {
        let mut status = in_step(12);
        status.note_stored_copy(Some(CopyIdentity {
            doc_key: Some("key-1".to_string()),
            daemon_version: Some(11),
            fingerprint: 7,
            saved_at_ms: 1_000,
            origin: CopyOrigin::Daemon,
        }));

        assert_eq!(status.stored_daemon_version(), Some(11));
    }

    fn record(origin: CopyOrigin, version: Option<u64>) -> StoredCopy {
        StoredCopy::new(
            Some("key-1".to_string()),
            "Dashboard".to_string(),
            origin,
            version,
            "{\"version\":\"1.0\"}".to_string(),
            1_700_000_000_000,
        )
    }

    #[test]
    fn a_stored_copy_round_trips_with_its_identity() {
        let stored = record(CopyOrigin::Daemon, Some(14));
        let read = StoredCopy::parse(&stored.to_json().expect("serializes")).expect("parses");

        assert_eq!(read, stored);
        assert_eq!(
            read.identity(),
            CopyIdentity {
                doc_key: Some("key-1".to_string()),
                daemon_version: Some(14),
                fingerprint: fingerprint("{\"version\":\"1.0\"}"),
                saved_at_ms: 1_700_000_000_000,
                origin: CopyOrigin::Daemon,
            }
        );
    }

    #[test]
    fn a_copy_this_tab_wrote_carries_no_daemon_version() {
        // The field a staleness comparison reads must not be invented: a copy
        // the daemon never confirmed has no version, and recording one would
        // make the tab claim an agreement it does not have.
        let stored = record(CopyOrigin::Local, None);
        assert_eq!(stored.daemon_version, None);
        assert_eq!(
            StoredCopy::parse(&stored.to_json().expect("serializes"))
                .expect("parses")
                .origin,
            CopyOrigin::Local
        );
    }

    #[test]
    fn a_record_whose_bytes_do_not_match_its_fingerprint_reads_as_no_copy() {
        // A truncated or interleaved write must not come back as a copy that is
        // quietly a different document: letting the reader refuse is the whole
        // point of the fingerprint.
        let stored = record(CopyOrigin::Daemon, Some(14));
        let json = stored.to_json().expect("serializes");
        let tampered = json.replace("1.0", "9.9");
        assert_ne!(tampered, json, "the fixture must actually change the bytes");
        assert_eq!(StoredCopy::parse(&tampered), None);
    }

    #[test]
    fn an_unreadable_record_is_not_a_copy() {
        for json in ["", "not json", "{}", "{\"origin\":\"daemon\"}"] {
            assert_eq!(StoredCopy::parse(json), None, "{json}");
        }
    }

    #[test]
    fn a_record_from_a_later_build_still_loads() {
        // Forward tolerance: a field this build does not know is not a reason
        // to lose the user's copy.
        let stored = record(CopyOrigin::Daemon, Some(14));
        let json = stored.to_json().expect("serializes");
        // Only the OUTER object gains a field — mutating the document's own
        // bytes as well would trip the fingerprint check and test something
        // else entirely.
        let with_extra = json.replacen('{', "{\"futureField\":42,", 1);
        assert_ne!(with_extra, json);
        assert_eq!(StoredCopy::parse(&with_extra), Some(stored));
    }

    #[test]
    fn a_documents_record_key_never_collides_with_the_draft_slot() {
        // Issue #92's rule at the store's own key: a key-less document is the
        // draft, and no document key may resolve to the draft's record.
        assert_eq!(record_key("anon", Some("key-1")), "anon::doc::key-1");
        assert_eq!(record_key("anon", None), "anon::draft");
        assert_eq!(record_key("anon", Some("  ")), "anon::draft");
        assert_ne!(record_key("anon", Some("draft")), record_key("anon", None));
        assert_ne!(
            record_key("anon", Some("key-1")),
            record_key("user-2", Some("key-1")),
            "two accounts never share a record"
        );
    }

    #[test]
    fn the_fingerprint_is_stable_and_content_sensitive() {
        // The fingerprint is what lets the store skip writing a document it
        // already holds, so it must be a function of the bytes alone.
        assert_eq!(fingerprint("{\"a\":1}"), fingerprint("{\"a\":1}"));
        assert_ne!(fingerprint("{\"a\":1}"), fingerprint("{\"a\":2}"));
        assert_ne!(fingerprint(""), fingerprint(" "));
    }
}
