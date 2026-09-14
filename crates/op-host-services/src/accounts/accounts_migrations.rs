//! The account schema's history: every step that ever changed it, in the order
//! those steps were taken.
//!
//! Here rather than in the spine because it is the one part of `accounts` that
//! only grows — each release that touches the schema appends to it, and never
//! shortens it — and because the rule that governs it (append, never edit) is
//! about this list rather than about the store that runs it. The runner is
//! `accounts::migrate`, which applies whatever is pending and records the
//! version in `meta`.
//!
//! The SQL strings are history: their text is what a field database recorded as
//! having run, so they are not edited, comments included. A statement that has
//! been overtaken by later work is corrected where the behaviour lives now, not
//! in the record of what the database was.

/// One step of the schema's history.
///
/// `pub(super)`: [`MIGRATIONS`] and this shape are read by the store that runs
/// them (and by its tests) and by nothing else in the crate. A step is not a
/// thing any other module has an opinion about.
pub(super) struct Migration {
    /// The version this step brings the database TO. Versions are consecutive
    /// and never reused, so a database at version N has exactly the migrations
    /// `1..=N` applied.
    pub(super) version: i64,
    /// The step itself, run as one `execute_batch`.
    pub(super) sql: &'static str,
}

/// Every step, in order.
///
/// Add one by appending — never by editing an earlier entry, because databases
/// in the field have already run it. A step that cannot be expressed as SQL (a
/// backfill that has to read the rows, say) belongs in its own function called
/// from `open`, guarded by its own `meta` key.
pub(super) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: "
    -- ## The account itself
    --
    -- One row per person a deployment knows about. `id` is the string every
    -- other table, and every document's `owner_id`, refers to; it is written by
    -- the caller because the identifier a deployment hands out is its own
    -- decision (a uuid, an email-shaped name, an opaque token), and a row whose
    -- id this store invented could not be reconciled with what the hub already
    -- calls that account.
    --
    -- `username` is what a person signs in with, so it is the one column that
    -- must be unique REGARDLESS of case: `Alice` and `alice` are one account
    -- with one password, and letting both exist would mean a login form whose
    -- result depends on how the person capitalised their own name. COLLATE
    -- NOCASE is set on the column rather than enforced by a lowercase copy of
    -- the name, so the value the person chose is what the database holds and
    -- the rule applies to any index or comparison built on it — including the
    -- UNIQUE index this declaration creates.
    --
    -- `email` is unique for the same reason and under the same collation, and
    -- is NULLABLE: an account created by an invite link has no address at all
    -- until somebody gives one. SQLite treats NULLs as distinct in a UNIQUE
    -- index, which is what makes a second such account possible; a NOT NULL
    -- column with '' for 'unknown' would have made the empty string the second
    -- account's collision.
    --
    -- `password_hash` NULL means the account exists and cannot be signed into
    -- yet — the state an invited person is in. The alternative (a random
    -- password) would be a credential nobody knows that nevertheless looks like
    -- one, and an operator reading the table could not tell the two apart.
    -- `hash_algo` is a name for how the string was produced. It is redundant
    -- while there is one algorithm — the PHC string names itself — but it is
    -- the column a second algorithm or a rehash pass reads to find the rows it
    -- still has to move, and it costs one word per account.
    --
    -- `roles` is a comma-separated list of role names, and it is empty when the
    -- account has none. A separate `user_roles` table would be the textbook
    -- shape, and it buys joins and per-role indexes that nothing in this
    -- product has asked for: roles here are a short tag list read together with
    -- the account, not a set anything queries by. When something does query by
    -- role, that is a migration to a table — with the CSV as its backfill.
    --
    -- `status` is present and interpreted by the reader, not by this schema:
    -- 'active' may sign in, 'disabled' may not, 'invited' has no password yet,
    -- 'orphan' is an account whose identity the deployment no longer holds.
    -- CHECK refuses a status this build did not name, so a typo in a hand
    -- repair is an error rather than an account whose state is undefined; the
    -- Rust side refuses one it cannot read for the case this CHECK cannot
    -- cover, an OLDER binary meeting a database a NEWER one wrote.
    CREATE TABLE users (
        id                TEXT PRIMARY KEY,
        username          TEXT NOT NULL COLLATE NOCASE UNIQUE,
        display_name      TEXT NOT NULL,
        email             TEXT COLLATE NOCASE UNIQUE,
        -- When the address was proved to belong to the account. NULL is 'not
        -- proved', which is not the same as 'no address' — that is `email`
        -- being NULL — and the two are checked separately on purpose.
        email_verified_at INTEGER,
        password_hash     TEXT,
        hash_algo         TEXT NOT NULL DEFAULT 'argon2id',
        roles             TEXT NOT NULL DEFAULT ''
                          CHECK (roles = ''
                              OR (roles NOT LIKE ',%'
                                  AND roles NOT LIKE '%,'
                                  AND roles NOT LIKE '%,,%')),
        status            TEXT NOT NULL DEFAULT 'active'
                          CHECK (status IN ('active', 'disabled', 'invited', 'orphan')),
        created_at        INTEGER NOT NULL,
        updated_at        INTEGER NOT NULL,
        -- Last time this account was seen, whatever it was doing. NULL until it
        -- has been seen once, so 'never' stays distinguishable from 'in 1970'.
        last_seen_at      INTEGER
    );

    -- ## A signed-in browser
    --
    -- `token_hash` is the PRIMARY KEY and it is SHA-256 of the value the client
    -- holds — never that value. A session table holding plaintext tokens is a
    -- file from which anybody who reads it can sign in as anybody, and the
    -- daemon already treats a stored credential as public (the hub verdict
    -- cache is keyed the same way). Hashing on the way in means a leaked
    -- database is a list of sessions that cannot be used, and the lookup is
    -- still a single index probe because the hash is what is stored, not a
    -- column to search.
    --
    -- The 32-byte check is the schema stating what the column holds: a
    -- truncated write, a hand import, or a future algorithm's 16-byte digest
    -- would otherwise land silently as a row no login can ever match.
    CREATE TABLE sessions (
        token_hash   BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
        -- The session goes when the account goes: a deleted account's browser
        -- must not keep working, and a row pointing at a user id that no longer
        -- exists is not a session anybody can act on. This is why
        -- `foreign_keys=ON` is set for the life of the connection rather than
        -- merely available.
        user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
        created_at   INTEGER NOT NULL,
        -- Absolute, not a duration: an expiry that is computed on each read
        -- drifts with every retry and cannot be inspected. Both reads and the
        -- sweep compare against this one number.
        expires_at   INTEGER NOT NULL,
        -- Touched by a request that used the session, which is what tells an
        -- operator whether an idle browser is still out there.
        last_seen_at INTEGER,
        -- Kept for the account holder's own 'where am I signed in' list: a
        -- session a person does not recognise is the signal that a credential
        -- leaked. Not parsed here — stored as the client sent it.
        user_agent   TEXT,
        ip           TEXT
    );

    -- A deleted account's sessions are found BY the cascade through this index:
    -- SQLite does not index a foreign key on its own, so without it every
    -- account deletion is a full scan of every session in the deployment.
    CREATE INDEX sessions_by_user ON sessions (user_id);
    -- The expiry sweep reads in this order; without the index it is a scan of
    -- the whole table on a timer.
    CREATE INDEX sessions_by_expiry ON sessions (expires_at);

    -- ## Links that expire after one use
    --
    -- Password reset and address verification are the same mechanism with a
    -- different sentence attached: a secret is handed out once, hashed on the
    -- way in, and consumed on the way back. One table rather than two, because
    -- the rules (one use, an absolute expiry, tied to an account) are identical
    -- and a second table would be a second place for them to drift.
    --
    -- `used_at` rather than a DELETE on redemption: a reset that was performed
    -- is a security event worth keeping — 'this account's password was changed
    -- at 12:04 from this token' — and a row that disappears leaves nothing to
    -- explain a password change nobody admits to.
    CREATE TABLE one_time_tokens (
        -- A rowid: this id reaches no path and no client, it only has to be
        -- unique. AUTOINCREMENT so ids are never reused — a log line naming
        -- token 7 keeps naming the same event after the row is swept.
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
        purpose    TEXT NOT NULL
                   CHECK (purpose IN ('password_reset', 'email_verify')),
        token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        used_at    INTEGER
    );

    -- 'the live token of this kind for this account' is the question the
    -- issuing path asks before it writes a new one, and what finds a row to
    -- sweep.
    CREATE INDEX one_time_tokens_by_user ON one_time_tokens (user_id, purpose);
    CREATE INDEX one_time_tokens_by_expiry ON one_time_tokens (expires_at);

    -- ## An invitation, as a link
    --
    -- This product sends no mail: an invite is a link the operator hands over
    -- however they like. The token is the whole of the invitation, so it is the
    -- primary key here too, and — as with sessions — only its SHA-256 is
    -- stored.
    --
    -- `email` is nullable and is not what the invite is keyed by: an operator
    -- who knows the address records it so the acceptance can be matched to the
    -- account it was meant for, and an operator who does not can still issue a
    -- link. A NOT NULL column would have forced a placeholder that every reader
    -- would have to know means 'unknown'.
    CREATE TABLE invites (
        token_hash  BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
        email       TEXT COLLATE NOCASE,
        roles       TEXT NOT NULL DEFAULT ''
                    CHECK (roles = ''
                        OR (roles NOT LIKE ',%'
                            AND roles NOT LIKE '%,'
                            AND roles NOT LIKE '%,,%')),
        -- Who handed the link out. SET NULL rather than CASCADE: the invite is
        -- a record of a decision that was made, and it must survive the
        -- departure of the person who made it — deleting the inviter cannot be
        -- allowed to invalidate a link that is already in somebody's hands, nor
        -- to erase the fact that it was issued.
        created_by  TEXT REFERENCES users (id) ON DELETE SET NULL,
        created_at  INTEGER NOT NULL,
        expires_at  INTEGER NOT NULL,
        -- Who used it. SET NULL for a different reason than `created_by`: here
        -- the inviter's departure is not in question, the ACCEPTING account is.
        -- The pair (accepted_at, accepted_by) therefore has no CHECK tying its
        -- halves together: a spent invite whose account was later deleted keeps
        -- accepted_at and loses accepted_by, and that is the honest record —
        -- the link was used, by somebody whose account is gone. What decides
        -- whether an invite may still be redeemed is `accepted_at`, never the
        -- id.
        accepted_by TEXT REFERENCES users (id) ON DELETE SET NULL,
        accepted_at INTEGER
    );

    -- 'is there an invite for this address' — the operator's list, and the
    -- acceptance path's sanity check that the link was meant for this person.
    CREATE INDEX invites_by_email ON invites (email);
    CREATE INDEX invites_by_expiry ON invites (expires_at);
",
}];
