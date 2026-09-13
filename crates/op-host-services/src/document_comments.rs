//! Comment threads: the conversations pinned to a document's elements.
//!
//! The tables are `document_db`'s — migration 2 — because that list is the
//! database's history, and a table created anywhere else would be a table no
//! field database ever recorded as having made. This module is what reads and
//! writes them, and it owns what a thread IS as the rest of the daemon sees it.
//!
//! ## Why every function is scoped by document key
//!
//! Not one of these takes a thread id without the key it must be found under,
//! including the two that write. The reason is authorization and nothing else:
//! a route decides what a caller may do from the KEY in its path — the document
//! is opened, its owner is compared, its rights are checked
//! ([`super::web_canvas_server::request_access`]). A thread id is not a
//! capability; it is a number in a URL. Resolving by id alone would let a caller
//! who may reach document A reply into, close, or read a thread of document B by
//! quoting a small integer, which is the same hole the key-owner check closes
//! one level up. So the key is a WHERE term on every statement here, and the
//! writes take it in their `INSERT ... SELECT` / `UPDATE ... WHERE` rather than
//! checking it in Rust first — one statement cannot lose a race with itself.
//!
//! ## Why a resolved thread is not a deleted one
//!
//! `resolved` is a column, not a DELETE. A review's most valuable artifact is
//! the record of what was asked and what was answered; erasing a thread once it
//! is dealt with throws that away and makes "we discussed this" unprovable. It
//! also keeps the row that a client may still be holding (a link, an open panel)
//! meaningful: it comes back closed rather than gone.
//!
//! ## Bounds live here
//!
//! [`MAX_COMMENT_CHARS`] and [`MAX_NODE_ID_CHARS`] are stated beside the record
//! they bound, because this module is what must never store an unbounded one:
//! a comment is read by everyone who can reach the document, and one caller
//! pasting a novel into a thread makes the conversation unreadable for all of
//! them. The route refuses the request with a 400; these are the numbers it
//! refuses against.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::document_db::{db_error, DocumentDb};
use crate::document_store::DocumentStoreError;

/// Longest comment body accepted, in characters.
///
/// Characters, not bytes: the limit is about how much prose a thread can carry,
/// and a byte limit would quietly allow a Latin comment three times the length
/// of a Russian one. Generous on purpose — this is a ceiling against abuse, not
/// an editorial style.
pub(crate) const MAX_COMMENT_CHARS: usize = 4_000;

/// Longest node id accepted for a pin.
///
/// The id is written by the client and resolved by the client, so this bound
/// only has to be longer than any id this editor issues; it exists so that a
/// public deployment cannot be made to store arbitrary-length keys.
pub(crate) const MAX_NODE_ID_CHARS: usize = 128;

/// Who is being recorded as having said or done something.
///
/// Two fields because they answer two different questions and only one of them
/// is allowed to age: [`Author::id`] is the account, which is what "was this me"
/// and "may I resolve it" are asked against, and [`Author::name`] is what the
/// moment looked like, kept so the history still reads as people after somebody
/// is renamed or leaves the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Author<'a> {
    /// Stable account id. `None` is the local operator, who has no account —
    /// the same meaning `documents.owner_id`'s NULL carries.
    pub id: Option<&'a str>,
    /// Display name at the time, or empty for the local operator (see
    /// [`Author::id`]): there is no account behind them to take a name from,
    /// and a name invented here would be this server's guess at somebody's.
    pub name: &'a str,
    /// The author's role on the wire, or `None` when the hub sent none this
    /// build knows. Shown as a colour, so it is recorded rather than looked up.
    pub role: Option<&'a str>,
}

/// A comment about to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NewComment<'a> {
    pub author: Author<'a>,
    pub body: &'a str,
}

/// What a resolve or a reopen asks a thread to become.
///
/// One enum rather than a `bool` and two stamps, because the three facts are
/// one fact: a thread is either open, and has no resolver, or closed, and has
/// one. Passed separately they could be given in a combination that means
/// nothing — a closed thread with no resolver, an open one with a resolver —
/// and nothing in the schema would refuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionChange<'a> {
    /// Close the thread, recording who closed it and when.
    Resolve { author: Author<'a>, at: u64 },
    /// Open it again. The stamp is cleared: an open thread has no resolver, and
    /// leaving the last one behind would make an open thread read as closed in
    /// any client that looks only at `resolved_by`. That the thread was once
    /// closed is deliberately not kept — history is an event log, which this
    /// schema is not (see the module notes on the report).
    Reopen,
}

