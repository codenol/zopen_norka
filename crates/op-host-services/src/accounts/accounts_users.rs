//! The account rows themselves: creating one, finding one, listing them, and
//! changing what a row holds.
//!
//! Everything the store decides about an account is decided here. What it does
//! NOT decide is what an account may then do: `status` and `roles` are written
//! as given and read back as stored, because the layer holding a request is the
//! only one that knows what was asked for. A store that answered "may this
//! account open that document" would be a store with an authorization policy
//! inside it, and the policy would have to be found here by everyone looking
//! for it.

use rusqlite::{params, Connection, OptionalExtension};

use super::accounts_error::AccountsError;
use super::accounts_model::{
    checked_display_name, checked_email, checked_id, checked_username, encode_roles, user_from_row,
    NewUser, User, UserStatus, USER_COLUMNS,
};
use super::accounts_password::{hash_password, HASH_ALGO_ARGON2ID};
use super::AccountsDb;

/// The most accounts one page may hold.
///
/// A ceiling rather than a default: a caller that asks for `usize::MAX` is
/// asking this process to allocate every row it has ever stored, and the answer
/// to that is a bounded page plus the count ([`AccountsDb::count_users`]), not
/// an allocation the daemon cannot take back.
pub const MAX_USER_PAGE: usize = 1000;

impl AccountsDb {
    /// Create an account.
    ///
    /// The status follows from whether a password was given — see [`NewUser`] —
    /// so an `active` account always has something to sign in with and an
    /// `invited` one never claims to.
    pub fn create_user(&self, new: &NewUser<'_>, now: i64) -> Result<User, AccountsError> {
        let id = checked_id(new.id)?;
        let username = checked_username(new.username)?;
        let display_name = checked_display_name(new.display_name)?;
        let email = checked_email(new.email)?;
        let roles = encode_roles(new.roles)?;
        let status = new.status();
        // Hashed BEFORE the connection is taken. Argon2id deliberately costs
        // tens of milliseconds and ~19 MiB; holding the store's single write
        // lock across it would put every other account operation — including
        // every session resolve — behind one account's creation.
        let password_hash = match new.password {
            Some(password) => Some(hash_password(password)?),
            None => None,
        };

        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        if let Err(error) = tx.execute(
            "INSERT INTO users
                 (id, username, display_name, email, email_verified_at, password_hash,
                  hash_algo, roles, status, created_at, updated_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?9, NULL)",
            params![
                id,
                username,
                display_name,
                email,
                password_hash,
                HASH_ALGO_ARGON2ID,
                roles,
                status.as_str(),
                now,
            ],
        ) {
            return Err(insert_conflict(&tx, error, &username, email.as_deref()));
        }
        // Read back rather than assembled: the row is what the database holds —
        // trimmed, defaulted, and with the columns this call did not name — and
        // a value built in Rust would be a second opinion about it.
        let user = tx.query_row(
            &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"),
            params![id],
            user_from_row,
        )?;
        tx.commit()?;
        Ok(user)
    }

    /// The account with this id, when one exists.
    ///
    /// The id is not trimmed on the way in: it is opaque and caller-chosen, and
    /// a deployment whose ids were allowed to carry a space would otherwise
    /// have two spellings of one account.
    pub fn find_user_by_id(&self, id: &str) -> Result<Option<User>, AccountsError> {
        find_user(&self.conn(), "id = ?1", params![id])
    }

    /// The account with this sign-in name, whatever case it was typed in.
    ///
    /// Trimmed on the way in because the write path trims: without that, a name
    /// typed with a stray space would be stored as one account and looked up as
    /// none, and the person who typed the space could never sign in again.
    /// `COLLATE NOCASE` on the column is what makes the comparison
    /// case-insensitive — not a lowercase copy of the key, which would be a
    /// second spelling of every name for some future writer to disagree with.
    pub fn find_user_by_username(&self, username: &str) -> Result<Option<User>, AccountsError> {
        find_user(&self.conn(), "username = ?1", params![username.trim()])
    }

