//! One document authority per account.
//!
//! The local daemon has exactly one `Mutex<WebCanvasState>` and one
//! [`SseHub`]; the online daemon has one of each **per tenant**, and a
//! request reaches the pair belonging to whoever was verified for that
//! connection. Everything that used to be process-wide state and is still
//! process-wide is refused by [`ServeMode`](super::online_policy::ServeMode)
//! instead of being shared.
//!
//! ## Why a lease and not a timestamp
//!
//! Eviction has to reclaim idle tenants without ever pulling one out from
//! under a live connection: a request that is mid-flight holds a `&Mutex<..>`
//! into the tenant, and an SSE stream holds it for minutes. So a connection
//! takes a [`TenantLease`] for its whole lifetime, and eviction only removes
//! tenants whose lease count is zero. Both the lease increment and the
//! eviction check happen under the registry lock, so there is no window where
//! a lease is being taken while its tenant is being removed.
//!
//! ## Why removal is a compare-and-remove
//!
//! M4 will persist a tenant's document as it is evicted. If eviction removed
//! the map entry first and wrote afterwards, a returning request could create
//! a *second*, empty tenant for the same account while the first is still
//! being written — two live authorities for one document, and whichever
//! finished last wins. [`TenantRegistry::evict_idle`] therefore resolves the
//! victim, and removes it only after re-checking that the entry in the map is
//! still the exact `Arc` it decided to evict.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use op_editor_core::{EditorState, ShareLevel};

use super::tenant_auth::ResolvedIdentity;
use super::tenant_store::{TenantStore, TenantStoreError};

/// One edit to a tenant's access list.
///
/// A grant carries the LEVEL and the GRANTOR, because the two questions the
/// Share dialog has to answer — "how much may this person do here" and "who
/// added them" — cannot be reconstructed later. Anyone holding the invite
/// right may add somebody (the operator's matrix), so "who added this person"
/// stops being trivia the moment there is more than one person who can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclChange {
    Grant {
        account: String,
        level: ShareLevel,
        /// The verified account that made the grant; `None` only for a caller
        /// with no account at all (a local deployment never reaches here).
        invited_by: Option<String>,
    },
    Revoke(String),
}

impl AclChange {
    /// The account this change is about.
    pub fn account(&self) -> &str {
        match self {
            Self::Grant { account, .. } => account,
            Self::Revoke(account) => account,
        }
    }
}

/// The access list of ONE document: who may open it, what each of them was
/// given, and whether anybody signed in may.
///
/// The three travel together because they are written together and read
/// together: a save that carried two of them would publish a level that belongs
/// to a different membership set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentAcl {
    /// A `BTreeSet` rather than a `HashSet`, so the persisted list has a stable
    /// order and two saves of the same ACL produce the same bytes.
    pub shared_with: BTreeSet<String>,
    /// Level and grantor per account. Absent for a grant written before levels
    /// existed, which reads as [`TenantGrant::default`].
    pub grants: std::collections::BTreeMap<String, TenantGrant>,
    /// `Some(level)` when anybody signed in may open THIS document. `None` is
    /// off, and off is the only safe default: this widens access past the
    /// access list, so it is written only by an explicit request.
    pub link_access: Option<ShareLevel>,
}

/// One document shared with the asking account, and how much of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedWithVisitor {
    /// The account whose document it is.
    pub owner: String,
    /// WHICH document — the handle the asker can actually open, and the thing
    /// a share is about.
    pub key: String,
    /// What that document's access list gives the asker.
    pub level: op_editor_core::ShareLevel,
}

/// What the access list records about one account beyond its membership.
///
/// Membership itself stays the `BTreeSet` it always was — admission is checked
/// on every request and must stay a set lookup. This is the metadata beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantGrant {
    pub level: ShareLevel,
    /// The account that made the grant. `None` for a grant written before
    /// attribution existed; the dialog says so rather than naming the owner by
    /// default, which would be a guess printed as a fact.
    pub invited_by: Option<String>,
}

impl Default for TenantGrant {
    /// The fail-closed reading of a grant with no recorded level: somebody was
    /// given access, and nothing written says how much — so the least.
    fn default() -> Self {
        Self {
            level: ShareLevel::DEFAULT,
            invited_by: None,
        }
    }
}

impl TenantGrant {
    /// One entry of the persisted `grants` map, or the fail-closed default.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let level = value
            .get("level")
            .and_then(|level| level.as_str())
            .and_then(|level| ShareLevel::from_wire(level).ok())
            .unwrap_or(ShareLevel::DEFAULT);
        let invited_by = value
            .get("invitedBy")
            .and_then(|inviter| inviter.as_str())
            .map(str::trim)
            .filter(|inviter| !inviter.is_empty())
            .map(str::to_string);
        Self { level, invited_by }
    }

    /// This entry as it is persisted.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "level": self.level.wire(),
            "invitedBy": self.invited_by,
        })
    }
}

