//! Password hashing: what goes into `users.password_hash`, and what checking a
//! password against it means.
//!
//! Argon2id, through the RustCrypto `argon2` crate — no home-made
//! construction, and no home-made storage format. What is stored is a PHC
//! string:
//!
//! ```text
//! $argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>
//! ```
//!
//! ## Why the parameters live in the stored value
//!
//! The cost, the parallelism, the salt and the algorithm are all INSIDE the
//! string. That is what makes raising the cost later a per-row rewrite rather
//! than a schema migration: a hash written today still verifies tomorrow, and a
//! hash written tomorrow can be recognised by its own parameters as the one to
//! re-encode. A bare digest in the column would have made the parameters a
//! property of the build that wrote the row, and the build that reads it is not
//! necessarily the same one.
//!
//! ## Why the password never appears in an error
//!
//! Nothing in this module puts a password, or a fragment of one, into a
//! message: errors travel into logs and into HTTP responses. The failures named
//! here are about the HASH — it could not be produced, or the stored one is not
//! a hash this build can read — and "wrong password" is not one of them. A
//! wrong password is not a failure: it is [`verify_password`] answering
//! `Ok(false)`.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use super::accounts_error::AccountsError;
use super::accounts_secret::random_bytes;

/// The value of `users.hash_algo` for hashes this module writes.
pub const HASH_ALGO_ARGON2ID: &str = "argon2id";

/// How many random bytes the salt is made of.
///
/// 128 bits, the length the PHC specification recommends for a salt and the one
/// `Salt::RECOMMENDED_LENGTH` names. Random per hash, which is what makes two
/// accounts with the same password have different rows: without a salt, one
/// leaked table of hashes can be matched against another, and a precomputed
/// table of common passwords answers for every account at once.
const SALT_BYTES: usize = 16;

/// Hash a password for storage.
pub fn hash_password(password: &str) -> Result<String, AccountsError> {
    // Refused rather than hashed: an empty password is a credential that
    // verifies, and once stored there is nothing in the row to distinguish it
    // from one somebody meant to set. The rest of a password policy — length,
    // character classes, a check against known-breached lists — belongs to the
    // layer with the form in front of it; this is the one rule the store cannot
    // leave to a caller, because a caller that forgets it writes a row no audit
    // could flag.
    if password.is_empty() {
        return Err(AccountsError::EmptyPassword);
    }
    let salt = SaltString::encode_b64(&random_bytes::<SALT_BYTES>()?)
        .map_err(|error| AccountsError::PasswordHash(error.to_string()))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| AccountsError::PasswordHash(error.to_string()))?;
    Ok(hash.to_string())
}

/// Check a password against a stored hash.
///
/// `Ok(false)` is the wrong password. `Err` is a stored value this build cannot
/// read at all — an empty column where a hash was expected, a PHC string for an
/// algorithm this build does not implement, a truncated write. The two are kept
/// apart on purpose: reported as "wrong password", a corrupt row would look
/// exactly like a person mistyping, and the owner of that account could try
/// forever.
///
/// The comparison is Argon2's own, and it is constant-time over the derived
/// key. It recomputes with the algorithm, version and cost NAMED IN THE STORED
/// STRING rather than with this build's defaults — measured, not assumed: an
/// `$argon2i$` hash verifies here even though [`Argon2::default`] is Argon2id.
/// That is the behaviour to want. A build that accepted only today's algorithm
/// would lock out every account whose hash was written before the defaults
/// changed, and the row already records which algorithm produced it in
/// `users.hash_algo`.
pub fn verify_password(password: &str, stored: &str) -> Result<bool, AccountsError> {
    let parsed = PasswordHash::new(stored).map_err(|error| {
        AccountsError::PasswordHash(format!("stored hash is not a PHC string: {error}"))
    })?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        // The one outcome that is an answer rather than a fault.
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(error) => Err(AccountsError::PasswordHash(error.to_string())),
    }
}
