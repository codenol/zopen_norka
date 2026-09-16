//! Waiting for an event a real worker has to produce, for this crate's tests.
//!
//! One module because the class of test it serves kept failing the same way:
//! a test waits for a genuine `spawn → exec → reap` (or for the worker that
//! owns one) under a budget written for an idle machine, and a loaded machine
//! — CI, `--test-threads=16`, several test binaries at once — exceeds it. The
//! failure then lands as `assertion failed:` in the test that *waited*, which
//! names neither the budget nor the scheduling delay that spent it. Measured
//! on an 8-core machine running the full crate, one such cycle took 3812 /
//! 6385 / 6444 / 7420 / 7514 ms, and under load (~18 loadavg on 8 cores) one
//! cycle was still running when a 10 s bound expired (issues #133, #144).
//!
//! So there is ONE budget for the whole class ([`WORKER_WAIT_BUDGET`]) and one
//! shape to wait in, and both live here rather than in each test that needs
//! them. A test that wants a different number has to say why in a comment and
//! bring its own measurements, because "this one is obviously fast" is exactly
//! the assumption that produced the bug above.
//!
//! The shape is [`wait_until`] / [`wait_for_worker`], and every part of it is
//! load-bearing:
//!
//! * **poll before consulting the budget** — a result that landed while this
//!   thread was off-CPU is already there, and a deadline-first loop (the shape
//!   these tests used to have) discards it and reports a failure that never
//!   happened;
//! * **give up as soon as the worker is gone** — a worker that finished
//!   without producing the event cannot produce it later, and its own panic
//!   message is the real diagnostic, so burning the rest of the budget in
//!   silence hides the cause;
//! * **back off instead of spinning** — the waiter must not *be* the load that
//!   makes a real subprocess slow.
//!
//! These are sync waits on purpose: they run on a test's own thread. A wait
//! inside an async block uses `tokio::time` with the same budget.

use std::thread;
use std::time::{Duration, Instant};

/// How long a test in this crate waits for an event it knows a real worker
/// will produce — a subprocess reaching its first line, a probe arriving, a
/// detached worker landing its result.
///
/// This bounds the WAIT, never the property under test: every test that uses
/// it still asserts the real outcome afterwards, so a broken path fails in
/// those assertions (or by never producing the event within this budget).
///
/// 60 s is ~8x the worst latency observed for one subprocess cycle
/// (3812 / 6385 / 6444 / 7420 / 7514 ms across runs of the full crate; a 10 s
/// bound was exceeded under load), so only a worker that is genuinely stuck
/// can reach it, and that IS the failure being reported. The same number is
/// what the #133 fix measured its way to and adopted.
pub(crate) const WORKER_WAIT_BUDGET: Duration = Duration::from_secs(60);

/// First sleep between polls. Small, because most events land immediately.
const FIRST_BACKOFF: Duration = Duration::from_millis(1);
/// Ceiling on that sleep: still responsive, never a spin.
const MAX_BACKOFF: Duration = Duration::from_millis(20);

/// Whether `arrived` holds, waiting up to [`WORKER_WAIT_BUDGET`] for it.
///
/// Returns `true` the moment it does, and `false` only when the budget ran
/// out — so a caller writes `assert!(wait_until(...), "what never happened")`
/// and gets a message that names the missing event rather than a bare
/// `assertion failed`.
pub(crate) fn wait_until(arrived: impl FnMut() -> bool) -> bool {
    // Nothing to give up on: the caller has no handle on whatever must
    // produce the event, so only the budget ends the wait.
    wait_until_with(|| false, arrived)
}

/// [`wait_until`] for an event a specific worker must produce, giving up at
/// once when that worker is already gone.
///
/// A finished worker means no amount of waiting can produce the event: either
/// it failed (and its panic is the diagnostic, which `expect` on the join
/// handle reproduces) or it took a path that does not emit the event at all.
/// Either way the budget cannot help, and spending it hides the cause.
pub(crate) fn wait_for_worker<T>(
    worker: &tokio::task::JoinHandle<T>,
    arrived: impl FnMut() -> bool,
) -> bool {
    // `is_finished` is true for a task that panicked as well as one that
    // returned, which is what makes this the give-up signal rather than a
    // completion check.
    wait_until_with(move || worker.is_finished(), arrived)
}

/// The loop both entry points share, so the shape cannot drift between them.
fn wait_until_with(mut gone: impl FnMut() -> bool, mut arrived: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + WORKER_WAIT_BUDGET;
    let mut backoff = FIRST_BACKOFF;
    loop {
        // Poll FIRST: a deadline-first loop throws away an event that already
        // landed while this thread was off-CPU.
        if arrived() {
            return true;
        }
        // Checked before the deadline so a worker that died early reports in
        // milliseconds instead of a minute — and checked after `arrived()`, so
        // a worker that finished *after* landing the event still counts.
        if gone() {
            return false;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(backoff);
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn an_event_that_already_landed_is_seen_without_spending_the_budget() {
        // The #133 regression, in miniature: the event is there before the
        // wait starts, and a deadline-first loop reports that it never came.
        let started = Instant::now();
        assert!(wait_until(|| true));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_worker_that_is_already_gone_ends_the_wait_at_once() {
        let worker = crate::chat_runtime::shared_runtime().spawn(async {});
        crate::chat_runtime::block_on_anywhere(async {
            tokio::task::yield_now().await;
        });

        let started = Instant::now();
        assert!(!wait_for_worker(&worker, || false));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a gone worker must not be waited out"
        );
    }

    #[test]
    fn an_event_produced_later_is_still_seen() {
        let flag = Arc::new(AtomicBool::new(false));
        let producer = Arc::clone(&flag);
        let spawned = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            producer.store(true, Ordering::Release);
        });

        assert!(wait_until(|| flag.load(Ordering::Acquire)));
        spawned.join().expect("producer");
    }
}
