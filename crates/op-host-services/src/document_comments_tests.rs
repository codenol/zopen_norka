//! The conversation tables: what `document_comments` writes, reads back, and
//! what the schema does when the document behind a thread goes away.
//!
//! The routes are pinned in `web_canvas_server::comment_routes_tests`; these
//! drive the storage layer directly, including the paths a running daemon would
//! never take on purpose — a thread with no comments, an id that has to be
//! reached through the wrong document, a document that vanishes under a
//! conversation.
//!
//! A thread is placed at a point on a page (see [`Placement`]) rather than
//! anchored to an element, so the pins here are coordinates. The element-typed
//! assertions this file used to make are not gone, they are read differently:
//! where a test said "the thread is on n1" it now says "the thread is at
//! (x, y) on p1", and what used to be `node_id` is the `anchor_hint` a migrated
//! thread carries — a record with a test of its own
//! ([`a_thread_without_coordinates_still_reads_as_a_thread`]).

use super::*;
use crate::document_store::DocumentEntry;
use crate::document_test_dir::TempDir;

/// A stored document, without a file: these tests are about rows.
fn seed_document(db: &DocumentDb, key: &str, name: &str) {
    crate::document_db::insert_entry(
        db,
        &DocumentEntry {
            key: key.to_string(),
            name: name.to_string(),
            // The local operator's own row, which is what the store writes when
            // a deployment has no accounts.
            owner_id: None,
            created_at: 1,
            updated_at: 1,
            size: 0,
            has_thumbnail: false,
        },
    )
    .expect("insert a document");
}

/// A pin at a point on a page.
fn pin(page_id: &str, x: f64, y: f64) -> Placement {
    Placement {
        page_id: page_id.to_string(),
        x,
        y,
    }
}

/// A comment about to be written.
fn comment<'a>(id: Option<&'a str>, name: &'a str, body: &'a str) -> NewComment<'a> {
    NewComment {
        author: Author { id, name, role: None },
        body,
    }
}

/// How many rows a table holds, read straight from the database.
///
/// What a cascade removed is not visible through this module's own API, which
/// by then answers "no such document" — and "the rows are gone" is the property
/// being pinned, not "the query says so".
fn count_rows(db: &DocumentDb, table: &str) -> i64 {
    db.conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count")
}

#[test]
fn a_thread_is_created_with_its_first_comment_and_reads_back_whole() {
    let dir = TempDir::new("comments-create");
    let db = dir.open();
    seed_document(&db, "aaaaaaaa00000001", "Work");

    let created = create_thread(
        &db,
        "aaaaaaaa00000001",
        pin("page-1", 120.5, -40.25),
        comment(Some("userA"), "Anya", "Fix the padding here"),
        100,
    )
    .expect("create")
    .expect("a document carries the key");
    assert_eq!(
        created.placement,
        Some(pin("page-1", 120.5, -40.25)),
        "the pin is where it was put, on the page it named"
    );
    assert_eq!(
        created.anchor_hint, None,
        "a comment placed now points at no element"
    );
    assert_eq!(created.created_at, 100);
    assert!(!created.resolved, "a new thread is open");
    assert_eq!(created.resolved_at, None);
    assert_eq!(created.resolved_by, None);
    assert_eq!(created.comments.len(), 1, "a thread is never empty");
    assert_eq!(created.comments[0].body, "Fix the padding here");
    assert_eq!(created.comments[0].author_id.as_deref(), Some("userA"));
    assert_eq!(created.comments[0].author_name, "Anya");

    // What a create answers and what a list answers are the same record: a
    // client must not have to reconcile two shapes of one thread.
    let listed = list_threads(&db, "aaaaaaaa00000001")
        .expect("list")
        .expect("a document carries the key");
    assert_eq!(listed, vec![created]);
}

#[test]
fn a_pin_is_read_back_exactly_as_it_was_written() {
    // Coordinates are stored as REAL and carried as f64 for a reason (see
    // `Placement`): the editor's geometry is fractional, and a pin that came
    // back on a rounded pixel would sit visibly beside the point somebody
    // clicked. The values here are the ones a rounding step would move — a
    // fractional part well past what an integer column keeps, a negative, and
    // the origin — and they are asserted equal, not approximately equal.
    let dir = TempDir::new("comments-pin-precision");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");

    for (page_id, x, y) in [
        ("page-1", 1234.567_890_123_4, -0.125),
        ("page-1", 0.0, 0.0),
        // The page is part of the address: the same numbers on another page
        // are another pin.
        ("page-2", 1234.567_890_123_4, -0.125),
    ] {
        create_thread(&db, key, pin(page_id, x, y), comment(None, "", "here"), 10)
            .expect("create")
            .expect("document");
    }

    let placements: Vec<Option<Placement>> = list_threads(&db, key)
        .expect("list")
        .expect("document")
        .into_iter()
        .map(|thread| thread.placement)
        .collect();
    assert_eq!(
        placements,
        vec![
            Some(pin("page-1", 1234.567_890_123_4, -0.125)),
            Some(pin("page-1", 0.0, 0.0)),
            Some(pin("page-2", 1234.567_890_123_4, -0.125)),
        ],
        "and the third is not the first: a pin is page and point together"
    );
}