/// One comment, with the author as they were when they wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Comment {
    pub id: i64,
    pub author_id: Option<String>,
    pub author_name: String,
    /// The author's role on the wire, as a snapshot beside the name.
    ///
    /// Kept for the same reason the name is: it is what the comment looked
    /// like when it was written. A role read live would repaint old comments
    /// whenever someone's roles changed, and the colour is meant to say who
    /// was speaking then. `None` means the hub sent no role this build knows.
    pub author_role: Option<String>,
    pub body: String,
    pub created_at: u64,
}

/// One thread, with its comments in the order they were written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommentThread {
    pub id: i64,
    /// The element the pin sits on.
    pub node_id: String,
    pub created_at: u64,
    pub resolved: bool,
    /// Set exactly when `resolved` is, and written together with it (see
    /// [`set_resolved`]).
    pub resolved_at: Option<u64>,
    pub resolved_by: Option<String>,
    /// The resolver's name at the time, `None` for an open thread and empty for
    /// the local operator — the same rule the comment author follows.
    pub resolved_by_name: Option<String>,
    /// Oldest first. Never empty for a thread this module wrote: a thread is
    /// created with its first comment.
    pub comments: Vec<Comment>,
}

/// A thread's columns, plus at most one of its comments — the shape the join
/// returns, before the rows are folded back into threads.
struct JoinedRow {
    id: i64,
    node_id: String,
    created_at: u64,
    resolved: bool,
    resolved_at: Option<u64>,
    resolved_by: Option<String>,
    resolved_by_name: Option<String>,
    comment: Option<Comment>,
}

/// The comment columns of a joined row, at `first` and the five after it.
///
/// A `LEFT JOIN` so a thread with no comments is still listed rather than
/// quietly dropped from the conversation. Nothing in this module writes one —
/// a thread is inserted together with its first comment — but a row can arrive
/// without comments from outside this code (a hand-repaired database, a future
/// import), and losing a whole thread because of that would be the wrong way to
/// find out.
fn comment_at(row: &Row<'_>, first: usize) -> rusqlite::Result<Option<Comment>> {
    let id: Option<i64> = row.get(first)?;
    Ok(match id {
        Some(id) => Some(Comment {
            id,
            author_id: row.get(first + 1)?,
            author_name: row.get(first + 2)?,
            author_role: row.get(first + 3)?,
            body: row.get(first + 4)?,
            created_at: row.get(first + 5)?,
        }),
        None => None,
    })
}

/// Threads with their comments, joined, so one document's whole conversation is
/// one query rather than one per thread.
const THREAD_QUERY: &str = "SELECT t.id, t.node_id, t.created_at, t.resolved,
            t.resolved_at, t.resolved_by, t.resolved_by_name,
            c.id, c.author_id, c.author_name, c.author_role, c.body, c.created_at
       FROM comment_threads AS t
       LEFT JOIN comments AS c ON c.thread_id = t.id";

/// Chronological, with the rowid as the tie-break.
///
/// `created_at` is seconds, so two comments written in the same second are
/// ordinary rather than exotic — a reply typed while the first message is still
/// warm is the common case. The rowid is monotonic within the table, which
/// makes it exactly the "what actually came first" a timestamp cannot answer.
const THREAD_ORDER: &str = " ORDER BY t.created_at, t.id, c.created_at, c.id";

/// Fold joined rows into threads, each carrying its own comments.
fn fold_threads(rows: Vec<JoinedRow>) -> Vec<CommentThread> {
    let mut threads: Vec<CommentThread> = Vec::new();
    for row in rows {
        let JoinedRow {
            id,
            node_id,
            created_at,
            resolved,
            resolved_at,
            resolved_by,
            resolved_by_name,
            comment,
        } = row;
        // The join orders by thread first, so a thread's comments are one
        // contiguous run and "the last thread pushed is this row's thread"
        // holds.
        if threads.last().map(|thread| thread.id) != Some(id) {
            threads.push(CommentThread {
                id,
                node_id,
                created_at,
                resolved,
                resolved_at,
                resolved_by,
                resolved_by_name,
                comments: Vec::new(),
            });
        }
        if let Some(comment) = comment {
            if let Some(thread) = threads.last_mut() {
                thread.comments.push(comment);
            }
        }
    }
    threads
}

