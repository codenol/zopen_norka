//! The public multi-account accept loop (`--serve-web --online`).
//!
//! Structurally the same daemon as [`run_web_canvas`](super::run_web_canvas):
//! bind, thread per connection, bounded concurrency. The difference is where
//! the document comes from. The single-user loop builds one
//! `Mutex<WebCanvasState>` up front and hands every connection the same one;
//! this loop builds none, and each connection resolves its own from the
//! identity its credentials verify to.
//!
//! ## Order of operations, and why
//!
//! 1. Read the request.
//! 2. Answer `OPTIONS` and the static bundle **anonymously** — the page has
//!    to load before the browser can present a credential, and neither
//!    branch touches a document.
//! 3. Verify the credential.
//! 4. Take a tenant lease.
//! 5. Dispatch.
//!
//! Nothing between steps 1 and 4 reads or writes any account's state, so an
//! unauthenticated caller can reach exactly the bytes the CDN would serve.
//!
//! ## What this loop does not run
//!
//! No collaboration driver. Relay collaboration needs a per-account device
//! ticket and the bridge holds one per process, so
//! `ServeMode::allows_relay_collaboration` is false online and every tenant's
//! availability stays at its default `Unavailable` — which is exactly what a
//! never-ticked runtime reports. M4 brings up in-service (Tier 1) sessions,
//! at which point the driver becomes per-tenant: it will walk the registry's
//! leased tenants (or run one lazy thread per live session) rather than
//! pumping a single global state. That is the extension point.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use super::online_policy::ServeMode;
use super::tenant::{now_unix, TenantLimits, TenantRegistry};
use super::tenant_auth::{IdentityVerifier, PresentedCredentials, StaticVerifier};
use super::*;

/// How long a controlled shutdown waits for in-flight requests before it
/// writes. Long enough for a large document push to finish installing, short
/// enough that a deploy is not held up by a long-lived SSE stream.
const SHUTDOWN_DRAIN_SECS: u64 = 10;

/// How often the write drain reports the blocked-writer count.
const WRITE_DRAIN_REPORT_SECS: u64 = 5;

/// When a still-blocked write drain escalates from warning to error. Not a
/// deadline — see [`drain_write_barrier`], which never gives up.
const WRITE_DRAIN_ERROR_SECS: u64 = 60;

/// Wait for the active-connection count to reach zero, up to the bound.
///
/// Returns whether it actually drained. SSE streams routinely outlive this,
/// which is fine: they hold no document lock, so flushing past them is safe —
/// the wait exists for the writers.
fn drain_connections(conn_count: &Arc<AtomicUsize>) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(SHUTDOWN_DRAIN_SECS);
    while std::time::Instant::now() < deadline {
        if conn_count.load(Ordering::Acquire) == 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    conn_count.load(Ordering::Acquire) == 0
}

/// Wait for in-flight document writes to finish.
///
/// Separate from the connection drain: an SSE stream holds a connection for
/// minutes and is safe to flush past, while a write in progress is not —
/// flushing over one loses work a client was already told had landed.
///
/// This waits WITHOUT an upper bound. Flushing over a live writer loses work a
/// client was already told had landed, and there is no deadline at which that
/// stops being true — so the process never chooses to do it. Every writer holds
/// the pass for one short locked operation, so 60s of blockage is pathological,
/// and a flush taken in that state would not rescue consistency anyway.
///
/// The outer bound is the orchestrator's: `stop_grace_period` expiring into a
/// SIGKILL. That is the right place for it, because only the operator knows how
/// long a stop may take. The flush duration is logged for exactly that sizing.
///
/// Progress is reported every [`WRITE_DRAIN_REPORT_SECS`] as a warning, and
/// escalated to an error past [`WRITE_DRAIN_ERROR_SECS`], so a stop that is
/// being held up says why.
#[cfg(test)]
pub(super) fn drain_write_barrier_for_test(barrier: &Arc<super::tenant::WriteBarrier>) {
    drain_write_barrier(barrier);
}

