//! The file browser's words.
//!
//! The screen used to paint English literals — it was reachable at `/files` and
//! on a `/` load right up to the moment those literals became the first thing a
//! signed-in person reads. Every string now comes from the shared catalogue
//! (`op_i18n`), and this struct is the one place the keys are named, so paint
//! and hit-test cannot drift into two spellings of the same label.
//!
//! Sibling of `files_screen.rs`: the screen is geometry and paint, this is copy.

use op_editor_core::Locale;

/// The screen's labels, resolved once per paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilesLabels {
    pub title: &'static str,
    pub new_file: &'static str,
    pub search: &'static str,
    pub loading: &'static str,
    pub empty: &'static str,
    pub no_match: &'static str,
    pub rename: &'static str,
    pub delete: &'static str,
    pub edited_recently: &'static str,
    pub edited_just_now: &'static str,
    /// `{n}` is the number of minutes / hours / days.
    pub edited_minutes: &'static str,
    pub edited_hours: &'static str,
    pub edited_days: &'static str,
}

impl FilesLabels {
    pub fn for_locale(locale: Locale) -> Self {
        let t = |key: &'static str| op_i18n::translate(locale, key);
        Self {
            title: t("files.title"),
            new_file: t("files.new"),
            search: t("files.search"),
            loading: t("files.loading"),
            empty: t("files.empty"),
            no_match: t("files.noMatch"),
            rename: t("files.rename"),
            delete: t("files.delete"),
            edited_recently: t("files.editedRecently"),
            edited_just_now: t("files.editedJustNow"),
            edited_minutes: t("files.editedMinutes"),
            edited_hours: t("files.editedHours"),
            edited_days: t("files.editedDays"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_speaks_the_products_language() {
        // The default locale is Russian (see `EditorUiState::default`), and the
        // file list is the first screen a signed-in person sees — so these are
        // the words on it.
        let ru = FilesLabels::for_locale(Locale::Ru);
        assert_eq!(ru.title, "Файлы");
        assert_eq!(ru.new_file, "Новый файл");
        assert_eq!(ru.search, "Поиск файлов");
        assert_eq!(ru.loading, "Загрузка файлов…");
        assert_eq!(ru.empty, "Пока нет файлов — создайте первый");
        assert_eq!(ru.no_match, "Ничего не найдено");
        assert_eq!(ru.rename, "Переименовать");
        assert_eq!(ru.delete, "Удалить");
        assert_eq!(ru.edited_just_now, "Изменён только что");
        assert_eq!(ru.edited_minutes, "Изменён {n} мин назад");
    }

    #[test]
    fn every_locale_resolves_every_label() {
        // `translate` falls back through English and then the raw key, so a key
        // missing from a catalogue is visible here rather than on somebody's
        // screen: the label would be the key itself.
        for locale in Locale::ALL {
            let labels = FilesLabels::for_locale(locale);
            for (name, value) in [
                ("title", labels.title),
                ("new", labels.new_file),
                ("search", labels.search),
                ("loading", labels.loading),
                ("empty", labels.empty),
                ("noMatch", labels.no_match),
                ("rename", labels.rename),
                ("delete", labels.delete),
                ("editedRecently", labels.edited_recently),
                ("editedJustNow", labels.edited_just_now),
                ("editedMinutes", labels.edited_minutes),
                ("editedHours", labels.edited_hours),
                ("editedDays", labels.edited_days),
            ] {
                assert!(!value.is_empty(), "{locale:?} {name} is empty");
                assert!(
                    !value.starts_with("files."),
                    "{locale:?} {name} fell through to its key"
                );
            }
            // The count placeholders are the interpolation contract.
            for template in [
                labels.edited_minutes,
                labels.edited_hours,
                labels.edited_days,
            ] {
                assert!(
                    template.contains("{n}"),
                    "{locale:?} lost the {{n}} placeholder: {template}"
                );
            }
        }
    }
}
