//! What this product accepts as a password.
//!
//! ## Why a floor, and why so low
//!
//! A password here is the only thing standing between a stranger and an
//! account's documents, and the hash that stores it (Argon2id) only buys time
//! against guessing — it does not buy entropy. So a password that is a word,
//! a name, or twelve `a`s is worth exactly what it costs to guess, however
//! slow the hash is. The rules below refuse the three shapes that are guessed
//! first and cost nothing to refuse.
//!
//! What this is NOT is a strength meter. There is no dictionary here, no
//! character-class arithmetic, no entropy estimate. A dictionary is a list
//! somebody has to keep current, and character classes are how a product ends
//! up telling people that `Password1!` is strong. Twelve characters of
//! anything is a floor an operator can explain to a colleague in one sentence,
//! which is the property that makes a floor hold.
//!
//! ## Where it is applied, and where it deliberately is not
//!
//! Not in [`crate::accounts::AccountsDb::create_user`]: the store records what
//! it is given and decides nothing about people, and a rule hidden in an
//! insert is a rule the next entry point will not know it has to satisfy. It
//! is applied at the points that ACCEPT a new password from somebody — the
//! first-admin bootstrap and the invitation flow — each of which calls
//! [`check_password_strength`] and turns a refusal into its own answer.

use op_editor_core::access::ProductRole;

/// Shortest password this product accepts.
///
/// Twelve, not eight: eight characters of a human-chosen password is a few
/// hours of offline work against a hash somebody has already stolen, and the
/// cost of asking for four more characters is one second of typing.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// Distinct characters a password must use.
///
/// Four, which is a shape test and not an entropy test: it refuses `aaaaaaaaaaaa`
/// and `abababababab` — the two strings a generator-free "make it twelve
/// characters" instruction produces — without pretending to measure anything
/// about the rest.
const MIN_DISTINCT_CHARS: usize = 4;

/// Shortest account name worth looking for inside a password.
///
/// A three-letter name (`qa`, `ann`) occurs inside honest passwords by
/// accident, and refusing those teaches people to fight the form.
const MIN_ACCOUNT_NAME_CHARS: usize = 4;

/// Why a password was refused.
///
/// Typed rather than a sentence, because the two callers say it differently —
/// the CLI prints it and re-prompts, an HTTP route turns it into a status code
/// and a machine-readable `error` — and because the reason is the whole value
/// of the check: "weak password" without a reason is a form that rejects
/// people and will not tell them why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeakPasswordReason {
    /// Fewer than [`MIN_PASSWORD_CHARS`] characters.
    TooShort,
    /// Uses fewer than [`MIN_DISTINCT_CHARS`] distinct characters.
    TooRepetitive,
    /// Contains the account name it is meant to protect.
    NamesTheAccount,
}

impl WeakPasswordReason {
    /// Stable machine-readable code for a REST body.
    pub const fn code(self) -> &'static str {
        match self {
            Self::TooShort => "password-too-short",
            Self::TooRepetitive => "password-too-repetitive",
            Self::NamesTheAccount => "password-names-the-account",
        }
    }
}

impl std::fmt::Display for WeakPasswordReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(
                f,
                "a password must be at least {MIN_PASSWORD_CHARS} characters"
            ),
            Self::TooRepetitive => write!(
                f,
                "a password must use at least {MIN_DISTINCT_CHARS} different characters"
            ),
            Self::NamesTheAccount => f.write_str("a password must not contain the account name"),
        }
    }
}

impl std::error::Error for WeakPasswordReason {}

/// Whether `password` may protect `username`.
///
/// The whole policy, in one function, so that "what does this deployment
/// accept" has a single answer a reviewer can read — and so that a new entry
/// point cannot invent a variant of it. Note what is NOT consulted: nothing
/// about the account, the store, or the deployment. Whether a password is
/// acceptable is a property of the password and the name.
pub fn check_password_strength(password: &str, username: &str) -> Result<(), WeakPasswordReason> {
    // Counted in characters, not bytes: a ten-character password in a
    // non-Latin script is ten characters of entropy to its owner, and refusing
    // it for being 20 bytes long (or accepting a five-character one for being
    // 15) would make the rule depend on the alphabet rather than on the length.
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(WeakPasswordReason::TooShort);
    }
    let mut seen: Vec<char> = Vec::with_capacity(MIN_DISTINCT_CHARS);
    for character in password.chars() {
        if !seen.contains(&character) {
            seen.push(character);
            if seen.len() >= MIN_DISTINCT_CHARS {
                break;
            }
        }
    }
    if seen.len() < MIN_DISTINCT_CHARS {
        return Err(WeakPasswordReason::TooRepetitive);
    }
    let name = username.trim().to_lowercase();
    if name.chars().count() >= MIN_ACCOUNT_NAME_CHARS && password.to_lowercase().contains(&name) {
        return Err(WeakPasswordReason::NamesTheAccount);
    }
    Ok(())
}

