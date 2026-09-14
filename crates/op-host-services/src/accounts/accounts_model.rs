//! The rows of the account store, as Rust types: the four tables' shapes, the
//! values they may hold, and the decoders that turn a row into a value.
//!
//! Reading is where a store earns the right to be trusted. Every decoder here
//! refuses a value it does not understand rather than defaulting it: a
//! `status` of `frozen` is not `active` with a typo, and an account whose state
//! cannot be read is not an account to guess about. That failure mode cannot be
//! produced by this build — the migration's CHECK refuses it — but it is
//! exactly what an older binary meeting a database a newer one wrote looks
//! like, and the older binary is the one that must fail closed.

use rusqlite::types::Type;
use rusqlite::Row;

use super::accounts_error::AccountsError;

/// The columns a [`User`] is built from, in one place so every reader agrees
/// about the order.
pub(super) const USER_COLUMNS: &str = "id, username, display_name, email, email_verified_at, \
     password_hash, hash_algo, roles, status, created_at, updated_at, last_seen_at";

/// The columns a [`Session`] is read from, with the token hash in front.
///
/// The hash comes first because the one reader of this table — resolving a
/// token — has to see the hash it matched, so that the row handed back is
/// checked against the value that was asked for rather than assumed to be it.
/// It is never part of a [`Session`]: a value nothing outside this module may
/// see does not belong in a struct that leaves it.
pub(super) const SESSION_COLUMNS_WITH_HASH: &str =
    "token_hash, user_id, created_at, expires_at, last_seen_at, user_agent, ip";

/// The columns an [`Invite`] is built from.
pub(super) const INVITE_COLUMNS: &str =
    "email, roles, created_by, created_at, expires_at, accepted_by, accepted_at";

/// The columns a [`OneTimeToken`] is built from.
pub(super) const ONE_TIME_TOKEN_COLUMNS: &str =
    "id, user_id, purpose, created_at, expires_at, used_at";

/// Where `status` sits in [`USER_COLUMNS`].
///
/// Named rather than written as a literal at the one call site that needs it: a
/// refused value is reported against its column index, and `8` in that call
/// would be a number no reader could check against the list above.
const USER_STATUS: usize = 8;

/// Where `purpose` sits in [`ONE_TIME_TOKEN_COLUMNS`]. Named for the same
/// reason as [`USER_STATUS`].
const ONE_TIME_TOKEN_PURPOSE: usize = 2;

/// Longest a stored username may be. A name, not a document: past a few dozen
/// characters a "username" is somebody probing what the column will hold, and a
/// UNIQUE index over unbounded text is a lookup nobody can predict the cost of.
const MAX_USERNAME: usize = 64;
/// Longest a display name may be.
const MAX_DISPLAY_NAME: usize = 128;
/// Longest an email address may be. 320 is the RFC 5321 upper bound — local
/// part and domain together — and it is the number the rest of the world uses
/// for the same column.
const MAX_EMAIL: usize = 320;
/// Longest an account id may be. Opaque and caller-chosen, but bounded: it is
/// the key of a UNIQUE index and part of the tenant directory naming.
const MAX_ID: usize = 128;
/// Longest a single role name may be.
const MAX_ROLE: usize = 64;
/// Longest the whole encoded role list may be. A tag list on an account, not a
/// group membership system.
const MAX_ROLES: usize = 512;

/// What an account is allowed to do, as far as the store is concerned.
///
/// The store RECORDS this and does not enforce it: whether a disabled account
/// may open a document is a question for the layer holding the request. What
/// the store does enforce is that the column holds one of these — a status it
/// cannot name is refused on write by the schema and on read by [`Self::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    /// May sign in.
    Active,
    /// Exists, may not sign in. Not deleted: the account still owns its
    /// documents and comments.
    Disabled,
    /// Created by an invite and has no password yet.
    Invited,
    /// The deployment no longer holds the identity behind this account. Kept
    /// for the same reason as `Disabled`, and distinct from it because the two
    /// call for different repairs.
    Orphan,
}

