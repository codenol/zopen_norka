//! What the account store can refuse, in one enum.
//!
//! Every failure this module produces is one of these, so a caller decides what
//! to do by matching on a variant rather than by reading a sentence. The two
//! variants that carry a `String` are the two whose text comes from outside —
//! the filesystem and SQLite — and they carry it because the alternative is
//! throwing away the only description of what actually went wrong.
//!
//! The identity variants carry the value that was refused. Nothing here is a
//! secret: a password is never a field of an error, and the values that are
//! (a name, an address) were sent by the caller and are on their way back to
//! them anyway.

/// Why an account operation did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountsError {
    /// The database file or its directory could not be created or read.
    Io(String),
    /// SQLite refused a statement, or returned a row this schema should not be
    /// able to hold.
    Database(String),
    /// The operating system's random source failed. Fatal for the operation —
    /// a token generated without real randomness is worse than no token.
    Random(String),
    /// A password hash could not be produced, or a stored one is not a hash
    /// this build can read.
    PasswordHash(String),
    /// An empty password was offered where one was required.
    ///
    /// Refused here rather than at a route because an empty string is a
    /// credential that verifies: it would be stored as a real Argon2id hash of
    /// "", indistinguishable in the table from a deliberate one.
    EmptyPassword,
    /// A name, address or role that would make its own row unreachable, list
    /// the reason it was refused.
    InvalidText {
        /// The column the value was meant for (`username`, `email`, `roles`, …).
        field: &'static str,
        /// Why it was refused, in a few words.
        reason: &'static str,
    },
    /// The username is already taken. Case-insensitively: `Alice` and `alice`
    /// are one account.
    UsernameTaken {
        /// The username that collided.
        username: String,
    },
    /// The email address already belongs to an account.
    EmailTaken {
        /// The address that collided.
        email: String,
    },
    /// No account with this id exists where one was required.
    NoSuchUser {
        /// The id that matched nothing.
        id: String,
    },
    /// The account holds no address, so there is nothing that could be proved
    /// about one.
    NoEmail {
        /// The account that has no address.
        id: String,
    },
    /// A `users.status` value this build does not know. Refused rather than
    /// mapped to a default: an account whose state is unknown is not one to
    /// guess about.
    UnknownStatus {
        /// The value found in the column.
        status: String,
    },
    /// A `one_time_tokens.purpose` value this build does not know.
    UnknownPurpose {
        /// The value found in the column.
        purpose: String,
    },
    /// No invite carries this token hash.
    InviteNotFound,
    /// The invite exists and its window has passed.
    InviteExpired,
    /// The invite was already used to create an account.
    InviteAlreadyAccepted,
}

impl std::fmt::Display for AccountsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(detail) => write!(f, "account store IO failed: {detail}"),
            Self::Database(detail) => write!(f, "account store query failed: {detail}"),
            Self::Random(detail) => write!(f, "could not read random bytes: {detail}"),
            Self::PasswordHash(detail) => write!(f, "password hash refused: {detail}"),
            Self::EmptyPassword => f.write_str("a password must not be empty"),
            Self::InvalidText { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::UsernameTaken { username } => {
                write!(f, "the username {username} is already taken")
            }
            Self::EmailTaken { email } => write!(f, "the email {email} is already in use"),
            Self::NoSuchUser { id } => write!(f, "no account with id {id}"),
            Self::NoEmail { id } => write!(f, "account {id} has no address to verify"),
            Self::UnknownStatus { status } => write!(f, "unknown account status {status}"),
            Self::UnknownPurpose { purpose } => {
                write!(f, "unknown one-time token purpose {purpose}")
            }
            Self::InviteNotFound => f.write_str("no invite carries this token"),
            Self::InviteExpired => f.write_str("this invite has expired"),
            Self::InviteAlreadyAccepted => f.write_str("this invite has already been accepted"),
        }
    }
}

impl std::error::Error for AccountsError {}

impl From<rusqlite::Error> for AccountsError {
    /// Every SQLite failure becomes one variant.
    ///
    /// Not because the callers treat them alike — several do look at whether a
    /// uniqueness constraint fired, and they do it by asking the database which
    /// row exists rather than by matching on SQLite's English message text —
    /// but because a statement that failed is a statement this store cannot
    /// interpret, and the message is the whole of what is known about it.
    ///
    /// The exception is the conversion failure a decoder raises when a COLUMN
    /// holds a value this build cannot read (an unknown `status`, an unknown
    /// token `purpose`). That one wraps an error from this enum, and it is
    /// unwrapped here: "the account's state cannot be read" is a different
    /// answer from "the query failed", and a caller that could not tell them
    /// apart would report a database fault for a row it must refuse.
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::FromSqlConversionFailure(_, _, source) => {
                match source.downcast::<Self>() {
                    Ok(refused) => *refused,
                    // A conversion failure raised by rusqlite itself (a column
                    // whose type does not match the one asked for), with no
                    // domain error inside. Its own message is the description.
                    Err(other) => Self::Database(other.to_string()),
                }
            }
            other => Self::Database(other.to_string()),
        }
    }
}
