//! The account list, as an administrator's routes.
//!
//! ## What was missing
//!
//! Accepting an invitation worked; ISSUING one had no front door at all —
//! [`AccountsDb::create_invite`](crate::accounts::AccountsDb::create_invite)
//! was reachable from no command and no route — so a deployment could describe
//! an account it had no way to make. These are the routes that close it, and
//! the two reads an operator needs beside them: which invitations are
//! outstanding, and who is in the deployment.
//!
//! ## Why they live in the account tier, ahead of identity resolution
//!
//! These are `/api/auth/*` routes, and the tier that owns that prefix is
//! dispatched before the identity verifier because the requests it serves
//! cannot present a session ([`super::account_routes`]). That is a property of
//! the prefix, not of the routes: an administration route needs a caller
//! exactly as much as sign-out does, and it resolves one itself, from the same
//! credential, through the same
//! [`AccountVerifier`](super::account_verifier::AccountVerifier) the accept
//! loop would have used. Nothing here trusts a body, a query or a header for
//! WHO the caller is; the only thing that names an account is the session.
//!
//! ## The gate
//!
//! Every route here, read or write, asks
//! [`RequestAccess::decide_account_administration`] first and renders the
//! refusal with the daemon's own [`refusal_reply`](super::request_access::refusal_reply)
//! — the same `{ok:false,error,message}` body and the same `403` every other
//! route in this daemon refuses with. A guest and an ordinary contributor
//! therefore get the answer they already know how to read, and so does a
//! client.
//!
//! ## The token, and the one time it exists
//!
//! The answer to `POST /api/auth/admin/invites` is the only place the invite
//! token ever appears. The store keeps SHA-256 of it and nothing else
//! ([`crate::accounts`]), so the link cannot be recovered from the database,
//! from this tier, or from the listing below — and the listing is deliberately
//! unable to print one. What it CAN print is the row's own id: the stored hash
//! in hex, which is one-way, which no route anywhere accepts as a credential,
//! and which exists so that an operator looking at a list can name the
//! invitation they want to withdraw.
//!
//! ## Why list-then-revoke rather than revoke
//!
//! [`AccountsDb::revoke_invite`](crate::accounts::AccountsDb::revoke_invite)
//! already takes a token, which is enough for "I still have the link". The
//! ordinary case is the other one — the link went to the wrong person, and the
//! operator has a list and no link — and it is the case the pair of routes
//! exists for: the list names the row, the withdrawal takes the name.
//!
//! ## What is deliberately absent
//!
//! **No account deletion.** `documents.owner_id` lives in a DIFFERENT database
//! (the document index, beside the artwork) and is not a foreign key this
//! store can cascade: deleting an account leaves its rows owned by an id no
//! account has, and [`RequestAccess::reaches_stored_document`] refuses an
//! unattributed row online — so the files would stay on disk and become
//! unreachable through the product. `disabled` is the outcome an operator
//! actually wants (the person can no longer sign in), it is reversible, and
//! the store already keeps `delete_user` for the case where the rows really
//! must go.
//!
//! **No password reset.** A reset link is a different one-time token with a
//! different lifetime ([`crate::accounts::PASSWORD_RESET_TTL_SECS`]) and a
//! delivery problem this product has not solved; an invitation is the flow
//! that exists, and an operator who needs somebody back in can issue a new one
//! for a new account. Half of a reset flow would be worse than none.
//!
//! ## Two lists, one page each
//!
//! Both reads are paged (`?limit=&offset=`, default 100, at most
//! [`MAX_PAGE`]), because neither table shrinks on its own: a redeemed
//! invitation is a record and an account is never swept. A caller that
//! receives exactly `limit` rows asks for the next page; there is no total,
//! because a count of a table that grows while it is being read is a number
//! that is wrong by the time it is printed.

use op_editor_core::route;

use super::account_routes::{AccountAuth, AccountReply};
use super::request_access::{self, AccessRefusal, RequestAccess};
use super::tenant_auth::{IdentityVerifier, PresentedCredentials, ResolvedIdentity};
use crate::accounts::{
    canonical_roles, now_secs, AccountsError, Invite, InviteWithdrawal, NewInvite, User,
    UserStatus, INVITE_TTL_SECS,
};
use crate::mcp_serve::HttpRequest;

