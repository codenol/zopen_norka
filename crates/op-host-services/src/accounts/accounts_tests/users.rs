//! Creating, finding, listing and changing accounts.

use super::*;
use crate::accounts::{AccountsError, NewUser, UserStatus};

#[test]
fn an_account_with_a_password_is_active_and_stores_a_hash_not_the_password() {
    let (dir, db) = store();

    let user = active_user(&db, "u1", "alice");

    assert_eq!(user.id, "u1");
    assert_eq!(user.username, "alice");
    assert_eq!(user.display_name, "Test Person");
    assert_eq!(user.status, UserStatus::Active);
    assert_eq!(user.hash_algo, HASH_ALGO_ARGON2ID);
    assert_eq!(user.created_at, NOW);
    assert_eq!(user.updated_at, NOW);
    assert_eq!(user.last_seen_at, None);
    assert_eq!(user.email, None);
    assert!(user.has_password());
    assert_ne!(user.password_hash.as_deref(), Some(PASSWORD));
    assert!(
        verify_password(PASSWORD, user.password_hash.as_deref().expect("a hash")).expect("verify")
    );
    assert!(
        !appears_in(&file_bytes(&dir), PASSWORD),
        "the password itself must not be anywhere in the database"
    );
}

#[test]
fn an_invited_account_has_no_password_and_says_it_is_invited() {
    let (_dir, db) = store();

    let user = invited_user(&db, "u1", "bob");

    // The status is not a separate argument a caller could get wrong: an
    // account with no password cannot claim it is active.
    assert_eq!(user.status, UserStatus::Invited);
    assert_eq!(user.password_hash, None);
    assert!(!user.has_password());
    assert_eq!(
        db.create_user(&NewUser::invited("u2", "carol", "Carol"), NOW)
            .expect("create")
            .status,
        UserStatus::Invited
    );
}

#[test]
fn a_username_is_unique_whatever_case_it_is_typed_in() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    assert_eq!(
        db.create_user(&NewUser::active("u2", "ALICE", "Other", PASSWORD), NOW)
            .expect_err("a second account with the same name"),
        AccountsError::UsernameTaken {
            username: "ALICE".to_string()
        },
        "the name that collided is trimmed but not lowercased: it is what the caller wrote"
    );
    // And the row that exists is found by any spelling of it, because the
    // collation is the column's.
    assert!(db.find_user_by_username("ALICE").expect("find").is_some());
    assert!(db
        .find_user_by_username("  alice ")
        .expect("find")
        .is_some());
}

#[test]
fn an_address_is_unique_whatever_case_it_is_typed_in() {
    let (_dir, db) = store();
    db.create_user(
        &NewUser::active("u1", "alice", "Alice", PASSWORD).with_email("Alice@Example.COM"),
        NOW,
    )
    .expect("create");

    assert_eq!(
        db.create_user(
            &NewUser::active("u2", "bob", "Bob", PASSWORD).with_email("alice@example.com"),
            NOW
        )
        .expect_err("a second account with the same address"),
        AccountsError::EmailTaken {
            email: "alice@example.com".to_string()
        }
    );
    assert!(db
        .find_user_by_email("ALICE@EXAMPLE.com")
        .expect("find")
        .is_some());
}

#[test]
fn several_accounts_may_have_no_address_at_all() {
    // SQLite treats NULLs as distinct in a UNIQUE index, which is the only
    // reason an invited account can exist before anybody knows its address. A
    // NOT NULL column with '' standing in for 'unknown' would have made the
    // second such account impossible.
    let (_dir, db) = store();

    invited_user(&db, "u1", "alice");
    invited_user(&db, "u2", "bob");

    assert_eq!(db.count_users().expect("count"), 2);
    assert_eq!(db.find_user_by_email("").expect("find"), None);
}

#[test]
fn an_account_is_found_by_id_by_name_or_by_address() {
    let (_dir, db) = store();
    let user = db
        .create_user(
            &NewUser::active("u1", "alice", "Alice", PASSWORD).with_email("alice@example.com"),
            NOW,
        )
        .expect("create");

    assert_eq!(db.find_user_by_id("u1").expect("by id"), Some(user.clone()));
    assert_eq!(
        db.find_user_by_username("alice").expect("by name"),
        Some(user.clone())
    );
    assert_eq!(
        db.find_user_by_email("alice@example.com")
            .expect("by address"),
        Some(user)
    );
    assert_eq!(db.find_user_by_id("nobody").expect("by id"), None);
    assert_eq!(db.find_user_by_username("nobody").expect("by name"), None);
}

