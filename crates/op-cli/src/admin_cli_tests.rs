//! What `op admin create` asks, and what it does with the answers.
//!
//! The terminal is the only part not exercised here: the flow takes its input,
//! its output and its password reads as arguments, so what is under test is the
//! conversation — which questions come in which order, what a weak password
//! does to it, and that no path prints the secret.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use super::*;
use op_host_services::accounts::{NewUser, UserStatus};

/// A directory that deletes itself, and the store inside it.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "op-admin-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(tag: &str) -> (TempDir, AccountsDb) {
    let dir = TempDir::new(tag);
    let db = AccountsDb::open(dir.path()).expect("open the account store");
    (dir, db)
}

/// Answers typed at the prompts, and everything the command printed.
struct Session {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Session {
    fn typing(text: &str) -> Self {
        Self {
            input: Cursor::new(text.as_bytes().to_vec()),
            output: Vec::new(),
        }
    }
}

/// Drive the flow with `typing` on stdin and `secrets` handed back by the
/// password reader, and return the outcome with everything printed.
fn drive(db: &AccountsDb, typing: &str, secrets: &[&str]) -> (Result<String, CliError>, String) {
    let mut session = Session::typing(typing);
    let mut secrets = secrets.iter().map(|secret| secret.to_string());
    let mut read_secret = move || {
        secrets
            .next()
            .ok_or_else(|| CliError::usage("no input on stdin: `op admin create` is interactive"))
    };
    let outcome = create_admin(
        &db.clone(),
        &mut session.input,
        &mut session.output,
        &mut read_secret,
    );
    (
        outcome,
        String::from_utf8_lossy(&session.output).into_owned(),
    )
}

/// The one assertion every path shares: the password is nowhere in the output.
fn assert_secret_never_printed(outcome: &Result<String, CliError>, printed: &str) {
    let summary = outcome.as_deref().unwrap_or_default();
    let error = outcome
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    for text in [printed, summary, &error] {
        assert!(!text.contains("a-good-long-password"), "{text}");
        assert!(!text.contains("another-good-password"), "{text}");
    }
}

#[test]
fn a_fresh_deployment_gets_its_first_administrator() {
    let (_dir, db) = store("fresh");
    let (outcome, printed) = drive(
        &db,
        "operator\n",
        &["a-good-long-password", "a-good-long-password"],
    );
    let summary = outcome.expect("the admin is created");
    assert_secret_never_printed(&Ok(summary.clone()), &printed);
    assert!(summary.contains("operator"), "{summary}");
    assert!(summary.contains(op_host_services::accounts::FIRST_ADMIN_ROLE));
    // It says which database it wrote, so an operator pointing at the wrong
    // directory learns it from the answer rather than from a sign-in much
    // later.
    assert!(summary.contains("accounts.db"), "{summary}");
    // The name was asked for, and the password twice.
    assert!(printed.contains("Username:"), "{printed}");

    let user = db
        .find_user_by_username("operator")
        .expect("look up")
        .expect("the admin exists");
    assert_eq!(user.status, UserStatus::Active);
    assert!(user.password_hash.is_some());
}

#[test]
fn a_weak_password_is_explained_and_asked_for_again() {
    let (_dir, db) = store("weak");
    let (outcome, printed) = drive(
        &db,
        "operator\n",
        &["short", "a-good-long-password", "a-good-long-password"],
    );
    let summary = outcome.expect("the second attempt is accepted");
    assert_secret_never_printed(&Ok(summary), &printed);
    // The refusal names the rule — including the number it wants — so the
    // person knows what to type differently.
    assert!(printed.contains("12"), "{printed}");
    assert_eq!(db.count_users().expect("count"), 1);
}

#[test]
fn a_password_that_names_the_account_is_refused_by_the_same_policy() {
    let (_dir, db) = store("names-account");
    let (outcome, printed) = drive(&db, "operator\n", &["operator-is-my-password"]);
    // The reader runs out after one password, so the second question ends the
    // conversation — which is the behaviour a script on stdin gets, rather
    // than a loop that never stops.
    assert!(outcome.is_err(), "{outcome:?}");
    assert_secret_never_printed(&outcome, &printed);
    assert!(
        printed.contains("must not contain the account name"),
        "{printed}"
    );
    assert_eq!(db.count_users().expect("count"), 0);
}

#[test]
fn a_mismatch_is_explained_and_asked_for_again() {
    let (_dir, db) = store("mismatch");
    let (outcome, printed) = drive(
        &db,
        "operator\n",
        &[
            "a-good-long-password",
            "a-different-long-one",
            "another-good-password",
            "another-good-password",
        ],
    );
    let summary = outcome.expect("the second pair matches");
    assert_secret_never_printed(&Ok(summary), &printed);
    assert!(printed.contains("do not match"), "{printed}");
    assert_eq!(db.count_users().expect("count"), 1);
}

#[test]
fn a_deployment_that_already_has_accounts_is_refused_without_asking_anything() {
    let (_dir, db) = store("provisioned");
    db.create_user(
        &NewUser::active("colleague", "Colleague", "an-existing-password-1234"),
        op_host_services::accounts::now_secs(),
    )
    .expect("seed an account");

    let mut session = Session::typing("operator\n");
    let mut read_secret = || -> Result<String, CliError> {
        panic!("nothing may be asked of an operator who is about to be refused")
    };
    let error = create_admin(
        &db,
        &mut session.input,
        &mut session.output,
        &mut read_secret,
    )
    .expect_err("the command refuses");
    // The message names the way forward — the whole point of refusing early is
    // that the operator is told what to do instead.
    assert!(error.to_string().contains("invitation"), "{error}");
    assert!(error.to_string().contains("1 account"), "{error}");
    assert_eq!(String::from_utf8_lossy(&session.output), "");
    assert_eq!(db.count_users().expect("count"), 1);
}

#[test]
fn an_empty_username_is_refused_before_a_password_is_asked_for() {
    let (_dir, db) = store("blank-name");
    let mut session = Session::typing("\n");
    let mut read_secret =
        || -> Result<String, CliError> { panic!("a password is not asked for without a name") };
    let error = create_admin(
        &db,
        &mut session.input,
        &mut session.output,
        &mut read_secret,
    )
    .expect_err("a blank name is refused");
    assert!(error.to_string().contains("username"), "{error}");
    assert_eq!(db.count_users().expect("count"), 0);
}

#[test]
fn a_stdin_with_nothing_in_it_says_where_to_go_instead() {
    let (_dir, db) = store("no-stdin");
    let mut session = Session::typing("");
    let mut read_secret =
        || -> Result<String, CliError> { Err(CliError::usage("no input on stdin")) };
    let error = create_admin(
        &db,
        &mut session.input,
        &mut session.output,
        &mut read_secret,
    )
    .expect_err("no input");
    // The container case: the answer names the variables that do the same
    // thing without a terminal.
    assert!(
        error.to_string().contains("NORKA_ADMIN_USERNAME"),
        "{error}"
    );
}

#[test]
fn the_subcommand_maps_and_nothing_else_does() {
    let mut flags = Flags::new();
    flags.insert("data-dir".into(), Some("/srv/norka".into()));
    assert_eq!(
        map_admin(&["create".to_string()], &flags).expect("maps"),
        Command::AdminCreate {
            data_dir: Some("/srv/norka".into())
        }
    );
    // The data directory is not required: the daemon's own variable is the
    // default, and `open_store` is what decides.
    assert_eq!(
        map_admin(&["create".to_string()], &Flags::new()).expect("maps"),
        Command::AdminCreate { data_dir: None }
    );
    assert!(map_admin(&[], &flags).is_err());
    assert!(map_admin(&["destroy".to_string()], &flags).is_err());
}

#[test]
fn a_data_directory_that_cannot_be_opened_is_an_error_and_not_a_new_deployment() {
    // A path whose parent is a FILE cannot be created, so the command says so
    // instead of quietly writing accounts somewhere else.
    let dir = TempDir::new("blocked");
    let blocker = dir.path().join("not-a-directory");
    std::fs::write(&blocker, b"x").expect("write the blocker");
    let error =
        open_store(Some(&blocker.join("accounts").to_string_lossy())).expect_err("cannot open");
    assert!(error.to_string().contains("cannot open"), "{error}");
}
