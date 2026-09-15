//! A section's properties: the analytics it references, its summary, its flows.
//!
//! One row per section, keyed by the section's address — the document it lives
//! in, and the node id that identifies it inside that document. The same shape,
//! and the same reasoning, as a comment thread
//! ([`crate::document_comments`]): nodes live inside the `.op` file, so a node
//! id is a name the client and the document agree on rather than a row here.
//!
//! ## What is NOT in this module
//!
//! The section itself. A section is a frame in the document — marked with
//! `role: "section"` — so it travels with the screens it groups, survives
//! copy-paste, and is visible on the canvas. `op_editor_core::section` owns
//! that model; this module only stores what hangs off it.
//!
//! ## Why a row and not a field of the frame
//!
//! Because the schema is vendored and a field on `FrameNode` is not a change
//! one stage can land — see `op_editor_core::section`'s module docs, which
//! record the measurement. The cost is stated there and repeated here because
//! it is the one thing a reader of this file has to know: the grouping survives
//! copy-paste with the frame, the metadata does not, because a copy is a new
//! node id. When the schema edit is scheduled, the row becomes a field and this
//! module's callers change where they read from and nothing else.
//!
//! ## Why the payload is JSON
//!
//! The properties are one authored document — references with their
//! fingerprints, four fields of summary, a list of flow graphs — read and
//! written together, and the flow graph inside it is still being designed. A
//! column per step would freeze a structure in SQL and make every change to the
//! model a migration. The payload carries its own format number
//! (`op_editor_core::section::SECTION_PROPERTIES_FORMAT`), and a build that does
//! not understand it refuses the row rather than reading half of somebody's
//! work.
//!
//! ## Why every function is scoped by document key
//!
//! Not one of these takes a node id without the document it must be found
//! under, including the writes. A route decides what a caller may do from the
//! KEY in its path — the document is opened and its owner compared — and a node
//! id is not a capability: it is a string in a body. Resolving by node id alone
//! would let a caller who may reach document A rewrite what document B says
//! about itself by quoting an id, which is the same hole the key-owner check
//! closes one level up. So the key is a WHERE term on every statement, and the
//! write states it in its `INSERT ... SELECT` rather than checking it in Rust
//! first: one statement cannot lose a race with itself.

use rusqlite::{params, OptionalExtension};

use op_editor_core::node_id::NodeId;
use op_editor_core::section::{SectionProperties, StoredSectionProperties};

use crate::document_db::{db_error, DocumentDb};
use crate::section_store_error::SectionStoreError;

/// One section's stored properties, with its address.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SectionRecord {
    pub document_key: String,
    pub node_id: NodeId,
    pub properties: SectionProperties,
    pub updated_at: u64,
}

