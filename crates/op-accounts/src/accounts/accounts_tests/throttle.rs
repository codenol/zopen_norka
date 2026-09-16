//! How often a name and an address may fail to sign in — issue #77.
//!
//! The tests that matter here are the ones that would pass against a limiter
//! that was decoration: a burst being refused proves nothing if the RIGHT
//! password still walks in, or if the refusal is paid for with an Argon2id
//! verification, or if the counter can be reset by the attacker, or if it
//! answers differently for a name that exists. So each of those is checked —
//! including, in [`a_refused_attempt_never_reaches_the_password_hash`], the
//! ORDER of the check, by making the credential itself unreadable.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rusqlite::{params, OptionalExtension};

use super::*;
use crate::accounts::{SignInAttempt, SignInLimits, SignInOutcome};

/// A deployment whose ceilings are small enough to reach in a test.
///
/// The defaults are five failures per name and twenty per address over fifteen
/// minutes — numbers a test would have to spend forty Argon2id verifications to
/// touch, which is a minute of CPU per test for a property that has nothing to
/// do with the size of the number.
fn budgets(max_failures_per_account: u32, max_failures_per_source: u32) -> SignInLimits {
    SignInLimits {
        max_failures_per_account,
        max_failures_per_source,
        window_secs: 15 * 60,
    }
}

/// An address, as the accept loop would hand it over.
fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address literal")
}

/// One attempt, assembled the way `account_routes::login` assembles it.
fn attempt<'a>(username: &'a str, password: &'a str, source: Option<IpAddr>) -> SignInAttempt<'a> {
    let attempt = SignInAttempt::new(username, password);
    match source {
        Some(source) => attempt.with_source(source),
        None => attempt,
    }
}

/// One attempt, with the store's answer demanded rather than propagated: every
/// call in this file is expected to be an answer, and a store fault would make
/// the assertion that follows meaningless.
fn run(
    db: &AccountsDb,
    username: &str,
    password: &str,
    source: Option<IpAddr>,
    limits: &SignInLimits,
    now: i64,
) -> SignInOutcome {
    db.authenticate(&attempt(username, password, source), limits, now)
        .expect("the store answers")
}

/// One counter row: failures, when the streak began, when it last grew.
///
/// Read out of the table rather than through an accessor, because what an
/// operator sees when they ask why an account is locked IS the table.
fn streak(db: &AccountsDb, scope: &str, subject: &str) -> Option<(i64, i64, i64)> {
    db.conn()
        .query_row(
            "SELECT failures, first_failure_at, last_failure_at FROM sign_in_attempts
             WHERE scope = ?1 AND subject = ?2",
            params![scope, subject],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .expect("read the counter")
}

/// How many counters the table holds.
fn counters(db: &AccountsDb) -> i64 {
    count(&db.conn(), "SELECT COUNT(*) FROM sign_in_attempts")
}

// ---------------------------------------------------------------------------
// The name's budget
// ---------------------------------------------------------------------------

#[test]
fn a_burst_of_wrong_passwords_is_refused_once_the_budget_is_spent() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(3, 100);

    for attempt_number in 1..=3 {
        assert_eq!(
            run(&db, "alice", "not-the-password", None, &limits, NOW),
            SignInOutcome::Rejected,
            "attempt {attempt_number} is still checked"
        );
    }
    // The fourth is refused rather than checked, and the wait it names is the
    // rest of the window measured from the last failure.
    assert_eq!(
        run(&db, "alice", "not-the-password", None, &limits, NOW),
        SignInOutcome::Throttled {
            retry_after_secs: limits.window_secs
        }
    );
}

#[test]
fn a_correct_password_is_refused_while_the_budget_is_spent() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);
    for _ in 0..2 {
        run(&db, "alice", "not-the-password", None, &limits, NOW);
    }

    // The whole point of the ceiling: a limiter that lets the right password
    // through is not a limiter, it is a delay. The cost of a guess is what the
    // attacker is spending, and the server cannot tell a guess from a typo
    // before it has paid for the verification.
    assert_eq!(
        run(&db, "alice", PASSWORD, None, &limits, NOW),
        SignInOutcome::Throttled {
            retry_after_secs: limits.window_secs
        }
    );
    // And the refusal is not the account being seen: a throttled attempt does
    // nothing at all.
    assert_eq!(
        db.find_user_by_id("u1")
            .expect("find")
            .expect("a row")
            .last_seen_at,
        None
    );
}