#[test]
fn the_order_is_creation_time_and_then_the_order_things_were_written() {
    // Timestamps are seconds, so a thread opened and answered inside one second
    // is the ordinary case rather than a corner: the rowid is what decides, and
    // this pins that it does.
    let dir = TempDir::new("comments-order");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");

    let first = create_thread(&db, key, pin("p", 1.0, 1.0), comment(None, "", "one"), 500)
        .expect("create")
        .expect("document");
    let second = create_thread(&db, key, pin("p", 2.0, 2.0), comment(None, "", "two"), 500)
        .expect("create")
        .expect("document");
    // Written last, but created first: the timestamp wins over the insert order.
    let oldest = create_thread(&db, key, pin("p", 3.0, 3.0), comment(None, "", "three"), 400)
        .expect("create")
        .expect("document");
    assert!(oldest.id > second.id, "ids are handed out in write order");

    let ids: Vec<i64> = list_threads(&db, key)
        .expect("list")
        .expect("document")
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    assert_eq!(ids, vec![oldest.id, first.id, second.id]);

    // Replies inside one thread follow the same rule.
    add_reply(&db, key, first.id, comment(None, "", "answer"), 500).expect("reply");
    add_reply(&db, key, first.id, comment(None, "", "another"), 500).expect("reply");
    let thread = list_threads(&db, key)
        .expect("list")
        .expect("document")
        .into_iter()
        .find(|thread| thread.id == first.id)
        .expect("the thread");
    let bodies: Vec<&str> = thread
        .comments
        .iter()
        .map(|comment| comment.body.as_str())
        .collect();
    assert_eq!(bodies, vec!["one", "answer", "another"]);
}

#[test]
fn a_reply_joins_its_thread_and_the_author_name_is_a_snapshot() {
    let dir = TempDir::new("comments-reply");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");
    let thread = create_thread(
        &db,
        key,
        pin("page-1", 10.0, 20.0),
        comment(Some("userA"), "Anya", "first"),
        10,
    )
    .expect("create")
    .expect("document");

    let answered = add_reply(
        &db,
        key,
        thread.id,
        comment(Some("userB"), "Boris", "second"),
        11,
    )
    .expect("reply")
    .expect("the thread is there");
    assert_eq!(answered.comments.len(), 2);
    assert_eq!(answered.comments[1].author_name, "Boris");
    // Answering does not move the pin: the thread is the same place, whatever is
    // said in it.
    assert_eq!(answered.placement, thread.placement);

    // Somebody renamed: the comment they already wrote still reads as the name
    // they had when they wrote it, which is the whole reason the name is stored
    // beside the id rather than looked up.
    add_reply(
        &db,
        key,
        thread.id,
        comment(Some("userA"), "Anya Petrova", "third"),
        12,
    )
    .expect("reply")
    .expect("the thread is there");
    let thread = list_threads(&db, key)
        .expect("list")
        .expect("document")
        .pop()
        .expect("the thread");
    assert_eq!(thread.comments[0].author_name, "Anya");
    assert_eq!(thread.comments[2].author_name, "Anya Petrova");
    assert!(thread
        .comments
        .iter()
        .all(|comment| comment.author_id.as_deref() == Some("userA")
            || comment.author_id.as_deref() == Some("userB")));
}

