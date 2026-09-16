//! What the sign-in route answers once a name or an address has spent its
//! budget of failures — issue #77, seen from the outside.
//!
//! The store's own tests (`accounts::accounts_tests::throttle`) hold the
//! counting. What is left for the route is the ANSWER, and the three things a
//! client and a login form depend on: that a spent budget is `429` with a
//! `Retry-After` rather than a lie about the password, that an ordinary refusal
//! never asks the caller to retry, and that the refusal is the same answer
//! whether the account exists or not.
//!
//! A sibling of `account_routes_tests` rather than part of it: the sign-in
//! surface's tests had reached the repository's line ceiling, and these are a
//! concern of their own — they are the only tests here that need budgets small
//! enough to spend, and the only ones that send a source address.

use std::net::{IpAddr, Ipv4Addr};

use super::*;
use crate::accounts::SignInLimits;

/// The same, as a caller whose address the accept loop knows.
trait HandleFromAddress {
    fn handle_from(&self, request: &HttpRequest, source: IpAddr) -> Option<AccountReply>;
}

impl HandleFromAddress for AccountAuth {
    fn handle_from(&self, request: &HttpRequest, source: IpAddr) -> Option<AccountReply> {
        self.handle(request, &allowed_origins(), Some(source))
    }
}

/// A deployment whose sign-in budgets are small enough to reach in a test.
///
/// The defaults are what a deployment runs with and are pinned by the store's
/// own tests; what these tests are about is the ANSWER a spent budget produces,
/// which is the same answer at any size.
fn deployment_with(limits: SignInLimits) -> (TempDir, AccountAuth) {
    let dir = TempDir::new("account-routes-throttled");
    let db = AccountsDb::open(dir.path()).expect("open the account store");
    (dir, AccountAuth::with_limits(Arc::new(db), limits))
}

/// A budget of `per_account` failures per name and `per_source` per address.
fn budgets(per_account: u32, per_source: u32) -> SignInLimits {
    SignInLimits {
        max_failures_per_account: per_account,
        max_failures_per_source: per_source,
        window_secs: 15 * 60,
    }
}

/// One sign-in through the route, and the answer it produced.
fn sign_in(auth: &AccountAuth, username: &str, password: &str) -> AccountReply {
    auth.handle_here(&request(
        "POST",
        auth_routes::LOGIN,
        &login_body(username, password),
    ))
    .expect("the login route is ours")
}

#[test]
fn a_spent_budget_is_answered_with_429_and_a_retry_after() {
    let (_dir, auth) = deployment_with(budgets(2, 100));
    account(&auth, "designer", &[]);

    for _ in 0..2 {
        assert_eq!(
            sign_in(&auth, "designer", "not-the-password").status,
            "401 Unauthorized"
        );
    }
    let refused = sign_in(&auth, "designer", "not-the-password");

    // `429` and not `401`: the credentials were not judged, and saying they are
    // wrong would be a lie told to somebody who may have typed them correctly.
    // `Retry-After` is the honest part — the caller is told how long the wait
    // is, which is the one thing a client can act on.
    assert_eq!(refused.status, "429 Too Many Requests");
    assert_eq!(json(&refused)["error"], "too-many-attempts");
    assert!(refused.cookies.is_empty(), "no session was started");
    let wait = refused
        .retry_after_secs
        .expect("a refusal that asks the caller to wait says for how long");
    assert!(wait > 0 && wait <= 15 * 60, "{wait}");
}

#[test]
fn a_correct_password_gets_the_same_429_while_the_budget_is_spent() {
    let (_dir, auth) = deployment_with(budgets(2, 100));
    account(&auth, "designer", &[]);
    for _ in 0..2 {
        sign_in(&auth, "designer", "not-the-password");
    }

    // The limit is not decoration: the right password is refused too, because
    // the ceiling is checked BEFORE the hash is verified — which is the whole
    // reason it also bounds what an online attack costs this process.
    let refused = sign_in(&auth, "designer", PASSWORD);
    assert_eq!(refused.status, "429 Too Many Requests");
    assert!(refused.cookies.is_empty());
}

#[test]
fn an_unknown_name_and_a_known_one_are_throttled_with_the_same_answer() {
    let (_dir, auth) = deployment_with(budgets(2, 100));
    account(&auth, "designer", &[]);
    for name in ["designer", "nobody"] {
        for _ in 0..2 {
            assert_eq!(
                sign_in(&auth, name, "not-the-password").status,
                "401 Unauthorized"
            );
        }
    }

    // Byte for byte, the RIGHT password for the account that exists and the
    // same attempt at a name that does not. A refusal that carried the name, a
    // count, or which ceiling fired would hand back the account-existence fact
    // the 401 above refuses to give; the wait travels in a header instead.
    let known = sign_in(&auth, "designer", PASSWORD);
    let unknown = sign_in(&auth, "nobody", PASSWORD);
    assert_eq!(known.status, unknown.status);
    assert_eq!(known.body, unknown.body);
    assert!(known.cookies.is_empty() && unknown.cookies.is_empty());

    // The header is NOT compared for equality, and that is not a hole: the wait
    // counts down from that name's own last failure, which the caller can
    // create for a name that does not exist just as easily as for one that
    // does. What would be an oracle is a body that described the account, and
    // those are byte-identical above.
    for wait in [known.retry_after_secs, unknown.retry_after_secs] {
        let wait = wait.expect("both say how long the wait is");
        assert!(wait > 0 && wait <= 15 * 60, "{wait}");
    }
}

#[test]
fn a_caller_spraying_unknown_names_is_refused_by_its_address() {
    let (_dir, auth) = deployment_with(budgets(100, 2));
    account(&auth, "designer", &[]);
    let source = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));

    // Each name is fresh, so the name-keyed budget never fires: what stops this
    // is the address, and this is the attack that costs the deployment an
    // Argon2id verification per request if nothing does.
    for name in ["n1", "n2"] {
        let reply = auth
            .handle_from(
                &request(
                    "POST",
                    auth_routes::LOGIN,
                    &login_body(name, "not-the-password"),
                ),
                source,
            )
            .expect("the login route is ours");
        assert_eq!(reply.status, "401 Unauthorized", "{name}");
    }
    let refused = auth
        .handle_from(
            &request(
                "POST",
                auth_routes::LOGIN,
                &login_body("n3", "not-the-password"),
            ),
            source,
        )
        .expect("the login route is ours");
    assert_eq!(refused.status, "429 Too Many Requests");
    assert!(refused.retry_after_secs.is_some());

    // And a correct password from the same address waits as well.
    let blocked = auth
        .handle_from(
            &request(
                "POST",
                auth_routes::LOGIN,
                &login_body("designer", PASSWORD),
            ),
            source,
        )
        .expect("the login route is ours");
    assert_eq!(blocked.status, "429 Too Many Requests");
}

#[test]
fn an_ordinary_refusal_never_asks_the_caller_to_retry() {
    let (_dir, auth) = deployment();
    account(&auth, "designer", &[]);

    // `Retry-After` on a 401 would invite exactly the retry the 401 is
    // describing as useless, and a client that honoured it would keep guessing.
    let refused = sign_in(&auth, "designer", "not-the-password");
    assert_eq!(refused.status, "401 Unauthorized");
    assert_eq!(refused.retry_after_secs, None);
}
