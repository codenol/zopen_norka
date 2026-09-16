//! What one read of an analytics asset established, from its status and body.
//!
//! A sibling file rather than a directory, like the test module below: the
//! crate convention keeps the split flat and the import paths unchanged.
//!
//! ## Why this is one function and not two
//!
//! The browser reads this route from two places — the canvas marks, which ask
//! about every linked asset in the document, and the selected section's panel,
//! which asks about one — and issue #145 is what happens when the two answer a
//! status code differently: a 5xx arrived as `Answered(None)`, which is the
//! value a 404 produces, and a designer was told their analytics had been
//! deleted because the store was down. The status rule itself is
//! `op_editor_core::section::read_outcome`'s, and this is the one place it is
//! asked.

use op_editor_core::section::{read_outcome, ReadOutcome, SectionDigest};

/// How far one attached analytics document has got.
#[derive(Clone)]
pub(super) enum Resolution {
    /// Not asked yet.
    Waiting,
    /// Asked, and this is what the store said — `None` meaning it does not have
    /// the document at all.
    Answered(Option<SectionDigest>),
    /// Asked, and the store refused this reader (a 403). Its own case rather
    /// than "no digest", because the two are different facts: the asset is
    /// there and not this reader's to open, where a `None` answer means it is
    /// GONE, and reporting a refusal as a deletion is the lie issue #110 names.
    Refused,
    /// Asked, and nothing was established: a server error, a status this build
    /// does not know, a body with no digest in it, or a request that never
    /// left. Its own case rather than "no digest", for the same reason as
    /// [`Self::Refused`] and one status code over — `None` means GONE, and a
    /// check that did not complete confirms no deletion (issue #145).
    Failed,
}

/// What the store has said about one asset the canvas is waiting on.
#[derive(Clone)]
pub(super) enum MarkAnswer {
    /// Asked, and the answer has not arrived yet — its own case so that a mark
    /// is never painted from a request still in flight.
    Asked,
    /// Asked, and the store will not hand it to THIS reader. Not "gone" — see
    /// [`Resolution::Refused`].
    Refused,
    /// Asked, and nothing was established about it — see [`Resolution::Failed`].
    Failed,
    /// Asked, and this is the digest now — `None` meaning the store does not
    /// have the asset at all.
    Answered(Option<SectionDigest>),
}

/// The digest a successful analytics body carries, when it carries one.
///
/// A 2xx body with no digest in it establishes nothing, and the caller below
/// reads that as an unfinished check rather than as an absent document.
fn digest_in(body: &str) -> Option<SectionDigest> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("digest")?.as_str().map(SectionDigest::of_hex))
}

/// What one read established, before either reader's vocabulary names it.
///
/// A type of its own rather than the status and a loose `Option<digest>`,
/// because the two readers must not be able to spell "the store answered with
/// nothing" as "the store answered that it has nothing" — which is precisely
/// the collision issue #145 is about. [`Self::Gone`] is produced by a 404 and
/// by nothing else.
enum Established {
    /// The store answered, and this is the document's digest now.
    Digest(SectionDigest),
    /// The store does not have this asset. The one confirmation of a deletion.
    Gone,
    /// The store has it and will not hand it to this reader.
    Refused,
    /// Nothing was established: a server error, a status this build does not
    /// know, a request that never arrived, or a body with no digest in it.
    Nothing,
}

/// What the store's answer establishes. The rule is [`read_outcome`]'s.
fn established(status: u16, body: &str) -> Established {
    match read_outcome(status) {
        ReadOutcome::Answered => match digest_in(body) {
            Some(digest) => Established::Digest(digest),
            // Answered, but with nothing this side can use: the check did not
            // complete, which is not news about the document.
            None => Established::Nothing,
        },
        ReadOutcome::Gone => Established::Gone,
        ReadOutcome::Refused => Established::Refused,
        ReadOutcome::Unconfirmed => Established::Nothing,
    }
}

/// What the canvas' reader has been told about one asset.
pub(super) fn mark_answer(status: u16, body: &str) -> MarkAnswer {
    match established(status, body) {
        Established::Digest(digest) => MarkAnswer::Answered(Some(digest)),
        Established::Gone => MarkAnswer::Answered(None),
        Established::Refused => MarkAnswer::Refused,
        Established::Nothing => MarkAnswer::Failed,
    }
}

/// The same answer for the panel's reader.
pub(super) fn resolution(status: u16, body: &str) -> Resolution {
    match established(status, body) {
        Established::Digest(digest) => Resolution::Answered(Some(digest)),
        Established::Gone => Resolution::Answered(None),
        Established::Refused => Resolution::Refused,
        Established::Nothing => Resolution::Failed,
    }
}
