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

// ---------------------------------------------------------------------------
// `op admin invite`
// ---------------------------------------------------------------------------

/// The link a run printed, taken the way a script takes it: the last line.
fn printed_link(summary: &str) -> String {
    summary
        .lines()
        .last()
        .expect("the summary is never empty")
        .trim()
        .to_string()
}

/// The token a printed link carries, as the browser shell would read it.
fn printed_token(summary: &str) -> String {
    let link = printed_link(summary);
    let path = link
        .split_once("://")
        .map(|(_, rest)| rest.split_once('/').map(|(_, path)| format!("/{path}")))
        .unwrap_or(Some(link.clone()))
        .expect("a path");
    op_editor_core::route::invite_token(&path)
        .unwrap_or_else(|| panic!("{link} is not a link this product recognises"))
        .to_string()
}

#[test]
fn the_invite_subcommand_maps_and_its_flags_come_through() {
    let mut flags = Flags::new();
    flags.insert("roles".into(), Some("qa,ux_ui".into()));
    flags.insert("email".into(), Some("new@example.com".into()));
    flags.insert("origin".into(), Some("https://canvas.example".into()));
    flags.insert("data-dir".into(), Some("/srv/norka".into()));
    assert_eq!(
        map_admin(&["invite".to_string()], &flags).expect("maps"),
        Command::AdminInvite {
            data_dir: Some("/srv/norka".into()),
            roles: Some("qa,ux_ui".into()),
            email: Some("new@example.com".into()),
            origin: Some("https://canvas.example".into()),
        }
    );
    assert_eq!(
        map_admin(&["invite".to_string()], &Flags::new()).expect("maps"),
        Command::AdminInvite {
            data_dir: None,
            roles: None,
            email: None,
            origin: None,
        }
    );
}

#[test]
fn a_link_printed_by_the_command_is_one_the_acceptance_page_reads() {
    let (_dir, store) = store("invite-link");
    let summary = invite(&store, Some("qa"), None, None, 1_700_000_000).expect("invite");

    // A path, because this command does not know which address the deployment
    // answers on; the token in it is the one the store wrote a hash of.
    let path = printed_link(&summary);
    let token = printed_token(&summary);
    assert_eq!(path, op_editor_core::route::to_invite_path(&token));

    let row = store
        .find_invite(&token)
        .expect("find")
        .expect("the invitation exists");
    assert_eq!(row.roles, vec!["qa".to_string()]);
    assert_eq!(row.created_by, None, "the command line is not an account");
    assert_eq!(row.expires_at, 1_700_000_000 + INVITE_TTL_SECS);
    assert!(row.is_redeemable_at(1_700_000_000));
}

#[test]
fn the_printed_link_is_the_only_place_the_token_exists() {
    // The whole promise of the format: an operator who loses this line has
    // lost the link, and nothing anywhere can produce it again.
    let (dir, store) = store("invite-once");
    let summary = invite(&store, None, None, None, 1_700_000_000).expect("invite");
    let token = printed_token(&summary);

    let listed = store.list_invites(10, 0).expect("list");
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].id.contains(&token));
    // And not in the file either, WAL included.
    let mut bytes = std::fs::read(dir.path().join("accounts.db")).expect("read the database");
    if let Ok(wal) = std::fs::read(dir.path().join("accounts.db-wal")) {
        bytes.extend_from_slice(&wal);
    }
    let needle = token.as_bytes();
    assert!(
        !bytes.windows(needle.len()).any(|part| part == needle),
        "an invitation table that leaked is a list of invitations nobody can accept"
    );
}

#[test]
fn an_origin_is_put_in_front_of_the_link_only_when_one_is_given() {
    let (_dir, store) = store("invite-origin");
    let bare = invite(&store, None, None, None, 1_700_000_000).expect("invite");
    assert!(printed_link(&bare).starts_with("/invite/"), "{bare}");

    // A trailing slash is how people write an origin, and `//invite/…` is a
    // link that resolves to nothing.
    let full = invite(
        &store,
        None,
        None,
        Some("https://canvas.example/"),
        1_700_000_000,
    )
    .expect("invite");
    let link = printed_link(&full);
    assert!(link.starts_with("https://canvas.example/invite/"), "{link}");
    assert!(
        printed_token(&full) != printed_token(&bare),
        "each run issues its own link"
    );
}