#[test]
fn a_page_of_accounts_is_ordered_and_bounded() {
    let (_dir, db) = store();
    for index in 0..5 {
        db.create_user(
            &NewUser::active(
                &format!("u{index}"),
                &format!("user{index}"),
                "Person",
                PASSWORD,
            ),
            NOW + index as i64,
        )
        .expect("create");
    }

    let first = db.list_users(2, 0).expect("first page");
    let second = db.list_users(2, 2).expect("second page");
    let third = db.list_users(2, 4).expect("third page");

    assert_eq!(
        first
            .iter()
            .map(|user| user.id.as_str())
            .collect::<Vec<_>>(),
        vec!["u4", "u3"],
        "newest first"
    );
    assert_eq!(
        second
            .iter()
            .map(|user| user.id.as_str())
            .collect::<Vec<_>>(),
        vec!["u2", "u1"]
    );
    assert_eq!(
        third
            .iter()
            .map(|user| user.id.as_str())
            .collect::<Vec<_>>(),
        vec!["u0"]
    );
    assert_eq!(db.list_users(0, 0).expect("nothing asked for").len(), 0);
    assert_eq!(
        db.list_users(usize::MAX, 0).expect("everything").len(),
        5,
        "a caller asking for more than the ceiling gets the ceiling, not the table"
    );
    assert_eq!(db.count_users().expect("count"), 5);
}

#[test]
fn a_new_password_replaces_the_old_one_and_the_row_says_when() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    db.set_password("u1", "a-different-secret", NOW + 60)
        .expect("set the password");

    let user = db.find_user_by_id("u1").expect("find").expect("a row");
    assert!(verify_password(
        "a-different-secret",
        user.password_hash.as_deref().expect("hash")
    )
    .expect("verify"));
    assert!(
        !verify_password(PASSWORD, user.password_hash.as_deref().expect("hash")).expect("verify"),
        "the old password must stop working"
    );
    assert_eq!(user.updated_at, NOW + 60);
    assert_eq!(user.created_at, NOW, "the row's history is not rewritten");
    assert_eq!(
        user.status,
        UserStatus::Active,
        "setting a password is not a status change: that belongs to the flow that knows why"
    );
}

#[test]
fn changing_the_password_of_an_account_that_does_not_exist_is_refused() {
    let (_dir, db) = store();

    assert_eq!(
        db.set_password("nobody", PASSWORD, NOW)
            .expect_err("no row"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
}

#[test]
fn a_status_and_a_role_list_are_written_as_given() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    db.set_status("u1", UserStatus::Disabled, NOW + 1)
        .expect("disable");
    db.set_roles("u1", &["editor", "admin", "editor"], NOW + 2)
        .expect("grant");
    let user = db.find_user_by_id("u1").expect("find").expect("a row");

    assert_eq!(user.status, UserStatus::Disabled);
    assert_eq!(
        user.roles,
        vec!["editor".to_string(), "admin".to_string()],
        "the same role twice is one grant, in the order it was written"
    );
    assert!(user.has_role("admin"));
    assert!(!user.has_role("owner"));
    assert_eq!(user.updated_at, NOW + 2);
    // Replacing rather than adding: an empty list is how a role is taken away.
    db.set_roles("u1", &[], NOW + 3).expect("clear");
    assert!(db
        .find_user_by_id("u1")
        .expect("find")
        .expect("a row")
        .roles
        .is_empty());
}

#[test]
fn an_address_can_only_be_verified_when_there_is_one() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    db.create_user(
        &NewUser::active("u2", "bob", "Bob", PASSWORD).with_email("bob@example.com"),
        NOW,
    )
    .expect("create");

    assert_eq!(
        db.mark_email_verified("u1", NOW + 5)
            .expect_err("no address to verify"),
        AccountsError::NoEmail {
            id: "u1".to_string()
        }
    );
    db.mark_email_verified("u2", NOW + 5).expect("verify");
    assert_eq!(
        db.mark_email_verified("nobody", NOW + 5)
            .expect_err("no account"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
    let user = db.find_user_by_id("u2").expect("find").expect("a row");
    assert_eq!(user.email_verified_at, Some(NOW + 5));
    assert_eq!(user.updated_at, NOW + 5);
}

#[test]
fn being_seen_records_when_and_is_not_a_change_to_the_account() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    db.touch_last_seen("u1", NOW + 30).expect("seen");

    let user = db.find_user_by_id("u1").expect("find").expect("a row");
    assert_eq!(user.last_seen_at, Some(NOW + 30));
    assert_eq!(
        user.updated_at, NOW,
        "`updated_at` answers 'when did the record change', and signing in does not"
    );
    assert_eq!(
        db.touch_last_seen("nobody", NOW).expect_err("no row"),
        AccountsError::NoSuchUser {
            id: "nobody".to_string()
        }
    );
}

#[test]
fn deleting_an_account_says_whether_there_was_one() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    assert!(db.delete_user("u1").expect("delete"));
    assert!(!db.delete_user("u1").expect("delete again"));
    assert_eq!(db.find_user_by_id("u1").expect("find"), None);
    assert_eq!(db.count_users().expect("count"), 0);
}

