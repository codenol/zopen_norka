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
use super::accounts_secret::{hash_from_hex, hash_hex, hash_token, issue_token};
use super::accounts_users::refused_for_unknown_user;
use super::AccountsDb;

/// An invitation as an operator's LIST shows it: the row, plus the identity
/// that names it.
///
/// The row itself has no id to show — its primary key is the hash of the token
/// this store deliberately never keeps — so a list that returned plain
/// [`Invite`] values could describe every invitation in the deployment and
/// name none of them. [`Self::id`] is that name: the stored hash in hex, which
/// grants nothing (see [`hash_hex`]) and exists so an operator can act on the
/// row they are looking at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedInvite {
    /// The row's identity: lowercase hex of the stored token hash.
    pub id: String,
    /// The row.
    pub invite: Invite,
}

/// What happened to an attempt to withdraw an invitation.
///
/// Three outcomes rather than a `bool`, because the caller is an operator
/// looking at a list and the three mean different things to them: one is the
/// withdrawal they asked for, one says the link was already used and the
/// account it made is the thing to deal with now, and one says their list is
/// out of date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteWithdrawal {
    /// The row was there and is gone.
    Revoked,
    /// The link had already been accepted.
    ///
    /// Withdrawing cannot un-create the account it made, and deleting the row
    /// would erase the record of how that account came to exist — see
    /// [`AccountsDb::revoke_invite`].
    AlreadyAccepted,
    /// No invitation this id names.
    NotFound,
}

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
        Ok(IssuedInvite {
            token,
            id: hash_hex(&hash),
            invite,
        })
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

    /// The invitations this deployment has issued, newest first.
    ///
    /// ## Why this read exists at all
    ///
    /// Because the token is stored nowhere, an operator who has lost the link
    /// has no other way to learn what happened to it — and "I sent the link,
    /// the person says it does not work" is the ordinary case, not the
    /// exceptional one. The row answers it: when it was issued, which roles it
    /// carries, when it stops being accepted, and whether anybody used it.
    ///
    /// ## Why it is paged
    ///
    /// The table never shrinks on its own — a redeemed invite is a record, an
    /// expired one is not swept — so a deployment that has been inviting
    /// people for a year has a year of rows, and a route that returned all of
    /// them would be a route whose cost grows with the deployment's age. The
    /// order is `created_at DESC, token_hash` so that page two is a
    /// continuation of page one rather than a re-shuffle: two rows created in
    /// the same second still have one order, and it is the index's.
    ///
    /// Spent and expired rows are returned, never filtered: this is the read
    /// an operator makes when they want to know what happened, and a query
    /// that hid the answer would be answering a different question.
    pub fn list_invites(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ListedInvite>, AccountsError> {
        let conn = self.conn();
        let mut statement = conn.prepare(&format!(
            // `token_hash` is selected last so that `invite_from_row`, which
            // reads the `INVITE_COLUMNS` positions, keeps working unchanged.
            "SELECT {INVITE_COLUMNS}, token_hash FROM invites
             ORDER BY created_at DESC, token_hash LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = statement.query_map(params![limit as i64, offset as i64], |row| {
            let hash: Vec<u8> = row.get(7)?;
            Ok(ListedInvite {
                id: hash_hex(&hash),
                invite: invite_from_row(row)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Withdraw the invitation `id` names.
    ///
    /// [`Self::revoke_invite`] takes the token, which only the person holding
    /// the link has; this takes the id a listing shows, which is what an
    /// operator has when the thing they want to withdraw is a row on a screen
    /// and the link is somewhere in a chat log. The two reach the same row —
    /// the token path is this one under a hash — and share its rule: an
    /// accepted invitation is not withdrawable, because doing so cannot
    /// un-create the account it made.
    pub fn revoke_invite_by_id(&self, id: &str) -> Result<InviteWithdrawal, AccountsError> {
        // Not a row id at all: reported as "no such invitation" rather than as
        // a malformed request, because that is what it is from here — the
        // caller asked to withdraw something, and there is nothing by that
        // name.
        let Some(hash) = hash_from_hex(id) else {
            return Ok(InviteWithdrawal::NotFound);
        };
        let conn = self.conn();
        let removed = conn.execute(
            "DELETE FROM invites WHERE token_hash = ?1 AND accepted_at IS NULL",
            params![&hash[..]],
        )?;
        if removed > 0 {
            return Ok(InviteWithdrawal::Revoked);
        }
        // Nothing was removed, and the row says which of the two it was. Read
        // AFTER the delete so that the answer is about one statement's
        // outcome rather than about a state that may have moved on.
        let accepted: Option<Option<i64>> = conn
            .query_row(
                "SELECT accepted_at FROM invites WHERE token_hash = ?1",
                params![&hash[..]],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match accepted {
            None => InviteWithdrawal::NotFound,
            Some(None) => InviteWithdrawal::NotFound,
            // A row that is still here, is not accepted, and was not removed
            // is not a state this code can reach — the `DELETE` above matches
            // exactly those rows. Reported as the withdrawal that did not
            // happen rather than guessed at.
            Some(Some(_)) => InviteWithdrawal::AlreadyAccepted,
        })
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
        let id = hash_hex(&hash_token(token));
        Ok(self.revoke_invite_by_id(&id)? == InviteWithdrawal::Revoked)
    }
}