impl UserStatus {
    /// The value stored in `users.status`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::Invited => "invited",
            Self::Orphan => "orphan",
        }
    }

    /// The status a stored value names, or the error that says it names none.
    pub fn parse(value: &str) -> Result<Self, AccountsError> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            "invited" => Ok(Self::Invited),
            "orphan" => Ok(Self::Orphan),
            other => Err(AccountsError::UnknownStatus {
                status: other.to_string(),
            }),
        }
    }

    /// Whether an account in this state may sign in.
    ///
    /// A method on the status rather than a match in the sign-in path, because
    /// the answer is a property of the state: `disabled` means "may not sign
    /// in" wherever it is read, and a second copy of that list would be the one
    /// that forgot `orphan`. What the status does NOT decide — what a signed-in
    /// account may then do — stays with the request, not here.
    pub const fn may_sign_in(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// An account, as the database holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The id every other table and every document's `owner_id` uses.
    pub id: String,
    /// The sign-in name, unique regardless of case.
    pub username: String,
    /// The name to show a person who is not reading the database.
    pub display_name: String,
    /// `None` when no address is known.
    pub email: Option<String>,
    /// When the address was proved. `None` is 'not proved', which is not the
    /// same as having no address.
    pub email_verified_at: Option<i64>,
    /// The Argon2id PHC string, or `None` for an account that cannot be signed
    /// into yet.
    ///
    /// Handed to a caller because a caller is the only thing that can VERIFY a
    /// password: this store never sees one being checked against a login form,
    /// and inventing an `authenticate` that decided account status as well
    /// would put a policy the next step owns inside the schema layer.
    pub password_hash: Option<String>,
    /// How `password_hash` was produced. `argon2id` today.
    pub hash_algo: String,
    /// Role names, in the order they were written, without duplicates.
    pub roles: Vec<String>,
    /// What the account may do, as far as the store records it.
    pub status: UserStatus,
    /// When the row was created.
    pub created_at: i64,
    /// When the row last changed.
    pub updated_at: i64,
    /// When the account was last seen, if it ever has been.
    pub last_seen_at: Option<i64>,
}

impl User {
    /// Whether this account can be signed into at all.
    ///
    /// False for an invited account: `password_hash` is NULL, and a verifier
    /// handed a NULL hash has nothing to check against. Answering that here
    /// keeps the "no password" test in one place instead of in every caller
    /// that has to unwrap the option.
    pub fn has_password(&self) -> bool {
        self.password_hash.is_some()
    }

    /// Whether the account carries `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|held| held == role)
    }
}

/// An id for an account this deployment invents.
///
/// ## Why the store mints it and not the caller
///
/// An account id is the one value every other table and every document's
/// `owner_id` refers to, so the rule about it has to live in one place or it
/// lives nowhere: a route that built an id from the username and a CLI that
/// built one from a counter would produce two accounts for one person the first
/// time somebody used both. Here, "no id given" means one thing.
///
/// ## Why random rather than derived
///
/// Derived ids collide: two deployments both naming their first account
/// `admin` is not a mistake, it is Tuesday, and a rename would move every
/// reference. 128 random bits do not collide, and they say nothing about the
/// person — an id ends up in log lines, in a tenant directory's name and in a
/// bug report, and `alice@example.com` printed in all three is a privacy leak
/// that a random string is not.
///
/// NOT a secret: it is an identifier, not a credential, so it is hex rather
/// than a token, and it is allowed to be read out of the database. The `u_`
/// prefix only makes a locally minted id recognisable in a log line beside an
/// id that came from somewhere else.
pub(super) fn new_account_id() -> Result<String, AccountsError> {
    let bytes = super::accounts_secret::random_bytes::<16>()?;
    let mut id = String::with_capacity(2 + bytes.len() * 2);
    id.push_str("u_");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    Ok(id)
}

