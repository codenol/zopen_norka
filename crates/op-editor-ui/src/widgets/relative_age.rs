//! Coarse, localized "how long ago" for chrome that shows a timestamp.
//!
//! Two surfaces need the same ladder — the recent-files menu's age column and
//! the recovery banner's "found …" clause — and they must not drift: a rule
//! like "under a minute is *just now*" is a product decision, and a second
//! copy of it is how two screens end up disagreeing about the same file.
//!
//! The phrases reuse the `fileMenu.*Ago` keys the menu already established
//! rather than adding a parallel family: they are the same words about the
//! same thing, already translated in all fifteen locales.

use op_i18n::Locale;

/// Elapsed seconds as a coarse phrase — "just now", "12m ago", "3h ago".
///
/// Deliberately coarse. Both callers answer "was this the thing I was working
/// on", and a second-accurate age answers nothing the coarse one does not.
pub fn relative_age_label(locale: Locale, elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        op_i18n::translate(locale, "fileMenu.justNow").to_string()
    } else if elapsed_secs < 3_600 {
        count(locale, "fileMenu.minutesAgo", elapsed_secs / 60)
    } else if elapsed_secs < 86_400 {
        count(locale, "fileMenu.hoursAgo", elapsed_secs / 3_600)
    } else {
        count(locale, "fileMenu.daysAgo", elapsed_secs / 86_400)
    }
}

/// One `{{count}}` template, filled from an integer.
fn count(locale: Locale, key: &'static str, value: u64) -> String {
    op_i18n::translate(locale, key).replace("{{count}}", &value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_switches_at_minute_hour_and_day() {
        let label = |secs| relative_age_label(Locale::EnUs, secs);
        assert_eq!(label(0), "just now");
        assert_eq!(label(59), "just now");
        assert_eq!(label(60), "1m ago");
        assert_eq!(label(3_599), "59m ago");
        assert_eq!(label(3_600), "1h ago");
        assert_eq!(label(86_399), "23h ago");
        assert_eq!(label(86_400), "1d ago");
        assert_eq!(label(86_400 * 9), "9d ago");
    }

    #[test]
    fn the_phrase_is_localized_not_just_translated_once() {
        // The recovery banner shares this ladder; a regression to a hard-coded
        // English string would be invisible in an English-only test.
        assert_eq!(relative_age_label(Locale::Ru, 120), "2м назад");
        assert_eq!(relative_age_label(Locale::Ru, 30), "только что");
        assert_eq!(relative_age_label(Locale::Ja, 30), "たった今");
    }
}