/// `GET` — the invitations this deployment has issued, newest first.
/// `POST` — issue one, and answer with the link.
///
/// Every path in this module stays under
/// [`op_editor_core::auth_routes::API_PREFIX`], and that is a rule rather than
/// a convention: the sensitive-POST gate keys on that prefix
/// ([`super::origin_guard`]), so a route outside it would silently lose the
/// one check standing between a cross-site page and a form it can post
/// "without a preflight". The tests pin it against that constant.
pub(super) const INVITES: &str = "/api/auth/admin/invites";
/// `POST` — withdraw an invitation by the id the listing gave it. `{"id"}`.
pub(super) const INVITE_REVOKE: &str = "/api/auth/admin/invites/revoke";
/// `GET` — the accounts in this deployment.
pub(super) const USERS: &str = "/api/auth/admin/users";
/// `POST` — replace an account's roles. `{"id","roles"}`.
pub(super) const USER_ROLES: &str = "/api/auth/admin/users/roles";
/// `POST` — enable or disable an account. `{"id","status"}`.
pub(super) const USER_STATUS: &str = "/api/auth/admin/users/status";

/// Largest page any of the list routes will return.
///
/// A ceiling rather than a policy: an operator's list of a few hundred rows is
/// a screen, and a request that asked for a million is either a bug or an
/// attempt to make this process allocate. Beyond it the caller is served a
/// page and told nothing, which is the same answer a client that did not ask
/// for a specific size gets.
const MAX_PAGE: usize = 500;

/// Rows a list route returns when the caller names no size.
const DEFAULT_PAGE: usize = 100;

/// `POST /api/auth/admin/invites` — issue an invitation, answer with its link.
///
/// The link is built from [`route::to_invite_path`], the same spelling the
/// browser shell parses (`route::invite_token`), so a link this route prints
/// is a link the accept page recognises. Only the PATH, never an absolute URL:
/// the daemon knows the origins it answers for but not which of them the
/// operator means, and a link with the wrong host is worse than one the
/// operator completes themselves.
pub(super) fn create_invite(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    let caller = match administrator(auth, request) {
        Ok(caller) => caller,
        Err(refusal) => return refusal,
    };
    let Some(fields) = InviteRequest::parse(&request.body) else {
        return AccountReply::refusal(
            "400 Bad Request",
            "malformed-body",
            "expected a JSON object, optionally with `roles` and `email`",
        );
    };
    let roles = match canonical_roles(&fields.roles) {
        Ok(roles) => roles,
        Err(unknown) => {
            return AccountReply::refusal("400 Bad Request", "unknown-role", &unknown.to_string())
        }
    };
    let role_refs: Vec<&str> = roles.iter().map(String::as_str).collect();
    let mut new_invite = NewInvite::new(&role_refs, Some(&caller.user_id), INVITE_TTL_SECS);
    if let Some(email) = fields.email.as_deref() {
        new_invite = new_invite.with_email(email);
    }
    let issued = match auth.db().create_invite(&new_invite, now_secs()) {
        Ok(issued) => issued,
        Err(error) => return store_error_reply(error),
    };
    let issued_at = now_secs();
    AccountReply::json(
        "201 Created",
        serde_json::json!({
            "ok": true,
            // The one moment this value exists on this side. It is in no
            // column, no log line and no later answer.
            "token": issued.token,
            "path": route::to_invite_path(&issued.token),
            "id": issued.id,
            "roles": roles,
            "expires_at": issued.invite.expires_at,
            "invite": invite_json(&issued.id, &issued.invite, issued_at),
        })
        .to_string(),
    )
}

/// `GET /api/auth/admin/invites` — what has been handed out, and what
/// happened to it.
pub(super) fn list_invites(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    if let Err(refusal) = administrator(auth, request) {
        return refusal;
    }
    let (limit, offset) = page_of(request);
    let now = now_secs();
    match auth.db().list_invites(limit, offset) {
        Ok(listed) => {
            let invites: Vec<serde_json::Value> = listed
                .iter()
                .map(|listed| invite_json(&listed.id, &listed.invite, now))
                .collect();
            AccountReply::json(
                "200 OK",
                serde_json::json!({ "ok": true, "invites": invites }).to_string(),
            )
        }
        Err(error) => store_error_reply(error),
    }
}