/// An account about to be created.
///
/// The status is not a field. It follows from whether a password was given:
/// an account with one is `active`, an account without is `invited`. Stating
/// both would let a caller write an `active` account whose `password_hash` is
/// NULL — an account the store says may sign in and that no password can ever
/// open — and no reader would be able to tell that from one that simply has not
/// been given a password yet.
///
/// The id is not a field either, for the same reason: a caller that has one
/// attaches it with [`Self::with_id`], and a caller that has none gets one
/// minted by the store ([`new_account_id`]). Making it required would mean
/// every deployment that has no external identity provider inventing ids of its
/// own — which is how two of them end up inventing the same format, or none at
/// all and a colliding timestamp.
#[derive(Debug, Clone)]
pub struct NewUser<'a> {
    /// The id to write, or `None` to have one minted.
    pub id: Option<&'a str>,
    /// The sign-in name.
    pub username: &'a str,
    /// The name to show.
    pub display_name: &'a str,
    /// The address, when one is known.
    pub email: Option<&'a str>,
    /// The password to hash and store, or `None` to leave the account unable to
    /// sign in until it is given one.
    pub password: Option<&'a str>,
    /// Roles to grant.
    pub roles: &'a [&'a str],
}

impl<'a> NewUser<'a> {
    /// An account that can sign in.
    pub fn active(username: &'a str, display_name: &'a str, password: &'a str) -> Self {
        Self {
            id: None,
            username,
            display_name,
            email: None,
            password: Some(password),
            roles: &[],
        }
    }

    /// An account created from an invite: it exists, it has no password, and
    /// its status says so.
    pub fn invited(username: &'a str, display_name: &'a str) -> Self {
        Self {
            id: None,
            username,
            display_name,
            email: None,
            password: None,
            roles: &[],
        }
    }

    /// The same account under an id this deployment already uses for it.
    ///
    /// For an account the deployment did not invent: a hub, a directory, a
    /// migration names the person already, and an id minted here would be a
    /// second name for one account — the document index and the tenant store
    /// would then know them by the other one.
    ///
    /// The value is taken exactly as given (see [`checked_opaque`]): an id is
    /// asserted by a machine, not typed by a person, so it is not trimmed.
    pub fn with_id(mut self, id: &'a str) -> Self {
        self.id = Some(id);
        self
    }

    /// The same account with an address.
    pub fn with_email(mut self, email: &'a str) -> Self {
        self.email = Some(email);
        self
    }

    /// The same account with roles.
    pub fn with_roles(mut self, roles: &'a [&'a str]) -> Self {
        self.roles = roles;
        self
    }

    /// The status this account will be written with.
    pub fn status(&self) -> UserStatus {
        if self.password.is_some() {
            UserStatus::Active
        } else {
            UserStatus::Invited
        }
    }
}

/// A signed-in browser, as the database holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The account this session belongs to.
    pub user_id: String,
    /// When it was created.
    pub created_at: i64,
    /// When it stops being accepted. Absolute, never recomputed.
    pub expires_at: i64,
    /// When a request last used it, if any has.
    pub last_seen_at: Option<i64>,
    /// The client's own description of itself, as it sent it.
    pub user_agent: Option<String>,
    /// The address the session was created from.
    pub ip: Option<String>,
}

impl Session {
    /// Whether the session may be used at `now`.
    ///
    /// Strictly after the expiry: a session that expires at `t` is over at `t`,
    /// which is the reading that fails closed for the one second where the two
    /// interpretations differ.
    pub fn is_live_at(&self, now: i64) -> bool {
        self.expires_at > now
    }
}

/// A session about to be created.
#[derive(Debug, Clone)]
pub struct NewSession<'a> {
    /// The account to sign in.
    pub user_id: &'a str,
    /// How long the session lasts. Chosen by the caller: a session's lifetime
    /// is a deployment's policy, and the store has no basis for picking one.
    pub ttl_secs: i64,
    /// The client's user agent, as received.
    pub user_agent: Option<&'a str>,
    /// The client's address, as received.
    pub ip: Option<&'a str>,
}