#[test]
fn the_roles_are_this_builds_own_and_a_typo_writes_nothing() {
    let (_dir, store) = store("invite-roles");
    // An alias folds onto the one wire spelling, so the row holds one role and
    // not four spellings of one.
    let summary = invite(&store, Some("UX/UI, qa"), None, None, 1_700_000_000).expect("invite");
    let token = printed_token(&summary);
    assert_eq!(
        store
            .find_invite(&token)
            .expect("find")
            .expect("the invitation")
            .roles,
        vec!["ux_ui".to_string(), "qa".to_string()]
    );

    // A role this build does not have is refused BEFORE the row exists: an
    // invitation whose role grants nothing is one that looks like it worked.
    let error = invite(&store, Some("qa,superuser"), None, None, 1_700_000_000)
        .expect_err("not a role here");
    assert!(error.to_string().contains("superuser"), "{error}");
    assert!(error.to_string().contains("qa"), "{error}");
    assert_eq!(store.list_invites(10, 0).expect("list").len(), 1);
}

#[test]
fn an_invitation_with_no_roles_is_a_guest_link() {
    let (_dir, store) = store("invite-guest");
    let summary = invite(&store, None, None, None, 1_700_000_000).expect("invite");
    let token = printed_token(&summary);
    let row = store
        .find_invite(&token)
        .expect("find")
        .expect("the invitation");
    assert!(row.roles.is_empty());
    assert!(
        row.is_redeemable_at(1_700_000_000),
        "and it is still a link"
    );
}

#[test]
fn the_email_is_recorded_when_one_is_given_and_a_blank_one_is_not() {
    let (_dir, store) = store("invite-email");
    let with =
        invite(&store, None, Some(" new@example.com "), None, 1_700_000_000).expect("invite");
    assert_eq!(
        store
            .find_invite(&printed_token(&with))
            .expect("find")
            .expect("the invitation")
            .email
            .as_deref(),
        Some("new@example.com"),
        "trimmed, because an address typed with a space is the same address"
    );
    let without = invite(&store, None, Some("   "), None, 1_700_000_000).expect("invite");
    assert_eq!(
        store
            .find_invite(&printed_token(&without))
            .expect("find")
            .expect("the invitation")
            .email,
        None,
        "blank is 'not asked for', not an address of spaces"
    );
}

#[test]
fn the_summary_says_what_was_made_and_never_what_it_is_worth() {
    let (_dir, store) = store("invite-summary");
    let summary = invite(
        &store,
        Some("admin"),
        Some("new@example.com"),
        None,
        1_700_000_000,
    )
    .expect("invite");
    assert!(summary.contains("accounts.db"), "{summary}");
    assert!(summary.contains("new@example.com"), "{summary}");
    assert!(summary.contains("admin"), "{summary}");
    // The lifetime is the store's own policy, and the summary says which one.
    assert!(
        summary.contains(&format!("{}", INVITE_TTL_SECS / 86_400)),
        "{summary}"
    );
    // One line of prose and one line of link: the second is what a script
    // takes and what a person copies, and nothing else may end up on it.
    assert_eq!(summary.lines().count(), 2, "{summary}");
    assert_eq!(
        printed_link(&summary),
        format!("/invite/{}", printed_token(&summary))
    );
}

#[test]
fn an_invitation_the_command_line_issued_can_be_withdrawn_by_the_id_the_listing_gives() {
    // The two front doors write the same row: what the command prints, the
    // route can withdraw, and both name it the same way.
    let (_dir, store) = store("invite-revoke");
    let summary = invite(&store, Some("qa"), None, None, 1_700_000_000).expect("invite");
    let token = printed_token(&summary);
    let listed = store.list_invites(10, 0).expect("list");
    assert_eq!(listed.len(), 1);

    assert_eq!(
        store.revoke_invite_by_id(&listed[0].id).expect("withdraw"),
        op_host_services::accounts::InviteWithdrawal::Revoked
    );
    assert_eq!(store.find_invite(&token).expect("find"), None);
}