/// The result of an applied access-list change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclUpdate {
    /// Whether the list actually moved (a repeated grant does not).
    pub changed: bool,
    /// The list as it now stands, both in memory and on disk.
    pub shared_with: BTreeSet<String>,
    /// The metadata beside the list, as it now stands.
    pub grants: Vec<op_editor_core::ShareGrant>,
}
use super::{SseHub, WebCanvasState};

/// Global connection ceiling for the online daemon.
pub const MAX_CONNS_ENV: &str = "OPENPENCIL_ONLINE_MAX_CONNS";
/// Per-tenant connection ceiling — one account cannot starve the rest.
pub const MAX_CONNS_PER_TENANT_ENV: &str = "OPENPENCIL_ONLINE_MAX_CONNS_PER_TENANT";
/// How many accounts may hold a live document at once.
pub const MAX_TENANTS_ENV: &str = "OPENPENCIL_ONLINE_MAX_TENANTS";
/// Idle seconds after which a tenant with no connections is reclaimed.
pub const IDLE_EVICT_SECS_ENV: &str = "OPENPENCIL_ONLINE_IDLE_EVICT_SECS";

pub const DEFAULT_MAX_CONNS: usize = 256;
pub const DEFAULT_MAX_CONNS_PER_TENANT: usize = 8;
pub const DEFAULT_MAX_TENANTS: usize = 100;
pub const DEFAULT_IDLE_EVICT_SECS: u64 = 1800;

/// Resource ceilings for one online daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantLimits {
    pub max_conns: usize,
    pub max_conns_per_tenant: usize,
    pub max_tenants: usize,
    pub idle_evict_secs: u64,
}

impl Default for TenantLimits {
    fn default() -> Self {
        Self {
            max_conns: DEFAULT_MAX_CONNS,
            max_conns_per_tenant: DEFAULT_MAX_CONNS_PER_TENANT,
            max_tenants: DEFAULT_MAX_TENANTS,
            idle_evict_secs: DEFAULT_IDLE_EVICT_SECS,
        }
    }
}

impl TenantLimits {
    /// Read the ceilings from the environment, falling back to the defaults.
    ///
    /// An unparseable or zero value keeps the default rather than disabling
    /// the ceiling: a typo in a deployment variable must not silently remove
    /// a resource bound.
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            max_conns: env_usize(MAX_CONNS_ENV, defaults.max_conns),
            max_conns_per_tenant: env_usize(
                MAX_CONNS_PER_TENANT_ENV,
                defaults.max_conns_per_tenant,
            ),
            max_tenants: env_usize(MAX_TENANTS_ENV, defaults.max_tenants),
            idle_evict_secs: env_u64(IDLE_EVICT_SECS_ENV, defaults.idle_evict_secs),
        }
    }
}

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn env_u64(name: &str, fallback: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

/// Why the registry could not hand out a tenant.
///
/// Both variants are capacity verdicts, never a statement about the
/// requesting account — a caller learns that the service is full, not
/// anything about other tenants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantError {
    /// The daemon already holds [`TenantLimits::max_tenants`] documents and
    /// none of them is idle enough to reclaim.
    TooManyTenants,
    /// This account already holds [`TenantLimits::max_conns_per_tenant`]
    /// connections.
    TooManyConnections,
    /// The caller asked for another account's tenant and is not on its
    /// access list.
    NotShared,
}

impl TenantError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::TooManyTenants => "server-busy",
            Self::TooManyConnections => "too-many-connections",
            Self::NotShared => "tenant-not-shared",
        }
    }

    pub const fn http_status(self) -> &'static str {
        match self {
            Self::TooManyTenants | Self::TooManyConnections => "503 Service Unavailable",
            // A forbidden share and a non-existent one answer the same way:
            // otherwise the difference is an oracle for which accounts exist.
            Self::NotShared => "403 Forbidden",
        }
    }
}

impl std::fmt::Display for TenantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyTenants => f.write_str("server busy"),
            Self::TooManyConnections => {
                f.write_str("too many concurrent connections for this account")
            }
            Self::NotShared => f.write_str("this document is not shared with you"),
        }
    }
}

impl std::error::Error for TenantError {}