#[test]
fn a_refused_attempt_never_reaches_the_password_hash() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);
    for _ in 0..2 {
        run(&db, "alice", "not-the-password", None, &limits, NOW);
    }

    // The order of the two decisions, proved without timing anything: the
    // stored hash is made unreadable, so an attempt that reached the
    // verification would be a store FAULT. It is not — it is the refusal — and
    // that is the denial-of-service half of the issue: a request that is going
    // to be refused must not cost an Argon2id verification first.
    db.conn()
        .execute(
            "UPDATE users SET password_hash = 'truncated' WHERE id = 'u1'",
            [],
        )
        .expect("corrupt the row");

    assert!(matches!(
        run(&db, "alice", PASSWORD, None, &limits, NOW),
        SignInOutcome::Throttled { .. }
    ));
}

#[test]
fn a_verified_password_gives_that_name_its_budget_back() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(3, 100);
    for _ in 0..2 {
        assert_eq!(
            run(&db, "alice", "not-the-password", None, &limits, NOW),
            SignInOutcome::Rejected
        );
    }
    assert_eq!(streak(&db, "account", "alice"), Some((2, NOW, NOW)));

    // Somebody who mistyped twice and then got it right is not punished for the
    // typos: the streak is theirs to clear by succeeding, and there is budget
    // left for the success to happen in.
    assert!(matches!(
        run(&db, "alice", PASSWORD, None, &limits, NOW + 3),
        SignInOutcome::SignedIn(_)
    ));
    assert_eq!(streak(&db, "account", "alice"), None);

    // A fresh streak, counted from one — not from the three it would have been
    // had the row merely been left alone, which is the difference between a
    // person who typos once after signing in and one who is one mistake from a
    // lockout.
    run(&db, "alice", "not-the-password", None, &limits, NOW + 4);
    assert_eq!(
        streak(&db, "account", "alice"),
        Some((1, NOW + 4, NOW + 4)),
        "the wait goes back to nothing, so the count starts over"
    );
}

#[test]
fn a_success_does_not_clear_another_names_budget() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    active_user(&db, "u2", "bob");
    let limits = budgets(3, 100);
    for name in ["alice", "bob"] {
        for _ in 0..2 {
            assert_eq!(
                run(&db, name, "not-the-password", None, &limits, NOW),
                SignInOutcome::Rejected
            );
        }
    }

    assert!(matches!(
        run(&db, "alice", PASSWORD, None, &limits, NOW),
        SignInOutcome::SignedIn(_)
    ));

    // Bob is exactly where he was, and the two failures alice just cleared are
    // NOT taken off his count: had the success cleared every counter, this
    // failure would leave him at one and he would still have a budget. It
    // leaves him at three, and the next attempt is refused.
    run(&db, "bob", "not-the-password", None, &limits, NOW + 5);
    assert_eq!(streak(&db, "account", "bob"), Some((3, NOW, NOW + 5)));
    assert!(matches!(
        run(&db, "bob", PASSWORD, None, &limits, NOW + 5),
        SignInOutcome::Throttled { .. }
    ));
}

#[test]
fn a_name_that_does_not_exist_is_refused_exactly_like_one_that_does() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);
    for name in ["alice", "nobody"] {
        for _ in 0..2 {
            run(&db, name, "not-the-password", None, &limits, NOW);
        }
    }

    // The strongest form of the claim: the RIGHT password for an account that
    // exists and an arbitrary password for a name that does not are the same
    // outcome, down to the wait. A counter kept per existing account would have
    // separated these two, and would have been a way to ask the login form
    // whether somebody has an account here.
    let existing = run(&db, "alice", PASSWORD, None, &limits, NOW);
    let missing = run(&db, "nobody", PASSWORD, None, &limits, NOW);
    assert_eq!(existing, missing);
    assert!(matches!(existing, SignInOutcome::Throttled { .. }));
}

