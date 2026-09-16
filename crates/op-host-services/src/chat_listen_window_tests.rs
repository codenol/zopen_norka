//! The listen-window setting's own rule: a typo must not remove the bound.

use super::*;

#[test]
fn an_absent_or_nonsense_value_keeps_the_default() {
    let default = Duration::from_secs(DEFAULT_LISTEN_WINDOW_SECS);
    for raw in [
        None,
        Some(""),
        Some("   "),
        Some("ten minutes"),
        Some("30s"),
        Some("-5"),
        Some("0"),
    ] {
        assert_eq!(
            listen_window_from(raw),
            default,
            "`{raw:?}` must not replace the shipped window"
        );
    }
}

#[test]
fn a_value_at_or_above_the_floor_is_honoured() {
    assert_eq!(
        listen_window_from(Some(" 90 ")),
        Duration::from_secs(90),
        "whitespace around a real number is what pasting produces"
    );
    assert_eq!(
        listen_window_from(Some("1")),
        Duration::from_secs(1),
        "the floor itself is a setting somebody can mean: fail fast on a known host"
    );
}

#[test]
fn the_shipped_default_clears_the_measured_spawn_cycle() {
    // #152's measurements: a single spawn-exec-reap cycle in this suite ran
    // 3812 / 6385 / 6444 / 7420 / 7514 ms, and 10 000 ms under load. A window
    // that cannot clear the slowest of those is the defect this file exists to
    // remove, so the assertion is stated against the measurements rather than
    // against a number copied from the constant.
    const SLOWEST_MEASURED_CYCLE_SECS: u64 = 10;
    // The window the daemon actually runs with, asked for the way a deployment
    // with no setting asks for it. Comparing the two constants directly would be
    // decided by the compiler and would not notice a reader that stopped
    // returning `DEFAULT_LISTEN_WINDOW_SECS` at all — which is the failure this
    // test is here to catch.
    let shipped = listen_window_from(None);
    assert!(
        shipped >= Duration::from_secs(SLOWEST_MEASURED_CYCLE_SECS),
        "the shipped window ({shipped:?}) must clear the slowest measured cycle, not sit inside it"
    );
}