#[test]
fn resolving_records_who_closed_the_thread_and_reopening_clears_it() {
    let dir = TempDir::new("comments-resolve");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");
    let thread = create_thread(
        &db,
        key,
        pin("page-1", 5.0, 6.0),
        comment(Some("userA"), "Anya", "hello"),
        10,
    )
    .expect("create")
    .expect("document");

    let closed = set_resolved(
        &db,
        key,
        thread.id,
        ResolutionChange::Resolve {
            author: Author {
                id: Some("userB"),
                name: "Boris",
            role: None,
            },
            at: 900,
        },
    )
    .expect("resolve")
    .expect("the thread is there");
    assert!(closed.resolved);
    assert_eq!(closed.resolved_at, Some(900));
    assert_eq!(closed.resolved_by.as_deref(), Some("userB"));
    assert_eq!(closed.resolved_by_name.as_deref(), Some("Boris"));
    // Closing a thread is not a way to lose what was said in it.
    assert_eq!(closed.comments.len(), 1);
    // Nor a way to lose where it was: a closed comment is still a comment
    // somebody has to be able to find on the canvas.
    assert_eq!(closed.placement, thread.placement);

    // Closing it again moves the stamp to whoever closed it now: the field
    // answers "who closed this", and the last person to do so is the answer.
    let again = set_resolved(
        &db,
        key,
        thread.id,
        ResolutionChange::Resolve {
            author: Author {
                id: Some("userC"),
                name: "Vera",
            role: None,
            },
            at: 950,
        },
    )
    .expect("resolve")
    .expect("the thread is there");
    assert!(again.resolved);
    assert_eq!(again.resolved_by.as_deref(), Some("userC"));
    assert_eq!(again.resolved_at, Some(950));

    let reopened = set_resolved(&db, key, thread.id, ResolutionChange::Reopen)
        .expect("reopen")
        .expect("the thread is there");
    assert!(!reopened.resolved);
    assert_eq!(
        reopened.resolved_at, None,
        "an open thread has no resolver, and leaving the last one behind would \
         read as closed in any client that looks only at resolved_by"
    );
    assert_eq!(reopened.resolved_by, None);
    assert_eq!(reopened.resolved_by_name, None);

    // Reopening an open thread is not an error: a client that retries must not
    // get a different answer the second time.
    assert_eq!(
        set_resolved(&db, key, thread.id, ResolutionChange::Reopen)
            .expect("reopen twice")
            .map(|thread| thread.resolved),
        Some(false)
    );
}

#[test]
fn a_thread_is_reached_only_through_its_own_document() {
    // The key is the authorization the route already made. A thread id is a
    // number in a URL, and a caller who may reach one document must not be able
    // to write into another's conversation by quoting one.
    let dir = TempDir::new("comments-scope");
    let db = dir.open();
    let (mine, theirs) = ("aaaaaaaa00000001", "aaaaaaaa00000002");
    seed_document(&db, mine, "Mine");
    seed_document(&db, theirs, "Theirs");
    let thread = create_thread(
        &db,
        mine,
        pin("page-1", 1.0, 2.0),
        comment(Some("userA"), "Anya", "hello"),
        10,
    )
    .expect("create")
    .expect("document");

    assert_eq!(
        add_reply(&db, theirs, thread.id, comment(None, "", "trespass"), 11).expect("reply"),
        None,
        "a reply cannot land in another document's thread"
    );
    assert_eq!(
        set_resolved(&db, theirs, thread.id, ResolutionChange::Reopen).expect("resolve"),
        None
    );
    assert_eq!(
        thread_author(&db, theirs, thread.id).expect("author"),
        ThreadAuthor::NoSuchThread
    );
    assert!(
        list_threads(&db, theirs)
            .expect("list")
            .expect("document")
            .is_empty(),
        "and the other document's list stays empty"
    );

    // The thread itself is untouched by any of it.
    let untouched = list_threads(&db, mine)
        .expect("list")
        .expect("document")
        .pop()
        .expect("the thread");
    assert_eq!(untouched.comments.len(), 1);
    assert!(!untouched.resolved);
    assert_eq!(untouched.placement, Some(pin("page-1", 1.0, 2.0)));
    assert_eq!(
        thread_author(&db, mine, thread.id).expect("author"),
        ThreadAuthor::Account("userA".to_string())
    );
}

#[test]
fn a_thread_cannot_be_opened_on_a_document_the_store_does_not_have() {
    // The foreign key is what says a conversation belongs to a stored document;
    // the insert asks for the row itself (`INSERT ... SELECT`) so that a key
    // naming no document is one answer — None — rather than a constraint
    // violation surfacing as a 500. The pin is written by the same statement,
    // so a refused key leaves no coordinates behind either.
    let dir = TempDir::new("comments-no-document");
    let db = dir.open();
    let key = "aaaaaaaa00000009";

    assert_eq!(
        create_thread(&db, key, pin("page-1", 1.0, 1.0), comment(None, "", "hello"), 10)
            .expect("create"),
        None
    );
    assert_eq!(count_rows(&db, "comment_threads"), 0);
    assert_eq!(count_rows(&db, "comments"), 0);
    assert_eq!(list_threads(&db, key).expect("list"), None);
}