#[test]
fn a_name_spends_one_budget_whatever_case_it_is_typed_in() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);

    // The lookup folds case and trims, so the budget has to fold the same way:
    // otherwise `ALICE` is a second budget for the same account, which is the
    // bypass the ceiling would be without.
    run(&db, "alice", "not-the-password", None, &limits, NOW);
    run(&db, "ALICE", "not-the-password", None, &limits, NOW);
    assert!(matches!(
        run(&db, "  Alice  ", "not-the-password", None, &limits, NOW),
        SignInOutcome::Throttled { .. }
    ));
    assert_eq!(counters(&db), 1, "one name, one row");
}

#[test]
fn a_streak_that_has_aged_out_is_not_a_lockout() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);
    for _ in 0..2 {
        run(&db, "alice", "not-the-password", None, &limits, NOW);
    }

    // Exactly one window later the budget is free again — the boundary is
    // stated once, in the store, and this reads it from the outside.
    let free_at = NOW + limits.window_secs;
    assert_eq!(
        run(&db, "alice", "not-the-password", None, &limits, free_at),
        SignInOutcome::Rejected,
        "a window later the attempt is checked again, not refused"
    );
    assert_eq!(
        streak(&db, "account", "alice"),
        Some((1, free_at, free_at)),
        "and the streak restarts rather than resuming"
    );
}

#[test]
fn a_name_a_stranger_invented_does_not_stay_in_the_table() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);

    run(&db, "invented", "not-the-password", None, &limits, NOW);
    assert_eq!(counters(&db), 1);

    // The sweep runs on the write path, so the growth of this table is bounded
    // by the failures still being counted: a name nobody will ever try again is
    // a row that disappears one window later. Without it, an attacker inventing
    // names would be filling the deployment's disk one row at a time.
    run(
        &db,
        "another-invented-name",
        "not-the-password",
        None,
        &limits,
        NOW + limits.window_secs,
    );
    assert_eq!(counters(&db), 1, "the aged-out row was swept");
    assert!(streak(&db, "account", "invented").is_none());
    assert_eq!(
        streak(&db, "account", "another-invented-name"),
        Some((1, NOW + limits.window_secs, NOW + limits.window_secs))
    );
}

#[test]
fn a_lockout_outlives_a_reopen() {
    let (dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(2, 100);
    for _ in 0..2 {
        run(&db, "alice", "not-the-password", None, &limits, NOW);
    }
    drop(db);

    // In the database and not in the process, because a restart is not a
    // security event: a deploy, a crash or an OOM is something an attacker is
    // better placed to cause than the account holder is, and a limit that a
    // restart clears is one of those levers.
    let reopened = reopen(&dir);
    assert_eq!(
        run(&reopened, "alice", PASSWORD, None, &limits, NOW),
        SignInOutcome::Throttled {
            retry_after_secs: limits.window_secs
        }
    );
}

#[test]
fn the_counters_are_named_the_way_an_operator_would_look_for_them() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(3, 100);
    let source = ip("203.0.113.7");

    // The case the person typed is folded away, and both dimensions are
    // recorded under the name an operator would search for: the account by its
    // name, the caller by its address. Neither is a hash, because "which
    // account is locked" and "who is doing it" are the two questions this table
    // exists to answer.
    run(&db, "Alice", "not-the-password", Some(source), &limits, NOW);
    assert_eq!(streak(&db, "account", "alice"), Some((1, NOW, NOW)));
    assert_eq!(streak(&db, "source", "203.0.113.7"), Some((1, NOW, NOW)));

    // `first_failure_at` is when the streak began and `last_failure_at` when it
    // last grew — the pair an operator reads as "since when, and still going".
    run(
        &db,
        "alice",
        "not-the-password",
        Some(source),
        &limits,
        NOW + 30,
    );
    assert_eq!(streak(&db, "account", "alice"), Some((2, NOW, NOW + 30)));
    assert_eq!(
        streak(&db, "source", "203.0.113.7"),
        Some((2, NOW, NOW + 30))
    );
}

