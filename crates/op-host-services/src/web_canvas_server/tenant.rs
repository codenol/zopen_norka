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

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use op_editor_core::{EditorState, ShareLevel};

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
    pub(super) leases: AtomicUsize,
    /// Unix seconds of the last lease acquire or release.
    last_active_unix: AtomicU64,
}

impl Tenant {
    pub(super) fn new(
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

    pub(super) fn touch(&self, now_unix: u64) {
        self.last_active_unix.store(now_unix, Ordering::Release);
    }

    pub(super) fn idle_secs(&self, now_unix: u64) -> u64 {
        now_unix.saturating_sub(self.last_active_unix.load(Ordering::Acquire))
    }
}

/// A connection's claim on a tenant.
///
/// Held for the whole request (including a long-lived SSE stream). While one
/// exists the tenant cannot be evicted, so the borrows a connection takes out
/// of it stay valid.
pub struct TenantLease {
    pub(super) tenant: Arc<Tenant>,
    /// The account this lease resolved to — the OWNER of the document, which
    /// for a shared visit is not the visitor.
    pub(super) user_id: String,
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

// The registry cluster — every live tenant, the limits it is held to, and
// the eviction that reclaims it — lives in the sibling `tenant_registry.rs`
// for the 800-line cap. Re-exported here so every `tenant::…` path the rest
// of the daemon already uses still resolves unchanged.
pub use super::tenant_registry::{now_unix, TenantRegistry};

// Used by `Tenant::share_grants`, which stayed with the tenant above.
use super::tenant_registry::collect_grants;

// `tenant_tests.rs` builds identities directly and reaches the type through
// `use super::*`. Nothing in the non-test spine names it any more — the
// registry took its only other user — so the import is test-only rather than
// an always-on unused one.
#[cfg(test)]
use super::tenant_auth::ResolvedIdentity;

#[cfg(test)]
#[path = "tenant_tests.rs"]
mod tests;
