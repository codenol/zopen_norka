//! The first administrator, from the deployment's environment.
//!
//! ## Why the environment, and why only once
//!
//! A container has no terminal. The operator who deploys one can set variables
//! in a compose file or a secret store, and cannot be relied on to `exec` in
//! and type a password — so this is the deployment branch of the bootstrap,
//! and `op admin create` is the one for a host somebody is sitting at.
//!
//! The variables are read on every start and obeyed on exactly one of them:
//! the start that finds a store with **no accounts in it** (see
//! [`crate::accounts::FirstAdmin`] for why the question is "no accounts" and
//! not "no admin"). A deployment whose password was rotated in the secret
//! store, or whose variables were left behind by a copy-pasted compose file,
//! therefore does not grow a second administrator — the condition is false and
//! nothing happens.
//!
//! ## Why a refusal is not a start-up failure
//!
//! A weak password here is reported on stderr and the daemon serves anyway.
//! The alternative — refusing to start — turns a password policy into a
//! crash-looping container, and the operator's remedy for a crash loop is to
//! remove the variable, which is the opposite of the outcome the policy wants.
//! The status route says the deployment has no admin yet, the log says why, and
//! the fix is either a better password or `op admin create`.

use crate::accounts::{AccountsDb, FirstAdmin, WeakPasswordReason};

/// The administrator's name. Absent or blank means "not asked for".
pub const ADMIN_USERNAME_ENV: &str = "NORKA_ADMIN_USERNAME";

/// The administrator's password. Read here and written straight into an
/// Argon2id hash: it is never logged, never echoed, and never stored as text.
pub const ADMIN_PASSWORD_ENV: &str = "NORKA_ADMIN_PASSWORD";

/// What the deployment did about its first administrator.
///
/// Typed rather than a log line, so the decision is testable without reading
/// stderr and so the run loop's message is a rendering of this rather than the
/// only statement of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminBootstrap {
    /// Neither variable is set. The ordinary case for a deployment that has
    /// been running for a while.
    NotRequested,
    /// The store already held accounts, so the variables were ignored.
    Ignored {
        /// How many accounts it already held.
        accounts: u64,
    },
    /// Exactly one of the two was set, so there is no account to create.
    Incomplete,
    /// The account was created, with the admin role.
    Created {
        /// The name it was created under (never the password).
        username: String,
    },
    /// The password was refused, and nothing was created.
    Refused(WeakPasswordReason),
    /// The store could not be read or written.
    Failed(String),
}

impl AdminBootstrap {
    /// Whether this outcome is worth a line in the log.
    ///
    /// False for the two that mean "nothing happened, as intended":
    /// `NotRequested` (no variables) and `Ignored` (the deployment already has
    /// accounts). A daemon that printed all six on every start would train its
    /// operator to stop reading the log. The run loop asks THIS rather than
    /// matching the variants itself, so adding an outcome cannot silently
    /// become silent.
    pub const fn is_quiet(&self) -> bool {
        matches!(self, Self::NotRequested | Self::Ignored { .. })
    }
}

/// Apply `NORKA_ADMIN_USERNAME` / `NORKA_ADMIN_PASSWORD` to this deployment.
pub fn ensure_first_admin_from_env(db: &AccountsDb) -> AdminBootstrap {
    let username = std::env::var(ADMIN_USERNAME_ENV).ok();
    let password = std::env::var(ADMIN_PASSWORD_ENV).ok();
    ensure_first_admin(
        db,
        username.as_deref(),
        password.as_deref(),
        crate::accounts::now_secs(),
    )
}

/// The decision, with the environment already read.
///
/// Takes the two values rather than reading them so that the rule — which
/// combination of them creates what, and what it does when the store is not
/// fresh — is a value a test can pin without mutating a process-wide variable
/// every other test in the binary shares.
///
/// Blank counts as unset, exactly as [`crate::accounts::DATA_DIR_ENV`] does: a
/// compose file that writes `NORKA_ADMIN_PASSWORD=` is a deployment that has
/// not decided, not one that decided on the empty password.
pub fn ensure_first_admin(
    db: &AccountsDb,
    username: Option<&str>,
    password: Option<&str>,
    now: i64,
) -> AdminBootstrap {
    let username = configured(username);
    let password = configured(password);
    let (Some(username), Some(password)) = (username, password) else {
        return if username.is_none() && password.is_none() {
            AdminBootstrap::NotRequested
        } else {
            // One half is a mistake worth naming: the operator believes they
            // provisioned an admin and did not. Creating one from a username
            // and an empty password is not on the table (the store refuses an
            // empty password), and inventing a password would be worse than
            // saying nothing happened.
            AdminBootstrap::Incomplete
        };
    };
    match db.create_first_admin(username, password, now) {
        Ok(FirstAdmin::Created(user)) => AdminBootstrap::Created {
            username: user.username,
        },
        Ok(FirstAdmin::AlreadyProvisioned { accounts }) => AdminBootstrap::Ignored { accounts },
        Ok(FirstAdmin::WeakPassword(reason)) => AdminBootstrap::Refused(reason),
        Err(error) => AdminBootstrap::Failed(error.to_string()),
    }
}

