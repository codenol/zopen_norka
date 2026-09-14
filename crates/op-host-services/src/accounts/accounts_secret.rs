//! Tokens: the values a client or an operator holds, and the hashes the
//! database holds instead of them.
//!
//! One rule governs everything in this file: **the plaintext exists on the way
//! out and never survives the call.** A session token, a password-reset link
//! and an invite link are all the same shape — a bearer secret that is checked
//! by looking up its hash — and all three are issued and stored here.
//!
//! ## Why a hash and not the token
//!
//! A table of live session tokens is a file that signs anybody in as anybody.
//! Storing SHA-256 instead means a leaked database is a list of sessions that
//! cannot be used: the value that opens them existed only in the browser that
//! received it. The lookup stays one index probe, because the hash — not the
//! token — is the primary key.
//!
//! ## Why SHA-256 is the right hash for this and the wrong one for a password
//!
//! A password is chosen by a person from a space small enough to enumerate, so
//! it needs a deliberately slow hash (Argon2id, [`super::accounts_password`]).
//! A token is 32 bytes this process read from the operating system's random
//! source: there is nothing to enumerate, and every candidate would have to be
//! generated rather than guessed. A fast hash is what makes a session resolve
//! cost one probe instead of a memory-hard computation on every request.

use base64::Engine as _;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::accounts_error::AccountsError;

/// How many random bytes a token is made of.
///
/// 256 bits, matching the hash that will cover it: a collision would require
/// finding two tokens with the same SHA-256, and the only way to present a
/// token that resolves to somebody else's session is to guess their token.
const TOKEN_BYTES: usize = 32;

/// Random bytes from the operating system.
///
/// Generic over the length so the salt of a password hash and the bytes of a
/// token come from the same place: one source of randomness in the store, not
/// one per call site.
pub(super) fn random_bytes<const N: usize>() -> Result<[u8; N], AccountsError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|error| AccountsError::Random(error.to_string()))?;
    Ok(bytes)
}

/// A fresh token, and the hash to store in its place.
///
/// The token is URL-safe base64 of 32 random bytes: it travels in a query
/// string (an invite link) and in a cookie, and standard base64's `+` and `/`
/// are one escaping rule away from being mangled by whichever layer touches it
/// first — a mangled token is a link that works for the sender and nobody else.
/// Padding is dropped for the same reason (`=` is another character a query
/// string may rewrite).
pub fn issue_token() -> Result<(String, [u8; 32]), AccountsError> {
    let bytes = random_bytes::<TOKEN_BYTES>()?;
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&token);
    Ok((token, hash))
}

/// The hash stored in place of a token.
///
/// Of the token's TEXT, not of the bytes it was built from: the client sends
/// the string, and whichever layer receives it has the string and nothing else.
/// A hash taken before encoding could only be recomputed by a caller that also
/// knew the encoding — which is the kind of coupling that breaks silently when
/// one side changes.
///
/// Unkeyed, and that is a deliberate limit: this protects against a leaked
/// database, not against an attacker who can compute a hash of a token they
/// already hold. Keying it would mean a secret the store must keep and rotate,
/// and would make every stored hash unrecoverable if that secret were lost —
/// a trade this store does not need, because the token is already unguessable.
pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// A stored hash as text, for a surface that has to NAME a row.
///
/// ## Why a hash may be written down
///
/// Everywhere else in this module the rule is that a hash never leaves the
/// store. This is the one exception, and it is safe for a reason worth stating:
/// hashing is one-way, the token behind this digest is 256 random bits, and
/// **no path in this product accepts a hash as a credential** — a presented
/// token is hashed and looked up, so a presented hash is hashed again and
/// matches nothing. What the hex buys is that a table keyed by `token_hash`
/// alone can be addressed by a human: without it, an operator's list of
/// invitations could say everything about a link except which one it is
/// talking about, and "withdraw the link I sent to the wrong person" would be
/// impossible without the link itself.
///
/// Lowercase and fixed width, because this value is compared as text by
/// whoever holds it.
pub fn hash_hex(hash: &[u8]) -> String {
    let mut out = String::with_capacity(hash.len() * 2);
    for byte in hash {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The 32 bytes a [`hash_hex`] string names, or `None` when it names none.
///
/// Total rather than fallible: a caller here is a request body, and "this is
/// not a row id" and "no such row" earn the same answer. Anything that is not
/// exactly 64 lowercase-or-uppercase hex digits — a token pasted by mistake, a
/// truncated copy, an id from some other system — is `None`, which the caller
/// reports as "no such invitation" rather than as a malformed request.
pub fn hash_from_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let pair = text.get(index * 2..index * 2 + 2)?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

/// Whether a stored token hash is the hash of the token just presented.
///
/// Constant-time in the bytes compared, so the answer cannot be turned into
/// "how many leading bytes were right" — the oracle that lets a token be
/// recovered one byte at a time. The length is not hidden, and does not need to
/// be: a hash of the wrong length is a row this schema refuses to hold.
///
/// The lookups in this module are by primary key, so SQLite has already matched
/// the value before this runs. The check is still made, because it is what
/// makes "this is the row that was asked for" a property of the code rather
/// than of the query planner, and because the day a caller reads a hash by
/// something other than the hash, this is the function that must be used.
pub fn token_hash_eq(stored: &[u8], presented: &[u8; 32]) -> bool {
    stored.len() == presented.len() && bool::from(stored.ct_eq(&presented[..]))
}
