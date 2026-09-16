//! `op admin create` — make this deployment's first administrator.
//!
//! ## Why a prompt and not a flag
//!
//! `op admin create --password hunter2` would put the password in the shell
//! history and in the process table, so it is asked for, twice, and not echoed.
//! The same act from a container (where nobody is sitting at a terminal) is the
//! `NORKA_ADMIN_USERNAME` / `NORKA_ADMIN_PASSWORD` pair the daemon reads at
//! start-up; both paths go through
//! [`AccountsDb::create_first_admin`](op_accounts::accounts::AccountsDb::create_first_admin),
//! so "who the first admin is" and "is this store fresh" are one decision with
//! two front doors.
//!
//! ## Why this command talks to the database and not to a server
//!
//! It has to work before there is anything to talk to: a deployment with no
//! accounts has nobody who could be authorized to create one over HTTP, and the
//! operator may be provisioning a stopped container. So it opens the same
//! `accounts.db` the daemon opens, in the same data directory, and the command
//! accepts the same `OPENPENCIL_ONLINE_DATA_DIR` the daemon reads (or
//! `--data-dir`, for an operator who would rather be explicit).
//!
//! ## What it prints
//!
//! The name, the status, and which database was written. Never the password —
//! not a length, not a hash, not a hint. A password that appears once in a log
//! has to be treated as compromised, so it does not appear at all.
//!
//! ## `op admin invite`, and why it is the same shape
//!
//! An invitation is the OTHER thing that has to be issuable when there is no
//! administrator to ask: the first link of a deployment whose only operator has
//! no browser in front of them, and the scripted case (a provisioning job that
//! makes one link per contractor). So it opens the same store, for the same
//! reason, and prints the link — once, because that is how many times the value
//! exists. The daemon's own route
//! (`POST /api/auth/admin/invites`) is the same act for an operator who DOES
//! have a browser; both write through
//! [`AccountsDb::create_invite`](op_accounts::accounts::AccountsDb::create_invite),
//! so the roles, the lifetime and the hashing are one decision with two front
//! doors, exactly as `create` is.

use std::io::{BufRead, Write};
use std::path::Path;

use op_accounts::accounts::{
    canonical_roles, check_password_strength, now_secs, split_roles, AccountsDb, FirstAdmin,
    NewInvite, INVITE_TTL_SECS,
};

use crate::cli_error::CliError;
use crate::command_helpers::flag_value;
use crate::{Command, Flags};

/// Map `op admin ...` onto its command.
pub(crate) fn map_admin(positionals: &[String], flags: &Flags) -> Result<Command, CliError> {
    let subcommand = positionals.first().map(String::as_str).unwrap_or("");
    match subcommand {
        "create" => Ok(Command::AdminCreate {
            data_dir: flag_value(flags, "data-dir"),
        }),
        "invite" => Ok(Command::AdminInvite {
            data_dir: flag_value(flags, "data-dir"),
            roles: flag_value(flags, "roles"),
            email: flag_value(flags, "email"),
            origin: flag_value(flags, "origin"),
        }),
        "" => Err(CliError::usage(
            "Usage: op admin create [--data-dir DIR] | op admin invite [--roles a,b] [--email ADDR] \
             [--origin URL] [--data-dir DIR]",
        )),
        other => Err(CliError::usage(format!(
            "unknown admin subcommand {other:?}; the two are `op admin create` and `op admin \
             invite`"
        ))),
    }
}

/// Run the interactive flow against the real terminal.
pub(crate) fn run_create(data_dir: Option<&str>) -> Result<String, CliError> {
    let store = open_store(data_dir)?;
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();
    let mut read_secret = read_secret_from_terminal;
    let summary = create_admin(&store, &mut stdin, &mut stdout, &mut read_secret)?;
    Ok(summary)
}

/// Run `op admin invite`, printing the link it made.
pub(crate) fn run_invite(
    data_dir: Option<&str>,
    roles: Option<&str>,
    email: Option<&str>,
    origin: Option<&str>,
) -> Result<String, CliError> {
    let store = open_store(data_dir)?;
    invite(&store, roles, email, origin, now_secs())
}