// ---------------------------------------------------------------------------
// The address's budget
// ---------------------------------------------------------------------------

#[test]
fn one_address_cannot_spray_names_that_do_not_exist() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(100, 3);
    let source = Some(ip("203.0.113.9"));

    // The cheap attack the per-name budget cannot see: ten thousand DIFFERENT
    // names, each with a budget of its own, each costing this process a full
    // Argon2id verification. Every one of these names has never been tried
    // before, and the fourth is refused anyway.
    for name in ["n1", "n2", "n3"] {
        assert_eq!(
            run(&db, name, "not-the-password", source, &limits, NOW),
            SignInOutcome::Rejected
        );
    }
    assert_eq!(
        run(&db, "n4", "not-the-password", source, &limits, NOW),
        SignInOutcome::Throttled {
            retry_after_secs: limits.window_secs
        }
    );
    // A correct password from the same address is refused too — the limiter is
    // checked before the credential, so it cannot be the caller's good luck
    // that lets it through.
    assert!(matches!(
        run(&db, "alice", PASSWORD, source, &limits, NOW),
        SignInOutcome::Throttled { .. }
    ));
}

#[test]
fn a_verified_sign_in_does_not_refund_the_address() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(100, 3);
    let source = Some(ip("203.0.113.9"));

    run(&db, "nobody", "not-the-password", source, &limits, NOW);
    run(&db, "nobody", "not-the-password", source, &limits, NOW);
    // A real sign-in from the same address, with budget still left: it clears
    // the name it used and nothing else.
    assert!(matches!(
        run(&db, "alice", PASSWORD, source, &limits, NOW),
        SignInOutcome::SignedIn(_)
    ));
    assert_eq!(streak(&db, "source", "203.0.113.9"), Some((2, NOW, NOW)));

    run(&db, "nobody", "not-the-password", source, &limits, NOW);
    // Had the success refunded the address, this next attempt would still be
    // checked. It is refused, which is what stops a caller that holds one
    // working account from washing away the budget it is attacking with.
    assert!(matches!(
        run(&db, "nobody", "not-the-password", source, &limits, NOW),
        SignInOutcome::Throttled { .. }
    ));
}

#[test]
fn two_addresses_do_not_share_a_budget() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(100, 2);
    let busy = Some(ip("203.0.113.9"));

    run(&db, "n1", "not-the-password", busy, &limits, NOW);
    run(&db, "n2", "not-the-password", busy, &limits, NOW);
    assert!(matches!(
        run(&db, "n3", "not-the-password", busy, &limits, NOW),
        SignInOutcome::Throttled { .. }
    ));
    // One caller being out of budget is not every caller being out of budget.
    assert_eq!(
        run(
            &db,
            "n3",
            "not-the-password",
            Some(ip("198.51.100.4")),
            &limits,
            NOW
        ),
        SignInOutcome::Rejected
    );
}

#[test]
fn one_ipv6_subscriber_spends_one_budget_across_its_whole_slash_64() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(100, 2);

    // A host is handed a /64, not an address: 2^64 source addresses it may use
    // without asking anybody. Counting the full address would therefore hand it
    // a fresh budget per request, so the counter is the prefix.
    let first = Some(ip("2001:db8:1:2::1"));
    let second_in_the_same_block = Some(ip("2001:db8:1:2:ffff::9"));
    run(&db, "n1", "not-the-password", first, &limits, NOW);
    run(&db, "n2", "not-the-password", first, &limits, NOW);
    assert!(matches!(
        run(
            &db,
            "n3",
            "not-the-password",
            second_in_the_same_block,
            &limits,
            NOW
        ),
        SignInOutcome::Throttled { .. }
    ));

    // The NEXT /64 is somebody else, and this is why the prefix — not a
    // shortened address — is what the row holds.
    assert_eq!(
        run(
            &db,
            "n3",
            "not-the-password",
            Some(ip("2001:db8:1:3::1")),
            &limits,
            NOW
        ),
        SignInOutcome::Rejected
    );
}

