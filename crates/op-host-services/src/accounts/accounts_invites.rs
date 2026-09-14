//! Invitations, as links.
//!
//! This product sends no mail: an invite is a token the operator hands over
//! however they like — a chat message, a printed sheet, a URL in a ticket. The
//! link IS the invitation, which is why the row holds nothing but the hash of
//! that token, the roles the accepting account is to be granted, and who issued
//! it.
//!
//! ## Why this path says WHICH kind of no
//!
//! [`AccountsDb::redeem_invite`] distinguishes "no such invite", "expired" and
//! "already accepted", where the one-time-token path deliberately does not. The
//! difference is where the person is standing: a one-time token is presented by
//! a client that already knows it holds one, and telling it which way it failed
//! is telling it something about the store. An invite is clicked by a person
//! who has just been told they were invited, and "this invite has already been
//! used" is the only answer they can act on. The token is 256 random bits
//! either way, so neither answer helps anybody guess one.

use rusqlite::{params, OptionalExtension};

use super::accounts_error::AccountsError;
use super::accounts_model::{
    checked_email, encode_roles, invite_from_row, Invite, IssuedInvite, NewInvite, INVITE_COLUMNS,
};
use super::accounts_secret::{hash_token, issue_token};
use super::accounts_users::refused_for_unknown_user;
use super::AccountsDb;

impl AccountsDb {
    /// Issue an invitation and hand back the link's token.
    ///
    /// The token is returned once and stored nowhere: this call is the only
    /// moment the value exists on this side, which is what makes an invite
    /// table that leaked a list of invitations nobody can accept.
    pub fn create_invite(
        &self,
        new: &NewInvite<'_>,
        now: i64,
    ) -> Result<IssuedInvite, AccountsError> {
        let email = checked_email(new.email)?;
        let roles = encode_roles(new.roles)?;
        let (token, hash) = issue_token()?;
        let expires_at = now.saturating_add(new.ttl_secs);
        let conn = self.conn();
        if let Err(error) = conn.execute(
            "INSERT INTO invites
                 (token_hash, email, roles, created_by, created_at, expires_at,
                  accepted_by, accepted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)",
            params![&hash[..], email, roles, new.created_by, now, expires_at],
        ) {
            return Err(match new.created_by {
                Some(inviter) => refused_for_unknown_user(&conn, error, inviter),
                // No inviter named, so the foreign key was NULL and cannot have
                // been what failed. Nothing to translate it into.
                None => error.into(),
            });
        }
        let invite = conn.query_row(
            &format!("SELECT {INVITE_COLUMNS} FROM invites WHERE token_hash = ?1"),
            params![&hash[..]],
            invite_from_row,
        )?;
        Ok(IssuedInvite { token, invite })
    }

    /// The invite this token names, spent or not.
    ///
    /// Spent and expired invites are returned rather than filtered out: this is
    /// the read an operator makes when they want to know what happened to a
    /// link, and a query that answered "no such invite" for one that was used
    /// last week would be a query that hides the answer.
    pub fn find_invite(&self, token: &str) -> Result<Option<Invite>, AccountsError> {
        let hash = hash_token(token);
        let conn = self.conn();
        let found = conn
            .query_row(
                &format!("SELECT {INVITE_COLUMNS} FROM invites WHERE token_hash = ?1"),
                params![&hash[..]],
                invite_from_row,
            )
            .optional()?;
        Ok(found)
    }

    /// Spend an invite on behalf of `accepted_by`.
    ///
    /// Marks the invitation used and nothing else: creating the account, giving
    /// it the roles the invite carries and setting its password is the
    /// acceptance flow's job, and it is a sequence of steps this store has no
    /// way to order. What this guarantees is the part that must not happen
    /// twice — one link, one account — and it guarantees it with a single
    /// guarded `UPDATE`, so two requests arriving together cannot both find the
    /// invite unused.
    pub fn redeem_invite(
        &self,
        token: &str,
        accepted_by: &str,
        now: i64,
    ) -> Result<Invite, AccountsError> {
        let hash = hash_token(token);
        let conn = self.conn();
        let claimed = match conn.execute(
            "UPDATE invites SET accepted_by = ?2, accepted_at = ?3
             WHERE token_hash = ?1 AND accepted_at IS NULL AND expires_at > ?3",
            params![&hash[..], accepted_by, now],
        ) {
            Ok(claimed) => claimed,
            // The only constraint this update can break is the foreign key to
            // the account doing the accepting.
            Err(error) => return Err(refused_for_unknown_user(&conn, error, accepted_by)),
        };
        if claimed == 0 {
            // Nothing was claimed, and the row says why. `accepted_at` is read
            // rather than `accepted_by` because deleting the accepting account
            // nulls the id and leaves the timestamp — see the schema.
            let existing = conn
                .query_row(
                    &format!("SELECT {INVITE_COLUMNS} FROM invites WHERE token_hash = ?1"),
                    params![&hash[..]],
                    invite_from_row,
                )
                .optional()?;
            return match existing {
                None => Err(AccountsError::InviteNotFound),
                Some(invite) if invite.accepted_at.is_some() => {
                    Err(AccountsError::InviteAlreadyAccepted)
                }
                Some(_) => Err(AccountsError::InviteExpired),
            };
        }
        let invite = conn.query_row(
            &format!("SELECT {INVITE_COLUMNS} FROM invites WHERE token_hash = ?1"),
            params![&hash[..]],
            invite_from_row,
        )?;
        Ok(invite)
    }

    /// Withdraw an invitation that has not been accepted. `false` when there is
    /// no such invite to withdraw.
    ///
    /// Deleting, not marking: an invite that was withdrawn is one nobody was
    /// meant to have, and unlike a redeemed token there is no event to keep —
    /// where a spent invite records that an account was created, a withdrawn
    /// one records that nothing happened.
    ///
    /// An accepted invite is left alone. Withdrawing cannot un-create the
    /// account it made, and removing the row would erase the record of how that
    /// account came to exist.
    pub fn revoke_invite(&self, token: &str) -> Result<bool, AccountsError> {
        let hash = hash_token(token);
        let removed = self.conn().execute(
            "DELETE FROM invites WHERE token_hash = ?1 AND accepted_at IS NULL",
            params![&hash[..]],
        )?;
        Ok(removed > 0)
    }
}