/// Issue one invitation and render it for a terminal or a script.
///
/// `now` is an argument for the same reason the store takes it: the lifetime
/// is a decision this command makes with a number, and a test that had to wait
/// for a clock to move would not be testing the decision.
///
/// `created_by` is deliberately `None`. The store records WHO issued a link
/// when an account did, and this command is not an account: naming an account
/// here would mean inventing an attribution — the operator's own name, or the
/// first administrator's — and putting it in a row that is read as a fact.
/// "Issued from the command line, by whoever holds the data directory" is the
/// truth, and `NULL` is how the column says it.
pub(crate) fn invite(
    store: &AccountsDb,
    roles: Option<&str>,
    email: Option<&str>,
    origin: Option<&str>,
    now: i64,
) -> Result<String, CliError> {
    // The product's own vocabulary, asked before anything is written: a role
    // this build does not have would be stored and would then grant nothing,
    // which is the failure that looks like a working invitation.
    let requested = split_roles(roles.unwrap_or_default());
    let canonical = canonical_roles(&requested)
        .map_err(|unknown| CliError::usage(format!("--roles: {unknown}")))?;
    let role_refs: Vec<&str> = canonical.iter().map(String::as_str).collect();

    let email = email.map(str::trim).filter(|email| !email.is_empty());
    let mut new_invite = NewInvite::new(&role_refs, None, INVITE_TTL_SECS);
    if let Some(email) = email {
        new_invite = new_invite.with_email(email);
    }
    let issued = store
        .create_invite(&new_invite, now)
        .map_err(|error| CliError::Io(format!("cannot write the account store: {error}")))?;

    let link = match origin.map(str::trim).filter(|origin| !origin.is_empty()) {
        // A trailing slash is how people write an origin, and `//invite/…` is
        // a link that resolves to nothing.
        Some(origin) => format!(
            "{}{}",
            origin.trim_end_matches('/'),
            op_editor_core::route::to_invite_path(&issued.token)
        ),
        None => op_editor_core::route::to_invite_path(&issued.token),
    };
    Ok(format!(
        "created an invitation in {}{}{}; it stops being accepted at {} ({} days from now)\n{}",
        store.dir().join("accounts.db").display(),
        match email {
            Some(email) => format!(" for {email}"),
            None => String::new(),
        },
        match canonical.is_empty() {
            true => " with no roles".to_string(),
            false => format!(" with the roles `{}`", canonical.join(", ")),
        },
        issued.invite.expires_at,
        INVITE_TTL_SECS / 86_400,
        // The link alone on its own line, because that is the line a script
        // takes and the line a person copies. Without `--origin` it is the path
        // only: this command does not know which address the deployment answers
        // on, and a link with the wrong host is worse than one to complete.
        link,
    ))
}

/// The store this command writes, and the path it came from.
///
/// `--data-dir` first, then the daemon's own variable: the command is run
/// beside a deployment more often than inside one, so an explicit path has to
/// win, and a deployment that configured itself in its environment should not
/// have to repeat it here.
pub(crate) fn open_store(data_dir: Option<&str>) -> Result<AccountsDb, CliError> {
    match data_dir.map(str::trim).filter(|dir| !dir.is_empty()) {
        Some(dir) => AccountsDb::open(Path::new(dir))
            .map_err(|error| CliError::Io(format!("cannot open {dir}: {error}"))),
        None => match AccountsDb::open_from_env() {
            Ok(Some(store)) => Ok(store),
            Ok(None) => Err(CliError::usage(format!(
                "no deployment data directory: pass --data-dir DIR, or set {}",
                op_accounts::accounts::DATA_DIR_ENV
            ))),
            Err(error) => Err(CliError::Io(format!(
                "cannot open the account store: {error}"
            ))),
        },
    }
}

