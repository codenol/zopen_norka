//! Fingerprints: what a link remembers, and what it is checked against.
//!
//! A section's link to an analytics document is binary — in sync, or broken —
//! and it is kept honest by two digests taken when the link was made: the
//! analytics document's, and the section's mockups'. The first change on either
//! side makes the stored digest disagree with the current one, and the section
//! can then say which side moved.
//!
//! ## Why a digest and not a version number
//!
//! Versions are a different, larger subject (#60): they need history, identity
//! over time and a place to keep the old ones. What this feature has to answer
//! is smaller and answerable today — "has anything changed since?" — and a
//! digest answers exactly that without inventing a numbering scheme the product
//! would then have to maintain and explain.
//!
//! ## Why the analytics digest normalizes line endings
//!
//! The digest answers "did the prose change", not "did the bytes change". A file
//! saved with CRLF line endings, or with one more trailing newline, is the same
//! analytics document, and a link that broke over that would teach people to
//! ignore the mark — which costs exactly what the mark is for. Everything else
//! in the text counts, including a single character.
//!
//! ## Why the mockup digest covers the whole subtree
//!
//! Every mockup the section owns is hashed as the canonical JSON the `.op` file
//! would carry for it, descendants included. Nothing is filtered out: a label
//! changed, a frame moved, a screen removed and a screen reordered are all
//! changes to what the section shows, and the section's job is to say that the
//! analytics it was built from were read against a different set of screens.
//! What the digest deliberately does NOT cover is the section's own name, colour
//! or properties: those are how the section is presented and what is written
//! about it, not the mockups, and hashing them would make renaming a section
//! look like editing its screens.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use jian_ops_schema::node::PenNode;

use crate::pen_node_ext::PenNodeExt;
use crate::section::section_mockups;

/// A SHA-256 digest, as 64 lower-case hex characters.
///
/// Not a security boundary and not treated as one: this answers "is this the
/// same as it was", and the only adversary is time. SHA-256 rather than a
/// cheaper 64-bit mix because it is already in this crate's build graph
/// (`jian-core` hashes with it) and because a truncated hash buys nothing here
/// but a lower bar.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SectionDigest(String);

impl SectionDigest {
    /// Characters in the hex form, for tests and for a caller sizing a column.
    pub const HEX_LEN: usize = 64;

    /// Digest of one byte string.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self::of_parts(&[bytes])
    }

    /// Digest of one string, verbatim.
    pub fn of_text(text: &str) -> Self {
        Self::of_parts(&[text.as_bytes()])
    }

    /// Digest of a node's canonical JSON — the bytes the `.op` file would carry.
    pub fn of_node(node: &PenNode) -> Self {
        Self::from_hasher(Self::fold_node(Sha256::new(), node))
    }

    /// Read a digest a caller spelled in hex — a fingerprint that came back
    /// over the wire.
    ///
    /// Not a validator: the value is compared with another digest and nothing
    /// else, so text that is not hex simply never compares equal. Its own
    /// constructor rather than `of_text` because hashing the hex would answer a
    /// different question and look right doing it.
    pub fn of_hex(hex: &str) -> Self {
        Self(hex.to_string())
    }

    /// The hex form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Digest of several fields, each length-prefixed.
    ///
    /// Length-prefixed because concatenation is ambiguous: `"ab"` followed by
    /// `"c"` and `"a"` followed by `"bc"` are the same bytes, and two different
    /// sections must never produce one digest.
    fn of_parts(parts: &[&[u8]]) -> Self {
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
        Self::from_hasher(hasher)
    }

    /// Fold one node into a running hash, length-prefixed like every field.
    fn fold_node(mut hasher: Sha256, node: &PenNode) -> Sha256 {
        match serde_json::to_vec(node) {
            Ok(bytes) => {
                hasher.update((bytes.len() as u64).to_be_bytes());
                hasher.update(&bytes);
            }
            // A node the schema cannot serialize is a node no reader can see
            // either. It still has to fold to something that cannot be confused
            // with a clean subtree, so the id goes in behind a leading NUL byte
            // — a JSON document never starts with one.
            Err(_) => {
                hasher.update([0u8]);
                let id = node.base().id.as_bytes();
                hasher.update((id.len() as u64).to_be_bytes());
                hasher.update(id);
            }
        }
        hasher
    }

    /// Wrap a finished hash as its hex form.
    fn from_hasher(hasher: Sha256) -> Self {
        let bytes = hasher.finalize();
        let mut hex = String::with_capacity(Self::HEX_LEN);
        for byte in bytes {
            hex.push(HEX_DIGITS[(byte >> 4) as usize] as char);
            hex.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
        }
        Self(hex)
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

impl std::fmt::Display for SectionDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The digest of an analytics document's prose.
///
/// Line endings are normalized and trailing whitespace is dropped, for the
/// reason the module docs give: the digest is about the prose, and a CRLF save
/// is not an edit. Leading whitespace and everything inside the text count.
pub fn analytics_fingerprint(markdown: &str) -> SectionDigest {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    SectionDigest::of_text(normalized.trim_end())
}

/// The digest of a section's mockups: its own children, in document order.
///
/// Empty for a node that is not a section, which is the honest answer — a frame
/// that is not a section has no mockups of its own to have changed.
pub fn mockup_fingerprint(section: &PenNode) -> SectionDigest {
    mockup_fingerprint_of(section_mockups(section))
}

/// The digest of one list of mockups.
///
/// Separate from [`mockup_fingerprint`] so that a future membership rule (a
/// screen named in the section's properties rather than held as a child) reuses
/// this one definition of "the mockups changed" instead of writing a second.
pub fn mockup_fingerprint_of(mockups: &[PenNode]) -> SectionDigest {
    let mut hasher = Sha256::new();
    // The count goes in first so that adding or removing a mockup changes the
    // digest even when the remaining bytes happen to line up.
    hasher.update((mockups.len() as u64).to_be_bytes());
    for mockup in mockups {
        hasher = SectionDigest::fold_node(hasher, mockup);
    }
    SectionDigest::from_hasher(hasher)
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