/// Read `document_key`'s threads, or the one thread `only` names.
fn read_threads(
    conn: &Connection,
    document_key: &str,
    only: Option<i64>,
) -> Result<Vec<CommentThread>, DocumentStoreError> {
    // Two SQL shapes rather than `(?2 IS NULL OR t.id = ?2)`: the second form
    // hands the planner a term it cannot use as an index probe, and which of
    // the two is meant is known here rather than in SQL.
    let sql = match only {
        Some(_) => format!("{THREAD_QUERY} WHERE t.document_key = ?1 AND t.id = ?2{THREAD_ORDER}"),
        None => format!("{THREAD_QUERY} WHERE t.document_key = ?1{THREAD_ORDER}"),
    };
    let mut statement = conn.prepare(&sql).map_err(db_error)?;
    // Collected before folding so the two arms can bind different parameter
    // lists while the fold itself stays one function: a conversation is a
    // handful of rows, and sharing one row iterator would need the statement to
    // outlive both arms.
    let rows: Vec<JoinedRow> = match only {
        Some(id) => statement
            .query_map(params![document_key, id], thread_row)
            .map_err(db_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?,
        None => statement
            .query_map(params![document_key], thread_row)
            .map_err(db_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?,
    };
    Ok(fold_threads(rows))
}

/// One thread, when this document has it.
fn read_thread(
    conn: &Connection,
    document_key: &str,
    thread_id: i64,
) -> Result<Option<CommentThread>, DocumentStoreError> {
    Ok(read_threads(conn, document_key, Some(thread_id))?
        .into_iter()
        .next())
}

/// Read one joined row's thread columns.
fn thread_row(row: &Row<'_>) -> rusqlite::Result<JoinedRow> {
    Ok(JoinedRow {
        id: row.get(0)?,
        node_id: row.get(1)?,
        created_at: row.get(2)?,
        resolved: row.get(3)?,
        resolved_at: row.get(4)?,
        resolved_by: row.get(5)?,
        resolved_by_name: row.get(6)?,
        comment: comment_at(row, 7)?,
    })
}

/// Whether the store has a row for `document_key`.
///
/// The threads hang off that row through a foreign key, so a conversation can
/// only exist for a document the store knows about. Asked through a statement
/// of this module's own rather than `document_store::find` because the answer
/// wanted is one bit and not a whole entry.
fn document_is_stored(conn: &Connection, document_key: &str) -> Result<bool, DocumentStoreError> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM documents WHERE key = ?1",
            params![document_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    Ok(found.is_some())
}

/// Who opened a thread — and whether the thread is there at all.
///
/// Three answers rather than an `Option<Option<String>>`, because the three mean
/// different things to whoever asks: a route turns the first into a 404, and the
/// other two into a question about the caller ("are you this account").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ThreadAuthor {
    /// No thread of this document carries that id.
    NoSuchThread,
    /// The thread, opened by the local operator: no account stands behind it.
    LocalOperator,
    /// The thread, opened by this account.
    Account(String),
}

/// The account that opened one of a document's threads.
///
/// The opening comment's author, not a column of its own. A thread has no author
/// field because it does not need one: [`create_thread`] writes the thread and
/// its first comment in one transaction, so who opened it is already recorded —
/// and a second copy of that fact is a second copy that can disagree with it.
///
/// Asked separately from the write that needs it, because it is an
/// AUTHORIZATION input: whether a caller may close a thread depends on who
/// opened it, and that answer has to exist before the decision rather than
/// inside the statement the decision guards.
pub(crate) fn thread_author(
    db: &DocumentDb,
    document_key: &str,
    thread_id: i64,
) -> Result<ThreadAuthor, DocumentStoreError> {
    let conn = db.conn();
    let found: Option<Option<String>> = conn
        .query_row(
            "SELECT c.author_id
               FROM comment_threads AS t
               JOIN comments AS c ON c.thread_id = t.id
              WHERE t.document_key = ?1 AND t.id = ?2
              ORDER BY c.created_at, c.id
              LIMIT 1",
            params![document_key, thread_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    Ok(match found {
        None => ThreadAuthor::NoSuchThread,
        Some(None) => ThreadAuthor::LocalOperator,
        Some(Some(account)) => ThreadAuthor::Account(account),
    })
}

/// Write one comment onto a thread that is known to be in this transaction.
fn insert_comment(
    conn: &Connection,
    thread_id: i64,
    comment: &NewComment<'_>,
    created_at: u64,
) -> Result<(), DocumentStoreError> {
    conn.execute(
        "INSERT INTO comments (thread_id, author_id, author_name, author_role, body, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            thread_id,
            comment.author.id,
            comment.author.name,
            comment.author.role,
            comment.body,
            created_at,
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Every thread of a stored document, oldest first, each with its comments.
///
/// `None` when no document carries `document_key`: the conversation belongs to
/// a document, and "there is no such document" is a different answer from "it
/// has no comments yet" — the one a client shows an empty panel for and the
/// other a 404. Every function here keeps that distinction, so a route can turn
/// it into one reply without asking a second question.
pub(crate) fn list_threads(
    db: &DocumentDb,
    document_key: &str,
) -> Result<Option<Vec<CommentThread>>, DocumentStoreError> {
    let conn = db.conn();
    if !document_is_stored(&conn, document_key)? {
        return Ok(None);
    }
    read_threads(&conn, document_key, None).map(Some)
}

/// Open a thread on `node_id` with its first comment.
///
/// The thread and that comment land in one transaction: a thread with no
/// comments is a pin that shows nothing when it is clicked, and there is no
/// second request that would fill it in. `None` when no document carries the
/// key — asked as part of the insert itself (`INSERT ... SELECT` from
/// `documents`), so the foreign key cannot be reached with a key that has no
/// row, and nothing is written when it has none.
pub(crate) fn create_thread(
    db: &DocumentDb,
    document_key: &str,
    node_id: &str,
    comment: NewComment<'_>,
    created_at: u64,
) -> Result<Option<CommentThread>, DocumentStoreError> {
    let conn = db.conn();
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let inserted = tx
        .execute(
            "INSERT INTO comment_threads (document_key, node_id, created_at, resolved)
             SELECT key, ?2, ?3, 0 FROM documents WHERE key = ?1",
            params![document_key, node_id, created_at],
        )
        .map_err(db_error)?;
    if inserted == 0 {
        return Ok(None);
    }
    // Safe as the row this statement just wrote: one writer at a time holds
    // this connection, and nothing between the insert and here can have
    // written another row.
    let thread_id = tx.last_insert_rowid();
    insert_comment(&tx, thread_id, &comment, created_at)?;
    let thread = read_thread(&tx, document_key, thread_id)?;
    tx.commit().map_err(db_error)?;
    Ok(thread)
}

/// Add a reply to one of a document's threads.
///
/// The whole thread comes back rather than the new comment alone. A reply is
/// the one moment a client is most likely to be holding a stale copy of the
/// conversation (it just showed it to somebody who then typed into it), and
/// handing back the thread lets it replace what it has instead of appending to
/// a version of it that may already be wrong. The cost is re-reading a handful
/// of rows, which for a conversation is nothing.
///
/// `None` when this document has no thread with that id — the same statement
/// that inserts asks it, so a reply cannot land in another document's thread by
/// quoting its id.
pub(crate) fn add_reply(
    db: &DocumentDb,
    document_key: &str,
    thread_id: i64,
    comment: NewComment<'_>,
    created_at: u64,
) -> Result<Option<CommentThread>, DocumentStoreError> {
    let conn = db.conn();
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let inserted = tx
        .execute(
            "INSERT INTO comments (thread_id, author_id, author_name, body, created_at)
             SELECT id, ?3, ?4, ?5, ?6 FROM comment_threads
              WHERE document_key = ?1 AND id = ?2",
            params![
                document_key,
                thread_id,
                comment.author.id,
                comment.author.name,
                comment.body,
                created_at,
            ],
        )
        .map_err(db_error)?;
    if inserted == 0 {
        return Ok(None);
    }
    let thread = read_thread(&tx, document_key, thread_id)?;
    tx.commit().map_err(db_error)?;
    Ok(thread)
}

/// Close or reopen one of a document's threads.
///
/// Writing the state a thread already has is not an error and not a no-op that
/// reads as one: SQLite counts the matched row either way, so resolving a
/// resolved thread updates its stamp to whoever closed it now (the field
/// answers "who closed this", and the last person to do so is the answer) and
/// reopening an open thread answers with the thread it already was. Neither is
/// worth a special case, and both keep the route idempotent for a client that
/// retries.
///
/// `None` when this document has no thread with that id.
pub(crate) fn set_resolved(
    db: &DocumentDb,
    document_key: &str,
    thread_id: i64,
    change: ResolutionChange<'_>,
) -> Result<Option<CommentThread>, DocumentStoreError> {
    let (resolved, at, by, by_name) = match change {
        ResolutionChange::Resolve { author, at } => (true, Some(at), author.id, Some(author.name)),
        ResolutionChange::Reopen => (false, None, None, None),
    };
    let conn = db.conn();
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let changed = tx
        .execute(
            "UPDATE comment_threads
                SET resolved = ?1, resolved_at = ?2, resolved_by = ?3, resolved_by_name = ?4
              WHERE id = ?5 AND document_key = ?6",
            params![resolved, at, by, by_name, thread_id, document_key],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Ok(None);
    }
    let thread = read_thread(&tx, document_key, thread_id)?;
    tx.commit().map_err(db_error)?;
    Ok(thread)
}

#[cfg(test)]
#[path = "document_comments_tests.rs"]
mod tests;