impl<'a> NewSession<'a> {
    /// A session for `user_id` lasting `ttl_secs`, with nothing known about the
    /// client.
    pub fn new(user_id: &'a str, ttl_secs: i64) -> Self {
        Self {
            user_id,
            ttl_secs,
            user_agent: None,
            ip: None,
        }
    }

    /// The same session with what the request knew about its client.
    pub fn with_client(mut self, user_agent: Option<&'a str>, ip: Option<&'a str>) -> Self {
        self.user_agent = user_agent;
        self.ip = ip;
        self
    }
}

/// A session that was just created, together with the token that opens it.
///
/// The token is here and nowhere else: it is not stored, so this is the only
/// moment in the session's life at which it exists on this side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedSession {
    /// The value the client keeps. The database holds only its SHA-256.
    pub token: String,
    /// The row that was written.
    pub session: Session,
}

/// Why a one-time token was handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneTimePurpose {
    /// Prove control of the account and set a new password.
    PasswordReset,
    /// Prove control of the address on the account.
    EmailVerify,
}

impl OneTimePurpose {
    /// The value stored in `one_time_tokens.purpose`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PasswordReset => "password_reset",
            Self::EmailVerify => "email_verify",
        }
    }

    /// The purpose a stored value names, or the error that says it names none.
    pub fn parse(value: &str) -> Result<Self, AccountsError> {
        match value {
            "password_reset" => Ok(Self::PasswordReset),
            "email_verify" => Ok(Self::EmailVerify),
            other => Err(AccountsError::UnknownPurpose {
                purpose: other.to_string(),
            }),
        }
    }
}

/// A single-use link, as the database holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneTimeToken {
    /// The row's id, for logs and sweeps. It reaches no client.
    pub id: i64,
    /// The account it was issued for.
    pub user_id: String,
    /// What it is for.
    pub purpose: OneTimePurpose,
    /// When it was issued.
    pub created_at: i64,
    /// When it stops being accepted.
    pub expires_at: i64,
    /// When it was consumed, if it has been.
    pub used_at: Option<i64>,
}

impl OneTimeToken {
    /// Whether the token may still be consumed at `now`.
    pub fn is_live_at(&self, now: i64) -> bool {
        self.used_at.is_none() && self.expires_at > now
    }
}

/// A one-time token that was just issued, with the value the client receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedToken {
    /// The plaintext token. Stored nowhere.
    pub token: String,
    /// The row's id.
    pub id: i64,
    /// When it stops being accepted.
    pub expires_at: i64,
}

/// An invitation about to be issued.
#[derive(Debug, Clone)]
pub struct NewInvite<'a> {
    /// The address the invite was meant for, when the operator knows it.
    pub email: Option<&'a str>,
    /// Roles the accepting account is to be granted.
    pub roles: &'a [&'a str],
    /// The account issuing the invite, when one is behind it.
    pub created_by: Option<&'a str>,
    /// How long the link lasts.
    pub ttl_secs: i64,
}

impl<'a> NewInvite<'a> {
    /// A link valid for `ttl_secs`, granting `roles`, with no address recorded.
    pub fn new(roles: &'a [&'a str], created_by: Option<&'a str>, ttl_secs: i64) -> Self {
        Self {
            email: None,
            roles,
            created_by,
            ttl_secs,
        }
    }

    /// The same invite carrying the address it was meant for.
    pub fn with_email(mut self, email: &'a str) -> Self {
        self.email = Some(email);
        self
    }
}

/// An invitation, as the database holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// The address it was meant for, if one was recorded.
    pub email: Option<String>,
    /// Roles the accepting account is to be granted.
    pub roles: Vec<String>,
    /// Who issued it, if that account still exists.
    pub created_by: Option<String>,
    /// When it was issued.
    pub created_at: i64,
    /// When it stops being accepted.
    pub expires_at: i64,
    /// Who accepted it, if that account still exists.
    pub accepted_by: Option<String>,
    /// When it was accepted, if it has been.
    pub accepted_at: Option<i64>,
}

