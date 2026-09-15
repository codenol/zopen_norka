//! Why a section or an analytics asset could not be read or written.
//!
//! One enum for the two stores that make up the sections feature
//! ([`crate::analytics_store`], [`crate::section_store`]), because they are one
//! domain and a caller that reads a section's analytics touches both: the
//! asset's file, the asset's row, and the section's own row. Two enums would
//! mean two spellings of "not found" and two places to look for "what does a
//! broken store look like".
//!
//! Structured fields rather than pre-formatted text, so a route can answer with
//! a status the caller can act on and a test can assert on the case rather than
//! on a sentence.

use op_editor_core::section::SectionFormatError;

use crate::document_store::DocumentStoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SectionStoreError {
    /// The key is not one this store could have issued — the only thing
    /// standing between a pasted URL and the filesystem.
    InvalidKey,
    /// No asset, or no section, carries that address.
    NotFound,
    /// A node id that is empty, and therefore names nothing.
    EmptyNodeId,
    /// The name is longer than a name this store will keep.
    NameTooLong { chars: usize, max: usize },
    /// The markdown is larger than this store will keep.
    TooLarge { bytes: usize, max: usize },
    /// The row is there and the file it accounts for is not.
    ///
    /// Its own case rather than [`Self::NotFound`], and deliberately: a row
    /// without its file is a store somebody has damaged, while "not found" is
    /// the ordinary "nobody ever loaded this". Reported as absent, a broken
    /// store would make a section announce that its analytics was deleted when
    /// nobody deleted it.
    MissingFile { path: String },
    /// The stored properties could not be read: not this shape, or a format
    /// this build does not know.
    Properties(SectionFormatError),
    /// The database could not be read or written. Carries what SQLite said,
    /// which is the only part a reader can act on.
    Database(String),
    /// A file beside the database could not be read, created or removed.
    Io(String),
}

impl std::fmt::Display for SectionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey => write!(f, "invalid key"),
            Self::NotFound => write!(f, "not found"),
            Self::EmptyNodeId => write!(f, "empty node id"),
            Self::NameTooLong { chars, max } => {
                write!(f, "name is {chars} characters, over the {max} limit")
            }
            Self::TooLarge { bytes, max } => {
                write!(f, "document is {bytes} bytes, over the {max} limit")
            }
            Self::MissingFile { path } => {
                write!(f, "the file this record accounts for is gone: {path}")
            }
            Self::Properties(error) => write!(f, "{error}"),
            Self::Database(detail) => write!(f, "document database: {detail}"),
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for SectionStoreError {}

impl From<DocumentStoreError> for SectionStoreError {
    /// The two stores share this database, so a failure from the side that
    /// owns the connection is this domain's failure too — collapsed here so a
    /// call site reads `?` rather than a `map_err` that only renames a variant.
    fn from(error: DocumentStoreError) -> Self {
        match error {
            DocumentStoreError::InvalidKey => Self::InvalidKey,
            DocumentStoreError::NotFound => Self::NotFound,
            DocumentStoreError::Database(detail) => Self::Database(detail),
            DocumentStoreError::Io(detail) => Self::Io(detail),
        }
    }
}

impl From<SectionFormatError> for SectionStoreError {
    fn from(error: SectionFormatError) -> Self {
        Self::Properties(error)
    }
}