/// `POST /api/auth/admin/invites/revoke` — withdraw an invitation.
///
/// Each of the three outcomes is its own answer, because each sends the
/// operator somewhere different: the withdrawal they asked for, a link that
/// was already used (the account it made is the thing to deal with now), or a
/// list that is out of date.
pub(super) fn revoke_invite(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    if let Err(refusal) = administrator(auth, request) {
        return refusal;
    }
    let Some(id) = required_text(&request.body, "id") else {
        return AccountReply::refusal(
            "400 Bad Request",
            "malformed-body",
            "expected a JSON object with `id` — the id a listing gives an invitation",
        );
    };
    match auth.db().revoke_invite_by_id(&id) {
        Ok(InviteWithdrawal::Revoked) => AccountReply::json(
            "200 OK",
            serde_json::json!({ "ok": true, "revoked": true }).to_string(),
        ),
        Ok(InviteWithdrawal::AlreadyAccepted) => AccountReply::refusal(
            "409 Conflict",
            "invite-already-accepted",
            "this invitation has been accepted: the account it made exists, and withdrawing the \
             link cannot un-create it — disable that account instead",
        ),
        Ok(InviteWithdrawal::NotFound) => AccountReply::refusal(
            "404 Not Found",
            "invite-not-found",
            "no invitation this deployment holds has that id",
        ),
        Err(error) => store_error_reply(error),
    }
}

/// `GET /api/auth/admin/users` — who is in this deployment.
pub(super) fn list_users(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    if let Err(refusal) = administrator(auth, request) {
        return refusal;
    }
    let (limit, offset) = page_of(request);
    match auth.db().list_users(limit, offset) {
        Ok(users) => {
            let users: Vec<serde_json::Value> = users.iter().map(user_json).collect();
            AccountReply::json(
                "200 OK",
                serde_json::json!({ "ok": true, "users": users }).to_string(),
            )
        }
        Err(error) => store_error_reply(error),
    }
}

/// `POST /api/auth/admin/users/roles` — replace an account's roles.
///
/// Whole-list replacement, matching
/// [`AccountsDb::set_roles`](crate::accounts::AccountsDb::set_roles): "grant
/// these" plus "revoke those" is two operations that can interleave into a
/// state neither caller asked for.
pub(super) fn set_user_roles(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    let caller = match administrator(auth, request) {
        Ok(caller) => caller,
        Err(refusal) => return refusal,
    };
    let Some(fields) = RoleChange::parse(&request.body) else {
        return AccountReply::refusal(
            "400 Bad Request",
            "malformed-body",
            "expected a JSON object with `id` and `roles`",
        );
    };
    let roles = match canonical_roles(&fields.roles) {
        Ok(roles) => roles,
        Err(unknown) => {
            return AccountReply::refusal("400 Bad Request", "unknown-role", &unknown.to_string())
        }
    };
    // An administrator editing their OWN roles is the one edit that can lock
    // the deployment out with no way back: `op admin create` only ever makes
    // the FIRST administrator and refuses a store that already has an account,
    // and the `NORKA_ADMIN_*` variables are ignored for the same reason. So
    // the edit that would take the caller's own account list away is refused,
    // and every other administrator can still make it for them.
    if fields.id == caller.user_id
        && caller_may_manage_users(&caller)
        && !roles_confer_admin(&roles)
    {
        return self_lockout_reply(
            "this is your own account: removing your ability to manage users would leave this \
             deployment with no administrator able to restore it — ask another administrator",
        );
    }
    let role_refs: Vec<&str> = roles.iter().map(String::as_str).collect();
    match auth.db().set_roles(&fields.id, &role_refs, now_secs()) {
        Ok(()) => user_reply(auth, &fields.id),
        Err(error) => store_error_reply(error),
    }
}