impl Invite {
    /// Whether the invite may still be redeemed at `now`.
    ///
    /// The test is `accepted_at`, not `accepted_by`: deleting an account nulls
    /// the id and leaves the timestamp, and a spent invite must not become
    /// usable again because the person who used it left.
    pub fn is_redeemable_at(&self, now: i64) -> bool {
        self.accepted_at.is_none() && self.expires_at > now
    }
}

/// An invite that was just issued, with the value the operator receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedInvite {
    /// The plaintext token. Stored nowhere: the operator has this copy and the
    /// database has the hash.
    pub token: String,
    /// The row that was written.
    pub invite: Invite,
}

/// The error for a column that holds a value this build does not understand.
///
/// Reported as SQLite's own "this is not a value of the type the reader asked
/// for", which is exactly what it is, and which keeps every decoder here in the
/// shape the rest of the crate reads rows in (`query_row` / `query_map` plus
/// `.optional()`, with no hand-rolled cursor loop). [`AccountsError::from`]
/// unwraps the domain error again on the way out, so a caller still matches on
/// `UnknownStatus` rather than on a sentence.
fn refused_value(index: usize, error: AccountsError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
}

/// Read a [`User`] out of the columns [`USER_COLUMNS`] names.
pub(super) fn user_from_row(row: &Row<'_>) -> rusqlite::Result<User> {
    let status: String = row.get(USER_STATUS)?;
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        email: row.get(3)?,
        email_verified_at: row.get(4)?,
        password_hash: row.get(5)?,
        hash_algo: row.get(6)?,
        roles: decode_roles(&row.get::<_, String>(7)?),
        status: UserStatus::parse(&status).map_err(|error| refused_value(USER_STATUS, error))?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        last_seen_at: row.get(11)?,
    })
}

/// Read a [`Session`] out of a row, starting at `offset`.
///
/// The offset exists because one reader has to take the token hash as well:
/// resolving a token wants the stored hash next to the session it belongs to,
/// so that the row handed back is checked against the value that was asked for
/// rather than assumed to be it. Every other reader passes 0.
pub(super) fn session_from_row(row: &Row<'_>, offset: usize) -> rusqlite::Result<Session> {
    Ok(Session {
        user_id: row.get(offset)?,
        created_at: row.get(offset + 1)?,
        expires_at: row.get(offset + 2)?,
        last_seen_at: row.get(offset + 3)?,
        user_agent: row.get(offset + 4)?,
        ip: row.get(offset + 5)?,
    })
}

/// Read an [`Invite`] out of the columns [`INVITE_COLUMNS`] names.
pub(super) fn invite_from_row(row: &Row<'_>) -> rusqlite::Result<Invite> {
    let roles: String = row.get(1)?;
    Ok(Invite {
        email: row.get(0)?,
        roles: decode_roles(&roles),
        created_by: row.get(2)?,
        created_at: row.get(3)?,
        expires_at: row.get(4)?,
        accepted_by: row.get(5)?,
        accepted_at: row.get(6)?,
    })
}

/// Read a [`OneTimeToken`] out of the columns [`ONE_TIME_TOKEN_COLUMNS`] names.
pub(super) fn one_time_token_from_row(row: &Row<'_>) -> rusqlite::Result<OneTimeToken> {
    let purpose: String = row.get(2)?;
    Ok(OneTimeToken {
        id: row.get(0)?,
        user_id: row.get(1)?,
        purpose: OneTimePurpose::parse(&purpose)
            .map_err(|error| refused_value(ONE_TIME_TOKEN_PURPOSE, error))?,
        created_at: row.get(3)?,
        expires_at: row.get(4)?,
        used_at: row.get(5)?,
    })
}