#[test]
fn a_value_that_could_not_be_found_again_is_refused() {
    let (_dir, db) = store();

    // Emptiness and control characters both produce a row that cannot be looked
    // up by the value that was written, which is the one thing a store must not
    // do quietly.
    assert_eq!(
        db.create_user(&NewUser::active("u1", "   ", "Alice", PASSWORD), NOW)
            .expect_err("a blank name"),
        AccountsError::InvalidText {
            field: "username",
            reason: "is empty"
        }
    );
    assert_eq!(
        db.create_user(&NewUser::active("u1", "al\u{0}ice", "Alice", PASSWORD), NOW)
            .expect_err("a control character"),
        AccountsError::InvalidText {
            field: "username",
            reason: "contains a control character"
        }
    );
    assert_eq!(
        db.create_user(&NewUser::active("u1", "alice", "", PASSWORD), NOW)
            .expect_err("a blank display name"),
        AccountsError::InvalidText {
            field: "display_name",
            reason: "is empty"
        }
    );
    assert_eq!(
        db.create_user(&NewUser::active("", "alice", "Alice", PASSWORD), NOW)
            .expect_err("a blank id"),
        AccountsError::InvalidText {
            field: "id",
            reason: "is empty"
        }
    );
    assert_eq!(
        db.create_user(
            &NewUser::active("u1", "alice", "Alice", PASSWORD).with_roles(&["a,b"]),
            NOW
        )
        .expect_err("a comma in a role"),
        AccountsError::InvalidText {
            field: "roles",
            reason: "a role name may not contain a comma"
        }
    );
    assert_eq!(
        db.create_user(
            &NewUser::active("u1", &"a".repeat(65), "Alice", PASSWORD),
            NOW
        )
        .expect_err("a name past the ceiling"),
        AccountsError::InvalidText {
            field: "username",
            reason: "is longer than the column allows"
        }
    );
    assert_eq!(
        db.create_user(&NewUser::active("u1", "alice", "Alice", ""), NOW)
            .expect_err("an empty password"),
        AccountsError::EmptyPassword,
        "an empty password is one that verifies, so it is refused rather than hashed"
    );
    assert_eq!(db.count_users().expect("count"), 0, "nothing was written");
}

#[test]
fn an_account_id_is_the_callers_choice_and_a_duplicate_is_a_database_fault() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");

    // The id is not a name the store interprets, so a collision is not one of
    // the named identity failures — it is the database refusing a write, and it
    // says so with the column it refused.
    let error = db
        .create_user(&NewUser::active("u1", "bob", "Bob", PASSWORD), NOW)
        .expect_err("a duplicate id");
    assert!(
        matches!(error, AccountsError::Database(_)),
        "expected a database fault, got {error:?}"
    );
    assert!(error.to_string().contains("id"), "{error}");
}

#[test]
fn a_trimmed_name_is_what_is_stored() {
    let (_dir, db) = store();

    let user = db
        .create_user(
            &NewUser::active(" u1 ", "  alice  ", "  Alice  ", PASSWORD),
            NOW,
        )
        .expect("create");

    // Trimmed is what makes the round trip through `find_user_by_username`
    // possible; the id keeps its spaces because it is opaque and caller-chosen.
    assert_eq!(user.username, "alice");
    assert_eq!(user.display_name, "Alice");
    assert_eq!(user.id, " u1 ");
}
