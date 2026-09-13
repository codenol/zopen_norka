//! Who said it, when, and in which colour — the shared reading of a comment.
//!
//! Three surfaces show the same three facts about a comment — the pin in the
//! canvas, the thread popover and the list panel — and they must not disagree:
//! a name resolved one way in the popover and another way in the list is the
//! kind of drift that makes a reviewer doubt which comment they are reading.
//! So the rules live here, as pure functions over plain data, and every widget
//! calls them.
//!
//! ## Why the role decides a colour and `None` decides nothing
//!
//! `author_role` is the wire string the hub sent, and the operator's seven
//! product roles are what the workspace's access matrix already keys on
//! (`op_editor_core::access`). Resolving the colour through that same enum is
//! what keeps "who is speaking" recognisable at a glance — and an unknown or
//! absent role deliberately resolves to nothing rather than to a made-up
//! colour: a role this build has never heard of must not be shown as if it were
//! one of the seven. The caller paints its neutral tone for `None`.

use op_editor_core::editor_ui_state::CommentAuthor;
use op_i18n::Locale;

use crate::widgets::relative_age::relative_age_label;
use crate::Color;

/// How an author is named, per the viewer.
///
/// The three cases are three different statements:
///
/// - an author with **no account id** is the daemon's local operator, which is
///   this browser's own session on a deployment with no accounts — the only
///   "me" this client can be sure of, so it reads as *you*;
/// - an author whose id **is** the viewer's is literally the viewer;
/// - a named account is shown by the name recorded at the time.
///
/// `viewer_id` is an `Option` because this build has no account-id projection in
/// the editor state: when it is `None` no id can be recognised, and a comment
/// carrying an id keeps its name (or reads as unknown when the hub sent an empty
/// one). Claiming "You" for somebody else's comment would be worse than saying
/// nothing.
pub fn author_label(author: &CommentAuthor, viewer_id: Option<&str>, locale: Locale) -> String {
    if author.id.is_none() {
        return op_i18n::translate(locale, "comments.author.local").to_string();
    }
    if viewer_id.is_some() && author.id.as_deref() == viewer_id {
        return op_i18n::translate(locale, "comments.author.you").to_string();
    }
    let name = author.name.trim();
    if name.is_empty() {
        return op_i18n::translate(locale, "comments.author.unknown").to_string();
    }
    name.to_string()
}

/// The colour a role is shown in, or `None` when there is no role to show.
///
/// Read from the model rather than from a table here: the operator's roles are
/// `op_editor_core::access`'s, and a second palette in the widget layer is how
/// the same person ends up two colours in two panels.
pub fn role_colour(role: Option<&str>) -> Option<Color> {
    let role = role?;
    let role = op_editor_core::ProductRole::from_wire(role).ok()?;
    crate::util::parse_hex_color(role.colour().hex)
}

/// How long ago a comment was written, in whole seconds.
///
/// Clamped at zero: `created_at` is the *server's* clock and `now_unix_ms` is
/// the browser's, so a machine a few seconds ahead of the daemon would
/// otherwise paint an age in the future — or, since the ladder takes `u64`,
/// panic in a debug build on the subtraction.
pub fn age_secs(created_at: u64, now_unix_ms: f64) -> u64 {
    let now_secs = (now_unix_ms / 1000.0).max(0.0) as u64;
    now_secs.saturating_sub(created_at)
}

/// "5m ago" for a comment, in the viewer's language.
///
/// The file menu's ladder rather than a new one: "under a minute is *just now*"
/// is a product decision this codebase already made once, and a comment is no
/// more precise about its own age than a file is about its last save.
pub fn age_label(created_at: u64, now_unix_ms: f64, locale: Locale) -> String {
    relative_age_label(locale, age_secs(created_at, now_unix_ms))
}