/// A value that is about to become part of a row's identity, trimmed and
/// checked.
///
/// Trimmed because a name typed with a trailing space is the same name: with
/// `COLLATE NOCASE` uniqueness, `bob ` and `bob` would otherwise be two
/// accounts, and the person who typed the space could never sign in again.
///
/// Checked for emptiness and control characters because either would produce a
/// row that cannot be found by the value that was written: an empty name is
/// matched by every other empty name, and a control character travels into
/// logs, headers and terminal output.
///
/// What is NOT checked is the character set — which letters a username may use,
/// whether an address is deliverable. That is a policy the layer with the form
/// in front of it owns, and it is the layer that can tell a person why their
/// name was refused.
pub(super) fn checked_text(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, AccountsError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AccountsError::InvalidText {
            field,
            reason: "is empty",
        });
    }
    if trimmed.chars().any(char::is_control) {
        return Err(AccountsError::InvalidText {
            field,
            reason: "contains a control character",
        });
    }
    if trimmed.chars().count() > max {
        return Err(AccountsError::InvalidText {
            field,
            reason: "is longer than the column allows",
        });
    }
    Ok(trimmed.to_string())
}

/// [`checked_text`] for a column that may be absent.
pub(super) fn checked_optional_text(
    field: &'static str,
    value: Option<&str>,
    max: usize,
) -> Result<Option<String>, AccountsError> {
    match value {
        None => Ok(None),
        Some(value) => checked_text(field, value, max).map(Some),
    }
}

/// A value taken EXACTLY as given — checked, but not trimmed.
///
/// For the account id, which is not something a person typed: it is asserted by
/// whatever issued it (a uuid, a name from the hub), and the store's whole use
/// for it is matching that assertion later. Trimming would make ` u1 ` and `u1`
/// one account, which is a correspondence no provider ever stated — so a padded
/// id is stored as written and looked up as written, and the caller who padded
/// it finds out at the next lookup rather than silently.
pub(super) fn checked_opaque(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, AccountsError> {
    if value.is_empty() {
        return Err(AccountsError::InvalidText {
            field,
            reason: "is empty",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(AccountsError::InvalidText {
            field,
            reason: "contains a control character",
        });
    }
    if value.chars().count() > max {
        return Err(AccountsError::InvalidText {
            field,
            reason: "is longer than the column allows",
        });
    }
    Ok(value.to_string())
}

/// The id of an account about to be created, checked.
pub(super) fn checked_id(id: &str) -> Result<String, AccountsError> {
    checked_opaque("id", id, MAX_ID)
}

/// The username of an account about to be created, checked.
pub(super) fn checked_username(username: &str) -> Result<String, AccountsError> {
    checked_text("username", username, MAX_USERNAME)
}

/// The display name of an account about to be created, checked.
pub(super) fn checked_display_name(display_name: &str) -> Result<String, AccountsError> {
    checked_text("display_name", display_name, MAX_DISPLAY_NAME)
}

/// The address of an account about to be created, checked.
pub(super) fn checked_email(email: Option<&str>) -> Result<Option<String>, AccountsError> {
    checked_optional_text("email", email, MAX_EMAIL)
}

/// A role list, encoded for the column that holds it.
///
/// Duplicates are dropped rather than written twice: the same role granted
/// twice is one grant, and a reader that had to dedupe on every load would be
/// doing work the writer can do once. The order given is kept, so the column
/// reads back the way it was written.
pub(super) fn encode_roles(roles: &[&str]) -> Result<String, AccountsError> {
    let mut held: Vec<String> = Vec::new();
    for role in roles {
        let role = checked_text("roles", role, MAX_ROLE)?;
        if role.contains(',') {
            return Err(AccountsError::InvalidText {
                field: "roles",
                reason: "a role name may not contain a comma",
            });
        }
        if !held.contains(&role) {
            held.push(role);
        }
    }
    let encoded = held.join(",");
    if encoded.chars().count() > MAX_ROLES {
        return Err(AccountsError::InvalidText {
            field: "roles",
            reason: "more roles than the column allows",
        });
    }
    Ok(encoded)
}

/// The role list a stored value holds.
///
/// Empty entries are dropped. The schema's CHECK refuses them, so this is the
/// decoder being total rather than trusting: a value a hand repair wrote as
/// `a,,b` decodes to two roles, which is what it means, instead of a role named
/// `""` that no reader would ever grant.
pub(super) fn decode_roles(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_string)
        .collect()
}
