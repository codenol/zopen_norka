//! One-time links: the reset and address-verification tokens, and the rule that
//! makes them one-time.
//!
//! ## Why consuming is one statement
//!
//! [`AccountsDb::consume_one_time_token`] is a single `UPDATE ... WHERE
//! used_at IS NULL AND expires_at > now`, and its row count is the answer. A
//! read-then-write pair — check the row, then mark it — has a window between
//! the two in which a second request sees the same unused row, and two password
//! resets are performed with one link. SQLite evaluates the guard and the write
//! together, so the second request's update matches nothing.
//!
//! ## Why one answer for three reasons
//!
//! A caller of `consume` learns only that the token was not consumable —
//! never whether it does not exist, had expired, or had already been used.
//! Accounting for the difference here would hand anybody holding a guessed
//! token a way to learn whether it is real. The invite path, which a person
//! reaches by clicking a link and where the token's 256 bits make guessing
//! pointless, does say which (see [`super::accounts_invites`]).

use rusqlite::{params, OptionalExtension};

use super::accounts_error::AccountsError;
use super::accounts_model::{
    one_time_token_from_row, IssuedToken, OneTimePurpose, OneTimeToken, ONE_TIME_TOKEN_COLUMNS,
};
use super::accounts_secret::{hash_token, issue_token};
use super::accounts_users::refused_for_unknown_user;
use super::AccountsDb;

impl AccountsDb {
    /// Hand out a single-use link for `user_id`.
    ///
    /// Issuing RETIRES every other live token of the same purpose for the same
    /// account, in the same transaction. Two live reset links would mean the
    /// older one — the one sitting in an inbox, possibly the one an attacker
    /// asked for — still resets the password after the owner requested a new
    /// one. Superseded tokens are marked used rather than deleted, so the table
    /// still shows that a link was issued and retired.
    pub fn issue_one_time_token(
        &self,
        user_id: &str,
        purpose: OneTimePurpose,
        ttl_secs: i64,
        now: i64,
    ) -> Result<IssuedToken, AccountsError> {
        let (token, hash) = issue_token()?;
        let expires_at = now.saturating_add(ttl_secs);
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE one_time_tokens SET used_at = ?3
             WHERE user_id = ?1 AND purpose = ?2 AND used_at IS NULL",
            params![user_id, purpose.as_str(), now],
        )?;
        if let Err(error) = tx.execute(
            "INSERT INTO one_time_tokens
                 (user_id, purpose, token_hash, created_at, expires_at, used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![user_id, purpose.as_str(), &hash[..], now, expires_at],
        ) {
            return Err(refused_for_unknown_user(&tx, error, user_id));
        }
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(IssuedToken {
            token,
            id,
            expires_at,
        })
    }

    /// The link this token opens, when it opens one and is still usable.
    ///
    /// For a page that has to show something before the token is spent — the
    /// reset form itself, which must not consume the link just by being opened.
    /// Nothing about the lookup is constant-time-sensitive: the row is matched
    /// by primary key, and the caller already holds the token it matched.
    pub fn find_valid_one_time_token(
        &self,
        token: &str,
        purpose: OneTimePurpose,
        now: i64,
    ) -> Result<Option<OneTimeToken>, AccountsError> {
        let hash = hash_token(token);
        let conn = self.conn();
        let found = conn
            .query_row(
                &format!(
                    "SELECT {ONE_TIME_TOKEN_COLUMNS} FROM one_time_tokens
                     WHERE token_hash = ?1 AND purpose = ?2
                       AND used_at IS NULL AND expires_at > ?3"
                ),
                params![&hash[..], purpose.as_str(), now],
                one_time_token_from_row,
            )
            .optional()?;
        Ok(found)
    }

    /// Spend a link, and say what it was for. `None` when it cannot be spent.
    ///
    /// The guard and the write are one statement on purpose — see this module's
    /// header — so a second attempt at the same token, however close behind the
    /// first, finds nothing to claim.
    pub fn consume_one_time_token(
        &self,
        token: &str,
        purpose: OneTimePurpose,
        now: i64,
    ) -> Result<Option<OneTimeToken>, AccountsError> {
        let hash = hash_token(token);
        let conn = self.conn();
        let claimed = conn.execute(
            "UPDATE one_time_tokens SET used_at = ?3
             WHERE token_hash = ?1 AND purpose = ?2
               AND used_at IS NULL AND expires_at > ?3",
            params![&hash[..], purpose.as_str(), now],
        )?;
        if claimed == 0 {
            return Ok(None);
        }
        // Read back the row that was just claimed: the caller needs to know
        // WHICH account the link belonged to, and the update it just made is
        // the only thing that changed.
        let row = conn.query_row(
            &format!("SELECT {ONE_TIME_TOKEN_COLUMNS} FROM one_time_tokens WHERE token_hash = ?1"),
            params![&hash[..]],
            one_time_token_from_row,
        )?;
        Ok(Some(row))
    }

    /// Delete every one-time token whose window has closed, and say how many
    /// there were.
    ///
    /// Rows that were USED are kept until then: `used_at` is the record that a
    /// password was changed or an address proved, and a token that was spent
    /// this morning is still the explanation for a password change this
    /// morning. Once its window has closed the row no longer explains anything
    /// a live token could do, and the sweep is what keeps the table from
    /// growing one row per reset forever.
    pub fn purge_expired_one_time_tokens(&self, now: i64) -> Result<u64, AccountsError> {
        let removed = self.conn().execute(
            "DELETE FROM one_time_tokens WHERE expires_at <= ?1",
            params![now],
        )?;
        Ok(removed as u64)
    }
}
