//! Who may do what with the document a request is served against.
//!
//! Until now the daemon answered this question by not asking it. The local
//! operator's daemon trusted its one client ([`ServeMode::Local`] still does,
//! deliberately), and the public deployment refused whole route families
//! instead of deciding per caller. That holds only while a document has no
//! owner and an account has no roles — which is exactly what the roles work
//! (#10) removes.
//!
//! [`RequestAccess`] is the input to that decision and nothing else: the
//! deployment mode, the document's owner, the caller's verified identity with
//! its roles, and whether the owner's access list admits the caller. It
//! deliberately holds no `WebCanvasState`, no `TenantLease` and no mutex —
//! a route needs to know who is asking, not how to reach the editor.
//!
//! [`RequestAccess::decide`] is the decision. It is pure: no state, no I/O, no
//! locks, so every rule below is provable by a unit test and no route can
//! drift from another route's answer. Routes call it and render the refusal;
//! they decide nothing themselves.
//!
//! ## The two questions, in this order
//!
//! 1. **May this caller see this document at all?** The owner always may;
//!    anyone else only if the owner's access list names them. Refused as
//!    [`AccessRefusal::NotShared`] — the same code the tenant lease already
//!    answers with, because it is the same statement about the same document.
//! 2. **Does a role the caller holds grant the write?** [`Rights::Edit`] is
//!    the right every mutating route needs; without it the caller reads.
//!    Refused as [`AccessRefusal::ReadOnly`].
//!
//! Order matters: an account with no roles must not learn whether a document
//! it may not open has anything worth changing, and a stranger must not get a
//! different answer for a read than for a write.
//!
//! ## Why an empty role list reads but never writes
//!
//! "No roles" means "no roles", not "no access". The hub may send none (an
//! account with no product role yet), and a deployment may have no role
//! vocabulary at all ([`StaticVerifier`](super::tenant_auth::StaticVerifier)'s
//! env token table). `RoleSet::rights` floors that at
//! [`Rights::VIEW_ONLY`], so such a caller reads a document they were given
//! and is refused every write — the fail-closed direction, and the one that
//! cannot silently hand out an edit.
//!
//! ## Ownership grants edit — on your own document only
//!
//! An owner may change the document they own, whatever roles they hold, and
//! that is the operator's decision: the file is theirs to work on, and a
//! deployment whose hub sends no roles would otherwise be read-only for the
//! very people the documents belong to.
//!
//! What this is **not** is the rule this module first rejected. "Ownership
//! implies edit" as a general principle would hand writes on *any* document to
//! whoever owns it and, when a hub fails to send roles, to every account at
//! once. Here it reaches exactly one file — the caller's own — and only after
//! question one has already established that the caller may see it. A stranger
//! is still refused at the door, and a shared visitor with no editing role
//! still reads.
//!
//! ## Why delete and restore are not separate rights
//!
//! [`DocumentAction::Delete`] and [`DocumentAction::Restore`] exist so a route
//! names what it does rather than choosing a right, and so a later split is a
//! change to one function instead of to every route. Today both require
//! [`Rights::Edit`]: the operator's matrix has no level that separates them
//! from editing, and inventing one here would be policy this code made up
//! rather than policy it models.

use op_editor_core::access::Rights;

use super::online_policy::ServeMode;
use super::tenant_auth::ResolvedIdentity;

/// One thing a route asks to do with a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocumentAction {
    /// Look at it: list the store, open a document, read its preview, ask
    /// about the recovery draft.
    View,
    /// Change it: create, save, autosave, rename, or write a draft.
    Edit,
    /// Remove it from the store.
    Delete,
    /// Adopt the recovery draft as the open document.
    Restore,
}

impl DocumentAction {
    /// Every action, in ascending authority.
    pub const ALL: [Self; 4] = [Self::View, Self::Edit, Self::Delete, Self::Restore];

    /// Stable name, for logs and for a refusal that has to say what was asked.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Edit => "edit",
            Self::Delete => "delete",
            Self::Restore => "restore",
        }
    }

    /// Whether this action changes stored state.
    pub const fn is_write(self) -> bool {
        !matches!(self, Self::View)
    }
}