/// A value that counts as configured, or nothing.
fn configured(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{NewUser, UserStatus, FIRST_ADMIN_ROLE};
    use crate::document_test_dir::TempDir;
    use op_editor_core::access::RoleSet;

    const NOW: i64 = 1_700_000_000;
    const PASSWORD: &str = "a-long-deployment-password";

    fn store() -> (TempDir, AccountsDb) {
        let dir = TempDir::new("account-admin");
        let db = AccountsDb::open(dir.path()).expect("open the account store");
        (dir, db)
    }

    #[test]
    fn a_fresh_deployment_creates_the_admin_the_variables_name() {
        let (_dir, db) = store();
        assert_eq!(
            ensure_first_admin(&db, Some("operator"), Some(PASSWORD), NOW),
            AdminBootstrap::Created {
                username: "operator".into()
            }
        );
        let user = db
            .find_user_by_username("operator")
            .expect("look up")
            .expect("the admin exists");
        assert_eq!(user.status, UserStatus::Active);
        assert!(RoleSet::from_wire(user.roles.iter().map(String::as_str))
            .rights()
            .can_manage_users());
        assert_eq!(user.roles, vec![FIRST_ADMIN_ROLE.to_string()]);
    }

    #[test]
    fn a_deployment_that_already_has_an_account_ignores_the_variables() {
        let (_dir, db) = store();
        // Even a variable pair that would create a perfectly good admin, and
        // even a disabled existing account: the question is whether the store
        // is fresh, not whether it has an admin.
        db.create_user(&NewUser::invited("colleague", "Colleague"), NOW)
            .expect("an invited account is an account");
        assert_eq!(
            ensure_first_admin(&db, Some("operator"), Some(PASSWORD), NOW),
            AdminBootstrap::Ignored { accounts: 1 }
        );
        assert!(db
            .find_user_by_username("operator")
            .expect("look up")
            .is_none());
    }

    #[test]
    fn half_a_pair_is_reported_rather_than_guessed_at() {
        let (_dir, db) = store();
        for (username, password) in [
            (Some("operator"), None),
            (None, Some(PASSWORD)),
            // Blank is unset: a compose file that wrote the variable and left
            // it empty has not decided.
            (Some("operator"), Some("")),
            (Some("   "), Some(PASSWORD)),
        ] {
            let outcome = ensure_first_admin(&db, username, password, NOW);
            assert_eq!(outcome, AdminBootstrap::Incomplete, "{username:?}");
        }
        assert_eq!(db.count_users().expect("count"), 0);
    }

    #[test]
    fn nothing_set_is_not_an_event() {
        let (_dir, db) = store();
        let outcome = ensure_first_admin(&db, None, None, NOW);
        assert_eq!(outcome, AdminBootstrap::NotRequested);
        assert!(outcome.is_quiet());
        assert!(AdminBootstrap::Ignored { accounts: 2 }.is_quiet());
        assert!(!AdminBootstrap::Incomplete.is_quiet());
    }

    #[test]
    fn a_weak_password_is_refused_and_named() {
        let (_dir, db) = store();
        assert_eq!(
            ensure_first_admin(&db, Some("operator"), Some("short"), NOW),
            AdminBootstrap::Refused(WeakPasswordReason::TooShort)
        );
        assert_eq!(db.count_users().expect("count"), 0);
        // The refusal is a message an operator can act on, and it does not
        // contain the password that was refused.
        let text = WeakPasswordReason::TooShort.to_string();
        assert!(text.contains("12"), "{text}");
        assert!(!text.contains("short-pass"), "{text}");
    }

    #[test]
    fn an_unreadable_store_is_reported_rather_than_crashing_the_daemon() {
        let (dir, db) = store();
        // A real fault: a second connection drops the table out from under the
        // store, so the count this decision starts with cannot be answered.
        let other = rusqlite::Connection::open(dir.path().join("accounts.db"))
            .expect("a second connection to the same file");
        other
            .execute_batch("DROP TABLE users")
            .expect("drop the table under the store");

        let outcome = ensure_first_admin(&db, Some("operator"), Some(PASSWORD), NOW);
        assert!(matches!(outcome, AdminBootstrap::Failed(_)), "{outcome:?}");
        // A deployment that cannot read its accounts has something to say, so
        // this is not one of the quiet outcomes.
        assert!(!outcome.is_quiet());
    }
}