/// `POST /api/auth/admin/users/status` — enable or disable an account.
///
/// Only `active` and `disabled`: those are the two states an operator
/// DECIDES. `invited` is derived by the store from whether a password exists
/// ([`crate::accounts::NewUser::status`]) and writing it by hand would produce
/// a row whose status contradicts its columns; `orphan` records that the
/// deployment lost the identity behind an account, which is a fact about a
/// provider rather than a choice made in a list. Both are named in the refusal
/// so the caller knows what to send instead.
pub(super) fn set_user_status(auth: &AccountAuth, request: &HttpRequest) -> AccountReply {
    let caller = match administrator(auth, request) {
        Ok(caller) => caller,
        Err(refusal) => return refusal,
    };
    let Some(fields) = StatusChange::parse(&request.body) else {
        return AccountReply::refusal(
            "400 Bad Request",
            "malformed-body",
            "expected a JSON object with `id` and `status`",
        );
    };
    let status = match fields.status.as_str() {
        "active" => UserStatus::Active,
        "disabled" => UserStatus::Disabled,
        other => {
            return AccountReply::refusal(
                "400 Bad Request",
                "unsupported-status",
                &format!(
                    "`{other}` is not a status an operator sets; send `active` or `disabled` \
                     (the other states are the product's own record)"
                ),
            )
        }
    };
    // Same rule as the role change above, and for the same reason: disabling
    // yourself signs you out on the next request and, if you are the only
    // administrator, leaves nobody able to undo it.
    if fields.id == caller.user_id && status == UserStatus::Disabled {
        return self_lockout_reply(
            "this is your own account: disabling it would sign you out and leave this deployment \
             with no administrator able to restore it — ask another administrator",
        );
    }
    match auth.db().set_status(&fields.id, status, now_secs()) {
        Ok(()) => user_reply(auth, &fields.id),
        Err(error) => store_error_reply(error),
    }
}

/// The caller, when this request may administer the deployment's accounts.
///
/// The one gate every route above goes through, read or write. It is here
/// rather than per-route because a route that forgot it would be a route the
/// whole account list is open through, and a route added later cannot forget a
/// call it does not have to remember to make.
fn administrator(
    auth: &AccountAuth,
    request: &HttpRequest,
) -> Result<ResolvedIdentity, AccountReply> {
    let presented = PresentedCredentials::from_request(request);
    let identity = match auth.verifier().resolve(&presented) {
        Ok(identity) => identity,
        // A missing, stale or unrecognised credential: the answer the online
        // loop already gives, with the same code and status, so a client has
        // one branch for "sign in first".
        Err(error) => {
            return Err(AccountReply::refusal(
                error.http_status(),
                error.code(),
                &error.to_string(),
            ))
        }
    };
    // The CREDENTIAL's own narrowing, before the account's rights.
    //
    // Every other REST route gets this from `super::tool_scopes` on the way
    // through the connection tier — and this tier is dispatched AHEAD of that
    // one, because `/api/auth/*` is where a request that has no session yet
    // has to arrive. Skipping it here would mean the same token held to a read
    // scope on `/api/mcp/document` and free to hand out administrator
    // invitations on `/api/auth/admin/*`: two answers to one question, which
    // is exactly what `tool_scopes` was written to stop.
    //
    // Inert today — every credential this deployment issues carries
    // `McpScopes::FULL` (`account_verifier::identity_from_user`) — and it is
    // here rather than left to the day narrower tokens exist, because that is
    // the day nobody would think to look at this file.
    if let Some(refusal) = credential_refusal(
        identity.via,
        identity.scopes,
        &request.method,
        &request.path,
    ) {
        return Err(refusal);
    }
    match RequestAccess::deployment(&identity).decide_account_administration() {
        Ok(()) => Ok(identity),
        Err(refusal) => Err(access_refusal_reply(refusal)),
    }
}

/// What the credential itself may do with this request, as this tier answers
/// it.
///
/// Its own function so the rule is provable without a credential the product
/// cannot yet mint: the account store hands every account `McpScopes::FULL`,
/// so no route-level test can reach the refusal below, and a rule with no test
/// is a rule that stops being true quietly.
fn credential_refusal(
    via: super::tenant_auth::IdentityVia,
    scopes: crate::mcp_serve::tool_profile::McpScopes,
    method: &str,
    path: &str,
) -> Option<AccountReply> {
    super::tool_scopes::check_rest_scope(via, scopes, method, path).map(|refusal| {
        AccountReply::refusal(refusal.http_status(), refusal.code(), &refusal.to_string())
    })
}