/// Why a caller may not do what it asked.
///
/// A named type rather than a status code because the two refusals mean
/// different things to whoever reads the response: one says "this document is
/// not yours", the other "you may read it but not change it". The daemon's
/// other refusal types were checked first and none fits —
/// [`WebCanvasError`](crate::web_canvas_server_error::WebCanvasError) has no
/// authorization variant, `OnlineRouteRefusal` is about a route being disabled
/// for the whole deployment, and `DaemonMutationRefusal` is about the state of
/// a live collaboration session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRefusal {
    /// The caller is not the document's owner and the owner's access list does
    /// not name it.
    NotShared,
    /// The caller may reach the document, but no role it holds grants a write.
    ReadOnly,
}

impl AccessRefusal {
    /// Stable machine-readable code for a REST body.
    pub const fn code(self) -> &'static str {
        match self {
            // Deliberately the same code `TenantError::NotShared` answers with
            // when the lease itself is refused. The two happen at different
            // points of one request, and a client must not have to learn two
            // spellings of "this document is not yours".
            Self::NotShared => "tenant-not-shared",
            Self::ReadOnly => "read-only-role",
        }
    }

    /// HTTP status this refusal maps to.
    ///
    /// `403` for both, and never `404`: the caller has already been told the
    /// document exists by the route it called, so hiding it now would be a
    /// lie the next request contradicts.
    pub const fn http_status(self) -> &'static str {
        "403 Forbidden"
    }
}

impl std::fmt::Display for AccessRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotShared => f.write_str("this document is not shared with you"),
            Self::ReadOnly => {
                f.write_str("your roles do not allow changing this document")
            }
        }
    }
}

impl std::error::Error for AccessRefusal {}

/// Who is asking, and what they may do — the input to every route decision.
///
/// Built where the facts are known and passed down as a value: the accept loop
/// has the lease and the verified identity, the routes have neither. See the
/// module docs for the model.
#[derive(Debug, Clone, Copy)]
pub struct RequestAccess<'a> {
    /// How this daemon is deployed. Local and managed answer every action
    /// `Ok`; online answers from the caller's roles.
    mode: ServeMode,
    /// The account the document belongs to. `None` outside online, where the
    /// owner is the local operator and has no account id.
    owner_id: Option<&'a str>,
    /// The verified caller. `None` in a deployment that has no accounts.
    caller: Option<&'a ResolvedIdentity>,
    /// Whether `owner_id`'s access list admits the caller.
    ///
    /// Supplied by the caller of this constructor because only the tenant
    /// registry can answer it — and it must be answered by the same check that
    /// decided the request may be served at all (`TenantRegistry::
    /// lease_for_shared`), so the two can never disagree.
    shared_with_caller: bool,
}

impl<'a> RequestAccess<'a> {
    /// The local operator: `Local` and `Managed` deployments, which have no
    /// accounts and therefore nothing to distinguish between callers.
    ///
    /// Handing this `ServeMode::Online` does not open anything: online always
    /// answers from an account, and there is none here, so every action is
    /// refused as [`AccessRefusal::NotShared`]. That is deliberate — a
    /// constructor that could only ever fail closed is worth more than one
    /// that has to be trusted to be called with the right mode.
    pub const fn local_operator(mode: ServeMode) -> Self {
        Self {
            mode,
            owner_id: None,
            caller: None,
            shared_with_caller: false,
        }
    }

    /// An online request, served on `owner_id`'s document on behalf of
    /// `caller`.
    ///
    /// `shared_with_caller` is the answer to "does the owner's access list
    /// name this account", which the tenant registry has already given when it
    /// handed out the lease. The owner needs no such answer: ownership is
    /// checked from the two ids, so a missing list entry cannot lock an owner
    /// out of their own document.
    pub fn online(
        owner_id: &'a str,
        caller: &'a ResolvedIdentity,
        shared_with_caller: bool,
    ) -> Self {
        Self {
            mode: ServeMode::Online,
            owner_id: Some(owner_id),
            caller: Some(caller),
            shared_with_caller,
        }
    }