/// One account's live document authority.
pub struct Tenant {
    /// This account's editor, version counter and collaboration state — the
    /// exact structure the single-user daemon keeps, one per account.
    pub(crate) state: Mutex<WebCanvasState>,
    /// This account's SSE subscribers. Separate per tenant so a version bump
    /// is only ever broadcast to the account that caused it.
    pub(crate) hub: SseHub,
    /// Access lists BY DOCUMENT KEY.
    ///
    /// Sharing is a property of a document, not of the account that owns it.
    /// It used to live here once per tenant, and the consequence was measured:
    /// a grant made in one document's Share dialog opened EVERY document that
    /// account owned, because the route carried no key and the list had
    /// nowhere to put one (issue #127).
    ///
    /// The key is the store's own name for a document — the same value
    /// `/api/files/<key>` addresses — so "which document" has exactly one
    /// answer in this daemon.
    acls: Mutex<std::collections::BTreeMap<String, DocumentAcl>>,
    /// Live leases. Non-zero means "do not evict".
    leases: AtomicUsize,
    /// Unix seconds of the last lease acquire or release.
    last_active_unix: AtomicU64,
}

impl Tenant {
    fn new(
        port: u16,
        allow_origins: &[String],
        editor: EditorState,
        acls: std::collections::BTreeMap<String, DocumentAcl>,
        now_unix: u64,
    ) -> Self {
        let mut state = WebCanvasState::new_for_tenant(editor, port);
        // Every tenant answers for the same public origin; the allowlist is a
        // deployment property, not an account one.
        state.allow_origins = allow_origins.to_vec();
        Self {
            state: Mutex::new(state),
            hub: SseHub::default(),
            acls: Mutex::new(acls),
            leases: AtomicUsize::new(0),
            last_active_unix: AtomicU64::new(now_unix),
        }
    }

    /// Install the persisted metadata that goes beside the access list.
    ///
    /// Called once, right after a tenant is materialised from the store — a
    /// separate call rather than a constructor parameter so the several places
    /// that build a tenant for a test keep working with the empty default
    /// (membership with no recorded level, which is exactly what a tenant with
    /// no stored ACL means).
    pub(super) fn restore_acls(&self, acls: std::collections::BTreeMap<String, DocumentAcl>) {
        *self.acls.lock().unwrap_or_else(|p| p.into_inner()) = acls;
    }

    /// Whether `visitor` may reach this tenant's document.
    ///
    /// Link access admits anybody at all — that is what it means — and the
    /// caller's level is still decided separately, so this widening cannot
    /// promote anyone past what the link hands out.
    pub fn admits(&self, visitor: &str, key: &str) -> bool {
        let acls = self.acls.lock().unwrap_or_else(|p| p.into_inner());
        let Some(acl) = acls.get(key) else {
            // No list for this document is no access to it. Fail closed: a
            // document nobody has shared is not one everybody may open.
            return false;
        };
        acl.link_access.is_some() || acl.shared_with.contains(visitor)
    }