/// The daemon's coded refusal, in the account tier's reply shape.
///
/// The body is rendered by the same function every other route uses
/// ([`request_access::refusal_reply`]) and only the shape is adapted, so a
/// client that already knows how this daemon says "not for you" needs no
/// branch for the account list.
fn access_refusal_reply(refusal: AccessRefusal) -> AccountReply {
    let reply = request_access::refusal_reply(refusal);
    AccountReply::json(reply.status, reply.body)
}

/// The refusal a self-lockout earns.
fn self_lockout_reply(message: &str) -> AccountReply {
    AccountReply::refusal("409 Conflict", "self-lockout-refused", message)
}

/// Whether the caller's own roles carry the account list.
fn caller_may_manage_users(caller: &ResolvedIdentity) -> bool {
    caller.roles.rights().can_manage_users()
}

/// Whether a role list would leave its holder able to manage users.
///
/// Asked through the same vocabulary the grant itself is read through, so
/// "which roles carry the account list" has one answer: the caller who keeps
/// `admin` keeps the right, whoever survives.
fn roles_confer_admin(roles: &[String]) -> bool {
    op_editor_core::access::RoleSet::from_wire(roles)
        .rights()
        .can_manage_users()
}

/// The account a change was made to, as the answer to the change.
///
/// Re-read rather than assembled from the request: what the caller gets back
/// is the row as the store now holds it, so a body that could not be written
/// (a role the vocabulary folded, a status the schema refused) is visible in
/// the answer instead of being inferred from the request.
fn user_reply(auth: &AccountAuth, user_id: &str) -> AccountReply {
    match auth.db().find_user_by_id(user_id) {
        Ok(Some(user)) => AccountReply::json(
            "200 OK",
            serde_json::json!({ "ok": true, "user": user_json(&user) }).to_string(),
        ),
        Ok(None) => AccountReply::refusal(
            "404 Not Found",
            "user-not-found",
            "no account this deployment holds has that id",
        ),
        Err(error) => store_error_reply(error),
    }
}

/// A stored failure, as this tier answers it.
///
/// Never "your request was wrong": a store that cannot be read is a deployment
/// that cannot answer, and the same distinction the sign-in routes make —
/// between a refusal and a fault — is what keeps a disk error from being read
/// as an authorization decision.
fn store_error_reply(error: AccountsError) -> AccountReply {
    match error {
        // A name, address or role list the store will not accept as written.
        AccountsError::InvalidText { .. } => {
            AccountReply::refusal("400 Bad Request", "invalid-account", &error.to_string())
        }
        AccountsError::NoSuchUser { .. } => AccountReply::refusal(
            "404 Not Found",
            "user-not-found",
            "no account this deployment holds has that id",
        ),
        _ => AccountReply::refusal(
            "503 Service Unavailable",
            "accounts-unavailable",
            "the account store cannot be written right now",
        ),
    }
}

/// One invitation, as a listing or a creation shows it.
fn invite_json(id: &str, invite: &Invite, now: i64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "email": invite.email,
        "roles": invite.roles,
        "created_by": invite.created_by,
        "created_at": invite.created_at,
        "expires_at": invite.expires_at,
        "accepted_by": invite.accepted_by,
        "accepted_at": invite.accepted_at,
        // The one thing an operator scanning a list actually reads: is this
        // link still good. Derived from the row's own predicate, so a listing
        // and the acceptance path cannot disagree about a link's state.
        "state": invite_state(invite, now),
    })
}

/// What an invitation's row amounts to at `now`.
fn invite_state(invite: &Invite, now: i64) -> &'static str {
    if invite.accepted_at.is_some() {
        // Checked first: a spent link that has also passed its date is
        // reported as spent, because that is the fact that explains the
        // account which exists.
        "accepted"
    } else if invite.is_redeemable_at(now) {
        "pending"
    } else {
        "expired"
    }
}