    /// The decision. See the module docs for the model it implements.
    pub fn decide(&self, action: DocumentAction) -> Result<(), AccessRefusal> {
        // A deployment with no accounts has nothing to decide. This is the
        // branch that lets the roles model land without touching local work:
        // the operator's own daemon answers exactly as it always has.
        if !self.mode.is_online() {
            return Ok(());
        }
        // Question one — the document. Refused before the action is looked at,
        // so a stranger cannot tell a read from a write by the answer.
        if !self.reaches_document() {
            return Err(AccessRefusal::NotShared);
        }
        // Question two — the roles. Reading needs only question one.
        if action == DocumentAction::View {
            return Ok(());
        }
        // The owner of a document may change it. Their own file is theirs to
        // work on, and a deployment whose hub sends no roles would otherwise
        // be read-only for the very people the documents belong to.
        //
        // This is not the "ownership implies edit" that the module docs reject:
        // that would have granted writes on *other people's* documents to
        // whoever the owner happens to be, and to every account when a hub
        // fails to send roles for anyone. Here it reaches exactly one file —
        // the caller's own — and a stranger is still stopped by question one.
        if self.is_owner() {
            return Ok(());
        }
        if self.rights().can_edit() {
            Ok(())
        } else {
            Err(AccessRefusal::ReadOnly)
        }
    }

    /// Whether the caller owns the document being served.
    fn is_owner(&self) -> bool {
        match (self.owner_id, self.caller) {
            (Some(owner), Some(caller)) => owner == caller.user_id,
            _ => false,
        }
    }

    /// Whether this caller may see the document at all.
    ///
    /// The owner passes by identity, everyone else by the access list. Both
    /// halves must be present: an online request with no owner or no verified
    /// caller answers `false`, so a carrier built without an identity refuses
    /// rather than falling open.
    fn reaches_document(&self) -> bool {
        match (self.owner_id, self.caller) {
            (Some(owner), Some(caller)) => {
                owner == caller.user_id.as_str() || self.shared_with_caller
            }
            _ => false,
        }
    }

    /// What the caller's roles allow.
    ///
    /// Outside online there is no caller and this is never consulted: the mode
    /// branch in [`Self::decide`] answers before it.
    fn rights(&self) -> Rights {
        self.caller
            .map(|caller| caller.roles.rights())
            .unwrap_or(Rights::NONE)
    }

    /// The deployment mode this decision is being made under.
    pub const fn mode(&self) -> ServeMode {
        self.mode
    }

    /// The verified caller's account id, when there is one.
    pub fn caller_id(&self) -> Option<&str> {
        self.caller.map(|caller| caller.user_id.as_str())
    }

    /// The account the document belongs to, when it has one.
    pub const fn owner_id(&self) -> Option<&'a str> {
        self.owner_id
    }
}

/// Render a refusal as the daemon's standard coded-error REST reply.
///
/// Same shape as a deployment-level refusal (`{ok:false, error, message}`), so
/// a client that already handles "this route is off here" needs no second
/// branch for "not for you".
pub(super) fn refusal_reply(refusal: AccessRefusal) -> super::WebReply {
    super::online_policy::coded_refusal_reply(
        refusal.http_status(),
        refusal.code(),
        &refusal.to_string(),
    )
}

/// The route tier under the local operator's access, for tests.
///
/// A local deployment has no accounts, so every decision a route makes is
/// already answered — this exists so the route tests written before roles
/// existed keep calling the dispatcher in the four-argument shape they were
/// written against, instead of each of them spelling out a `RequestAccess`.
/// An online caller's narrowed authority is asserted by the tests that build
/// one deliberately (`files_routes_access_tests`), never by this shim.
///
/// The access it builds mirrors the state's own mode, so a test that drives an
/// online tenant through it is refused rather than quietly granted.
#[cfg(test)]
pub(crate) fn handle_local_request(
    method: &str,
    path: &str,
    body: &str,
    state: &mut super::WebCanvasState,
) -> super::WebReply {
    super::handle_web_canvas_request(
        method,
        path,
        body,
        state,
        &RequestAccess::local_operator(state.mode),
    )
}

#[cfg(test)]
#[path = "request_access_tests.rs"]
mod tests;
