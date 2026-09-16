//! The FIRST administrator: the one account a deployment cannot sign its way
//! into.
//!
//! ## Why this exists at all
//!
//! Every other account in this product is made by somebody who is already
//! signed in — an admin issuing an invite, an invitee accepting it. A fresh
//! deployment has nobody, so it has no way to make its first account, and the
//! tempting shortcut is to let the first visitor become the admin. That
//! shortcut is a race with whoever reaches the URL first, which on a public
//! address is not the operator.
//!
//! So the first admin is a deliberate act by whoever owns the machine: either
//! two environment variables on the deployment's first open, or
//! `op admin create` against the same data directory. Both go through
//! [`AccountsDb::create_first_admin`] below, which is what makes them the same
//! act rather than two implementations of it.
//!
//! ## Why "no accounts at all" and not "no admin"
//!
//! The question this asks is [`AccountsDb::count_users`] — is this store
//! fresh? — and never "is there an admin". A deployment that already holds one
//! DISABLED account, or a single invited colleague, is not fresh: its
//! operator has already made decisions about who exists, and quietly minting a
//! second administrator from an environment variable at that point is how a
//! deployment grows an admin nobody chose. An operator who wants another
//! administrator issues an invitation.
//!
//! ## What it does not do
//!
//! It does not sign anybody in, and it does not touch the environment: this
//! module takes a name and a password and answers what happened. Reading
//! `NORKA_ADMIN_*` is the deployment's business
//! (`op_host_services::web_canvas_server::account_admin`), and a store that read
//! the environment would be a store whose contents depend on when the variable
//! was last written.

use super::accounts_error::AccountsError;
use super::accounts_model::{NewUser, User};
use super::accounts_password_strength::{
    check_password_strength, WeakPasswordReason, FIRST_ADMIN_ROLE,
};
use super::AccountsDb;

/// What came of asking for a first administrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstAdmin {
    /// The account was created, with the admin role and an active status.
    Created(User),
    /// The store already holds at least one account, so nothing was created.
    ///
    /// Carries the count because it is the answer to the only question the
    /// caller has left ("did I misunderstand, or is this deployment in use?"),
    /// and it costs nothing — the count is what the decision was made on.
    AlreadyProvisioned {
        /// How many accounts the store already held.
        accounts: u64,
    },
    /// The password was refused, and nothing was created.
    ///
    /// Separate from an [`AccountsError`] because it is not a failure of the
    /// store: the store was asked a question it can answer, and the answer was
    /// no. It is also the one outcome the operator can fix by retyping.
    WeakPassword(WeakPasswordReason),
}

