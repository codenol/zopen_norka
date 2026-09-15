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
//! 2. **Does a role the caller holds grant what was asked?** A read is answered
//!    by question one alone. Everything that writes asks for the right its
//!    [`DocumentAction`] names: [`Rights::Edit`] for changing the document,
//!    [`Rights::Comment`] for taking part in the conversation about it — the
//!    one write the operator's matrix grants below editing (see the `Comment`
//!    variant). Refused as [`AccessRefusal::ReadOnly`].
//!
//! Order matters: an account with no roles must not learn whether a document
//! it may not open has anything worth changing, and a stranger must not get a
//! different answer for a read than for a write.
//!
//! ## Why one action is not one right
//!
//! [`DocumentAction`] exists so a route names what it does instead of choosing
//! a right, and so that a change to the policy is a change to one function
//! rather than to every route. The map from action to right is not the identity
//! because the operator's levels are not nested on the writing side: the five
//! contributor roles may comment and may not edit, so "may write" is two
//! questions, not one. It stays a small closed set — an action is added when a
//! route's decision genuinely differs from every existing one, not to describe
//! what a handler happens to do.
//!
//! ## Whose document, though — a question about the STORE
//!
//! Those two questions are about "the document this request is served
//! against", and the accept loop names that by taking a lease: the lease says
//! whose workspace the request is for. A stored document is named by a KEY
//! instead, and every account's files live in one directory, so the key on its
//! own says nothing about whose row it is. [`RequestAccess::
//! reaches_stored_document`] is that third question, asked by the
//! stored-document routes after the gate above and before they touch a file:
//! the row's owner is compared with the caller's id and with the lease's. It
//! is what a shared deployment was missing when it refused the whole
//! stored-document family instead (#20).
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
use op_editor_core::ShareLevel;

use super::online_policy::ServeMode;
use super::tenant_auth::ResolvedIdentity;

/// One thing a route asks to do with a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocumentAction {
    /// Look at it: list the store, open a document, read its preview, ask
    /// about the recovery draft, read its comments.
    View,
    /// Take part in the conversation about it: open a thread on an element,
    /// reply, close or reopen one.
    ///
    /// Its own action rather than a spelling of [`DocumentAction::Edit`],
    /// because the operator's matrix has a level that the two split: the five
    /// contributor roles (ПО, Аналитик, Фронт, Бэк, QA) may comment and may not
    /// edit. Asking for `Edit` here would refuse the very people comments exist
    /// for, and asking for `View` would hand them to a guest who was given only
    /// a link to read. The right itself already exists in the model
    /// (`op_editor_core::access::Rights::can_comment`, which is what
    /// [`Rights::CONTRIBUTOR`] carries); this is the action that reaches it.
    Comment,
    /// Change it: create, save, autosave, rename, or write a draft.
    Edit,
    /// Add somebody else to it: write an entry on this document's access list.
    ///
    /// Its own action rather than a spelling of [`DocumentAction::Edit`],
    /// because the operator's matrix gives the invite right to a level that
    /// does not edit: the five contributor roles may add people and may not
    /// change the document. It is the right the Share dialog's Invite button
    /// asks about, and the same question the `/api/share/*` routes decide —
    /// asked here so there is one answer rather than two.
    Invite,
    /// Remove it from the store.
    Delete,
    /// Adopt the recovery draft as the open document.
    Restore,
}

impl DocumentAction {
    /// Every action, in ascending authority.
    pub const ALL: [Self; 6] = [
        Self::View,
        Self::Comment,
        Self::Invite,
        Self::Edit,
        Self::Delete,
        Self::Restore,
    ];