    /// The access list of one document, or the empty list when it has none.
    pub fn acl_of(&self, key: &str) -> DocumentAcl {
        self.acls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    /// What the access list records about `visitor`, or `None` when it does
    /// not name them at all.
    pub fn grant_for(&self, visitor: &str, key: &str) -> Option<TenantGrant> {
        let acls = self.acls.lock().unwrap_or_else(|p| p.into_inner());
        let acl = acls.get(key)?;
        if let Some(grant) = acl.grants.get(visitor) {
            return Some(grant.clone());
        }
        // An account on the list with no metadata — a grant written before
        // levels existed, or by an older build — still has access; what it
        // does not have is a recorded level.
        acl.shared_with.contains(visitor).then(TenantGrant::default)
    }

    /// The level this visitor may hold on this tenant's document, or `None`
    /// when the tenant does not admit them at all.
    ///
    /// ONE function for the question, because two callers ask it about the same
    /// request: admission (`admits`) and the rights ceiling the routes apply.
    /// A grant names its own level; a caller admitted only by link access takes
    /// the link's level — which is the whole reason link access can be turned
    /// on without turning it up.
    pub fn level_for(&self, visitor: &str, key: &str) -> Option<ShareLevel> {
        if let Some(grant) = self.grant_for(visitor, key) {
            return Some(grant.level);
        }
        self.link_access(key)
    }

    /// The level ONE document is open to anybody with the link at.
    pub fn link_access(&self, key: &str) -> Option<ShareLevel> {
        self.acl_of(key).link_access
    }

    /// The metadata beside one document's list, for persisting and for
    /// answering the Share dialog.
    pub fn grants(&self, key: &str) -> std::collections::BTreeMap<String, TenantGrant> {
        self.acl_of(key).grants
    }

    /// The whole ACL — membership, metadata and link access — as one value.
    ///
    /// `list` is passed in because every writer already holds its guard: taking
    /// it again here would deadlock on a non-reentrant mutex.
    pub(super) fn acl_snapshot(&self, key: &str) -> super::tenant_store::TenantAcl {
        let acl = self.acl_of(key);
        super::tenant_store::TenantAcl {
            shared_with: acl.shared_with,
            grants: acl.grants,
            link_access: acl.link_access,
        }
    }

    /// The document map, locked for a read-modify-write.
    ///
    /// A grant reads the list, edits it and writes the whole map back, so two
    /// concurrent grants that each took a snapshot would each publish a map
    /// missing the other's account — measured: sixteen concurrent grants left
    /// four. Holding this across the edit AND the disk write is what makes them
    /// serial (the same discipline the single-list writer used).
    pub(super) fn acls_guard(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<String, DocumentAcl>> {
        self.acls.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Every document this tenant holds an access list for.
    pub(super) fn acls(&self) -> std::collections::BTreeMap<String, DocumentAcl> {
        self.acls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Every account the document is shared with, and at what level.
    ///
    /// Membership drives the list: an account on it with no metadata still
    /// appears, at [`TenantGrant::default`]'s level, because it does have
    /// access.
    pub fn share_grants(&self, key: &str) -> Vec<op_editor_core::ShareGrant> {
        let acl = self.acl_of(key);
        collect_grants(&acl.shared_with, &acl.grants)
    }

    /// Add an account to the access list. Returns whether it was new.
    pub fn grant(&self, visitor: &str, key: &str) -> bool {
        self.acls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(key.to_string())
            .or_default()
            .shared_with
            .insert(visitor.to_string())
    }

    /// Remove an account. Returns whether it had been granted.
    pub fn revoke(&self, visitor: &str, key: &str) -> bool {
        let mut acls = self.acls.lock().unwrap_or_else(|p| p.into_inner());
        let Some(acl) = acls.get_mut(key) else {
            return false;
        };
        acl.grants.remove(visitor);
        acl.shared_with.remove(visitor)
    }

    /// The access list, locked for a read-modify-write.
    ///
    /// `update_acl` holds this across both the edit and the disk write so two
    /// concurrent grants cannot each write back a list missing the other's.
    /// Replace one document's ACL wholesale — used to roll a failed write back
    /// and to set the link level.
    pub(super) fn set_acl(&self, key: &str, acl: DocumentAcl) {
        self.acls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), acl);
    }

    /// Put the metadata beside one document's membership: the sets are changed
    /// by `grant`/`revoke`, this carries the level and grantor with them.
    pub(super) fn set_grant_meta(&self, key: &str, visitor: &str, grant: TenantGrant) {
        self.acls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(key.to_string())
            .or_default()
            .grants
            .insert(visitor.to_string(), grant);
    }

    /// A snapshot of one document's access list.
    pub fn shared_with(&self, key: &str) -> BTreeSet<String> {
        self.acl_of(key).shared_with
    }

    /// Which document this tenant currently holds, when it holds one that came
    /// from the store.
    ///
    /// The fallback for a request that addresses a tenant without naming a
    /// document: the owner opened a document, so "the owner's document" has an
    /// answer. A tenant holding a document of its own (a bare `--file`
    /// session, or a brand-new account) has none, and a share that must name a
    /// document cannot be satisfied — which is the correct refusal, not a
    /// crash (issue #127).
    pub fn current_document_key(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .editor
            .editor_ui
            .file_key
            .clone()
    }

    /// Live lease count. Zero is the only evictable value.
    pub fn lease_count(&self) -> usize {
        self.leases.load(Ordering::Acquire)
    }

    fn touch(&self, now_unix: u64) {
        self.last_active_unix.store(now_unix, Ordering::Release);
    }

    fn idle_secs(&self, now_unix: u64) -> u64 {
        now_unix.saturating_sub(self.last_active_unix.load(Ordering::Acquire))
    }
}

/// A connection's claim on a tenant.
///
/// Held for the whole request (including a long-lived SSE stream). While one
/// exists the tenant cannot be evicted, so the borrows a connection takes out
/// of it stay valid.
pub struct TenantLease {
    tenant: Arc<Tenant>,
    /// The account this lease resolved to — the OWNER of the document, which
    /// for a shared visit is not the visitor.
    user_id: String,
}

impl std::fmt::Debug for TenantLease {
    /// Deliberately opaque: a lease points at an account's whole editor, and
    /// a derived `Debug` would print its document and its credentials into
    /// whatever log or test failure rendered it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantLease")
            .field("leases", &self.tenant.lease_count())
            .finish_non_exhaustive()
    }
}

impl TenantLease {
    /// The owning account id. For a shared visit this is the owner, not the
    /// visitor — it is the tenant's key, not the caller's identity.
    pub fn owner_id(&self) -> &str {
        &self.user_id
    }

    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    pub fn state(&self) -> &Mutex<WebCanvasState> {
        &self.tenant.state
    }