/// Ask for a name and a password, and create the admin.
///
/// The readable and writable halves are arguments so the flow can be driven by
/// a test: what is being checked is which questions are asked, in what order,
/// and what a refusal does to the session — none of which needs a terminal.
/// `read_secret` is separate because a password is read without echo, which is
/// a property of the terminal rather than of the input stream.
pub(crate) fn create_admin(
    store: &AccountsDb,
    input: &mut impl BufRead,
    output: &mut impl Write,
    read_secret: &mut impl FnMut() -> Result<String, CliError>,
) -> Result<String, CliError> {
    // Refused before a single question: an operator who runs this on a live
    // deployment should not type a password in order to be told no, and the
    // rule is the store's (it is the same call the daemon's environment
    // bootstrap makes).
    let accounts = store
        .count_users()
        .map_err(|error| CliError::Io(format!("cannot read the account store: {error}")))?;
    if accounts > 0 {
        return Err(CliError::usage(format!(
            "this deployment already has {accounts} account(s); `op admin create` only makes the \
             FIRST administrator — issue an invitation from the running deployment instead"
        )));
    }

    let username = ask(input, output, "Username: ")?;
    if username.trim().is_empty() {
        return Err(CliError::usage("a username must not be empty"));
    }
    let username = username.trim().to_string();

    let password = loop {
        let password = read_secret()?;
        // The same policy the store applies, asked here so a weak password is
        // answered with a reason and another question rather than with a
        // failure after the confirmation.
        if let Err(weak) = check_password_strength(&password, &username) {
            writeln!(output, "  {weak}").map_err(io_error)?;
            continue;
        }
        let repeated = read_secret()?;
        if repeated != password {
            writeln!(output, "  the two passwords do not match").map_err(io_error)?;
            continue;
        }
        break password;
    };

    match store
        .create_first_admin(&username, &password, op_accounts::accounts::now_secs())
        .map_err(|error| CliError::Io(format!("cannot write the account store: {error}")))?
    {
        FirstAdmin::Created(user) => Ok(format!(
            "created the administrator `{}` ({}), with the role `{}`, in {}",
            user.username,
            user.status.as_str(),
            op_accounts::accounts::FIRST_ADMIN_ROLE,
            store.dir().join("accounts.db").display()
        )),
        // Unreachable — the count above was zero a moment ago — but reported
        // rather than assumed away: another process may have created an
        // account in between, and saying so is better than printing a success
        // that did not happen.
        FirstAdmin::AlreadyProvisioned { accounts } => Err(CliError::usage(format!(
            "another process created this deployment's first account ({accounts} now); nothing \
             was written"
        ))),
        FirstAdmin::WeakPassword(weak) => Err(CliError::usage(weak.to_string())),
    }
}

/// Ask one question and read one answer.
fn ask(
    input: &mut impl BufRead,
    output: &mut impl Write,
    prompt: &str,
) -> Result<String, CliError> {
    write!(output, "{prompt}").map_err(io_error)?;
    output.flush().map_err(io_error)?;
    let mut line = String::new();
    if input.read_line(&mut line).map_err(io_error)? == 0 {
        return Err(CliError::usage(
            "no input on stdin: `op admin create` is interactive — for a container, set \
             NORKA_ADMIN_USERNAME and NORKA_ADMIN_PASSWORD instead",
        ));
    }
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// Read a password from the terminal without echoing it.
///
/// Two attempts, honestly labelled: the first and its confirmation are the two
/// reads a caller makes, and a mismatch re-asks both. A read that hits end of
/// input is the non-interactive case, and it stops the loop rather than
/// spinning.
fn read_secret_from_terminal() -> Result<String, CliError> {
    let _echo = EchoOff::engage();
    ask_prompt("Password (not echoed): ")?;
    let first = read_line_raw()?;
    println!();
    Ok(first)
}

/// Say one line, then read.
fn ask_prompt(prompt: &str) -> Result<(), CliError> {
    let mut stdout = std::io::stdout();
    write!(stdout, "{prompt}").map_err(io_error)?;
    stdout.flush().map_err(io_error)
}

fn read_line_raw() -> Result<String, CliError> {
    let mut line = String::new();
    if std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(io_error)?
        == 0
    {
        return Err(CliError::usage(
            "no input on stdin: `op admin create` is interactive",
        ));
    }
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// Turns the terminal's echo off for as long as it lives.
///
/// `stty` rather than a crate: this is the only place in the product that needs
/// a password read without echo, and a password that appears on the screen
/// while it is typed is a password in a screenshot, a screen share, or a
/// shoulder. Where `stty` is not there (Windows, a terminal that refuses), the
/// password is still read — with a warning, because reading it silently would
/// be worse than saying so.
struct EchoOff {
    engaged: bool,
}

impl EchoOff {
    fn engage() -> Self {
        let engaged = std::process::Command::new("stty")
            .arg("-echo")
            .stdin(std::process::Stdio::inherit())
            .status()
            .is_ok_and(|status| status.success());
        if !engaged {
            eprintln!(
                "op: cannot turn off terminal echo; the password will be visible as you type"
            );
        }
        Self { engaged }
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        if self.engaged {
            let _ = std::process::Command::new("stty")
                .arg("echo")
                .stdin(std::process::Stdio::inherit())
                .status();
        }
    }
}

fn io_error(error: std::io::Error) -> CliError {
    CliError::Io(format!("cannot read or write the terminal: {error}"))
}

#[cfg(test)]
#[path = "admin_cli_tests.rs"]
mod tests;