    /// Stable name, for logs and for a refusal that has to say what was asked.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Comment => "comment",
            Self::Invite => "invite",
            Self::Edit => "edit",
            Self::Delete => "delete",
            Self::Restore => "restore",
        }
    }

    /// Whether this action changes stored state.
    ///
    /// A comment does, and it is worth being exact about which state: it writes
    /// a row, and it writes it to the conversation rather than to the document
    /// (see `super::comment_routes` — no comment route touches the editor, the
    /// document's version or its file). Reading is still the only action that
    /// changes nothing, which is what this predicate is for.
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
    /// The caller is a perfectly ordinary signed-in account, and the thing it
    /// asked for is not about documents at all: it is the deployment's account
    /// list, which only a role carrying
    /// [`Rights::can_manage_users`](op_editor_core::access::Rights::can_manage_users)
    /// reaches.
    ///
    /// A refusal of its own rather than a spelling of [`Self::ReadOnly`],
    /// because the two send the person who reads them to different places:
    /// "your roles do not allow changing this document" is about a file, and
    /// the account list is not a file. The status and the shape are the same
    /// `403` every other refusal here uses.
    NotAnAdministrator,
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
            Self::NotAnAdministrator => "admin-role-required",
        }
    }

    /// HTTP status this refusal maps to.
    ///
    /// `403` for all three, and never `404`: the caller has already been told
    /// the document exists by the route it called, so hiding it now would be a
    /// lie the next request contradicts. For the account list the same holds
    /// for a different reason — the route's existence is not a secret, and
    /// answering `404` would tell a signed-in colleague that the deployment
    /// has no account list rather than that they may not open it.
    pub const fn http_status(self) -> &'static str {
        "403 Forbidden"
    }
}