#[test]
fn a_caller_whose_address_is_unknown_is_limited_by_the_name_alone() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    // An address budget of ONE failure: if an absent address were keyed at all,
    // the second attempt here would already be refused.
    let limits = budgets(3, 1);

    for _ in 0..2 {
        assert_eq!(
            run(&db, "alice", "not-the-password", None, &limits, NOW),
            SignInOutcome::Rejected
        );
    }
    // The name's own budget is the one that runs out.
    run(&db, "alice", "not-the-password", None, &limits, NOW);
    assert!(matches!(
        run(&db, "alice", PASSWORD, None, &limits, NOW),
        SignInOutcome::Throttled { .. }
    ));
    assert_eq!(counters(&db), 1, "nothing was counted against an address");
}

#[test]
fn the_address_row_holds_the_prefix_an_operator_recognises() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(3, 100);

    run(
        &db,
        "alice",
        "not-the-password",
        Some(ip("2001:db8:1:2:3:4:5:6")),
        &limits,
        NOW,
    );
    // Written the way the address is written, so that whoever reads the row can
    // act on it — the low 64 bits are the part that is not a network.
    assert_eq!(
        streak(&db, "source", "2001:db8:1:2::/64"),
        Some((1, NOW, NOW))
    );
    assert_eq!(
        count(&db.conn(), "SELECT COUNT(*) FROM sign_in_attempts"),
        2,
        "one row for the name, one for the address"
    );
}

#[test]
fn an_address_is_written_as_a_plain_address() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(3, 100);

    // IPv4 has no prefix to fold: the address IS the unit a provider hands out,
    // so `203.0.113.7` is what the row says.
    let source = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
    run(&db, "alice", "not-the-password", Some(source), &limits, NOW);
    assert_eq!(streak(&db, "source", "203.0.113.7"), Some((1, NOW, NOW)));

    let v6 = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 3, 4, 5, 6));
    run(&db, "alice", "not-the-password", Some(v6), &limits, NOW);
    assert_eq!(
        streak(&db, "source", "2001:db8:1:2::/64"),
        Some((1, NOW, NOW))
    );
}

#[test]
fn the_default_budgets_are_the_two_the_policy_module_states() {
    // A number in a constant is a claim, and this is the claim: five failures
    // per name, twenty per address, fifteen minutes. What the ratio is FOR is
    // in `accounts_policy`; that it is not zero, and that the address is the
    // larger of the two, is what a deployment relies on.
    let limits = SignInLimits::default();
    assert_eq!(
        limits.max_failures_per_account,
        crate::accounts::SIGN_IN_MAX_FAILURES_PER_ACCOUNT
    );
    assert_eq!(
        limits.max_failures_per_source,
        crate::accounts::SIGN_IN_MAX_FAILURES_PER_SOURCE
    );
    assert!(limits.max_failures_per_account < limits.max_failures_per_source);
    assert_eq!(
        limits.window_secs,
        crate::accounts::SIGN_IN_FAILURE_WINDOW_SECS
    );
}

#[test]
fn the_wait_it_names_is_the_time_left_in_the_window() {
    let (_dir, db) = store();
    active_user(&db, "u1", "alice");
    let limits = budgets(1, 100);

    run(&db, "alice", "not-the-password", None, &limits, NOW);
    // The window is measured from the LAST failure, so a burst that keeps going
    // keeps its own lock alive and the wait shrinks as time passes — never to
    // zero, because "retry in 0 seconds" is an instruction to retry now.
    assert_eq!(
        run(&db, "alice", PASSWORD, None, &limits, NOW + 10),
        SignInOutcome::Throttled {
            retry_after_secs: limits.window_secs - 10
        }
    );
    assert_eq!(
        run(
            &db,
            "alice",
            PASSWORD,
            None,
            &limits,
            NOW + limits.window_secs - 1
        ),
        SignInOutcome::Throttled {
            retry_after_secs: 1
        }
    );
}
