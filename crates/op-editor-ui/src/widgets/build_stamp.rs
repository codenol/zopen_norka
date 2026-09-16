//! How fresh the running build is, and how the top bar says so.
//!
//! The stamp exists because "am I looking at an old build?" is a real
//! question here: the kit manifest is compiled in, so a config change is
//! invisible until a rebuild. Colour answers it without reading a timestamp —
//! green is a build made minutes ago, amber is one that is getting old, red
//! is one that predates the work you are looking at.

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

/// Freshness buckets, by build age.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildFreshness {
    /// Under three minutes old.
    Fresh,
    /// Three to six minutes.
    Ageing,
    /// Over six minutes.
    Stale,
}

/// Seconds between the build and the wall clock (never negative).
///
/// `now_unix_ms` is milliseconds (`Date.now()`); `BUILD_EPOCH` is seconds.
/// Mixing the two silently reports a decades-old build — which is exactly
/// how the first version of this stamped every build red.
pub fn build_age_secs(now_unix_ms: f64) -> u64 {
    if now_unix_ms <= 0.0 || BUILD_EPOCH == 0 {
        return 0;
    }
    ((now_unix_ms / 1000.0) as u64).saturating_sub(BUILD_EPOCH)
}

pub fn freshness(age_secs: u64) -> BuildFreshness {
    if age_secs <= 180 {
        BuildFreshness::Fresh
    } else if age_secs <= 360 {
        BuildFreshness::Ageing
    } else {
        BuildFreshness::Stale
    }
}

/// How long one on/off cycle lasts, or `None` when the stamp holds steady.
pub fn blink_period_ms(freshness: BuildFreshness) -> Option<u64> {
    match freshness {
        BuildFreshness::Fresh => None,
        BuildFreshness::Ageing => Some(3_000),
        BuildFreshness::Stale => Some(1_000),
    }
}

/// Whether the stamp is in its visible half of the blink cycle.
///
/// `Some(0)` is "fixed on", not a division by zero: a zero period would
/// otherwise panic in the arm below.
pub fn blink_visible(now_ms: u64, period: Option<u64>) -> bool {
    match period {
        None => true,
        Some(0) => true,
        Some(period) => (now_ms % period) < period / 2,
    }
}

/// The instant the stamp next needs a repaint, while it is blinking.
pub fn next_blink_deadline_ms(now_ms: u64, period: Option<u64>) -> Option<u64> {
    let period = period?;
    if period == 0 {
        return None;
    }
    let phase = now_ms % period;
    let half = period / 2;
    Some(
        now_ms
            + if phase < half {
                half - phase
            } else {
                period - phase
            },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milliseconds_are_converted_before_the_comparison() {
        // A build made one minute ago, expressed the way the host reports it.
        let now_ms = (BUILD_EPOCH as f64 + 60.0) * 1000.0;
        assert_eq!(build_age_secs(now_ms), 60);
        assert_eq!(freshness(build_age_secs(now_ms)), BuildFreshness::Fresh);
    }

    #[test]
    fn the_buckets_follow_the_three_and_six_minute_edges() {
        assert_eq!(freshness(0), BuildFreshness::Fresh);
        assert_eq!(freshness(180), BuildFreshness::Fresh);
        assert_eq!(freshness(181), BuildFreshness::Ageing);
        assert_eq!(freshness(360), BuildFreshness::Ageing);
        assert_eq!(freshness(361), BuildFreshness::Stale);
    }

    #[test]
    fn only_a_fresh_build_holds_steady() {
        assert_eq!(blink_period_ms(BuildFreshness::Fresh), None);
        assert_eq!(blink_period_ms(BuildFreshness::Ageing), Some(3_000));
        assert_eq!(blink_period_ms(BuildFreshness::Stale), Some(1_000));
    }

    #[test]
    fn the_stamp_blinks_half_on_half_off() {
        assert!(blink_visible(0, Some(1_000)));
        assert!(!blink_visible(500, Some(1_000)));
        assert!(blink_visible(1_000, Some(1_000)));
        assert!(blink_visible(12_345, None));
    }

    #[test]
    fn the_next_deadline_lands_on_the_flip() {
        assert_eq!(next_blink_deadline_ms(0, Some(1_000)), Some(500));
        assert_eq!(next_blink_deadline_ms(700, Some(1_000)), Some(1_000));
        assert_eq!(next_blink_deadline_ms(0, None), None);
    }
}