impl std::fmt::Display for AccessRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotShared => f.write_str("this document is not shared with you"),
            Self::ReadOnly => f.write_str("your roles do not allow changing this document"),
            Self::NotAnAdministrator => {
                f.write_str("this deployment's account list is for accounts that may manage users")
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
    /// The level `owner_id`'s access list grants the caller, or `None` when it
    /// does not name them.
    ///
    /// Supplied by the caller of this constructor because only the tenant
    /// registry can answer it — and it must be answered by the same check that
    /// decided the request may be served at all (`TenantRegistry::
    /// lease_for_shared`), so the two can never disagree.
    ///
    /// A level rather than a `bool` because a share is not all-or-nothing any
    /// more: the document's access list says HOW MUCH, and the answer is a
    /// ceiling over the caller's roles (see [`Self::rights`]).
    grant: Option<ShareLevel>,
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
            grant: None,
        }
    }

    /// An online request, served on `owner_id`'s document on behalf of
    /// `caller`.
    ///
    /// `grant` is the answer to "does the owner's access list name this
    /// account, and at what level" — which the tenant registry has already
    /// given when it handed out the lease. The owner needs no such answer:
    /// ownership is checked from the two ids, so a missing list entry cannot
    /// lock an owner out of their own document.
    pub fn online(
        owner_id: &'a str,
        caller: &'a ResolvedIdentity,
        grant: Option<ShareLevel>,
    ) -> Self {
        Self {
            mode: ServeMode::Online,
            owner_id: Some(owner_id),
            caller: Some(caller),
            grant,
        }
    }

    /// A request against the DEPLOYMENT itself, on behalf of a verified
    /// `caller`: the account list, the invitations, the roles.
    ///
    /// ## Why this is not [`Self::online`] with the caller as its own owner
    ///
    /// Because that is exactly the mistake the account list must not make.
    /// Every online request is served on somebody's tenant, and a caller with
    /// no `?tenant=` is the owner of its own — so "is this workspace yours to
    /// configure" is true for every signed-in account, which is right for a
    /// settings file and catastrophic for the account list. The deployment's
    /// accounts are not a tenant's property: there is one `accounts.db` for
    /// the whole deployment, and the question "whose workspace is this" has no
    /// answer for it at all.
    ///
    /// So `owner_id` is `None` here, deliberately. Every document-shaped
    /// decision below answers [`AccessRefusal::NotShared`] for a carrier built
    /// this way — a route that reached for [`Self::decide`] with it would
    /// refuse rather than fall open — and
    /// [`Self::decide_account_administration`] is the only question it can
    /// answer.
    pub const fn deployment(caller: &'a ResolvedIdentity) -> Self {
        Self {
            mode: ServeMode::Online,
            owner_id: None,
            caller: Some(caller),
            grant: None,
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
        let rights = self.rights();
        // Which right answers is chosen by the action, and the match is
        // exhaustive so that adding an action forces the question rather than
        // silently inheriting `Edit` (or, worse, inheriting the `View` answer
        // above).
        let allowed = match action {
            // Unreachable — `View` returned above. Kept explicit because the
            // match is what makes the next action a decision.
            DocumentAction::View => true,
            DocumentAction::Comment => rights.can_comment(),
            // Inviting is about a document's ACCESS LIST, and only the account
            // whose list it is may rewrite it. The owner returned above; here
            // the caller is a visitor, and the invite right a contributor role
            // carries is the right to add people to their OWN document rather
            // than a licence to re-share somebody else's. The `/api/share/*`
            // routes say the same thing structurally — they edit the caller's
            // list and never the one a `?tenant=` parameter pointed at — and
            // this line is what makes the two agree instead of merely
            // coinciding. An account that manages users (a deployment
            // administrator) is the deliberate exception: that power belongs to
            // the deployment rather than to the document.
            DocumentAction::Invite => self.role_rights().can_manage_users(),
            DocumentAction::Edit | DocumentAction::Delete | DocumentAction::Restore => {
                rights.can_edit()
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(AccessRefusal::ReadOnly)
        }
    }

    /// May this caller close or reopen one thread — this one?
    ///
    /// A question [`Self::decide`] cannot answer, because it is about an OBJECT
    /// rather than about the document. The operator's rule, and Figma's: a
    /// thread may be closed by whoever opened it, and by anyone who may edit
    /// the document — a review is triaged by the designer, who is closing other
    /// people's threads all day, and a conversation nobody can close is a
    /// conversation nobody finishes.
    ///
    /// `thread_author` is the account id the thread records; `None` for a
    /// thread the local operator opened, which has no account behind it, so
    /// only the editing half can answer for it.
    ///
    /// Refused as [`AccessRefusal::ReadOnly`] — the code the daemon already has
    /// for "you may reach this but may not change it". A refusal of its own
    /// would make every client learn a second spelling of the same 403, and the
    /// caller who sees it here may genuinely take part in the conversation;
    /// they are simply not its author and not an editor.
    ///
    /// The route asks [`DocumentAction::Comment`] BEFORE this, so the caller
    /// who arrives here may comment. That order is what keeps a guest who was
    /// given a link to read from closing threads: they are refused at the floor
    /// and never reach the author test at all.
    pub fn decide_thread_resolution(
        &self,
        thread_author: Option<&str>,
    ) -> Result<(), AccessRefusal> {
        // One operator, one client, nothing to decide — the same branch, and
        // the same reason, as every other decision here.
        if !self.mode.is_online() {
            return Ok(());
        }
        // Reach first: a stranger learns nothing about a thread they cannot
        // open its document to see.
        if !self.reaches_document() {
            return Err(AccessRefusal::NotShared);
        }
        // The owner of a document may change it (see [`Self::decide`]), so they
        // may close what is said about it — including when no role grants them
        // anything.
        if self.is_owner() {
            return Ok(());
        }
        if let (Some(author), Some(caller)) = (thread_author, self.caller_id()) {
            if author == caller {
                return Ok(());
            }
        }
        if self.rights().can_edit() {
            Ok(())
        } else {
            Err(AccessRefusal::ReadOnly)
        }
    }

    /// May this caller change the account's own configuration — the AI provider
    /// credentials and the MCP server settings?
    ///
    /// A different question from [`Self::decide`], and deliberately so. Being
    /// allowed to edit a document someone shared with you must not hand over
    /// the keys that account pays for, so the right that answers here is not
    /// "may you write" but "is this workspace yours to configure": the owner,
    /// and whoever the operator's matrix gives the account list to
    /// ([`Rights::can_manage_users`], which today only the admin role sets).
    ///
    /// Refused as [`AccessRefusal::ReadOnly`] rather than `NotShared`: a shared
    /// visitor is not a stranger — the document is theirs to read — it simply
    /// holds no right over this workspace's configuration. A carrier with no
    /// owner or no verified caller answers `NotShared`, because then there is
    /// no workspace to attribute at all; that is the fail-closed direction and
    /// keeps [`Self::local_operator`] handed `Online` refusing rather than
    /// granting.
    pub fn decide_workspace_settings(&self) -> Result<(), AccessRefusal> {
        // A deployment with no accounts configures itself the way it always
        // has: one operator, one settings file, nothing to decide.
        if !self.mode.is_online() {
            return Ok(());
        }
        if self.owner_id.is_none() || self.caller.is_none() {
            return Err(AccessRefusal::NotShared);
        }
        if self.is_owner() || self.role_rights().can_manage_users() {
            Ok(())
        } else {
            Err(AccessRefusal::ReadOnly)
        }
    }

    /// May this caller read and change the DEPLOYMENT's account list — who
    /// else is in it, what roles they hold, which invitations are outstanding?
    ///
    /// ## Why this is not [`Self::decide_workspace_settings`]
    ///
    /// That question is "is this workspace yours to configure", and it answers
    /// `Ok` for the tenant's owner. Applied to the account list it would be
    /// true for EVERY signed-in account — every request without a `?tenant=`
    /// is served on the caller's own tenant, so every caller is an owner of
    /// something — and the deployment's account list would be readable, and
    /// re-roleable, by anybody who can sign in. It is the right answer one
    /// right over: fine for a settings file that belongs to an account, wrong
    /// for a table that belongs to the deployment.
    ///
    /// So the owner half is deliberately absent and the question is the roles
    /// alone: [`Rights::can_manage_users`], which today only the admin role
    /// sets. It is the same right `decide_workspace_settings` consults for the
    /// half of ITS answer that is not the owner, so there is still one place
    /// that decides what "may manage users" means.
    ///
    /// `owner_id` is not read, and a carrier built by [`Self::deployment`] has
    /// none to read — see its docs for why the deployment is nobody's tenant.
    ///
    /// Refused as [`AccessRefusal::NotAnAdministrator`]; a caller with no
    /// verified identity at all is [`AccessRefusal::NotShared`], because there
    /// is no account to ask the question about and "no roles" must not read as
    /// "no roles needed".
    pub fn decide_account_administration(&self) -> Result<(), AccessRefusal> {
        // A deployment with no accounts administers nothing and has no
        // account list: the operator's own daemon answers exactly as it always
        // has, and this branch is what keeps a local route from being refused
        // by a question about roles it cannot have.
        if !self.mode.is_online() {
            return Ok(());
        }
        let Some(caller) = self.caller else {
            return Err(AccessRefusal::NotShared);
        };
        if caller.roles.rights().can_manage_users() {
            Ok(())
        } else {
            Err(AccessRefusal::NotAnAdministrator)
        }
    }

    /// Whether the caller owns the document being served.
    fn is_owner(&self) -> bool {
        match (self.owner_id, self.caller) {
            (Some(owner), Some(caller)) => owner == caller.user_id,
            _ => false,
        }
    }

    /// Whether a document the STORE holds belongs to an account this caller may
    /// address.
    ///
    /// A second question, and a different one from [`Self::decide`]. `decide`
    /// asks about "the document this request is served against", which the
    /// accept loop names by taking a lease: it establishes that the caller may
    /// reach *that account's workspace*. A stored document is addressed by a
    /// key, and every account's documents live in one directory, so the key on
    /// its own says nothing about who owns the row it names. Answering the
    /// lease question and skipping this one is how a caller of one tenant reads
    /// another tenant's file by naming its key (#20).
    ///
    /// `owner` is the account the stored row belongs to, `None` when no account
    /// stands behind it — the local operator's own rows, and every row the
    /// legacy index import brought over. Online that is nobody's: there is no
    /// operator in a shared deployment, and handing an unattributed file to the
    /// first caller that names its key is the leak this check exists to close.
    ///
    /// The three ways a document is reachable online:
    ///
    /// 1. the caller owns it;
    /// 2. the lease names its owner AND that owner's access list admitted this
    ///    caller (which is what `grant` records — the registry
    ///    answered it when it handed out the lease, so this cannot claim a
    ///    share nobody checked);
    /// 3. nothing else. A caller whose roles are empty reaches exactly the same
    ///    set of documents as one whose roles allow edits: the roles decide what
    ///    may be *done*, never what may be *seen*, which is why this check asks
    ///    a question [`Self::decide`] cannot.
    pub fn reaches_stored_document(&self, owner: Option<&str>) -> bool {
        // One operator, one directory: everything in it is theirs. This is the
        // branch that keeps the local daemon's file screen working.
        if !self.mode.is_online() {
            return true;
        }
        let (Some(lease_owner), Some(caller)) = (self.owner_id, self.caller) else {
            // No owner or no verified caller: online that is a carrier built
            // without an identity, which refuses rather than falling open.
            return false;
        };
        let Some(owner) = owner else {
            return false;
        };
        if owner == caller.user_id {
            return true;
        }
        self.grant.is_some() && owner == lease_owner
    }

    /// Whether this caller may see the document at all.
    ///
    /// The owner passes by identity, everyone else by the access list. Both
    /// halves must be present: an online request with no owner or no verified
    /// caller answers `false`, so a carrier built without an identity refuses
    /// rather than falling open.
    ///
    /// `pub(super)` because the subject decisions a section needs
    /// ([`super::section_rights`]) ask the same question about the same
    /// document, and a second copy of this rule is exactly where the two would
    /// drift apart.
    pub(super) fn reaches_document(&self) -> bool {
        match (self.owner_id, self.caller) {
            (Some(owner), Some(caller)) => owner == caller.user_id.as_str() || self.grant.is_some(),
            _ => false,
        }
    }

    /// What the caller's roles allow.
    ///
    /// Outside online there is no caller and this is never consulted: the mode
    /// branch in [`Self::decide`] answers before it.
    /// A visitor reaches the document through a grant, and the grant names a
    /// LEVEL — so the effective rights are the INTERSECTION of the caller's
    /// product roles with what was granted. Intersecting, not choosing: an
    /// editor's roles must not silently raise a grant that says "can view",
    /// and a grant of "can edit" must not promote an account whose roles never
    /// included editing.
    ///
    /// The owner is exempt because ownership is not a grant — it is answered
    /// by [`Self::decide`] before this is consulted, and an owner of a
    /// document necessarily holds it.
    ///
    /// [`ShareLevel::document_rights`] carries no `ManageUsers`, which is what
    /// keeps this cap from reaching the deployment's account list: the rights
    /// this returns are what [`Self::decide_workspace_settings`] reads.
    fn rights(&self) -> Rights {
        let roles = self
            .caller
            .map(|caller| caller.roles.rights())
            .unwrap_or(Rights::NONE);
        match self.grant {
            Some(level) if !self.is_owner() => roles.intersect(level.document_rights()),
            _ => roles,
        }
    }

    /// What the caller's product ROLES alone allow, with no grant applied.
    ///
    /// The question about the WORKSPACE is asked of this, not of [`Self::rights`],
    /// and the difference is deliberate. A document grant is a ceiling on what
    /// may be done TO A DOCUMENT; it says nothing about what an account may do
    /// to the workspace it signs in to. The operator's decision — recorded in
    /// the test that pins it — is that an admin maintains the workspace, so an
    /// admin who was given a read-only link to somebody's file may still
    /// configure that deployment's credentials; and conversely an account whose
    /// roles grant nothing may not configure its own workspace's secrets merely
    /// because it owns the document. Two questions, two answers, one function
    /// each.
    fn role_rights(&self) -> Rights {
        self.caller
            .map(|caller| caller.roles.rights())
            .unwrap_or(Rights::NONE)
    }

    /// The level the caller was granted, when it was granted one.
    pub const fn granted_level(&self) -> Option<ShareLevel> {
        self.grant
    }

    /// The deployment mode this decision is being made under.
    pub const fn mode(&self) -> ServeMode {
        self.mode
    }

    /// The verified caller's account id, when there is one.
    pub fn caller_id(&self) -> Option<&str> {
        self.caller.map(|caller| caller.user_id.as_str())
    }

    /// The product roles the caller holds, in the order the hub sent them.
    ///
    /// Empty for the local operator (no account, no roles) and for an account
    /// whose roles this build does not recognise — the same answer
    /// [`RequestAccess::decide`] acts on, so a subject decision cannot be made
    /// from a role string nobody understood.
    ///
    /// `pub(super)`: the subject matrix in [`super::section_rights`] asks which
    /// roles a caller holds, and it must ask the same set this module answers
    /// rights from.
    pub(super) fn caller_roles(&self) -> &[op_editor_core::access::ProductRole] {
        self.caller
            .map(|caller| caller.roles.roles())
            .unwrap_or(&[])
    }

    /// The name to record against something this caller says or does.
    ///
    /// The verified identity's display name, falling back to its username when
    /// the hub sent none — an empty name is the local operator's mark (see
    /// `crate::document_comments::Author::name`), and an account that arrived
    /// without one must not be recorded as if it were the operator. Never read
    /// from a request body: a body a caller can write is not a statement about
    /// who the caller is.
    /// The caller's role as the wire names it, for anything that records it.
    ///
    /// `None` for the local operator (no account, no roles) and for an account
    /// whose roles this build does not recognise — the same answer
    /// [`RequestAccess::decide`] would act on, so a comment cannot claim a
    /// colour the caller was not granted.
    pub fn caller_role(&self) -> Option<&'static str> {
        self.caller
            .and_then(|caller| caller.roles.leading_role())
            .map(|role| role.as_wire())
    }

    pub fn caller_name(&self) -> Option<&str> {
        self.caller.map(|caller| {
            if caller.display_name.trim().is_empty() {
                caller.username.as_str()
            } else {
                caller.display_name.as_str()
            }
        })
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