fn drain_write_barrier(barrier: &Arc<super::tenant::WriteBarrier>) {
    let started = std::time::Instant::now();
    let mut next_report = std::time::Duration::from_secs(WRITE_DRAIN_REPORT_SECS);
    while barrier.active() != 0 {
        let waited = started.elapsed();
        if waited >= next_report {
            let level = if waited >= std::time::Duration::from_secs(WRITE_DRAIN_ERROR_SECS) {
                "ERROR"
            } else {
                "warning"
            };
            eprintln!(
                "openpencil --serve-web --online: {level} still waiting on {} in-flight \
                 write(s) after {}s; the flush cannot run until they finish",
                barrier.active(),
                waited.as_secs()
            );
            next_report += std::time::Duration::from_secs(WRITE_DRAIN_REPORT_SECS);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Longest gap between idle sweeps, however long the idle deadline is.
const MAX_SWEEP_INTERVAL_SECS: u64 = 300;

/// Opt-in to running with no persistence at all. Demo use only.
pub const EPHEMERAL_ENV: &str = "OPENPENCIL_ONLINE_EPHEMERAL";

/// Run the multi-account web-canvas daemon.
///
/// `options.path` is ignored: a tenant is never backed by a local file, and
/// booting every account from one operator-chosen document would put one
/// account's content in front of all of them.
pub fn run_online_web_canvas(options: ServeWebOptions) -> Result<()> {
    let ServeWebOptions {
        port, host, path, ..
    } = options;
    if path.is_some() {
        eprintln!(
            "openpencil --serve-web --online: ignoring the start-up document — every account \
             starts from the starter document"
        );
    }
    let limits = TenantLimits::from_env();
    let store = super::tenant_store::TenantStore::from_env();
    // Eviction without persistence silently destroys documents: a tenant goes
    // idle, is reclaimed to free memory, and the account's work is simply
    // gone. That is defensible for a demo and indefensible for a deployment,
    // and the two are indistinguishable from inside the process — so the
    // operator has to say which one this is.
    // Fail closed on an unwritable data directory: the alternative is an
    // eviction failing silently half an hour after start.
    store.check_writable().map_err(|error| {
        WebCanvasError::Config(format!("--online cannot use its data directory: {error}"))
    })?;
    check_persistence_configured(
        store.is_enabled(),
        ephemeral_opt_in(),
        limits.idle_evict_secs,
    )?;
    if !store.is_enabled() {
        eprintln!(
            "openpencil --serve-web --online: {EPHEMERAL_ENV} is set — evicted accounts lose \
             their documents. Never use this for a deployment."
        );
    }
    let allow_origins = online_policy::allowed_origins_from_env();
    if allow_origins.is_empty() {
        // Not fatal: a same-origin deployment behind a reverse proxy needs no
        // CORS header at all. It IS fatal for cookie-authenticated writes,
        // which have no other CSRF boundary — so say so rather than letting
        // the first failed save be the discovery.
        eprintln!(
            "openpencil --serve-web --online: no public origin configured (set {}); \
             cookie-authenticated writes will be refused",
            super::origin_guard::WEB_ALLOWED_ORIGINS_ENV
        );
    }
    let verifier = resolve_verifier();

    let listener = TcpListener::bind((host.as_str(), port))
        .map_err(|e| WebCanvasError::Config(format!("bind {host}:{port}: {e}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| WebCanvasError::Config(e.to_string()))?;
    let bound = local_addr.port();
    eprintln!("openpencil --serve-web --online: listening on {host}:{bound}");
    eprintln!(
        "openpencil --serve-web --online: limits — {} conns, {} per account, {} accounts, \
         {}s idle eviction",
        limits.max_conns, limits.max_conns_per_tenant, limits.max_tenants, limits.idle_evict_secs
    );
    match crate::web_static::resolve_bundle_dir() {
        Some(dir) => eprintln!(
            "openpencil --serve-web --online: serving web bundle from {}",
            dir.display()
        ),
        None => eprintln!(
            "openpencil --serve-web --online: no web bundle found — `/` serves build \
             instructions (tools/check-wasm-bundle.sh, or set OPENPENCIL_WEB_BUNDLE_DIR)"
        ),
    }

    let registry = Arc::new(TenantRegistry::with_store(
        bound,
        limits,
        allow_origins,
        store,
    ));
    let conn_count = Arc::new(AtomicUsize::new(0));
    let write_barrier = Arc::new(super::tenant::WriteBarrier::default());
    let shutdown = Arc::new(AtomicBool::new(false));
    // A container stop is a SIGTERM, and without a handler it kills the
    // process where it stands — losing every resident tenant that had not
    // happened to be evicted. The handler only raises the flag the accept
    // loop already observes, so the existing exit path (which flushes) runs.
    install_shutdown_signals(&shutdown, local_addr)?;
    spawn_sweeper(&registry, &shutdown)?;

    for stream in listener.incoming() {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let mut s = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("openpencil --serve-web --online: accept: {e}");
                continue;
            }
        };
        if conn_count.load(Ordering::Acquire) >= limits.max_conns {
            let _ = s.set_write_timeout(Some(IO_TIMEOUT));
            let _ = crate::mcp_serve::write_mcp_http_response(
                &mut s,
                "503 Service Unavailable",
                r#"{"ok":false,"error":"server busy"}"#,
            );
            continue;
        }
        conn_count.fetch_add(1, Ordering::AcqRel);
        let registry = Arc::clone(&registry);
        let verifier = Arc::clone(&verifier);
        let conns = Arc::clone(&conn_count);
        let write_barrier = Arc::clone(&write_barrier);
        let shutdown_flag = Arc::clone(&shutdown);
        let spawned = thread::Builder::new()
            .name("op-serve-web-online-conn".into())
            .spawn(move || {
                let _conn_guard = ConnGuard(conns);
                let _ = s.set_read_timeout(Some(IO_TIMEOUT));
                let _ = s.set_write_timeout(Some(IO_TIMEOUT));
                match serve_one_online(
                    &mut s,
                    registry.as_ref(),
                    verifier.as_ref(),
                    write_barrier.as_ref(),
                ) {
                    Ok(true) => {
                        shutdown_flag.store(true, Ordering::Release);
                        let _ = std::net::TcpStream::connect(("127.0.0.1", bound));
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!("openpencil --serve-web --online: {e}"),
                }
            });
        if spawned.is_err() {
            conn_count.fetch_sub(1, Ordering::AcqRel);
        }
    }
    // The sweeper observes the same flag and retires on its next wake.
    shutdown.store(true, Ordering::Release);
    // Let in-flight requests finish before writing. A push that is mid-install
    // when the flush runs would otherwise be written in its pre-push state and
    // the client's work lost — it acked 200 and then vanished. Bounded, because
    // an SSE stream can hold a connection open indefinitely and a deploy
    // cannot wait for it.
    // Stop admitting writes FIRST: a worker past the drain check but not yet
    // holding a pass would otherwise commit after the flush snapshotted the
    // document, having already answered 200.
    write_barrier.close();
    // Returns only once no write is in flight: the process never flushes over
    // a live writer. If that takes longer than the orchestrator's grace period
    // it SIGKILLs us, which is the correct outer bound.
    drain_write_barrier(&write_barrier);
    let drained = drain_connections(&conn_count);
    let flush_started = std::time::Instant::now();
    let flushed = registry.flush_all();
    if !drained {
        eprintln!(
            "openpencil --serve-web --online: {} connection(s) still active after {}s",
            conn_count.load(Ordering::Acquire),
            SHUTDOWN_DRAIN_SECS
        );
    }
    // The duration is logged because a slow volume is what makes
    // `stop_grace_period` too short, and there is no other way to size it.
    eprintln!(
        "openpencil --serve-web --online: shutdown requested; flushed {flushed} account(s) in \
         {} ms; exiting",
        flush_started.elapsed().as_millis()
    );
    Ok(())
}

/// Refuse to start a deployment that evicts accounts but keeps nothing.
///
/// Pure so the decision is testable without a socket or the environment. The
/// two states this separates — "a demo that intentionally forgets" and "a
/// deployment whose data directory was left unset" — look identical from
/// inside the process, and only one of them is acceptable. So the operator
/// has to say which, rather than the daemon guessing and silently destroying
/// documents on the first idle timer.
pub(super) fn check_persistence_configured(
    store_enabled: bool,
    ephemeral: bool,
    idle_evict_secs: u64,
) -> Result<()> {
    if store_enabled || ephemeral {
        return Ok(());
    }
    Err(WebCanvasError::Config(format!(
        "--online evicts idle accounts after {idle_evict_secs}s but no {} is configured, so \
         their documents would be discarded. Set {} to persist them, or set {EPHEMERAL_ENV}=1 \
         to accept that this deployment keeps nothing.",
        super::tenant_store::DATA_DIR_ENV,
        super::tenant_store::DATA_DIR_ENV,
    )))
}

/// Raised by the signal handler. A `static` because that is all a handler may
/// safely touch: it runs on an arbitrary thread with almost nothing allowed,
/// so it sets one atomic and returns, and the watcher thread below does the
/// rest. Unix-only — the handler and its watcher are the only readers, and
/// Windows keeps the supervisor-driven shutdown it has always had.
#[cfg(unix)]
static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Route SIGTERM and SIGINT into the daemon's existing shutdown path.
///
/// Only installed by the online loop. The local and managed daemons keep the
/// lifecycle they have always had — a token-authed shutdown request, or
/// stdin EOF under a supervisor — and adding a handler there would change
/// what Ctrl-C means for an interactive operator.
fn install_shutdown_signals(
    shutdown: &Arc<AtomicBool>,
    local_addr: std::net::SocketAddr,
) -> Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: `handle_shutdown_signal` is async-signal-safe — it performs
        // exactly one relaxed atomic store and returns. It allocates nothing,
        // takes no lock, and calls nothing re-entrant.
        unsafe {
            let handler = handle_shutdown_signal as *const () as libc::sighandler_t;
            libc::signal(libc::SIGTERM, handler);
            libc::signal(libc::SIGINT, handler);
        }
        let shutdown = Arc::clone(shutdown);
        // The handler cannot wake a blocked `accept`, so a watcher does it:
        // it raises the real flag and pokes the listener, exactly as the
        // token-authed shutdown path does.
        let spawned = thread::Builder::new()
            .name("op-serve-web-online-signal".into())
            .spawn(move || loop {
                if SIGNAL_RECEIVED.load(Ordering::Acquire) {
                    if !shutdown.swap(true, Ordering::AcqRel) {
                        let _ = std::net::TcpStream::connect(local_addr);
                    }
                    return;
                }
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            });
        // A container stop that does not flush is data loss on every deploy,
        // so a watcher that cannot start is a start-up failure.
        spawned.map(|_| ()).map_err(|error| {
            WebCanvasError::Config(format!(
                "could not start the shutdown signal watcher: {error}"
            ))
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (shutdown, local_addr);
        Ok(())
    }
}

/// The signal handler itself. See the safety note at its installation.
#[cfg(unix)]
extern "C" fn handle_shutdown_signal(_signal: libc::c_int) {
    SIGNAL_RECEIVED.store(true, Ordering::Relaxed);
}

/// Whether the operator accepted a deployment that persists nothing.
fn ephemeral_opt_in() -> bool {
    std::env::var(EPHEMERAL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

/// Run the idle sweep on its own clock.
///
/// Eviction used to piggyback on connection arrivals, which meant the one
/// state it exists for — a daemon nobody is talking to — was exactly the state
/// it never ran in: idle accounts stayed resident indefinitely and were never
/// written to disk. This thread observes `shutdown` on every wake, so it
/// retires within one interval of the daemon being asked to stop.
fn spawn_sweeper(registry: &Arc<TenantRegistry>, shutdown: &Arc<AtomicBool>) -> Result<()> {
    let registry = Arc::clone(registry);
    let shutdown = Arc::clone(shutdown);
    let interval = sweep_interval_secs(registry.limits().idle_evict_secs);
    let spawned = thread::Builder::new()
        .name("op-serve-web-online-sweeper".into())
        .spawn(move || {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_secs(interval));
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                let evicted = registry.evict_idle(now_unix());
                if evicted > 0 {
                    eprintln!(
                        "openpencil --serve-web --online: reclaimed {evicted} idle account(s)"
                    );
                }
            }
        });
    spawned.map(|_| ()).map_err(|error| {
        // Without the sweeper nothing is ever evicted OR persisted, so a
        // "degraded" daemon here quietly stops saving anyone's work.
        WebCanvasError::Config(format!("could not start the idle sweeper: {error}"))
    })
}

/// How often to sweep, given the idle deadline.
///
/// A quarter of the deadline bounds how long past its timer a tenant can
/// linger, and the ceiling keeps a very long deadline from meaning the daemon
/// effectively never sweeps. The floor keeps a short test deadline from
/// spinning the thread.
pub(super) const fn sweep_interval_secs(idle_evict_secs: u64) -> u64 {
    let quarter = idle_evict_secs / 4;
    if quarter < 1 {
        1
    } else if quarter > MAX_SWEEP_INTERVAL_SECS {
        MAX_SWEEP_INTERVAL_SECS
    } else {
        quarter
    }
}

/// Pick the identity verifier this deployment runs.
///
/// The hub is the production answer. `StaticVerifier` stays reachable so the
/// M1 development smoke still works with no hub in sight, and so a
/// misconfigured hub URL does not silently downgrade to it — a hub that is
/// configured but unbuildable is a hard failure, not a fallback.
fn resolve_verifier() -> Arc<dyn IdentityVerifier> {
    match super::hub_verifier::HubVerifier::from_env() {
        Ok(Some(verifier)) => {
            eprintln!(
                "openpencil --serve-web --online: verifying identities against the hub at {}",
                std::env::var(crate::hub_auth_client::HUB_BASE_URL_ENV).unwrap_or_default()
            );
            if crate::hub_auth_client::internal_auth_from_env().is_none() {
                eprintln!(
                    "openpencil --serve-web --online: no {} configured; API-token \
                     introspection will be refused and only browser sessions will work",
                    crate::hub_auth_client::HUB_INTERNAL_AUTH_FILE_ENV
                );
            }
            return Arc::new(verifier);
        }
        Ok(None) => {}
        Err(error) => {
            // Configured but unusable. Serving with the development verifier
            // here would mean a production deployment quietly accepting an
            // env token table instead of real accounts.
            eprintln!(
                "openpencil --serve-web --online: {} is set but unusable ({error}); every \
                 authenticated route will answer 503",
                crate::hub_auth_client::HUB_BASE_URL_ENV
            );
            return Arc::new(StaticVerifier::parse(""));
        }
    }
    let static_verifier = StaticVerifier::from_env();
    if static_verifier.is_empty() {
        // Fail loud but keep serving: every request answers 503
        // `verifier-unavailable`, which is a diagnosable state. Serving
        // requests with NO verifier would be the unsafe alternative.
        eprintln!(
            "openpencil --serve-web --online: no identity verifier configured (set {} for a \
             hub deployment, or {} for development); every authenticated route will answer 503",
            crate::hub_auth_client::HUB_BASE_URL_ENV,
            super::tenant_auth::STATIC_IDENTITIES_ENV
        );
    } else {
        eprintln!(
            "openpencil --serve-web --online: using the DEVELOPMENT static identity table; \
             set {} for a real deployment",
            crate::hub_auth_client::HUB_BASE_URL_ENV
        );
    }
    Arc::new(static_verifier)
}

/// Serve one online connection: anonymous prefix, then verify, then dispatch
/// against the caller's own tenant.
///
/// Returns `Ok(true)` on an accepted operator shutdown, exactly as
/// [`serve_one`](super::serve_one) does.
pub(super) fn serve_one_online<S: Read + Write>(
    stream: &mut S,
    registry: &TenantRegistry,
    verifier: &dyn IdentityVerifier,
    write_barrier: &super::tenant::WriteBarrier,
) -> Result<bool> {
    let req = crate::mcp_serve::read_http_request(stream)?;
    let allow_origins = registry.allow_origins();
    let cors_origin = online_policy::online_cors_origin(allow_origins, req.origin.as_deref());
    if let Some(done) = serve_anonymous_prefix(stream, &req, cors_origin.as_deref())? {
        return Ok(done);
    }
    // The tenant key comes from here and nowhere else. Note that the request
    // body has not been looked at yet, and never contributes.
    let identity = match verifier.resolve(&PresentedCredentials::from_request(&req)) {
        Ok(identity) => identity,
        Err(error) => {
            crate::mcp_serve::write_mcp_http_response(
                stream,
                error.http_status(),
                &serde_json::json!({
                    "ok": false,
                    "error": error.code(),
                    "message": error.to_string(),
                })
                .to_string(),
            )?;
            return Ok(false);
        }
    };
    // CSRF boundary. A session cookie rides along on any cross-site request
    // the browser makes, so on a public origin the cookie alone does not
    // establish that this deployment's own page initiated the write. A bearer
    // token is exempt: it is only ever attached by code that already holds it.
    if identity.via == super::tenant_auth::IdentityVia::SessionCookie
        && !matches!(req.method.as_str(), "GET" | "HEAD" | "OPTIONS")
        && !online_policy::cookie_write_origin_allowed(allow_origins, req.origin.as_deref())
    {
        crate::mcp_serve::write_mcp_http_response_with_origin(
            stream,
            "403 Forbidden",
            &serde_json::json!({
                "ok": false,
                "error": "cross-origin-write-forbidden",
                "message": "a cookie-authenticated write must come from this deployment's origin",
            })
            .to_string(),
            cors_origin.as_deref(),
        )?;
        return Ok(false);
    }
    // Which tenant is this request addressed to? The query names an OWNER;
    // whether this caller may reach it is still decided by the owner's access
    // list, never by the parameter. Absent, it is the caller's own tenant.
    let requested_owner = req
        .query
        .as_deref()
        .and_then(op_editor_core::share_routes::tenant_from_query)
        .map(str::to_string);
    let lease = match requested_owner.as_deref() {
        Some(owner) => registry.lease_for_shared(owner, &identity),
        None => registry.lease_for(&identity),
    };
    let lease = match lease {
        Ok(lease) => lease,
        Err(error) => {
            crate::mcp_serve::write_mcp_http_response(
                stream,
                error.http_status(),
                &serde_json::json!({
                    "ok": false,
                    "error": error.code(),
                    "message": error.to_string(),
                })
                .to_string(),
            )?;
            return Ok(false);
        }
    };
    // Sharing is administered on the CALLER's own tenant, so it is answered
    // here rather than in the document tier — a visitor holding a `?tenant=`
    // lease on someone else's document must not be able to re-share it.
    if super::share_routes::is_share_route(&req.path) {
        // Same gate the connection tier applies, because this route is
        // dispatched before it ever runs: a grant is a write.
        if let Some(refusal) = super::tool_scopes::check_rest_scope(
            identity.via,
            identity.scopes,
            &req.method,
            &req.path,
        ) {
            crate::mcp_serve::write_mcp_http_response_with_origin(
                stream,
                refusal.http_status(),
                &serde_json::json!({
                    "ok": false,
                    "error": refusal.code(),
                    "message": refusal.to_string(),
                })
                .to_string(),
                cors_origin.as_deref(),
            )?;
            return Ok(false);
        }
        let own = match registry.lease_for(&identity) {
            Ok(own) => own,
            Err(error) => {
                crate::mcp_serve::write_mcp_http_response_with_origin(
                    stream,
                    error.http_status(),
                    &serde_json::json!({
                        "ok": false,
                        "error": error.code(),
                        "message": error.to_string(),
                    })
                    .to_string(),
                    cors_origin.as_deref(),
                )?;
                return Ok(false);
            }
        };
        let reply = super::share_routes::handle(
            &req.method,
            &req.path,
            &req.body,
            &identity,
            &own,
            registry,
        );
        crate::mcp_serve::write_mcp_http_response_with_origin(
            stream,
            reply.status,
            &reply.body,
            cors_origin.as_deref(),
        )?;
        return Ok(false);
    }
    // The lease outlives the dispatch (including a minutes-long SSE stream),
    // so the tenant these borrows point at cannot be evicted underneath them.
    //
    // The access decision's inputs are read off the SAME two things that
    // admitted this request: the lease names the document's owner, and
    // reaching this point at all means `lease_for_shared` found the caller on
    // that owner's access list. So the flag below cannot claim a share the
    // registry never checked — a visitor whose grant was revoked is refused
    // here, before a route ever asks.
    let access = RequestAccess::online(
        lease.owner_id(),
        &identity,
        lease.owner_id() != identity.user_id,
    );
    dispatch(
        stream,
        &req,
        &ConnCtx {
            state: lease.state(),
            hub: lease.hub(),
            mode: ServeMode::Online,
            // The public tool profile, narrowed further by whatever scopes
            // this particular credential carries.
            mcp_profile: crate::mcp_serve::tool_profile::McpAccessProfile::online(identity.scopes),
            rest_identity: Some(identity.clone()),
            write_barrier: Some(write_barrier),
            access,
        },
    )
}

/// The part of the route table that is served before anyone is identified.
///
/// `Ok(None)` means the request still needs an identity. Every branch here is
/// either a CORS preflight or a static asset — nothing that reads or writes an
/// account's document — because the browser cannot present a session until the
/// page that holds it has loaded.
fn serve_anonymous_prefix<S: Read + Write>(
    stream: &mut S,
    req: &crate::mcp_serve::HttpRequest,
    cors_origin: Option<&str>,
) -> Result<Option<bool>> {
    if req.method == "OPTIONS" {
        crate::mcp_serve::write_mcp_http_response_with_origin(
            stream,
            "204 No Content",
            "",
            cors_origin,
        )?;
        return Ok(Some(false));
    }
    if req.method == "GET" {
        let bundle_dir = crate::web_static::resolve_bundle_dir();
        if let Some(reply) =
            crate::web_static::handle_static_request(&req.path, bundle_dir.as_deref())
        {
            return crate::web_static::write_static_response(stream, &reply, cors_origin)
                .map(|()| Some(false));
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "online_run_loop_tests.rs"]
mod tests;
