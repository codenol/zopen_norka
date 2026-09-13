//! The comment-thread catalog is complete in all fifteen locales.
//!
//! Same guard the collaboration and Scene Template catalogs carry, for the same
//! reason: `translate` falls back through English and then returns the raw key,
//! so a locale that is missing one of these ships a canvas pin whose panel says
//! `comments.action.resolve`. The cross-locale key-set test would catch that
//! today, but it compares against whatever English happens to hold — this names
//! the keys a feature actually needs, so deleting one from every table at once
//! is still a failure.

/// Every key the comment-thread chrome looks up, in one list.
///
/// Kept as a literal rather than derived from the tables: the point is to state
/// what the widgets ask for, independently of what the catalogs happen to hold.
const COMMENT_KEYS: [&str; 27] = [
    "comments.panel.title",
    "comments.panel.loading",
    "comments.panel.empty",
    "comments.panel.error",
    "comments.panel.unpinned",
    "comments.panel.replyCount",
    "comments.panel.more",
    "comments.panel.open",
    "comments.panel.resolved",
    "comments.pin.arm",
    "comments.pin.armed",
    "comments.composer.placeholder",
    "comments.composer.replyPlaceholder",
    "comments.composer.send",
    "comments.action.resolve",
    "comments.action.reopen",
    "comments.action.close",
    "comments.action.showList",
    "comments.author.you",
    "comments.author.local",
    "comments.author.unknown",
    "comments.error.refused",
    "comments.error.gone",
    "comments.error.rejected",
    "comments.error.transport",
    "comments.error.unsaved",
    "comments.resolvedBy",
];

#[test]
fn every_comment_key_is_translated_in_every_locale() {
    for key in COMMENT_KEYS {
        for locale in crate::Locale::ALL {
            let translated = super::translate(locale, key);
            assert_ne!(
                translated, key,
                "locale `{}` has no direct value for `{key}`",
                locale.code()
            );
            assert!(
                !translated.is_empty(),
                "locale `{}` translates `{key}` to nothing",
                locale.code()
            );
        }
    }
}

#[test]
fn the_english_wording_is_the_one_the_widgets_were_written_against() {
    // A canary, not a style rule: if English moves, the two strings a reviewer
    // reads most often moved with it, and somebody should have meant that.
    assert_eq!(
        super::translate(crate::Locale::EnUs, "comments.composer.send"),
        "Send"
    );
    assert_eq!(
        super::translate(crate::Locale::EnUs, "comments.action.resolve"),
        "Resolve"
    );
    assert_eq!(
        super::translate(crate::Locale::EnUs, "comments.panel.title"),
        "Comments"
    );
}
