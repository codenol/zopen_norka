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
//!
//! ## A rebuilt table is declared twice, and exactly one copy is live
//!
//! A rebuild has to write the child's DDL a second time — migration 3's rebuild
//! of `comment_threads` could not drop the parent without first copying
//! `comments` out, and the copy has to be put back somewhere — so two migrations
//! can both declare the same table. Both copies are needed and neither is
//! removable: the earlier one is what a database stopped at that version has,
//! and the later one is what the rebuild reads.
//!
//! **The declaration in the LAST migration that declares a table is the one in
//! force**: that is the copy every database at the newest version has, whether
//! it was created fresh or carried there by the rebuild, and it is the copy a
//! reader of `sqlite_master` finds. Every earlier declaration is history.
//! Migration 3 says so above the copy of `comments` it writes, and
//! `document_db_schema_tests` holds it to that (issue #57).
//!
//! What follows for a CHANGE to such a table is the ordinary rule and not an
//! exception to it: it goes in a NEW migration. Editing the live declaration is
//! right for a fresh database and for anyone below that version, and reaches
//! nobody who has already run it — which is every deployment once the declaring
//! release has shipped. The live declaration is where the table's shape is READ
//! from; it is never where it is changed.

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

        -- A database at the newest version does NOT have this `comments`: the
        -- rebuild in migration 3 drops it and writes its own copy back. What is
        -- here is history — keep it exactly as it is, because it is what a
        -- database stopped at v2 has, and because that rebuild reads it. Which
        -- declaration is live is stated above migration 3's copy.
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
        -- back where it was.
        --
        -- ## This is the LIVE declaration of `comments`
        --
        -- Migration 2 declares the same table, and this copy is the one in
        -- force: a database at the newest version has THIS definition, whether
        -- it was created fresh or carried here by the rebuild above, and this
        -- is the text a reader of `sqlite_master` finds. Migration 2's copy is
        -- history — what a database stopped at v2 has, and what the copy three
        -- statements up read (issue #57: two identical declarations with
        -- nothing to tell them apart).
        --
        -- The shape is read from here, and a CHANGE still belongs in a new
        -- migration: editing this text would reach nobody who has already run
        -- it, which is the same reason no other step in this list is edited.
        -- If a later migration has to rebuild this table, ITS copy becomes the
        -- live one and this comment moves there with it.
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
    Migration {
        version: 4,
        sql: "
        -- Sections (#59): the analytics a section was built from, and the
        -- properties that hang off it.
        --
        -- The section itself is NOT here. It is a frame in the `.op` file — a
        -- frame whose `role` is `section` — so it travels with the screens it
        -- groups, survives copy-paste and is visible on the canvas. What a
        -- database can hold and a file cannot is the ASSET: one markdown
        -- document, loaded once and referenced by any number of sections.
        --
        -- ## Why the analytics is a table of its own
        --
        -- An analytics document is not a document of this store, and it is not
        -- a property of one. The operator's decision is that it is an asset —
        -- like a component or a recipe — so it is owned by an ACCOUNT, it is
        -- referenced from more than one place, and deleting a section must not
        -- delete it. Giving it a row on `documents` would have made it a file
        -- with a name in the file list, which is not what it is.
        --
        -- The markdown itself is a file beside the `.op` files (`analytics/
        -- <key>.md`), for the same reason the `.op` files are: the point of
        -- analytics is that a person who is not in the app can read it, which
        -- means a real file with a real format and not a column. This table is
        -- the accounting — who it belongs to, what it is called, when it
        -- changed — exactly the split `documents` already makes.
        CREATE TABLE analytics_assets (
            key        TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            -- NULL means 'not attributed to an account': the local operator's
            -- own assets, and the same meaning this column carries on
            -- `documents.owner_id`. An unattributed asset is nobody's online,
            -- which is the fail-closed direction.
            owner_id   TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            size       INTEGER NOT NULL
        );

        -- The asset list is a recency list, like the file list, and an index in
        -- its own order makes the ORDER BY a scan instead of a sort.
        CREATE INDEX analytics_assets_by_recency
            ON analytics_assets (updated_at DESC, key DESC);

        -- The list of one account's assets. One directory holds every
        -- account's, so 'list everything' is the statement that would show one
        -- account another's work — the same reason `documents` has its owner
        -- filter.
        CREATE INDEX analytics_assets_by_owner
            ON analytics_assets (owner_id, updated_at DESC, key DESC);

        -- ## Why a section's properties are one JSON payload
        --
        -- A row per section, keyed by the section's address: the document it
        -- lives in and the node id that identifies it inside that document.
        -- The same shape, and the same reason, as a comment thread: nodes live
        -- inside the `.op` file, so a node id is a name the client and the
        -- document agree on rather than a row here.
        --
        -- The payload holds the analytics references and their fingerprints,
        -- the summary and the UX flows. It is JSON and not columns because it
        -- is ONE authored document whose parts are read and written together,
        -- and because the flow graph inside it will grow: a column per step
        -- would freeze a structure that is still being designed into SQL, and
        -- every change to the model would be a migration. It carries its own
        -- `format` number (`op_editor_core::section::
        -- SECTION_PROPERTIES_FORMAT`), so a build that does not understand it
        -- refuses it instead of reading half of somebody's work.
        --
        -- ## Why the cascade, and why no index of its own
        --
        -- The properties go with the document, exactly as the conversation
        -- does: a deleted document must not leave rows behind for whoever takes
        -- its key next. `PRIMARY KEY (document_key, node_id)` is also what the
        -- cascade from `documents` finds its children by — SQLite scans the
        -- leading column — so a second index on `document_key` would be a write
        -- cost with no reader.
        CREATE TABLE section_properties (
            document_key TEXT NOT NULL REFERENCES documents (key) ON DELETE CASCADE,
            node_id      TEXT NOT NULL,
            properties   TEXT NOT NULL,
            updated_at   INTEGER NOT NULL,
            PRIMARY KEY (document_key, node_id)
        );
    ",
    },
];