/// The properties of one section, when the document has them.
///
/// `None` is the ordinary case for a section nobody has written anything about
/// yet: the frame exists in the document, and there is no row. That is not a
/// failure — a section with no analytics is a state the operator named.
pub(crate) fn load(
    db: &DocumentDb,
    document_key: &str,
    node_id: &NodeId,
) -> Result<Option<SectionProperties>, SectionStoreError> {
    let conn = db.conn();
    let stored: Option<String> = conn
        .query_row(
            "SELECT properties FROM section_properties
             WHERE document_key = ?1 AND node_id = ?2",
            params![document_key, node_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    match stored {
        Some(text) => Ok(Some(StoredSectionProperties::decode(&text)?.properties)),
        None => Ok(None),
    }
}

/// Write a section's properties, replacing whatever was there.
///
/// The document must be stored: the row references it, and the rule is the same
/// one comments follow — a section's properties are saved against a stored
/// document, not against a draft this daemon has never seen. The check is part
/// of the statement (`WHERE EXISTS`) rather than a read-then-write in Rust, so
/// two requests cannot race between the check and the insert.
pub(crate) fn save(
    db: &DocumentDb,
    document_key: &str,
    node_id: &NodeId,
    properties: &SectionProperties,
) -> Result<(), SectionStoreError> {
    if !node_id.is_real() {
        return Err(SectionStoreError::EmptyNodeId);
    }
    let payload = StoredSectionProperties::current(properties.clone()).encode();
    let conn = db.conn();
    let changed = conn
        .execute(
            "INSERT INTO section_properties (document_key, node_id, properties, updated_at)
             SELECT ?1, ?2, ?3, ?4
              WHERE EXISTS (SELECT 1 FROM documents WHERE key = ?1)
             ON CONFLICT (document_key, node_id)
             DO UPDATE SET properties = excluded.properties, updated_at = excluded.updated_at",
            params![
                document_key,
                node_id.as_str(),
                payload,
                crate::document_store::now_secs(),
            ],
        )
        .map_err(db_error)?;
    if changed == 0 {
        // No document carries that key, so there is nothing to hang a section
        // off. The same answer the store gives for a document it does not have.
        return Err(SectionStoreError::NotFound);
    }
    Ok(())
}

/// Every section of one document, in node-id order.
///
/// Ordered by node id rather than by time: a caller listing what a document
/// says about itself wants it in a stable order it can compare between calls,
/// and the document's own order is the frame tree's business, not this table's.
pub(crate) fn list(
    db: &DocumentDb,
    document_key: &str,
) -> Result<Vec<SectionRecord>, SectionStoreError> {
    let conn = db.conn();
    let mut statement = conn
        .prepare(
            "SELECT node_id, properties, updated_at FROM section_properties
             WHERE document_key = ?1 ORDER BY node_id",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![document_key], |row| {
            let node_id: String = row.get(0)?;
            let payload: String = row.get(1)?;
            let updated_at: u64 = row.get(2)?;
            Ok((node_id, payload, updated_at))
        })
        .map_err(db_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?;

    let mut records = Vec::with_capacity(rows.len());
    for (node_id, payload, updated_at) in rows {
        // A row whose node id is empty could only have been written by
        // something other than this module; skipped rather than fatal, so one
        // bad row does not take a document's whole list down.
        let Some(node_id) = NodeId::new_opt(node_id) else {
            continue;
        };
        records.push(SectionRecord {
            document_key: document_key.to_string(),
            node_id,
            properties: StoredSectionProperties::decode(&payload)?.properties,
            updated_at,
        });
    }
    Ok(records)
}

/// Whether any section of this document links `asset_key`.
///
/// The reverse of the reference a section carries, and the only question that
/// can let an asset be read by somebody who does not own it (issue #110): an
/// analytics asset is reached by its OWN key, so there is no document in its
/// address to ask about — the document that vouches for a reader has to be found
/// from the link instead. Asked of ONE document, the one the request named,
/// rather than of every document in the store: the caller's own share is the
/// only thing that may vouch for them, so nothing else has to be looked at, and
/// the cost of reading one asset stays that document's rows.
///
/// A row this build cannot decode is an error rather than a `false`. Whether it
/// holds the link is exactly what could not be read, and a vouch nobody can
/// verify is not one — the caller answers "no" and the owner still reads their
/// own asset.
pub(crate) fn references_asset(
    db: &DocumentDb,
    document_key: &str,
    asset_key: &str,
) -> Result<bool, SectionStoreError> {
    let conn = db.conn();
    let mut statement = conn
        .prepare("SELECT properties FROM section_properties WHERE document_key = ?1")
        .map_err(db_error)?;
    let payloads = statement
        .query_map(params![document_key], |row| row.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?;
    for payload in payloads {
        let properties = StoredSectionProperties::decode(&payload)?.properties;
        if properties
            .analytics
            .iter()
            .any(|link| link.key == asset_key)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Drop a section's properties. `false` when there was nothing to drop.
///
/// What a deleted section frame leaves behind, and what unmarking a frame
/// calls: the row is the properties OF a section, and a node that is no longer
/// one has none.
pub(crate) fn delete(
    db: &DocumentDb,
    document_key: &str,
    node_id: &NodeId,
) -> Result<bool, SectionStoreError> {
    let removed = db
        .conn()
        .execute(
            "DELETE FROM section_properties WHERE document_key = ?1 AND node_id = ?2",
            params![document_key, node_id.as_str()],
        )
        .map_err(db_error)?;
    Ok(removed > 0)
}

#[cfg(test)]
#[path = "section_store_tests.rs"]
mod tests;
