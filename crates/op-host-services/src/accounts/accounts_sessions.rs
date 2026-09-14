//! Signed-in browsers: starting a session, resolving one, and taking it away.
//!
//! A session is the one credential this product issues to a browser, and the
//! whole of it is a row: a hash of the token, the account it belongs to, and
//! when it stops being accepted. Nothing about a session is carried in the
//! token itself — no signed claim, no embedded expiry. A token that described
//! its own session would be a token that keeps working after the session was
//! revoked, and revocation is the entire reason a stored session exists.

use rusqlite::{params, OptionalExtension};

use super::accounts_error::AccountsError;
use super::accounts_model::{
    session_from_row, IssuedSession, NewSession, Session, SESSION_COLUMNS_WITH_HASH,
};
use super::accounts_secret::{hash_token, issue_token, token_hash_eq};
use super::accounts_users::refused_for_unknown_user;
use super::AccountsDb;

impl AccountsDb {
    /// Start a session and hand back the token that opens it.
    ///
    /// The store does not decide whether the account MAY sign in — an invited
    /// account has no password, a disabled account is refused by policy — and
    /// it does not check here. What it refuses is a session for an account that
    /// does not exist, which the foreign key makes impossible rather than
    /// merely unlikely.
    ///
    /// The lifetime is the caller's: `ttl_secs` is added to `now` once, and the
    /// result is stored. A ttl that is not positive produces a session that is
    /// never live, which is a caller's mistake rather than a corruption — the
    /// row says exactly what it was asked to say.
    pub fn create_session(
        &self,
        new: &NewSession<'_>,
        now: i64,
    ) -> Result<IssuedSession, AccountsError> {
        let (token, hash) = issue_token()?;
        let session = Session {
            user_id: new.user_id.to_string(),
            created_at: now,
            expires_at: now.saturating_add(new.ttl_secs),
            last_seen_at: None,
            user_agent: new.user_agent.map(str::to_string),
            ip: new.ip.map(str::to_string),
        };
        let conn = self.conn();
        if let Err(error) = conn.execute(
            "INSERT INTO sessions
                 (token_hash, user_id, created_at, expires_at, last_seen_at, user_agent, ip)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
            params![
                &hash[..],
                session.user_id,
                session.created_at,
                session.expires_at,
                session.user_agent,
                session.ip,
            ],
        ) {
            return Err(refused_for_unknown_user(&conn, error, &session.user_id));
        }
        Ok(IssuedSession { token, session })
    }

    /// The session this token opens, when it opens one and is still live.
    ///
    /// `None` covers three cases a caller must treat the same way — no such
    /// token, an expired session, a session whose account was deleted (the
    /// cascade removed the row) — because the answer to all three is "this
    /// browser is not signed in". The expired row is left where it is: a read
    /// that deleted would turn every stale cookie into a write transaction
    /// against the store's single writer, and [`Self::purge_expired_sessions`]
    /// is what removes rows.
    ///
    /// What this does NOT answer is whether the account may act: a session of a
    /// DISABLED account resolves exactly like any other, because the status is
    /// a policy the layer holding the request applies. Answering it here would
    /// hide a policy in the store, and the store is where nobody looks for one.
    pub fn resolve_session(&self, token: &str, now: i64) -> Result<Option<Session>, AccountsError> {
        let presented = hash_token(token);
        let conn = self.conn();
        let found = conn
            .query_row(
                &format!("SELECT {SESSION_COLUMNS_WITH_HASH} FROM sessions WHERE token_hash = ?1"),
                params![&presented[..]],
                |row| {
                    let stored: Vec<u8> = row.get(0)?;
                    Ok((stored, session_from_row(row, 1)?))
                },
            )
            .optional()?;
        let Some((stored, session)) = found else {
            return Ok(None);
        };
        // The lookup was by primary key, so this is the row that was asked for.
        // The comparison is made anyway — constant-time, and the one place that
        // decides "these are the same token" — so the answer does not rest on
        // how SQLite compared two blobs.
        if !token_hash_eq(&stored, &presented) {
            return Ok(None);
        }
        if !session.is_live_at(now) {
            return Ok(None);
        }
        Ok(Some(session))
    }

    /// Record that a request just used this session. `false` when no such
    /// session exists.
    ///
    /// Not guarded on expiry: a caller that has already resolved the session
    /// does not need the guard, and one that has not would be marking a dead
    /// session as seen — which is a wasted write, not a wrong answer.
    pub fn touch_session(&self, token: &str, now: i64) -> Result<bool, AccountsError> {
        let hash = hash_token(token);
        let changed = self.conn().execute(
            "UPDATE sessions SET last_seen_at = ?2 WHERE token_hash = ?1",
            params![&hash[..], now],
        )?;
        Ok(changed > 0)
    }

    /// End one session. `false` when the token opens nothing.
    ///
    /// Deleting rather than marking: unlike a password reset, a session that
    /// was signed out is not a record anybody needs — the interesting event is
    /// the sign-in, and keeping dead sessions is how a session table grows
    /// without bound.
    pub fn revoke_session(&self, token: &str) -> Result<bool, AccountsError> {
        let hash = hash_token(token);
        let removed = self.conn().execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            params![&hash[..]],
        )?;
        Ok(removed > 0)
    }

    /// End every session of one account, and say how many there were.
    ///
    /// What "sign out everywhere" is, and what a password change must do: a
    /// credential that was changed because it leaked would otherwise leave the
    /// thief's session working, which is the one thing the change was for.
    pub fn revoke_user_sessions(&self, user_id: &str) -> Result<u64, AccountsError> {
        let removed = self
            .conn()
            .execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])?;
        Ok(removed as u64)
    }

    /// Delete every session that has expired, and say how many there were.
    ///
    /// A sweep rather than a rule, and the difference matters: expiry is
    /// enforced on every resolve, so a row this misses is never accepted. What
    /// the sweep prevents is a table nobody ever cleans.
    pub fn purge_expired_sessions(&self, now: i64) -> Result<u64, AccountsError> {
        let removed = self
            .conn()
            .execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
        Ok(removed as u64)
    }
}