#[test]
fn deleting_a_document_takes_its_threads_and_their_comments_with_it() {
    // Proves the cascade reaches two levels down: the document's threads go
    // through one foreign key, and each thread's comments through the next.
    // Without `foreign_keys=ON` — or without SQLite cascading further from a
    // cascaded delete — the rows would survive the document they describe. This
    // is the property migration 3 had to keep while it rebuilt both tables, and
    // the reason it drops the child before the parent.
    let dir = TempDir::new("comments-cascade");
    let db = dir.open();
    let (mine, theirs) = ("aaaaaaaa00000001", "aaaaaaaa00000002");
    seed_document(&db, mine, "Mine");
    seed_document(&db, theirs, "Theirs");
    let mine_thread = create_thread(
        &db,
        mine,
        pin("page-1", 1.0, 1.0),
        comment(None, "", "mine"),
        10,
    )
    .expect("create")
    .expect("document");
    add_reply(&db, mine, mine_thread.id, comment(None, "", "more"), 11).expect("reply");
    create_thread(
        &db,
        theirs,
        pin("page-1", 2.0, 2.0),
        comment(None, "", "theirs"),
        10,
    )
    .expect("create")
    .expect("document");
    assert_eq!(count_rows(&db, "comment_threads"), 2);
    assert_eq!(count_rows(&db, "comments"), 3);

    assert!(crate::document_db::delete_entry(&db, mine).expect("delete"));

    assert_eq!(
        count_rows(&db, "comment_threads"),
        1,
        "only the other document's thread is left"
    );
    assert_eq!(count_rows(&db, "comments"), 1);
    assert_eq!(list_threads(&db, mine).expect("list"), None);
    assert_eq!(
        list_threads(&db, theirs)
            .expect("list")
            .expect("document")
            .len(),
        1,
        "and the document nobody deleted keeps its conversation"
    );
}

#[test]
fn the_local_operator_is_recorded_with_no_account_and_no_invented_name() {
    // A deployment with no accounts has nobody to name. The record says so
    // rather than guessing: an empty name is the client's to label, and a name
    // this server made up would be indistinguishable from a real one.
    let dir = TempDir::new("comments-local");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");
    let thread = create_thread(
        &db,
        key,
        pin("page-1", 3.5, 4.5),
        comment(None, "", "local"),
        10,
    )
    .expect("create")
    .expect("document");
    assert_eq!(thread.comments[0].author_id, None);
    assert_eq!(thread.comments[0].author_name, "");
    assert_eq!(
        thread_author(&db, key, thread.id).expect("author"),
        ThreadAuthor::LocalOperator,
        "and the resolver's rule can tell the two apart"
    );
}

#[test]
fn a_thread_without_comments_is_still_listed() {
    // Nothing in this module writes one — a thread is created with its first
    // comment — but the read is a LEFT JOIN on purpose: a row that arrived from
    // outside this code must not take a whole thread out of the conversation
    // silently, which is how a comment becomes invisible rather than visibly
    // broken.
    let dir = TempDir::new("comments-empty-thread");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");
    db.conn()
        .execute(
            "INSERT INTO comment_threads (document_key, page_id, x, y, created_at, resolved)
             VALUES (?1, 'page-9', 7.5, 8.5, 7, 0)",
            params![key],
        )
        .expect("insert a thread by hand");

    let threads = list_threads(&db, key).expect("list").expect("document");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].placement, Some(pin("page-9", 7.5, 8.5)));
    assert!(threads[0].comments.is_empty());
}

#[test]
fn a_thread_without_coordinates_still_reads_as_a_thread() {
    // The shape migration 3 leaves behind: a thread that was anchored to an
    // element under migration 2, carried over with its hint and with no
    // coordinates. It must still be LISTED, with its conversation intact — that
    // is the whole reason the migration keeps the rows instead of dropping the
    // ones it cannot place — and it must read as having no pin rather than as
    // having one this build made up.
    let dir = TempDir::new("comments-no-pin");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");
    db.conn()
        .execute(
            "INSERT INTO comment_threads (document_key, anchor_hint, created_at, resolved)
             VALUES (?1, 'n7', 7, 1)",
            params![key],
        )
        .expect("insert a thread in the migrated shape");
    db.conn()
        .execute(
            "INSERT INTO comments (thread_id, author_id, author_name, body, created_at)
             SELECT id, 'userA', 'Anya', 'written before pins were coordinates', 7
               FROM comment_threads WHERE document_key = ?1",
            params![key],
        )
        .expect("insert its comment");

    let threads = list_threads(&db, key).expect("list").expect("document");
    assert_eq!(threads.len(), 1);
    assert_eq!(
        threads[0].placement, None,
        "there is no point to draw, and none is invented for it"
    );
    assert_eq!(
        threads[0].anchor_hint.as_deref(),
        Some("n7"),
        "the element it used to point at is what is left to say about it"
    );
    assert!(threads[0].resolved, "and the rest of the record is untouched");
    assert_eq!(threads[0].comments.len(), 1);
    assert_eq!(threads[0].comments[0].body, "written before pins were coordinates");
}