/// The role the first administrator is created with.
///
/// Named here — beside the password floor and the bootstrap that uses both —
/// rather than spelled as a string at each call site, so that "who is the
/// first admin" is answerable by reading one module. The value is the role's
/// own canonical wire name, not a second copy of it: the account's `roles`
/// column holds wire strings, and a rename in
/// [`op_editor_core::access::ProductRole`] must reach the database.
pub const FIRST_ADMIN_ROLE: &str = ProductRole::Admin.as_wire();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_password_that_is_not_the_account_name_is_accepted() {
        assert_eq!(
            check_password_strength("karta-mosta-42", "designer"),
            Ok(())
        );
        // Non-ASCII is counted as characters, not bytes.
        assert_eq!(
            check_password_strength("пароль-к-канвасу", "designer"),
            Ok(())
        );
    }

    #[test]
    fn a_short_password_is_refused_whatever_alphabet_it_is_in() {
        assert_eq!(
            check_password_strength("short-pass", "designer"),
            Err(WeakPasswordReason::TooShort)
        );
        // Ten characters of Cyrillic is 20 bytes and still too short: the rule
        // is about what the person typed.
        assert_eq!(
            check_password_strength("десятьсимв", "designer"),
            Err(WeakPasswordReason::TooShort)
        );
        assert_eq!(
            check_password_strength(&"a".repeat(MIN_PASSWORD_CHARS - 1), "designer"),
            Err(WeakPasswordReason::TooShort)
        );
    }

    #[test]
    fn a_password_made_of_one_or_two_characters_is_refused_at_any_length() {
        assert_eq!(
            check_password_strength(&"a".repeat(40), "designer"),
            Err(WeakPasswordReason::TooRepetitive)
        );
        assert_eq!(
            check_password_strength(&"ab".repeat(20), "designer"),
            Err(WeakPasswordReason::TooRepetitive)
        );
        // Four distinct characters is the floor and it passes.
        assert_eq!(
            check_password_strength(&"abcd".repeat(3), "designer"),
            Ok(())
        );
    }

    #[test]
    fn a_password_that_names_its_own_account_is_refused() {
        assert_eq!(
            check_password_strength("designer-2026-secret", "Designer"),
            Err(WeakPasswordReason::NamesTheAccount)
        );
        assert_eq!(
            check_password_strength("DESIGNER-2026-secret", "designer"),
            Err(WeakPasswordReason::NamesTheAccount)
        );
        // The name is looked for as a substring, so padding it does not help.
        assert_eq!(
            check_password_strength("!!alice-in-wonderland!!", "alice"),
            Err(WeakPasswordReason::NamesTheAccount)
        );
    }

    #[test]
    fn a_short_account_name_is_not_searched_for_inside_the_password() {
        // `qa` occurs inside honest passwords by accident; refusing those would
        // teach people to fight the form rather than to pick better passwords.
        assert_eq!(check_password_strength("qa-is-my-craft", "qa"), Ok(()));
    }

    #[test]
    fn a_refusal_says_which_rule_it_broke_and_never_repeats_the_password() {
        // A CLI re-prompts on this text and a route ships it to a form: it has
        // to be actionable, and it must not carry the secret back out.
        let secret = "aaaaaaaaaaaa";
        let text = WeakPasswordReason::TooRepetitive.to_string();
        assert!(!text.contains(secret), "{text}");
        assert_eq!(WeakPasswordReason::TooShort.code(), "password-too-short");
        assert_eq!(
            WeakPasswordReason::NamesTheAccount.code(),
            "password-names-the-account"
        );
        assert!(WeakPasswordReason::TooShort.to_string().contains("12"));
    }

    #[test]
    fn the_first_admin_role_is_the_admin_roles_own_wire_name() {
        // One spelling, taken from the role itself: a rename in op-editor-core
        // reaches the database instead of leaving two vocabularies behind.
        assert_eq!(FIRST_ADMIN_ROLE, "admin");
        assert!(ProductRole::from_wire(FIRST_ADMIN_ROLE).is_ok());
    }
}