    /// The account holding this address, whatever case it was typed in.
    ///
    /// An address has no account more often than not: an account with no
    /// address at all matches nothing, and NULL is never equal to anything in
    /// SQL — which is the fail-closed direction for a lookup that is about to
    /// decide whether a reset link may be sent.
    pub fn find_user_by_email(&self, email: &str) -> Result<Option<User>, AccountsError> {
        find_user(&self.conn(), "email = ?1", params![email.trim()])
    }

    /// One page of accounts, newest first.
    ///
    /// The order is `created_at` with the id as a tiebreak, so it is total:
    /// without the second key, two accounts created in the same second could
    /// come back in either order, and a caller paging through them would see
    /// one twice and never see the other.
    pub fn list_users(&self, limit: usize, offset: usize) -> Result<Vec<User>, AccountsError> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            "SELECT {USER_COLUMNS} FROM users
             ORDER BY created_at DESC, id DESC LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = statement
            .query_map(
                params![limit.min(MAX_USER_PAGE) as i64, offset as i64],
                user_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// How many accounts exist.
    pub fn count_users(&self) -> Result<u64, AccountsError> {
        let count: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Replace an account's password.
    ///
    /// Does not touch `status`. An invited person setting their first password
    /// is a transition the acceptance flow owns, and it knows things this call
    /// does not — that the invite was valid, that the address was proved. This
    /// writes the credential and nothing else.
    ///
    /// Also rewrites `hash_algo`: the column names how the NEW string was
    /// produced, and after this call the old name would be a lie about the row.
    pub fn set_password(
        &self,
        user_id: &str,
        password: &str,
        now: i64,
    ) -> Result<(), AccountsError> {
        // Hashed before the lock is taken, for the reason `create_user` states.
        let hash = hash_password(password)?;
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE users SET password_hash = ?2, hash_algo = ?3, updated_at = ?4 WHERE id = ?1",
            params![user_id, hash, HASH_ALGO_ARGON2ID, now],
        )?;
        require_row(changed, user_id)
    }

    /// Set an account's status.
    ///
    /// The status is written as given rather than validated against the one it
    /// replaces: `invited` -> `active` is the acceptance flow, `active` ->
    /// `disabled` is an operator, `*` -> `orphan` is a deployment that lost an
    /// identity provider. Enumerating the legal transitions here would be a
    /// second copy of those flows, and the copy would be the one that is wrong.
    pub fn set_status(
        &self,
        user_id: &str,
        status: UserStatus,
        now: i64,
    ) -> Result<(), AccountsError> {
        let changed = self.conn().execute(
            "UPDATE users SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![user_id, status.as_str(), now],
        )?;
        require_row(changed, user_id)
    }

    /// Replace an account's roles.
    ///
    /// Whole-list replacement, not add/remove: a role set is small enough to
    /// state, and "grant these" plus "revoke those" is two operations that can
    /// interleave into a state neither caller asked for.
    pub fn set_roles(&self, user_id: &str, roles: &[&str], now: i64) -> Result<(), AccountsError> {
        let encoded = encode_roles(roles)?;
        let changed = self.conn().execute(
            "UPDATE users SET roles = ?2, updated_at = ?3 WHERE id = ?1",
            params![user_id, encoded, now],
        )?;
        require_row(changed, user_id)
    }

    /// Record that the account's address has been proved to belong to it.
    ///
    /// Refused for an account with no address: there is nothing that could have
    /// been proved, and the row would say otherwise — a verified address that
    /// does not exist, which an operator reading the table has no way to
    /// distinguish from a stale one.
    pub fn mark_email_verified(&self, user_id: &str, now: i64) -> Result<(), AccountsError> {
        let conn = self.conn();
        let has_email: Option<bool> = conn
            .query_row(
                "SELECT email IS NOT NULL FROM users WHERE id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .optional()?;
        match has_email {
            None => Err(AccountsError::NoSuchUser {
                id: user_id.to_string(),
            }),
            Some(false) => Err(AccountsError::NoEmail {
                id: user_id.to_string(),
            }),
            Some(true) => {
                conn.execute(
                    "UPDATE users SET email_verified_at = ?2, updated_at = ?2 WHERE id = ?1",
                    params![user_id, now],
                )?;
                Ok(())
            }
        }
    }

    /// Record that the account has just been seen.
    ///
    /// Deliberately does NOT touch `updated_at`: a sign-in is not a change to
    /// the account. Treating it as one would make `updated_at` mean "last
    /// touched by anything", and then it could no longer answer the question it
    /// exists for — when the record itself last changed.
    pub fn touch_last_seen(&self, user_id: &str, now: i64) -> Result<(), AccountsError> {
        let changed = self.conn().execute(
            "UPDATE users SET last_seen_at = ?2 WHERE id = ?1",
            params![user_id, now],
        )?;
        require_row(changed, user_id)
    }

    /// Delete an account, and with it everything keyed to it. `false` when
    /// there was no such account.
    ///
    /// What goes is what the schema cascades — sessions, one-time tokens — and
    /// what does not is what was written as a snapshot or a record: a comment
    /// keeps its author's name (it is a fact about the conversation), and an
    /// invite keeps its acceptance (`accepted_by` is nulled, `accepted_at` is
    /// not, so a spent link stays spent).
    pub fn delete_user(&self, user_id: &str) -> Result<bool, AccountsError> {
        let removed = self
            .conn()
            .execute("DELETE FROM users WHERE id = ?1", params![user_id])?;
        Ok(removed > 0)
    }
}

/// One account, found by whatever the caller matched on.
fn find_user<P: rusqlite::Params>(
    conn: &Connection,
    predicate: &str,
    params: P,
) -> Result<Option<User>, AccountsError> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE {predicate}"),
        params,
        user_from_row,
    )
    .optional()
    .map_err(AccountsError::from)
}

