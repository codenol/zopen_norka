//! Tests for the per-account document registry and its lease/eviction rules.

use super::*;
use crate::web_canvas_server::online_policy::ServeMode;
use crate::web_canvas_server::tenant_auth::IdentityVia;
use crate::web_canvas_server::SseTick;

fn identity(user_id: &str) -> ResolvedIdentity {
    ResolvedIdentity {
        user_id: user_id.into(),
        username: user_id.into(),
        display_name: user_id.into(),
        // These tests are about tenant isolation, which roles do not touch.
        roles: op_editor_core::access::RoleSet::empty(),
        via: IdentityVia::ApiToken,
        scopes: crate::mcp_serve::tool_profile::McpScopes::FULL,
    }
}

fn registry(limits: TenantLimits) -> TenantRegistry {
    TenantRegistry::new(3100, limits, Vec::new())
}

#[test]
fn two_accounts_get_two_independent_documents() {
    let registry = registry(TenantLimits::default());
    let a = registry.lease_for(&identity("userA")).expect("lease A");
    let b = registry.lease_for(&identity("userB")).expect("lease B");

    a.state().lock().unwrap_or_else(|p| p.into_inner()).version = 42;

    assert_eq!(
        b.state().lock().unwrap_or_else(|p| p.into_inner()).version,
        0,
        "one account's write must not be visible to another"
    );
    assert_eq!(registry.tenant_count(), 2);
}

#[test]
fn the_same_account_returns_to_the_same_document() {
    let registry = registry(TenantLimits::default());
    {
        let first = registry.lease_for(&identity("userA")).expect("lease");
        first
            .state()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .version = 7;
    }
    let second = registry.lease_for(&identity("userA")).expect("lease again");
    assert_eq!(
        second
            .state()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .version,
        7
    );
    assert_eq!(registry.tenant_count(), 1);
}

#[test]
fn every_tenant_owns_its_own_broadcast_hub() {
    let registry = registry(TenantLimits::default());
    let a = registry.lease_for(&identity("userA")).expect("lease A");
    let b = registry.lease_for(&identity("userB")).expect("lease B");
    let a_sub = a.hub().subscribe();
    let b_sub = b.hub().subscribe();

    a.hub().broadcast(SseTick {
        version: 5,
        collab_seq: 0,
    });

    assert_eq!(a_sub.pending().expect("A hears its own bump").version, 5);
    assert!(
        b_sub.pending().is_none(),
        "a tenant's version bump must not reach another tenant's subscribers"
    );
}

#[test]
fn a_tenant_starts_from_the_starter_document_in_online_mode() {
    let registry = registry(TenantLimits::default());
    let lease = registry.lease_for(&identity("userA")).expect("lease");
    let guard = lease.state().lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(guard.version, 0);
    assert_eq!(guard.mode, ServeMode::Online);
}

#[test]
fn a_leased_tenant_is_never_evicted_however_idle_the_clock_says_it_is() {
    let registry = registry(TenantLimits {
        idle_evict_secs: 1,
        ..TenantLimits::default()
    });
    let lease = registry.lease_for(&identity("userA")).expect("lease");
    lease
        .state()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .version = 9;

    assert_eq!(registry.evict_idle(now_unix() + 86_400), 0);
    assert_eq!(registry.tenant_count(), 1);
    assert_eq!(
        lease
            .state()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .version,
        9,
        "the leased state is still the live one"
    );
}

#[test]
fn an_idle_unleased_tenant_is_reclaimed_and_comes_back_as_a_fresh_document() {
    let registry = registry(TenantLimits {
        idle_evict_secs: 60,
        ..TenantLimits::default()
    });
    {
        let lease = registry.lease_for(&identity("userA")).expect("lease");
        lease
            .state()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .version = 9;
    }

    assert_eq!(registry.evict_idle(now_unix() + 3600), 1);
    assert_eq!(registry.tenant_count(), 0);

    // M1 does not persist, so the reclaimed document is gone on purpose —
    // the account is served a new starter document. M4 replaces this with a
    // load from `$OPENPENCIL_ONLINE_DATA_DIR`.
    let again = registry.lease_for(&identity("userA")).expect("lease again");
    assert_eq!(
        again
            .state()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .version,
        0
    );
}

#[test]
fn a_recently_released_tenant_is_not_yet_idle() {
    let registry = registry(TenantLimits {
        idle_evict_secs: 600,
        ..TenantLimits::default()
    });
    drop(registry.lease_for(&identity("userA")).expect("lease"));
    assert_eq!(registry.evict_idle(now_unix()), 0);
    assert_eq!(registry.tenant_count(), 1);
}

#[test]
fn the_tenant_ceiling_reports_a_busy_service_rather_than_sharing_a_document() {
    let registry = registry(TenantLimits {
        max_tenants: 2,
        ..TenantLimits::default()
    });
    let _a = registry.lease_for(&identity("userA")).expect("lease A");
    let _b = registry.lease_for(&identity("userB")).expect("lease B");
    assert_eq!(
        registry.lease_for(&identity("userC")).unwrap_err(),
        TenantError::TooManyTenants
    );
    // An account that already has a tenant is still served at the ceiling.
    assert!(registry.lease_for(&identity("userA")).is_ok());
}

#[test]
fn one_account_cannot_hold_more_than_its_connection_share() {
    let registry = registry(TenantLimits {
        max_conns_per_tenant: 2,
        ..TenantLimits::default()
    });
    let _one = registry.lease_for(&identity("userA")).expect("lease 1");
    let two = registry.lease_for(&identity("userA")).expect("lease 2");
    assert_eq!(
        registry.lease_for(&identity("userA")).unwrap_err(),
        TenantError::TooManyConnections
    );
    // Another account is unaffected by a noisy neighbour.
    assert!(registry.lease_for(&identity("userB")).is_ok());
    // And the slot frees on release.
    drop(two);
    assert!(registry.lease_for(&identity("userA")).is_ok());
}

#[test]
fn a_lease_count_tracks_acquire_and_release() {
    let registry = registry(TenantLimits::default());
    let first = registry.lease_for(&identity("userA")).expect("lease");
    assert_eq!(first.tenant().lease_count(), 1);
    let second = registry.lease_for(&identity("userA")).expect("lease");
    assert_eq!(first.tenant().lease_count(), 2);
    drop(second);
    assert_eq!(first.tenant().lease_count(), 1);
}

#[test]
fn a_capacity_refusal_is_a_503_and_says_nothing_about_other_accounts() {
    for error in [TenantError::TooManyTenants, TenantError::TooManyConnections] {
        assert_eq!(error.http_status(), "503 Service Unavailable");
        assert!(!error.code().is_empty());
        assert!(!error.to_string().contains("user"));
    }
}

#[test]
fn the_default_limits_are_the_documented_ones() {
    let limits = TenantLimits::default();
    assert_eq!(limits.max_conns, 256);
    assert_eq!(limits.max_conns_per_tenant, 8);
    assert_eq!(limits.max_tenants, 100);
    assert_eq!(limits.idle_evict_secs, 1800);
}
