//! The tenant registry: every live account's document authority, the
//! capacity limits it is held to, and the eviction that reclaims idle ones.
//!
//! A sibling of `tenant.rs` for the 800-line cap: that file keeps the
//! per-tenant driver (the [`Tenant`](super::tenant::Tenant) itself, its
//! lease and the write barrier), this one keeps the map of them and the
//! store it is backed by. Everything below is the original `tenant.rs`
//! lines 617-1108, byte-for-byte.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use op_editor_core::{EditorState, ShareLevel};

use super::tenant::{
    AclChange, AclUpdate, SharedWithVisitor, Tenant, TenantError, TenantGrant, TenantLease,
    TenantLimits,
};
use super::tenant_auth::ResolvedIdentity;
use super::tenant_store::{TenantStore, TenantStoreError};

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
                    .or(acl.link_access);
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
                // The directory is the route's to consult, not the tenant's:
                // an access list is stored by id and stays readable without one.
                display_name: None,
                username: None,
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