/// The first line of a comment, shortened to fit one row of the list.
///
/// Collapses every run of whitespace — including the newlines a multi-paragraph
/// comment is full of — because a row is one line, and a body that wrapped would
/// push every row below it down by an amount the panel's row geometry cannot
/// know about. Truncation counts characters, so a Cyrillic comment is not cut
/// three times shorter than a Latin one.
pub fn excerpt(body: &str, max_chars: usize) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let kept: String = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::editor_ui_state::{Comment, CommentThread};

    fn author(id: Option<&str>, name: &str, role: Option<&str>) -> CommentAuthor {
        CommentAuthor {
            id: id.map(str::to_string),
            name: name.to_string(),
            role: role.map(str::to_string),
        }
    }

    #[test]
    fn the_local_operator_reads_as_a_place_not_a_person() {
        let local = author(None, "", None);
        assert_eq!(author_label(&local, None, Locale::EnUs), "Local operator");
        assert_eq!(
            author_label(&local, Some("u1"), Locale::Ru),
            "Локальный оператор"
        );
    }

    #[test]
    fn only_the_viewers_own_id_claims_to_be_the_viewer() {
        let mine = author(Some("u1"), "Kay", Some("ux_ui"));
        assert_eq!(author_label(&mine, Some("u1"), Locale::EnUs), "You");
        // Somebody else's comment keeps its name even when the viewer is known.
        assert_eq!(author_label(&mine, Some("u2"), Locale::EnUs), "Kay");
        // With no id of our own, no account comment may claim to be us.
        assert_eq!(author_label(&mine, None, Locale::EnUs), "Kay");
    }

    #[test]
    fn a_comment_by_an_account_with_no_name_is_unknown_not_blank() {
        let nameless = author(Some("u9"), "   ", None);
        assert_eq!(author_label(&nameless, Some("u1"), Locale::EnUs), "Unknown");
        assert_eq!(author_label(&nameless, Some("u1"), Locale::Ja), "不明");
    }

    #[test]
    fn the_yous_are_localized() {
        assert_eq!(
            author_label(&author(Some("u1"), "Kay", None), Some("u1"), Locale::Ru),
            "Вы"
        );
        assert_eq!(
            author_label(&author(Some("u1"), "Kay", None), Some("u1"), Locale::De),
            "Du"
        );
    }

    #[test]
    fn a_known_role_gets_the_models_colour() {
        assert_eq!(
            role_colour(Some("ux_ui")),
            crate::util::parse_hex_color("#8B5CF6")
        );
        assert_eq!(
            role_colour(Some("admin")),
            crate::util::parse_hex_color("#E0A800")
        );
    }

    #[test]
    fn an_unknown_absent_or_blank_role_gets_no_colour() {
        assert!(role_colour(None).is_none());
        assert!(role_colour(Some("")).is_none());
        // A role this build does not know must not be painted as one it does.
        assert!(role_colour(Some("chief-vibes-officer")).is_none());
    }

    #[test]
    fn the_age_ladder_is_the_one_the_file_menu_already_uses() {
        let now = 1_700_000_000_000.0;
        assert_eq!(age_secs(1_699_999_700, now), 300);
        assert_eq!(age_label(1_699_999_700, now, Locale::EnUs), "5m ago");
        assert_eq!(age_label(1_699_999_999, now, Locale::EnUs), "just now");
        assert_eq!(age_label(1_699_999_700, now, Locale::Ru), "5м назад");
    }

    #[test]
    fn a_clock_behind_the_server_yields_no_negative_age() {
        // The browser's clock is a few seconds behind the daemon's.
        assert_eq!(age_secs(1_700_000_100, 1_700_000_000_000.0), 0);
        assert_eq!(
            age_label(1_700_000_100, 1_700_000_000_000.0, Locale::EnUs),
            "just now"
        );
    }

    #[test]
    fn an_age_computed_from_a_zero_clock_does_not_panic() {
        // The host seeds `now_unix_ms` to 0 before its first frame.
        assert_eq!(age_secs(1_700_000_000, 0.0), 0);
    }

    #[test]
    fn an_excerpt_collapses_newlines_into_one_line() {
        assert_eq!(
            excerpt("first line\nsecond line", 40),
            "first line second line"
        );
        assert_eq!(excerpt("  spaced \n\t out  ", 40), "spaced out");
    }

    #[test]
    fn an_excerpt_longer_than_its_budget_is_cut_with_an_ellipsis() {
        assert_eq!(excerpt("abcdefghij", 5), "abcd…");
        assert_eq!(excerpt("abcdefghij", 10), "abcdefghij");
        assert_eq!(excerpt("", 10), "");
    }

    #[test]
    fn the_excerpt_budget_counts_characters_not_bytes() {
        // Ten Cyrillic characters are twenty bytes; a byte limit would cut this
        // string after five of them.
        assert_eq!(excerpt("абвгдеёжзи", 10), "абвгдеёжзи");
        assert_eq!(excerpt("абвгдеёжзи", 6), "абвгд…");
    }

    #[test]
    fn a_thread_with_no_comments_has_no_author_to_name() {
        let empty = CommentThread::default();
        assert!(empty.first().is_none());
        // The helper is what the widgets call; a thread with no comments simply
        // has nobody to look up, which the callers gate on.
        let named: Option<String> = empty
            .first()
            .map(|comment| author_label(&comment.author, None, Locale::EnUs));
        assert!(named.is_none());
        let _ = Comment::default();
    }
}
