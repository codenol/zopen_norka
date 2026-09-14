//! The schema's history: every step that ever changed it, in the order those
//! steps were taken.
//!
//! Here rather than in the spine because it is the one part of `document_db`
//! that only grows — each release that touches the schema appends to it, and
//! never shortens it — and because the rule that governs it (append, never
//! edit) is about this list rather than about the store that runs it. The
//! runner is `document_db::migrate`, which applies whatever is pending and
//! records the version in `meta`.
//!
//! The SQL strings are history: their text is what a field database recorded as
//! having run, so they are not edited, comments included. A comment that has
//! been overtaken by later work (migration 1 says every row is ownerless
//! "today", and migration 2 pins a thread to an element) is corrected where the
//! behaviour lives now, not in the record of what the database was.

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
/// backfill that has to read the documents, say) belongs in its own function
/// called from `open`, guarded by its own `meta` key.
///
/// A step that rebuilds a table has one more thing to get right than the SQL
/// suggests — which table may be dropped first — and migration 3 states it
/// where it is done.
pub(super) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: "
        CREATE TABLE documents (
            key           TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            -- NULL means 'not attributed to an account', which is every row
            -- the local daemon creates or imports today (#10).
            owner_id      TEXT,
            created_at    INTEGER NOT NULL,
            updated_at    INTEGER NOT NULL,
            size          INTEGER NOT NULL,
            has_thumbnail INTEGER NOT NULL
        );

        -- The file list is a recency list, and an index in the file list's own
        -- order makes that ORDER BY a scan instead of a sort.
        CREATE INDEX documents_by_recency ON documents (updated_at DESC, key DESC);

        CREATE TABLE last_opened (
            owner_id TEXT PRIMARY KEY NOT NULL,
            -- The pointer is worthless without its document, and deleting a
            -- document should not leave one behind: ON DELETE CASCADE is why
            -- `foreign_keys=ON` is set below rather than merely available.
            key      TEXT NOT NULL REFERENCES documents (key) ON DELETE CASCADE
        );
    ",
    },
    Migration {
        version: 2,
        sql: "
        -- A conversation about a document, pinned to an ELEMENT. A thread
        -- anchored to a coordinate would point at empty canvas the first time
        -- somebody moved the frame it was about, where a node id travels with
        -- its node and is deleted only with it.
        CREATE TABLE comment_threads (
            -- A rowid, not a key: `documents.key` is opaque because it builds
            -- filesystem paths, and a thread id reaches no path. AUTOINCREMENT
            -- is the part that matters — ids are never reused, so an id a client
            -- is still holding answers 'no such thread' once its row is gone,
            -- instead of naming somebody else's conversation.
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            -- The conversation goes with the document it is about: a deleted
            -- document must not leave its threads behind for whoever asks for
            -- that key next. The same rule, and the same reason
            -- `foreign_keys=ON` is set, that `last_opened` above states.
            document_key     TEXT NOT NULL REFERENCES documents (key) ON DELETE CASCADE,
            -- The node the pin sits on, as the client names it. Not a foreign
            -- key: nodes live inside the `.op` file, so this is a name the
            -- client and the document agree on rather than a row here.
            node_id          TEXT NOT NULL,
            created_at       INTEGER NOT NULL,
            -- Closed, not deleted: a resolved thread is the record of what was
            -- asked and answered, which is most of a review's value.
            resolved         INTEGER NOT NULL DEFAULT 0,
            -- Written and cleared WITH `resolved`, by one UPDATE
            -- (`document_comments::set_resolved`), so an open thread never
            -- carries a resolver.
            resolved_at      INTEGER,
            resolved_by      TEXT,
            -- The resolver's name at the time, for the reason a comment keeps
            -- one: a stamp nobody can read as a person is not a record.
            resolved_by_name TEXT
        );

        -- How the threads are read: one document's, in the order written. The
        -- leading column is also what the cascade from `documents` finds its
        -- children by — SQLite does not index a foreign key on its own.
        CREATE INDEX comment_threads_by_document
            ON comment_threads (document_key, created_at, id);

        CREATE TABLE comments (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id   INTEGER NOT NULL REFERENCES comment_threads (id) ON DELETE CASCADE,
            author_role TEXT,
            -- NULL is the local operator, who has no account: the same meaning
            -- this column carries on `documents.owner_id`.
            author_id   TEXT,
            -- A snapshot, not a join: a comment must keep reading as the person
            -- who wrote it after they are renamed or leave the workspace.
            author_name TEXT NOT NULL,
            body        TEXT NOT NULL,
            created_at  INTEGER NOT NULL
        );

        -- Replies are found by their thread, oldest first — and the same index
        -- is what the cascade from a deleted thread finds its children by.
        -- No index on `author_id`: no route reads comments by author, and an
        -- index nothing queries is a write cost with no reader.
        CREATE INDEX comments_by_thread ON comments (thread_id, created_at, id);
    ",
    },
    Migration {
        version: 3,
        sql: "
        -- A comment is a POINT somebody put on a page, not a property of an
        -- element. Migration 2 pinned a thread to `node_id`; this replaces the
        -- anchor with `page_id`, `x`, `y` and demotes the node to `anchor_hint`,
        -- which is a record of what the thread used to point at and decides
        -- nothing. The position of a pin is now a fact about the page it names
        -- and nothing else: not about a node, not about the viewport.
        --
        -- ## Why the table is copied and replaced instead of altered
        --
        -- `node_id` is NOT NULL, and it has to become nullable: a thread opened
        -- on empty canvas has no element behind it, and any placeholder that
        -- stood in for one (an empty string, say) would be a value every reader
        -- of the table — this module, a future import, an operator with the
        -- sqlite3 shell — would have to know means 'nothing'. SQLite can raise
        -- a column's shape with ADD COLUMN but never lower it, so the shape
        -- change is a rebuild.
        --
        -- ## Why the replies are copied out before anything is dropped
        --
        -- `comments.thread_id` references `comment_threads (id) ON DELETE
        -- CASCADE`, and `foreign_keys` is ON for the life of the connection. A
        -- DROP TABLE on the parent performs an implicit DELETE FROM, and that
        -- fires the child's cascade: dropping `comment_threads` destroys every
        -- reply in the database, not only the threads. So the replies are copied
        -- out first and put back once the tables have their new shape.
        --
        -- Two things about that copy are load-bearing, and both are here because
        -- they were MEASURED, not reasoned about — both fail silently, with the
        -- migration committing and the comments simply gone:
        --
        --   * It happens BEFORE either drop. Copied afterwards there is nothing
        --     left to copy: the cascade has already emptied `comments`
        --     (measured: 0 of 3 rows survived).
        --   * It carries NO foreign key (`CREATE TABLE ... AS SELECT`, rather
        --     than a table declared like the original). A copy that kept
        --     `thread_id`'s reference is a second child of the table about to be
        --     dropped, and the same cascade empties it (measured: 0 of 3).
        --
        -- Which of the two DROPs comes first changes neither number — the child
        -- goes first because that is how a pair is dropped — so the ordering
        -- above is not what saves the rows; the copy is. The test that catches
        -- both mistakes counts the comments before and after rather than
        -- trusting the threads to stand for them:
        -- `document_db::tests::` +
        -- `a_database_of_element_anchored_threads_is_rebuilt_without_losing_a_comment`
        -- (in `document_db_tests`).
        --
        -- The recipe in the SQLite manual — `PRAGMA foreign_keys = OFF` around
        -- the rebuild — is not available here: a PRAGMA is a no-op inside a
        -- transaction, and `migrate` runs every step inside one. A crash between
        -- two statements of a rebuild is how a database ends up with its
        -- replies in a scratch table nobody reads, so the single atomic
        -- transaction is worth more than the recipe.
        CREATE TABLE comment_threads_v3 (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            -- The conversation goes with the document it is about, exactly as
            -- migration 2 stated it.
            document_key     TEXT NOT NULL REFERENCES documents (key) ON DELETE CASCADE,
            -- Where the pin sits. NULL for a thread written before pins were
            -- coordinates: see the copy below, which has nothing to put here.
            --
            -- `page_id` because the coordinates are relative to a page: two
            -- pages both have a point (100, 100), and without the page a pin on
            -- one would be indistinguishable from a pin on the other. Not a
            -- foreign key, for the reason `node_id` was not one: pages live
            -- inside the `.op` file, so this is a name the client and the
            -- document agree on and not a row here.
            page_id          TEXT,
            -- Page coordinates, in the page's own system: the one the document
            -- is authored in, before the viewport's pan and zoom are applied.
            -- A pin must not move relative to the artwork when the canvas is
            -- zoomed or scrolled, and screen coordinates would do exactly that.
            --
            -- REAL, not INTEGER: the editor's geometry is f32 throughout
            -- (`Node` positions, the viewport transform), and a click at zoom
            -- 2.5 lands between document pixels. Rounding to whole ones moves
            -- the pin by up to half a DOCUMENT pixel — 1.25 screen pixels at
            -- that zoom, and the error grows with the zoom, since a document
            -- pixel is drawn wider — a pin that appears beside the cursor that
            -- placed it, and two pins a hair apart collapsing onto one point.
            -- A coordinate is a measurement, not a count.
            x                REAL,
            y                REAL,
            -- What an older thread was pinned to, when it was pinned to an
            -- element. Kept rather than deleted: it is the only surviving
            -- record of what those comments were about, and the schema's own
            -- stance (a resolved thread is closed, not deleted) is that the
            -- record of a review is worth more than the space it takes. NULL
            -- for every thread this build opens — a new pin is coordinates and
            -- carries no element at all — and nothing decides a position from
            -- it.
            anchor_hint      TEXT,
            created_at       INTEGER NOT NULL,
            -- Closed, not deleted: a resolved thread is the record of what was
            -- asked and answered.
            resolved         INTEGER NOT NULL DEFAULT 0,
            resolved_at      INTEGER,
            resolved_by      TEXT,
            resolved_by_name TEXT,
            -- A pin is all three or none: a page with no point, or a point with
            -- no page, is not a place anything can be drawn and would leave a
            -- client holding a thread it cannot show. Refused by the schema
            -- rather than by the reader, so a row written by anything at all —
            -- a hand repair, a future import — cannot be half a pin either.
            CHECK ((page_id IS NULL AND x IS NULL AND y IS NULL)
                OR (page_id IS NOT NULL AND x IS NOT NULL AND y IS NOT NULL))
        );

        -- Every thread survives, with everything it knew and no coordinates.
        -- Filling x/y with a guess (the node's position, the origin) is
        -- deliberately not done: the position of a node in migration 2's world
        -- was never recorded, and the origin is a place somebody did not point
        -- at. A pin that would be a lie is worse than a thread that says it has
        -- none.
        INSERT INTO comment_threads_v3
               (id, document_key, page_id, x, y, anchor_hint, created_at,
                resolved, resolved_at, resolved_by, resolved_by_name)
        SELECT  id, document_key, NULL, NULL, NULL, node_id, created_at,
                resolved, resolved_at, resolved_by, resolved_by_name
          FROM comment_threads;

        CREATE TABLE comments_saved AS SELECT * FROM comments;
        DROP TABLE comments;
        DROP TABLE comment_threads;
        ALTER TABLE comment_threads_v3 RENAME TO comment_threads;

        -- Migration 2's `comments` table again, column for column. It is
        -- created rather than left alone because the rows it held could not
        -- survive the drop above: this is the copy taken before that drop, put
        -- back where it was. A later migration that changes `comments` changes
        -- it from here.
        CREATE TABLE comments (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id   INTEGER NOT NULL REFERENCES comment_threads (id) ON DELETE CASCADE,
            author_role TEXT,
            -- NULL is the local operator, who has no account.
            author_id   TEXT,
            author_name TEXT NOT NULL,
            body        TEXT NOT NULL,
            created_at  INTEGER NOT NULL
        );
        INSERT INTO comments (id, thread_id, author_role, author_id, author_name, body, created_at)
        SELECT id, thread_id, author_role, author_id, author_name, body, created_at
          FROM comments_saved;
        DROP TABLE comments_saved;

        -- The two indexes again, under the same names, now that the tables that
        -- carried them are gone. They are what the reads use and what the
        -- cascade from `documents` finds its children by, so a database without
        -- them would be slower rather than wrong — which is exactly the kind of
        -- difference nobody notices until a file list is large.
        CREATE INDEX comment_threads_by_document
            ON comment_threads (document_key, created_at, id);
        CREATE INDEX comments_by_thread ON comments (thread_id, created_at, id);
    ",
    },
];
