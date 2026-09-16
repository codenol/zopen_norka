//! What `op admin create` asks, and what it does with the answers.
//!
//! The terminal is the only part not exercised here: the flow takes its input,
//! its output and its password reads as arguments, so what is under test is the
//! conversation — which questions come in which order, what a weak password
//! does to it, and that no path prints the secret.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use super::*;
use op_accounts::accounts::{NewUser, UserStatus};

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
///
/// The reader ignores the handle it is given on purpose: what is under test
/// here is the conversation (which questions, in what order, what a refusal
/// does), and supplying the secrets from a list keeps each case readable. The
/// handle itself is what
/// [`the_password_comes_from_the_handle_the_dialogue_owns`] covers.
fn drive(db: &AccountsDb, typing: &str, secrets: &[&str]) -> (Result<String, CliError>, String) {
    let mut session = Session::typing(typing);
    let mut secrets = secrets.iter().map(|secret| secret.to_string());
    let mut read_secret = move |_input: &mut dyn BufRead| {
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
    assert!(summary.contains(op_accounts::accounts::FIRST_ADMIN_ROLE));
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
fn a_password_line_is_taken_from_the_reader_it_is_given() {
    // Issue #155 in its smallest form: the password reader must read the
    // handle it is handed. This cannot prove the command no longer deadlocks —
    // only running it can, because the broken line was the WIRING — but it
    // pins the reader to its argument, so a later edit cannot quietly put
    // `std::io::stdin().lock()` back inside it.
    let mut input = Cursor::new(b"a-good-long-password\nrest of the stream\n".to_vec());
    assert_eq!(
        read_line_raw(&mut input).expect("the first line"),
        "a-good-long-password"
    );
    // The rest of the stream is still there, so the reader took one line and
    // not the buffer.
    assert_eq!(
        read_line_raw(&mut input).expect("the second line"),
        "rest of the stream"
    );

    let mut empty = Cursor::new(Vec::new());
    let error = read_line_raw(&mut empty).expect_err("end of input is not an empty password");
    assert!(error.to_string().contains("no input on stdin"), "{error}");
}

#[test]
fn the_password_comes_from_the_handle_the_dialogue_owns() {
    // The wiring `run_create` uses, which no unit test could reach before the
    // fix: the reader is handed the dialogue's own reader, so a name and two
    // passwords arriving on one stream are consumed in order and the flow
    // completes. The command hung here because its reader reached for the
    // process-global stdin instead of this handle.
    let (_dir, db) = store("one-handle");
    let mut session = Session::typing("operator\na-good-long-password\na-good-long-password\n");
    let mut read_secret = |input: &mut dyn BufRead| -> Result<String, CliError> {
        let mut line = String::new();
        if input.read_line(&mut line).map_err(io_error)? == 0 {
            return Err(CliError::usage(
                "no input on stdin: `op admin create` is interactive",
            ));
        }
        Ok(line.trim_end_matches(['\n', '\r']).to_string())
    };
    let summary = create_admin(
        &db,
        &mut session.input,
        &mut session.output,
        &mut read_secret,
    )
    .expect("one stream carries the name and both passwords");

    assert!(summary.contains("operator"), "{summary}");
    assert_secret_never_printed(&Ok(summary), &String::from_utf8_lossy(&session.output));
    assert_eq!(db.count_users().expect("count"), 1);
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
        op_accounts::accounts::now_secs(),
    )
    .expect("seed an account");

    let mut session = Session::typing("operator\n");
    let mut read_secret = |_: &mut dyn BufRead| -> Result<String, CliError> {
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
    let mut read_secret = |_: &mut dyn BufRead| -> Result<String, CliError> {
        panic!("a password is not asked for without a name")
    };
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
    let mut read_secret = |_: &mut dyn BufRead| -> Result<String, CliError> {
        Err(CliError::usage("no input on stdin"))
    };
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
        op_accounts::accounts::InviteWithdrawal::Revoked
    );
    assert_eq!(store.find_invite(&token).expect("find"), None);
}

// ── `op admin add-user` and `op admin reset-password` ───────────────────────
//
// Both exist because a self-hosted deployment has to be operable from a
// terminal: `op admin create` only ever makes the FIRST administrator, and the
// daemon's environment bootstrap only seeds a fresh store, so without these a
// deployment whose operator has no browser could never gain a second account
// and could never recover a forgotten password.

/// Answer the prompts of whichever administrative flow is handed in, and
/// return the outcome with everything printed.
///
/// Each test builds its own reader instead of sharing one: the flows take
/// `&mut impl FnMut`, so a helper forwarding a `&mut dyn FnMut` would not
/// compile — and the three lines it would save are not worth a wrapper.
macro_rules! drive_flow {
    ($run:expr, $db:expr, $typing:expr, $secrets:expr) => {{
        let mut session = Session::typing($typing);
        let mut secrets = $secrets.iter().map(|secret| secret.to_string());
        let mut read_secret = move |_input: &mut dyn BufRead| {
            secrets
                .next()
                .ok_or_else(|| CliError::usage("no input on stdin: this command is interactive"))
        };
        let outcome = $run(
            &$db,
            &mut session.input,
            &mut session.output,
            &mut read_secret,
        );
        let printed = String::from_utf8(session.output).expect("utf-8 output");
        (outcome, printed)
    }};
}

#[test]
fn add_user_creates_an_account_with_the_roles_it_was_asked_for() {
    let (_dir, db) = store("add-user");
    let (outcome, printed) = drive_flow!(
        |db, input, output, read_secret| add_user(
            db,
            Some("designer"),
            Some("frontend"),
            Some("d@example.com"),
            input,
            output,
            read_secret,
            false
        ),
        db,
        "",
        ["a-long-enough-password", "a-long-enough-password"]
    );

    let summary = outcome.expect("the account is created");
    assert!(summary.contains("designer"), "{summary}");
    assert!(!summary.contains("a-long-enough-password"), "{summary}");
    assert!(!printed.contains("a-long-enough-password"), "{printed}");

    let created = db
        .find_user_by_username("designer")
        .expect("read the store")
        .expect("the account exists");
    assert_eq!(created.email.as_deref(), Some("d@example.com"));
    assert_eq!(created.status, UserStatus::Active);
    assert!(
        !created.roles.is_empty(),
        "the requested role is stored: {:?}",
        created.roles
    );
}

#[test]
fn add_user_refuses_a_name_that_already_exists() {
    let (_dir, db) = store("add-user-dup");
    // A fresh store has nobody, so the name has to exist before the refusal
    // means anything: the deployment's first administrator is the case an
    // operator actually hits.
    db.create_first_admin("admin", "a-long-enough-password", now_secs())
        .expect("seed the first administrator");
    let (outcome, _) = drive_flow!(
        |db, input, output, read_secret| add_user(
            db,
            Some("admin"),
            None,
            None,
            input,
            output,
            read_secret,
            false
        ),
        db,
        "",
        ["a-long-enough-password", "a-long-enough-password"]
    );

    let error = outcome.expect_err("a duplicate is refused before anything is written");
    assert!(
        format!("{error}").contains("reset-password"),
        "the refusal names the way forward: {error}"
    );
    assert_eq!(db.count_users().expect("count"), 1, "nothing was added");
}

#[test]
fn add_user_asks_again_after_a_password_the_store_would_refuse() {
    let (_dir, db) = store("add-user-weak");
    // The policy is the store's own: a weak password is answered with a reason
    // and another question rather than with a write that fails afterwards.
    let (outcome, printed) = drive_flow!(
        |db, input, output, read_secret| add_user(
            db,
            Some("weakling"),
            None,
            None,
            input,
            output,
            read_secret,
            false
        ),
        db,
        "",
        ["short", "a-long-enough-password", "a-long-enough-password"]
    );

    outcome.expect("the second, strong password is accepted");
    assert!(
        db.find_user_by_username("weakling")
            .expect("read the store")
            .is_some(),
        "the account exists"
    );
    assert!(
        !printed.contains("a-long-enough-password"),
        "no secret is echoed: {printed}"
    );
}

#[test]
fn reset_password_changes_the_password_and_never_prints_it() {
    let (_dir, db) = store("reset-password");
    let user = db
        .create_user(
            &NewUser::active("forgetful", "forgetful", "the-original-password"),
            now_secs(),
        )
        .expect("seed the account");

    let (outcome, printed) = drive_flow!(
        |db, input, output, read_secret| reset_password(
            db,
            Some("forgetful"),
            input,
            output,
            read_secret,
            false
        ),
        db,
        "",
        ["the-new-longer-password", "the-new-longer-password"]
    );

    let summary = outcome.expect("the password is changed");
    assert!(summary.contains("forgetful"), "{summary}");
    assert!(!printed.contains("the-new-longer-password"), "{printed}");

    let stored = db
        .find_user_by_id(&user.id)
        .expect("read the store")
        .expect("the account exists");
    let hash = stored.password_hash.as_deref().expect("a hash is stored");
    assert!(
        op_accounts::accounts::verify_password("the-new-longer-password", hash).expect("verify"),
        "the new password verifies"
    );
    assert!(
        !op_accounts::accounts::verify_password("the-original-password", hash).expect("verify"),
        "the old password does not"
    );
}

#[test]
fn reset_password_refuses_a_name_the_store_does_not_have() {
    let (_dir, db) = store("reset-password-missing");
    let (outcome, _) = drive_flow!(
        |db, input, output, read_secret| reset_password(
            db,
            Some("nobody"),
            input,
            output,
            read_secret,
            false
        ),
        db,
        "",
        ["a-long-enough-password", "a-long-enough-password"]
    );

    let error = outcome.expect_err("an unknown account is refused");
    assert!(format!("{error}").contains("nobody"), "{error}");
}