    pub fn hub(&self) -> &SseHub {
        &self.tenant.hub
    }
}

impl Drop for TenantLease {
    fn drop(&mut self) {
        self.tenant.leases.fetch_sub(1, Ordering::AcqRel);
        // Idleness is measured from the last release, not the last acquire:
        // a tenant that just finished a 20-minute SSE stream has been active
        // the whole time.
        self.tenant.touch(now_unix());
    }
}

/// Counts requests that are inside a document write, and refuses new ones
/// once shutdown has begun.
///
/// Draining connections alone is not enough: a worker can be past the drain
/// check and about to take the state lock when the flush snapshots the
/// document. It then commits and answers 200 for work the flush never saw.
/// The barrier closes that window — shutdown stops admitting writes and then
/// waits for the ones already inside to finish.
#[derive(Debug, Default)]
pub struct WriteBarrier {
    active: AtomicUsize,
    closed: AtomicBool,
}

impl WriteBarrier {
    /// Enter the write path, unless shutdown has closed it.
    ///
    /// `None` means the caller must refuse the request (503): the daemon is
    /// stopping and cannot durably accept a write.
    pub fn enter(&self) -> Option<WritePass<'_>> {
        if self.closed.load(Ordering::Acquire) {
            return None;
        }
        self.active.fetch_add(1, Ordering::AcqRel);
        // Re-checked after the increment: a close landing between the two
        // would otherwise admit a writer the drain has already stopped
        // waiting for.
        if self.closed.load(Ordering::Acquire) {
            self.active.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(WritePass { barrier: self })
    }

    /// Enter the write path regardless of the closed flag. Tests only — it is
    /// how a test stands in for a writer that was admitted before the close.
    #[cfg(test)]
    pub fn enter_for_test(&self) -> WritePass<'_> {
        self.active.fetch_add(1, Ordering::AcqRel);
        WritePass { barrier: self }
    }

    /// Stop admitting writes. Idempotent.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

/// Proof that a write is in flight. Decrements on drop.
pub struct WritePass<'a> {
    barrier: &'a WriteBarrier,
}