#[test]
fn the_schema_refuses_half_a_pin() {
    // Migration 3's CHECK. A page with no point, or a point with no page, is
    // not a place anything can be drawn, and the reader folds such a row into
    // "no pin" (see `thread_row`) — a silent answer for a row that should never
    // exist. The constraint is what makes that unreachable for writes from
    // anywhere, including the ones that do not come through this module.
    let dir = TempDir::new("comments-half-pin");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");

    let page_only = db.conn().execute(
        "INSERT INTO comment_threads (document_key, page_id, created_at, resolved)
         VALUES (?1, 'page-1', 7, 0)",
        params![key],
    );
    assert!(
        page_only.is_err(),
        "a page with no coordinates is not a pin: {page_only:?}"
    );

    let point_only = db.conn().execute(
        "INSERT INTO comment_threads (document_key, x, y, created_at, resolved)
         VALUES (?1, 1.0, 2.0, 7, 0)",
        params![key],
    );
    assert!(
        point_only.is_err(),
        "coordinates with no page are not a pin either: {point_only:?}"
    );

    assert_eq!(count_rows(&db, "comment_threads"), 0);
}

#[test]
fn a_thread_id_is_never_handed_out_twice() {
    // AUTOINCREMENT, and the reason it is there: a client may be holding an id
    // (in a link, in an open panel). If ids were reused, that id would come back
    // naming somebody else's conversation — a reply meant for a deleted thread
    // would land in a new one. Reuse is what makes a stale id dangerous; a
    // monotonic one makes it answer "no such thread".
    let dir = TempDir::new("comments-id-reuse");
    let db = dir.open();
    let key = "aaaaaaaa00000001";
    seed_document(&db, key, "Work");
    let first = create_thread(
        &db,
        key,
        pin("page-1", 1.0, 1.0),
        comment(None, "", "first"),
        10,
    )
    .expect("create")
    .expect("document");

    // The highest row goes away with its document, which is exactly when SQLite
    // would reuse the id without AUTOINCREMENT.
    crate::document_db::delete_entry(&db, key).expect("delete");
    seed_document(&db, key, "Work again");
    let second = create_thread(
        &db,
        key,
        pin("page-1", 2.0, 2.0),
        comment(None, "", "second"),
        20,
    )
    .expect("create")
    .expect("document");
    assert!(
        second.id > first.id,
        "a new thread must not take the id of a deleted one: {} vs {}",
        second.id,
        first.id
    );
}

#[test]
fn the_authors_role_is_a_snapshot_beside_the_name() {
    // The colour a comment is drawn in comes from here, so the role is
    // recorded rather than looked up: a role read live would repaint old
    // comments whenever someone's roles changed, and the colour is meant to
    // say who was speaking *then*.
    let dir = TempDir::new("comments-author-role");
    let db = dir.open();
    seed_document(&db, "aaaaaaaa00000002", "Work");

    create_thread(
        &db,
        "aaaaaaaa00000002",
        pin("page-1", 1.0, 1.0),
        NewComment {
            author: Author {
                id: Some("userA"),
                name: "Ada",
                role: Some("ux_ui"),
            },
            body: "first",
        },
        100,
    )
    .expect("create")
    .expect("a document carries the key");

    let threads = list_threads(&db, "aaaaaaaa00000002")
        .expect("list")
        .expect("a document carries the key");
    assert_eq!(threads[0].comments[0].author_role.as_deref(), Some("ux_ui"));

    // A local operator has no role, and none is invented for them.
    seed_document(&db, "aaaaaaaa00000003", "Elsewhere");
    create_thread(
        &db,
        "aaaaaaaa00000003",
        pin("page-1", 1.0, 1.0),
        comment(None, "", "local"),
        100,
    )
    .expect("create")
    .expect("a document carries the key");
    let threads = list_threads(&db, "aaaaaaaa00000003")
        .expect("list")
        .expect("a document carries the key");
    assert_eq!(threads[0].comments[0].author_role, None);
}