/// One account, as the account list shows it.
///
/// No password hash, no session, no credential of any kind — the fields are
/// the ones an operator acts on, and `password_hash` is the one a route must
/// never have a reason to carry. `roles` is the stored list verbatim: a role
/// this build does not recognise is shown as what the database holds rather
/// than hidden, because the account carrying it is exactly the one somebody
/// needs to look at.
fn user_json(user: &User) -> serde_json::Value {
    serde_json::json!({
        "id": user.id,
        "username": user.username,
        "display_name": user.display_name,
        "email": user.email,
        "roles": user.roles,
        "status": user.status.as_str(),
        // When the account was last seen anywhere, which is the difference
        // between an account nobody uses and an account that was never
        // finished being set up.
        "last_seen_at": user.last_seen_at,
        "created_at": user.created_at,
        // Whether the account can be signed into at all. `invited` already
        // says so, and this is what answers it for a `disabled` account whose
        // password may or may not ever have been set.
        "has_password": user.has_password(),
    })
}

/// `POST /api/auth/admin/invites` body: both fields optional.
struct InviteRequest {
    roles: Vec<String>,
    email: Option<String>,
}

impl InviteRequest {
    fn parse(body: &str) -> Option<Self> {
        let parsed: serde_json::Value = parse_object(body)?;
        let email = parsed["email"]
            .as_str()
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map(str::to_string);
        Some(Self {
            roles: text_list(&parsed, "roles")?,
            email,
        })
    }
}

/// `POST /api/auth/admin/users/roles` body.
struct RoleChange {
    id: String,
    roles: Vec<String>,
}

impl RoleChange {
    fn parse(body: &str) -> Option<Self> {
        let parsed: serde_json::Value = parse_object(body)?;
        Some(Self {
            id: required_text(body, "id")?,
            // `roles` may be absent or empty, and that means "no roles" — the
            // view-only floor — rather than a malformed request: taking every
            // role away is a thing an administrator does.
            roles: text_list(&parsed, "roles")?,
        })
    }
}

/// `POST /api/auth/admin/users/status` body.
struct StatusChange {
    id: String,
    status: String,
}

impl StatusChange {
    fn parse(body: &str) -> Option<Self> {
        // Same reason as `RoleChange`: the body has to BE an object before
        // anything in it is read, and every field is read out of it directly.
        parse_object(body)?;
        Some(Self {
            id: required_text(body, "id")?,
            status: required_text(body, "status")?,
        })
    }
}

/// A body that is a JSON object, or nothing.
fn parse_object(body: &str) -> Option<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    // An array or a bare number is not a body this route has a reading for,
    // and treating it as an empty object would answer "you sent nothing" for
    // something the caller plainly sent.
    parsed.is_object().then_some(parsed)
}

/// A required non-empty string field of a request body.
///
/// Parses the body itself so the two callers cannot disagree about whether
/// "an object" was required — the helper is total, and an unparsable body
/// simply has no fields.
fn required_text(body: &str, field: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed[field]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// A list-of-strings field, or nothing when it is present and not one.
///
/// An ABSENT field is an empty list, not a failure: both bodies here treat
/// "no roles" as a statement rather than as a missing one. A field that is
/// there and holds something else — a string, a number, a nested object — is a
/// body this route has no reading for, and `None` is what says so.
fn text_list(parsed: &serde_json::Value, field: &str) -> Option<Vec<String>> {
    match parsed.get(field) {
        None | Some(serde_json::Value::Null) => Some(Vec::new()),
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .map(|entry| entry.as_str().map(str::to_string))
            .collect(),
        Some(_) => None,
    }
}

/// The page a list route was asked for, clamped to what it will serve.
fn page_of(request: &HttpRequest) -> (usize, usize) {
    let query = request.query.as_deref().unwrap_or_default();
    let limit = query_number(query, "limit").unwrap_or(DEFAULT_PAGE);
    let offset = query_number(query, "offset").unwrap_or(0);
    // Zero rows is not a page, it is a request that cannot be satisfied
    // usefully; one row is. And `offset` is unbounded on purpose — a table
    // that grew past a page is exactly the case the caller is walking.
    (limit.clamp(1, MAX_PAGE), offset)
}

/// One non-negative integer out of a query string.
fn query_number(query: &str, name: &str) -> Option<usize> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| value.parse().ok())
}

/// Every route here, driven the way the accept loop drives them.
#[cfg(test)]
#[path = "account_admin_routes/tests.rs"]
mod tests;
