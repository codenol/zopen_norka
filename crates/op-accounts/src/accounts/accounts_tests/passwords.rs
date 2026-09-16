//! What reaches `users.password_hash`, and what it means to check a password
//! against it.

use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

use super::*;
use crate::accounts::{hash_password, verify_password, AccountsError, HASH_ALGO_ARGON2ID};

#[test]
fn a_password_verifies_against_its_own_hash_and_no_other() {
    let hash = hash_password(PASSWORD).expect("hash");

    assert!(verify_password(PASSWORD, &hash).expect("verify"));
    assert!(!verify_password("almost-the-right-password", &hash).expect("verify"));
    assert!(!verify_password("", &hash).expect("verify"));
    assert!(
        !verify_password(&format!("{PASSWORD} "), &hash).expect("verify"),
        "a trailing space is a different password, not a trimmed one"
    );
}

#[test]
fn the_stored_string_names_its_algorithm_and_its_cost() {
    let hash = hash_password(PASSWORD).expect("hash");

    // The parameters travel in the value, which is what makes raising the cost
    // later a rewrite of the rows rather than a migration of the schema — and
    // what `hash_algo` has to agree with.
    assert!(hash.starts_with("$argon2id$v=19$m="), "{hash}");
    assert!(hash.contains(",t="), "{hash}");
    assert_eq!(HASH_ALGO_ARGON2ID, "argon2id");
}

#[test]
fn the_same_password_never_hashes_the_same_way_twice() {
    let first = hash_password(PASSWORD).expect("hash");
    let second = hash_password(PASSWORD).expect("hash");

    assert_ne!(
        first, second,
        "a fresh salt per hash is what stops one leaked table answering for another"
    );
    assert!(verify_password(PASSWORD, &first).expect("verify"));
    assert!(verify_password(PASSWORD, &second).expect("verify"));
}

#[test]
fn the_password_is_nowhere_in_the_database_file() {
    let (dir, db) = store();
    let user = active_user(&db, "u1", "alice");

    let bytes = file_bytes(&dir);
    assert!(
        !appears_in(&bytes, PASSWORD),
        "the plaintext must not be in accounts.db or its WAL"
    );
    // And not in a column under another name either: the hash is the only place
    // the password can be, and it is not the password.
    let stored: String = db
        .conn()
        .query_row(
            "SELECT password_hash FROM users WHERE id = 'u1'",
            [],
            |row| row.get(0),
        )
        .expect("the hash");
    assert_ne!(stored, PASSWORD);
    assert_eq!(Some(stored), user.password_hash);
}

#[test]
fn a_stored_value_this_build_cannot_read_is_an_error_not_a_wrong_password() {
    // Reported as "wrong password", a corrupt row would look exactly like a
    // person mistyping, and the owner of that account could try forever.
    for broken in [
        "",
        "not-a-phc-string",
        "$argon2id$v=19$m=x,t=y,p=z$salt$hash",
    ] {
        let error = verify_password(PASSWORD, broken).expect_err("a hash this build cannot read");
        assert!(
            matches!(error, AccountsError::PasswordHash(_)),
            "{broken} gave {error:?}"
        );
    }
}

#[test]
fn a_hash_written_by_another_argon2_variant_still_verifies() {
    // Verification follows the algorithm, version and cost NAMED IN THE STORED
    // STRING, not this build's defaults — that is what keeps a hash written by
    // an older build verifiable after the defaults are raised, and it means the
    // `hash_algo` column, not this function, is the record of what was written.
    // Pinned here because the opposite behaviour (refusing anything that is not
    // today's algorithm) is the intuitive one and would lock accounts out.
    let salt = SaltString::encode_b64(b"0123456789abcdef").expect("salt");
    let argon2i = Argon2::new(Algorithm::Argon2i, Version::V0x13, Params::default())
        .hash_password(PASSWORD.as_bytes(), &salt)
        .expect("hash")
        .to_string();

    assert!(verify_password(PASSWORD, &argon2i).expect("verify"));
    assert!(!verify_password("something-else", &argon2i).expect("verify"));
}

#[test]
fn a_hash_from_an_algorithm_this_build_does_not_implement_is_an_error() {
    // A PBKDF2 PHC string is a well-formed hash this build cannot compute. It
    // is reported as a fault rather than as a wrong password: the owner of that
    // account must not be told to try again, because trying again never works.
    let pbkdf2 = "$pbkdf2-sha256$i=1000$c2FsdHNhbHQ$aGFzaGhhc2g";

    let error = verify_password(PASSWORD, pbkdf2).expect_err("another algorithm");
    assert!(
        matches!(error, AccountsError::PasswordHash(_)),
        "got {error:?}"
    );
}

#[test]
fn an_empty_password_is_refused_rather_than_hashed() {
    assert_eq!(
        hash_password("").expect_err("an empty password"),
        AccountsError::EmptyPassword
    );
}