/// Report an update that matched no row as "no such account".
///
/// SQLite counts a row the statement MATCHED, whether or not the values it
/// wrote differ, so zero here means the id named nothing — not that the write
/// was a no-op.
fn require_row(changed: usize, user_id: &str) -> Result<(), AccountsError> {
    if changed == 0 {
        return Err(AccountsError::NoSuchUser {
            id: user_id.to_string(),
        });
    }
    Ok(())
}

/// Turn a refused insert into the reason it was refused.
///
/// The name and the address are unique, so a constraint violation is almost
/// always one of them — but only "almost": a duplicate id violates the primary
/// key, and so does a hand-written row. Which one it was is settled by ASKING
/// THE DATABASE which row already exists, rather than by matching on SQLite's
/// message text, which is an English sentence this build does not own and
/// which says `users.username` today.
///
/// The question is asked inside the caller's transaction, so what it sees is
/// the state the insert was refused against.
fn insert_conflict(
    conn: &Connection,
    error: rusqlite::Error,
    username: &str,
    email: Option<&str>,
) -> AccountsError {
    let constraint = matches!(
        &error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::ConstraintViolation
    );
    if !constraint {
        return error.into();
    }
    if let Ok(Some(_)) = find_user(conn, "username = ?1", params![username]) {
        return AccountsError::UsernameTaken {
            username: username.to_string(),
        };
    }
    if let Some(email) = email {
        if let Ok(Some(_)) = find_user(conn, "email = ?1", params![email]) {
            return AccountsError::EmailTaken {
                email: email.to_string(),
            };
        }
    }
    // A violation that is neither of those — a duplicate id — keeps SQLite's
    // own description, which names the column it refused.
    error.into()
}

/// Report an insert that was refused because its account does not exist.
///
/// Sessions, one-time tokens and invites all point at `users (id)`, so the only
/// constraint their inserts can plausibly break is that foreign key and the
/// only useful answer is the same one: there is no such account. Shared rather
/// than copied into each of the three, so the three cannot drift into reporting
/// the same failure differently.
///
/// A collision on the row's own key — a token hash, a 256-bit value — is not
/// that, and keeps SQLite's description instead of being reported as something
/// it is not.
pub(super) fn refused_for_unknown_user(
    conn: &Connection,
    error: rusqlite::Error,
    user_id: &str,
) -> AccountsError {
    let constraint = matches!(
        &error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::ConstraintViolation
    );
    if constraint {
        let known: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM users WHERE id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);
        if known.is_none() {
            return AccountsError::NoSuchUser {
                id: user_id.to_string(),
            };
        }
    }
    error.into()
}