impl Drop for WritePass<'_> {
    fn drop(&mut self) {
        self.barrier.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Every live tenant, keyed by verified account id.
pub struct TenantRegistry {
    tenants: Mutex<HashMap<String, Arc<Tenant>>>,
    limits: TenantLimits,
    /// Reported by `GET /api/mcp/server`; the same bound port for everyone.
    port: u16,
    /// Public origins this deployment answers for, stamped onto every tenant.
    allow_origins: Vec<String>,
    /// Where an evicted tenant is written and a returning one is read from.
    store: TenantStore,
}

impl TenantRegistry {
    pub fn new(port: u16, limits: TenantLimits, allow_origins: Vec<String>) -> Self {
        Self::with_store(port, limits, allow_origins, TenantStore::from_env())
    }

    pub fn with_store(
        port: u16,
        limits: TenantLimits,
        allow_origins: Vec<String>,
        store: TenantStore,
    ) -> Self {
        Self {
            tenants: Mutex::new(HashMap::new()),
            limits,
            port,
            allow_origins,
            store,
        }
    }

    pub const fn store(&self) -> &TenantStore {
        &self.store
    }

    /// The deployment's public origin allowlist.
    pub fn allow_origins(&self) -> &[String] {
        &self.allow_origins
    }

    pub const fn limits(&self) -> TenantLimits {
        self.limits
    }

    /// Which document `owner_id` is holding, without materialising the tenant.
    pub fn current_document_key(&self, owner_id: &str) -> Option<String> {
        self.lock().get(owner_id)?.current_document_key()
    }

    pub fn tenant_count(&self) -> usize {
        self.lock().len()
    }

    /// Take a lease on `identity`'s tenant, creating it if this is the
    /// account's first live connection.
    ///
    /// The key is `identity.user_id` and nothing else — see the module docs
    /// on [`super::tenant_auth`] for why no request-supplied value may ever
    /// reach this argument.
    ///
    /// A tenant that is not in memory is restored from disk when the store
    /// holds one, and starts from [`EditorState::starter`] otherwise — so an
    /// eviction is invisible to the account beyond the first request's cost.
    pub fn lease_for(&self, identity: &ResolvedIdentity) -> Result<TenantLease, TenantError> {
        self.lease_tenant(&identity.user_id)
    }

    /// Take a lease on `owner_id`'s tenant on behalf of `visitor`.
    ///
    /// The owner always passes. Anyone else must appear in the owner's access
    /// list — and note that the list is consulted on EVERY request, so a
    /// revoke takes effect on the visitor's next call rather than whenever
    /// some session expires.
    ///
    /// A tenant that is not resident is restored first: a visitor must be
    /// able to open a shared document whose owner is offline, and refusing
    /// until the owner next signs in would make sharing useless.
    ///
    /// ## Admission precedes materialisation
    ///
    /// Materialising first would let an unauthenticated-in-practice caller
    /// spend the daemon's whole tenant budget: `?tenant=` names an arbitrary
    /// account, and creating the tenant to discover the caller is not on its
    /// list means every refused request still costs a resident tenant. So a
    /// non-resident owner is admitted from the PERSISTED access list, which
    /// reads one small file and materialises nothing.
    ///
    /// A resident owner is checked against the live list, which is
    /// authoritative — a revoke that has not been written yet still takes
    /// effect immediately.
    pub fn lease_for_shared(
        &self,
        owner_id: &str,
        visitor: &ResolvedIdentity,
        key: &str,
    ) -> Result<TenantLease, TenantError> {
        if owner_id == visitor.user_id {
            return self.lease_tenant(owner_id);
        }
        if !self.admits_visitor(owner_id, &visitor.user_id, key) {
            return Err(TenantError::NotShared);
        }
        let lease = self.lease_tenant(owner_id)?;
        // Re-checked against the live list now that the tenant is resident: a
        // revoke may have landed between the two, and the in-memory list is
        // the authority.
        if !lease.tenant().admits(&visitor.user_id, key) {
            return Err(TenantError::NotShared);
        }
        Ok(lease)
    }

    /// Whether `visitor` is on `owner_id`'s access list, WITHOUT materialising
    /// the tenant.
    ///
    /// Resident tenants answer from memory; the rest answer from the persisted
    /// list. A deployment with no store therefore admits nobody to a
    /// non-resident tenant, which is the fail-closed direction — the share was
    /// never durable in the first place.
    fn admits_visitor(&self, owner_id: &str, visitor: &str, key: &str) -> bool {
        if let Some(tenant) = self.lock().get(owner_id) {
            return tenant.admits(visitor, key);
        }
        let acl = self.store.load_acl_file(owner_id).documents.remove(key);
        let Some(acl) = acl else {
            // No list for that document is no access to it, resident or not.
            return false;
        };
        // Link access admits anybody, resident or not — the stored flag is the
        // same fact the resident tenant would answer with.
        acl.link_access.is_some() || acl.shared_with.contains(visitor)
    }

    /// Who has shared with `visitor`, across every resident tenant.
    ///
    /// Resident only, and deliberately: a full answer would mean reading every
    /// directory in the store on every call. The owners a visitor is actually
    /// working with are resident by definition, and the visitor can always
    /// open a share they were told about directly.
    ///
    /// Each entry carries the LEVEL the visitor holds, because a guest who is
    /// never told what they were given cannot act on it: the Share dialog drew
    /// them as view-only whatever the grant said (#121). The level is the
    /// grant's own, or the link's when the document is open to anybody signed
    /// in.
    pub fn shared_with_visitor(&self, visitor: &str) -> Vec<SharedWithVisitor> {
        let tenants = self.lock();
        let mut shared: Vec<SharedWithVisitor> = Vec::new();
        for (owner, tenant) in tenants.iter() {
            if owner.as_str() == visitor {
                continue;
            }
            for (key, acl) in tenant.acls() {
                let level = acl
                    .grants
                    .get(visitor)
                    .map(|grant| grant.level)
                    .or_else(|| {
                        acl.shared_with
                            .contains(visitor)
                            .then(|| acl.grants.get(visitor).map(|g| g.level))
                            .flatten()
                    })
                    .or_else(|| acl.link_access);
                let Some(level) = level else {
                    continue;
                };
                shared.push(SharedWithVisitor {
                    owner: owner.clone(),
                    key,
                    level,
                });
            }
        }
        shared.sort_by(|a, b| (&a.owner, &a.key).cmp(&(&b.owner, &b.key)));
        shared
    }

    fn lease_tenant(&self, user_id: &str) -> Result<TenantLease, TenantError> {
        let now = now_unix();
        let mut tenants = self.lock();
        let tenant = match tenants.get(user_id) {
            Some(existing) => Arc::clone(existing),
            None => {
                if tenants.len() >= self.limits.max_tenants {
                    return Err(TenantError::TooManyTenants);
                }
                let stored = self.store.load_acl_file(user_id);
                let created = Arc::new(Tenant::new(
                    self.port,
                    &self.allow_origins,
                    self.restore_editor(user_id),
                    stored.documents.clone(),
                    now,
                ));
                // The metadata is installed here rather than passed to the
                // constructor so a tenant built for a test carries the same
                // meaning a tenant built from an empty store does.
                created.restore_acls(stored.documents);
                tenants.insert(user_id.to_string(), Arc::clone(&created));
                created
            }
        };
        if tenant.lease_count() >= self.limits.max_conns_per_tenant {
            return Err(TenantError::TooManyConnections);
        }
        // Both this increment and `evict_idle`'s zero-check run under the
        // registry lock, so a tenant can never be evicted between being
        // resolved above and being leased here.
        tenant.leases.fetch_add(1, Ordering::AcqRel);
        tenant.touch(now);
        Ok(TenantLease {
            tenant,
            user_id: user_id.to_string(),
        })
    }

    /// The document to open a tenant with.
    ///
    /// A stored document that will not load has already been moved aside by
    /// the store, so the account gets a starter rather than a failed request —
    /// losing a document is bad, but refusing to serve the account at all
    /// because of it is worse.
    fn restore_editor(&self, user_id: &str) -> EditorState {
        match self.store.load_document(user_id) {
            Ok(state) => state,
            Err(super::tenant_store::TenantStoreError::Disabled)
            | Err(super::tenant_store::TenantStoreError::NotStored) => EditorState::starter(),
            Err(error) => {
                eprintln!(
                    "openpencil --serve-web --online: starting a fresh document for an \
                     account whose stored one could not be loaded ({error})"
                );
                EditorState::starter()
            }
        }
    }

    /// Write every resident tenant to disk.
    ///
    /// Called on controlled shutdown. Eviction is the only other writer, and
    /// a daemon that is asked to stop has by definition not waited out anyone's
    /// idle timer — so without this, every account that was active at the
    /// moment of a deploy loses whatever it had not had evicted.
    ///
    /// Returns how many were written. Tenants are NOT removed: the process is
    /// going away regardless, and removing them would only race the requests
    /// still draining.
    pub fn flush_all(&self) -> usize {
        if !self.store.is_enabled() {
            return 0;
        }
        let tenants = self.lock();
        let mut written = 0;
        for (id, tenant) in tenants.iter() {
            let guard = tenant.state.lock().unwrap_or_else(|p| p.into_inner());
            // Held across the write, exactly as `update_acl` does: otherwise a
            // grant landing mid-flush is written by one path and overwritten
            // by the other, and the user's share silently disappears.
            let acls = tenant.acls();
            match self.store.save(id, &guard.editor, &acls) {
                Ok(()) => written += 1,
                Err(error) => eprintln!(
                    "openpencil --serve-web --online: could not flush a tenant on shutdown \
                     ({error})"
                ),
            }
        }
        written
    }

    /// Apply one access-list change and persist the result atomically.
    ///
    /// The edit and its write happen under the SAME lock, for two reasons.
    /// Snapshot-then-write let two concurrent grants each read the list before
    /// the other's insert and write back a version missing it — the second
    /// write silently dropping the first grant. And a write that fails has to
    /// be reported: the previous code logged it and answered 200, so a user
    /// was told a share had succeeded that would vanish on the next restart.
    ///
    /// On a write failure the in-memory change is ROLLED BACK, so memory and
    /// disk agree and the caller's retry starts from a known state. The
    /// alternative — keep it in memory and mark it pending — would mean the
    /// share works until the process restarts and then silently stops, which
    /// is the harder failure to diagnose.
    pub fn update_acl(
        &self,
        user_id: &str,
        tenant: &Tenant,
        key: &str,
        change: AclChange,
    ) -> Result<AclUpdate, TenantStoreError> {
        // ONE document's list, and the whole map held across the edit and the
        // write. The key is what makes a share a share: without it the same
        // call edited the account's single list and opened every document that
        // account owned (issue #127). The guard is what keeps two concurrent
        // grants from each publishing a map without the other's account.
        let mut acls = tenant.acls_guard();
        let acl = acls.entry(key.to_string()).or_default();
        if let AclChange::Grant { account, .. } = &change {
            // The store writes at most `MAX_SHARED_ACCOUNTS`, so accepting a
            // grant past the ceiling would report success for a share that
            // silently vanishes on the next save. Refuse it instead.
            if !acl.shared_with.contains(account.as_str())
                && acl.shared_with.len() >= super::tenant_store::MAX_SHARED_ACCOUNTS
            {
                return Err(TenantStoreError::ShareLimitReached(
                    super::tenant_store::MAX_SHARED_ACCOUNTS,
                ));
            }
        }
        // The whole ACL is captured before the edit so a failed write can put
        // all of it back; restoring only part would leave memory claiming a
        // level for somebody it no longer lists (or the reverse).
        let previous = acl.clone();
        let changed = match &change {
            AclChange::Grant {
                account,
                level,
                invited_by,
            } => {
                let new = TenantGrant {
                    level: *level,
                    invited_by: invited_by.clone(),
                };
                let moved = acl.grants.get(account) != Some(&new);
                acl.grants.insert(account.clone(), new);
                acl.shared_with.insert(account.clone()) || moved
            }
            AclChange::Revoke(account) => {
                let had_meta = acl.grants.remove(account.as_str()).is_some();
                acl.shared_with.remove(account.as_str()) || had_meta
            }
        };
        let answer = AclUpdate {
            changed,
            shared_with: acl.shared_with.clone(),
            grants: collect_grants(&acl.shared_with, &acl.grants),
        };
        if !changed || !self.store.is_enabled() {
            return Ok(answer);
        }
        // The file holds every document's list, so the write is the whole map.
        match self.store.save_acls_for(user_id, &acls) {
            Ok(()) => Ok(answer),
            Err(error) => {
                acls.insert(key.to_string(), previous);
                Err(error)
            }
        }
    }

    /// Turn link access on, off, or to another level.
    ///
    /// Persisted immediately and rolled back on a failed write, exactly as a
    /// grant is: this is a widening of who may open the document, and a
    /// widening that exists in memory but not on disk is one that silently
    /// disappears on the next restart.
    fn update_link_access_inner(
        &self,
        user_id: &str,
        tenant: &Tenant,
        key: &str,
        level: Option<ShareLevel>,
    ) -> Result<Option<ShareLevel>, TenantStoreError> {
        // The switch belongs to ONE document: "anybody with the link" is a
        // property of what the link points at, and turning it on for one
        // document turned it on for every document the account owned
        // (issue #127).
        let mut acls = tenant.acls_guard();
        let acl = acls.entry(key.to_string()).or_default();
        let previous = acl.link_access;
        if previous == level {
            return Ok(previous);
        }
        let previous_acl = acl.clone();
        acl.link_access = level;
        if self.store.is_enabled() {
            if let Err(error) = self.store.save_acls_for(user_id, &acls) {
                acls.insert(key.to_string(), previous_acl);
                return Err(error);
            }
        }
        // The level the document hands out NOW. Answering with `previous` made
        // every caller — the Share dialog included — show the state from one
        // click ago, and a switch that reports the wrong state is worse than
        // one that lags.
        Ok(level)
    }

    /// Turn link access on, off, or to another level, and persist it.
    ///
    /// Delegates to the tenant's own access-list machinery so the write, the
    /// rollback and the "is persistence even on" question are answered in one
    /// place — a second writer would be a second chance to publish a widening
    /// that does not survive a restart.
    pub fn update_link_access(
        &self,
        user_id: &str,
        tenant: &Tenant,
        key: &str,
        level: Option<ShareLevel>,
    ) -> Result<Option<ShareLevel>, TenantStoreError> {
        self.update_link_access_inner(user_id, tenant, key, level)
    }

    /// Reclaim tenants that hold no lease and have been idle past the limit.
    ///
    /// Returns how many were reclaimed. The removal is a compare-and-remove
    /// against the exact `Arc` that was chosen (see the module docs): M4
    /// writes the document to disk between the choice and the removal, and
    /// that write must not race a concurrent `lease_for` re-creating the
    /// same account.
    pub fn evict_idle(&self, now_unix: u64) -> usize {
        let mut tenants = self.lock();
        let victims: Vec<(String, Arc<Tenant>)> = tenants
            .iter()
            .filter(|(_, tenant)| {
                tenant.lease_count() == 0
                    && tenant.idle_secs(now_unix) >= self.limits.idle_evict_secs
            })
            .map(|(id, tenant)| (id.clone(), Arc::clone(tenant)))
            .collect();
        let mut evicted = 0;
        for (id, victim) in victims {
            // Write BEFORE the compare-and-remove, so at no instant is the
            // tenant both absent from the map and absent from disk: a request
            // arriving mid-eviction either finds the resident tenant (the
            // registry lock is held, so it waits) or, afterwards, the file.
            if self.store.is_enabled() {
                let guard = victim.state.lock().unwrap_or_else(|p| p.into_inner());
                let acls = victim.acls();
                if let Err(error) = self.store.save(&id, &guard.editor, &acls) {
                    // A tenant that cannot be written is kept resident. Evicting
                    // it anyway would discard the document to reclaim memory,
                    // which is the wrong trade for the user whose work it is.
                    eprintln!(
                        "openpencil --serve-web --online: keeping a tenant resident because \
                         it could not be persisted ({error})"
                    );
                    continue;
                }
            }
            let still_the_same = tenants
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &victim));
            if still_the_same && victim.lease_count() == 0 {
                tenants.remove(&id);
                evicted += 1;
            }
        }
        evicted
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Tenant>>> {
        self.tenants.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Pair the membership set with its metadata, in list order.
///
/// One function so the answer `/api/share/list` gives and the answer a grant
/// response gives are built the same way — two builders is two chances for a
/// row to carry a level one of them did not read.
pub(super) fn collect_grants(
    list: &BTreeSet<String>,
    grants: &std::collections::BTreeMap<String, TenantGrant>,
) -> Vec<op_editor_core::ShareGrant> {
    list.iter()
        .map(|account| {
            let grant = grants.get(account).cloned().unwrap_or_default();
            op_editor_core::ShareGrant {
                account: account.clone(),
                level: grant.level,
                invited_by: grant.invited_by,
            }
        })
        .collect()
}

/// Seconds since the Unix epoch, saturating to 0 before it.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "tenant_tests.rs"]
mod tests;