/// Creates the deployment's first administrator, or says why it did not.
impl AccountsDb {
    /// Make `username` the first administrator of this deployment.
    ///
    /// Idempotent in the direction that matters: called twice, the second call
    /// finds accounts and creates nothing. Two daemons racing on one data
    /// directory could both find an empty store and both create an account —
    /// they are already a broken deployment (the tenancy store assumes one
    /// writer), and the alternative, a lock held across the whole boot, buys a
    /// guarantee nothing here can use.
    ///
    /// The password is checked BEFORE the account is created, so a refused
    /// password leaves no half-made administrator behind: an account row with
    /// no password would look like an invitation to whoever read the table
    /// next.
    pub fn create_first_admin(
        &self,
        username: &str,
        password: &str,
        now: i64,
    ) -> Result<FirstAdmin, AccountsError> {
        let accounts = self.count_users()?;
        if accounts > 0 {
            return Ok(FirstAdmin::AlreadyProvisioned { accounts });
        }
        if let Err(weak) = check_password_strength(password, username) {
            return Ok(FirstAdmin::WeakPassword(weak));
        }
        // `display_name` is the username: the operator has told us one name,
        // and inventing a second (title-cased, say) would be this module
        // deciding how somebody is addressed. They can change it once they are
        // signed in.
        let user = self.create_user(
            &NewUser {
                id: None,
                username,
                display_name: username,
                email: None,
                password: Some(password),
                roles: &[FIRST_ADMIN_ROLE],
            },
            now,
        )?;
        Ok(FirstAdmin::Created(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TempDir;

    const NOW: i64 = 1_700_000_000;
    /// Long and distinctive: the strength floor is 12 characters and this test
    /// is not about the floor.
    const PASSWORD: &str = "first-admin-of-this-deployment";

    fn store() -> (TempDir, AccountsDb) {
        let dir = TempDir::new("accounts-bootstrap");
        let db = AccountsDb::open(dir.path()).expect("open the account store");
        (dir, db)
    }

    #[test]
    fn a_fresh_store_gets_its_admin_with_the_admin_role_and_active_status() {
        let (_dir, db) = store();
        let created = db
            .create_first_admin("operator", PASSWORD, NOW)
            .expect("create the first admin");
        let FirstAdmin::Created(user) = created else {
            panic!("expected a created admin, got {created:?}");
        };
        assert_eq!(user.username, "operator");
        assert_eq!(user.display_name, "operator");
        assert_eq!(user.roles, vec![FIRST_ADMIN_ROLE.to_string()]);
        assert_eq!(user.status, super::super::UserStatus::Active);
        assert!(user.password_hash.is_some());
        // The role reaches the decision points that read it, which is what
        // makes this an ADMIN and not just an account with a label.
        let roles =
            op_editor_core::access::RoleSet::from_wire(user.roles.iter().map(String::as_str));
        assert!(roles.rights().can_manage_users());
    }

    #[test]
    fn a_deployment_that_already_holds_an_account_creates_nothing() {
        let (_dir, db) = store();
        db.create_first_admin("operator", PASSWORD, NOW)
            .expect("create the first admin");
        let second = db
            .create_first_admin("someone-else", "another-long-password", NOW)
            .expect("ask again");
        assert_eq!(second, FirstAdmin::AlreadyProvisioned { accounts: 1 });
        assert_eq!(db.count_users().expect("count"), 1);
        assert!(db
            .find_user_by_username("someone-else")
            .expect("look up")
            .is_none());
    }

    #[test]
    fn one_disabled_account_is_not_a_fresh_deployment() {
        // The rule is "no accounts", never "no admin": an operator who
        // disabled their own account while keeping it has already made
        // decisions about who exists, and an environment variable must not
        // silently outvote them.
        let (_dir, db) = store();
        let user = db
            .create_user(&NewUser::active("operator", "Operator", PASSWORD), NOW)
            .expect("create an account");
        db.set_status(&user.id, super::super::UserStatus::Disabled, NOW)
            .expect("disable it");
        assert_eq!(
            db.create_first_admin("rescue", "a-long-rescue-password", NOW)
                .expect("ask for a first admin"),
            FirstAdmin::AlreadyProvisioned { accounts: 1 }
        );
    }

    #[test]
    fn a_weak_password_creates_no_administrator_at_all() {
        let (_dir, db) = store();
        let outcome = db
            .create_first_admin("operator", "short", NOW)
            .expect("ask for a first admin");
        assert_eq!(
            outcome,
            FirstAdmin::WeakPassword(WeakPasswordReason::TooShort)
        );
        // Nothing half-made: an account row without a usable password would
        // read as an invitation to whoever looked at the table next.
        assert_eq!(db.count_users().expect("count"), 0);
        assert!(db
            .find_user_by_username("operator")
            .expect("look up")
            .is_none());
    }

    #[test]
    fn a_password_naming_the_account_is_refused_too() {
        let (_dir, db) = store();
        assert_eq!(
            db.create_first_admin("operator", "operator-is-my-password", NOW)
                .expect("ask"),
            FirstAdmin::WeakPassword(WeakPasswordReason::NamesTheAccount)
        );
        assert_eq!(db.count_users().expect("count"), 0);
    }
}
