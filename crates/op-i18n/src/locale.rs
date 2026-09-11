/// UI locale supported by Norka.
///
/// Every variant has a complete direct translation table. [`Locale::code`]
/// returns its stable BCP-47 identifier for persistence and transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    EnUs,
    ZhCn,
    ZhTw,
    Ja,
    Ko,
    Fr,
    Es,
    De,
    Pt,
    Ru,
    Hi,
    Tr,
    Th,
    Vi,
    Id,
}

impl Locale {
    /// All locales in the UI picker order.
    pub const ALL: [Locale; 15] = [
        Locale::EnUs,
        Locale::ZhCn,
        Locale::ZhTw,
        Locale::Ja,
        Locale::Ko,
        Locale::Fr,
        Locale::Es,
        Locale::De,
        Locale::Pt,
        Locale::Ru,
        Locale::Hi,
        Locale::Tr,
        Locale::Th,
        Locale::Vi,
        Locale::Id,
    ];

    /// Stable BCP-47 identifier used in settings and wire payloads.
    pub const fn code(self) -> &'static str {
        match self {
            Locale::EnUs => "en-US",
            Locale::ZhCn => "zh-CN",
            Locale::ZhTw => "zh-TW",
            Locale::Ja => "ja",
            Locale::Ko => "ko",
            Locale::Fr => "fr",
            Locale::Es => "es",
            Locale::De => "de",
            Locale::Pt => "pt",
            Locale::Ru => "ru",
            Locale::Hi => "hi",
            Locale::Tr => "tr",
            Locale::Th => "th",
            Locale::Vi => "vi",
            Locale::Id => "id",
        }
    }

    /// Parse a BCP-47 tag or a common POSIX locale spelling.
    ///
    /// Matching is ASCII case-insensitive. `_` separators, encoding suffixes
    /// such as `.UTF-8`, and modifiers such as `@latin` are accepted so this
    /// can consume `LANG`/`LC_*` values directly. Unsupported languages and
    /// the POSIX `C` locale return `None`.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim();
        if tag.is_empty() {
            return None;
        }
        let base = tag
            .split(['.', '@'])
            .next()
            .unwrap_or(tag)
            .replace('_', "-")
            .to_ascii_lowercase();
        if matches!(base.as_str(), "c" | "posix") {
            return None;
        }
        let parts: Vec<&str> = base.split('-').filter(|part| !part.is_empty()).collect();
        let language = *parts.first()?;
        Some(match language {
            "en" => Locale::EnUs,
            "zh" => {
                // An explicit script subtag is more specific than the
                // region, including intentionally conflicting tags such as
                // `zh-Hans-TW`. Only infer the script from the region when
                // the tag does not name one.
                match parts.iter().skip(1).find_map(|part| match *part {
                    "hans" => Some(Locale::ZhCn),
                    "hant" => Some(Locale::ZhTw),
                    _ => None,
                }) {
                    Some(locale) => locale,
                    None if parts
                        .iter()
                        .skip(1)
                        .any(|part| matches!(*part, "tw" | "hk" | "mo")) =>
                    {
                        Locale::ZhTw
                    }
                    // Bare `zh`, CN and SG all use Simplified Chinese.
                    None => Locale::ZhCn,
                }
            }
            "ja" => Locale::Ja,
            "ko" => Locale::Ko,
            "fr" => Locale::Fr,
            "es" => Locale::Es,
            "de" => Locale::De,
            "pt" => Locale::Pt,
            "ru" => Locale::Ru,
            "hi" => Locale::Hi,
            "tr" => Locale::Tr,
            "th" => Locale::Th,
            "vi" => Locale::Vi,
            // `in` is the deprecated ISO 639 code still emitted by some
            // Android/JVM environments.
            "id" | "in" => Locale::Id,
            _ => return None,
        })
    }

    /// Parse a tag and default unsupported or empty values to English.
    pub fn parse_lossy(tag: &str) -> Self {
        Self::from_tag(tag).unwrap_or(Locale::EnUs)
    }

    /// Resolve locale environment values in POSIX precedence order.
    ///
    /// The first non-empty value controls the result. `C`, `POSIX`, and
    /// unsupported languages use English rather than falling through to a
    /// lower-precedence variable.
    pub fn from_env_values(
        lc_all: Option<&str>,
        lc_messages: Option<&str>,
        lang: Option<&str>,
    ) -> Option<Self> {
        [lc_all, lc_messages, lang]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|value| !value.is_empty())
            .map(Self::parse_lossy)
    }

    /// Read `LC_ALL`, `LC_MESSAGES`, then `LANG` from the process environment.
    pub fn from_environment() -> Option<Self> {
        let lc_all = std::env::var("LC_ALL").ok();
        let lc_messages = std::env::var("LC_MESSAGES").ok();
        let lang = std::env::var("LANG").ok();
        Self::from_env_values(lc_all.as_deref(), lc_messages.as_deref(), lang.as_deref())
    }

    /// Cycle to the next locale (round-trips through `ALL`).
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&l| l == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Native-script display name (matches the TS dropdown).
    pub fn display_name(self) -> &'static str {
        match self {
            Locale::EnUs => "English",
            Locale::ZhCn => "简体中文",
            Locale::ZhTw => "繁體中文",
            Locale::Ja => "日本語",
            Locale::Ko => "한국어",
            Locale::Fr => "Français",
            Locale::Es => "Español",
            Locale::De => "Deutsch",
            Locale::Pt => "Português",
            Locale::Ru => "Русский",
            Locale::Hi => "हिन्दी",
            Locale::Tr => "Türkçe",
            Locale::Th => "ไทย",
            Locale::Vi => "Tiếng Việt",
            Locale::Id => "Bahasa Indonesia",
        }
    }
}

impl std::str::FromStr for Locale {
    type Err = ();

    fn from_str(tag: &str) -> Result<Self, Self::Err> {
        Self::from_tag(tag).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::Locale;

    #[test]
    fn stable_codes_round_trip() {
        for locale in Locale::ALL {
            assert_eq!(Locale::from_tag(locale.code()), Some(locale));
        }
    }

    #[test]
    fn parses_bcp47_and_posix_aliases() {
        assert_eq!(Locale::from_tag("en"), Some(Locale::EnUs));
        assert_eq!(Locale::from_tag("EN_us.UTF-8"), Some(Locale::EnUs));
        assert_eq!(Locale::from_tag("zh-CN"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh-Hans-SG"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh-Hans-TW"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-Hant-HK"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-Hant-CN"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("in-ID"), Some(Locale::Id));
        assert_eq!(Locale::from_tag("C"), None);
        assert_eq!(Locale::from_tag("xx-ZZ"), None);
    }

    #[test]
    fn environment_values_follow_posix_precedence() {
        assert_eq!(
            Locale::from_env_values(Some("fr-FR"), Some("de-DE"), Some("ja-JP")),
            Some(Locale::Fr)
        );
        assert_eq!(
            Locale::from_env_values(Some("C"), Some("zh_Hant.UTF-8"), Some("en_US")),
            Some(Locale::EnUs)
        );
        assert_eq!(
            Locale::from_env_values(Some(""), Some("zh_Hant.UTF-8"), Some("en_US")),
            Some(Locale::ZhTw)
        );
        assert_eq!(
            Locale::from_env_values(Some("xx-ZZ"), None, Some("ja-JP")),
            Some(Locale::EnUs)
        );
        assert_eq!(Locale::from_env_values(None, None, None), None);
        assert_eq!(Locale::parse_lossy("unsupported"), Locale::EnUs);
    }
}
