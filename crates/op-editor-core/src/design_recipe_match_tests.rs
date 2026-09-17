//! Selection tests for the recipe matcher (issues #181 and #187).
//!
//! Every assertion goes through `crate::select_recipe` — the function the
//! product calls — so reverting the matching rule makes these fail rather
//! than only making an internal helper disagree with itself.
//!
//! The prompts are named for what they ask for, not for the run they came
//! from: the four measured baseline prompts appear once, as evidence for the
//! defects they exposed, and the rest are prompts the rules were *not*
//! written against.

use super::*;
use crate::kit_manifest::KitManifest;

fn kit() -> &'static KitManifest {
    crate::session_kit()
}

/// The shipped kit binds one recipe, and these tests are about what it does.
fn selects(prompt: &str) -> bool {
    crate::select_recipe(prompt, kit()).is_some()
}

// ---------------------------------------------------------------- measured ---

/// Issue #181, verbatim: the mobile profile prompt. It scored 17 — more than
/// the genuine switch-list prompt below — off `пользовател` and `список`,
/// and the answer was a 1440x850 desktop equipment table with zero nodes in
/// the mobile width band.
#[test]
fn the_measured_mobile_profile_prompt_places_nothing() {
    let prompt = "Нарисуй мобильный экран профиля пользователя: аватар, имя, \
                  список из четырёх пунктов настроек и нижняя навигация";
    assert!(!selects(prompt));
    assert_eq!(
        recipe_decision(prompt, kit()),
        RecipeDecision::Refuse(MatchRefusal::NoSubjectWord),
        "the mobile prompt is refused for its generic vocabulary, before shape"
    );
}

/// Issue #181's `лог`-in-`логина`, and the same defect one language over:
/// `log` in "login". Both requests are for a login screen.
#[test]
fn a_short_needle_does_not_reach_into_a_longer_word() {
    assert!(!selects(
        "Нарисуй экран логина: карточка по центру, поля email и password, кнопка Sign in"
    ));
    assert!(!selects(
        "Design a login screen: a centered card with email and password fields, and a Sign in button"
    ));
    // The same needle found mid-word, in a prompt that has nothing to do with
    // either an ops screen or a login: `лог` inside “ката**лог**а”.
    assert!(!selects(
        "Сделай экран каталога товаров с карточками и фильтром по цене"
    ));
    assert_eq!(
        recipe_decision("Сделай экран каталога товаров с карточками", kit()),
        RecipeDecision::Refuse(MatchRefusal::NotMentioned),
        "a needle inside a word is not a mention of the recipe"
    );
}

/// Issue #187: an English settings prompt was answered in Russian because
/// `list` selected the Cyrillic ops screen.
#[test]
fn the_measured_english_settings_prompt_places_nothing() {
    assert!(!selects(
        "Design a settings page with sections Profile, Notifications, Security and a \
         Save button, plus a left navigation list"
    ));
}

// ------------------------------------------------------------- subject word ---

/// Words that fit any screen with rows are not a request for *this* screen.
#[test]
fn generic_vocabulary_alone_never_places_a_recipe() {
    for prompt in [
        "Сделай экран настроек пользователя со списком опций переключателями",
        "Нарисуй экран профиля пользователя: аватар и имя",
        "Make a team management page with a member list and role dropdowns",
        "Сделай журнал событий: список записей с датой и автором",
        "Draw a mobile list screen: header, five rows, bottom navigation",
    ] {
        assert!(
            !selects(prompt),
            "only generic words here — the kit has no screen to offer: {prompt}"
        );
    }
}

/// The kit still fires for the vocabulary that names its own screen, which is
/// the case the old matcher got right and must not lose.
#[test]
fn the_kits_own_subject_wins() {
    for prompt in [
        "Собери экран: список коммутаторов с фильтром",
        "Сделай экран серверов",
        "Сделай таблицу инвентаря: коммутаторы, стойки, устройства",
        "Экран инвентаря: список серверов, коммутаторов и стоек в таблице",
        "Нарисуй реестр оборудования с поиском",
    ] {
        assert!(
            selects(prompt),
            "this request names the ops screen: {prompt}"
        );
    }
    let selected =
        crate::select_recipe("Собери экран: список коммутаторов", kit()).expect("the ops recipe");
    assert_eq!(selected.id, "ops-servers-screen");
    assert_eq!(selected.template, "tpl-recipe-ops-servers");
}

/// A Russian stem carrying an ending still matches — the reason a needle is
/// allowed to be a stem at all.
#[test]
fn a_stem_carries_its_own_inflections() {
    let cases = [
        ("коммутатор", "добавь в таблицу коммутаторов колонку модели"),
        ("сервер", "список серверов с их статусом"),
        ("оборудован", "нарисуй реестр оборудования"),
        ("устройств", "таблица устройств с фильтром"),
    ];
    for (needle, prompt) in cases {
        assert!(
            selects(prompt),
            "`{needle}` should carry its inflected form in: {prompt}"
        );
    }
}

// ------------------------------------------------------------- shape intent ---

/// A request that names the ops subject *and* asks for a phone is refused:
/// the recipe is a 1440-wide master and cannot be the screen that was asked
/// for. None of these prompts is in the baseline corpus.
#[test]
fn a_mobile_request_is_not_answered_with_the_desktop_master() {
    for prompt in [
        "Сделай мобильный экран со списком коммутаторов",
        "Нарисуй экран списка серверов для телефона, 375x812",
        "Draw the server inventory as a phone screen",
        "Сделай мобильную версию сайта с таблицей устройств",
        // No word for a phone at all — the artboard pair on its own.
        "Сделай список серверов 375x812",
    ] {
        assert!(
            !selects(prompt),
            "a phone screen and a desktop master are not the same screen: {prompt}"
        );
    }
    assert_eq!(
        recipe_decision("Сделай мобильный экран со списком коммутаторов", kit()),
        RecipeDecision::Refuse(MatchRefusal::ShapeContradicted)
    );
}

/// The shape check refuses a contradiction, not a shape: the same request
/// without a phone in it still gets the recipe, and so does one that states
/// the desktop width the master is drawn at.
#[test]
fn naming_no_shape_still_places_the_recipe() {
    for prompt in [
        "Сделай экран списка коммутаторов",
        "Сделай десктопный экран списка коммутаторов 1440x900",
    ] {
        assert!(selects(prompt), "no phone was asked for: {prompt}");
    }
}

// ----------------------------------------------------------------- script ---

/// Issue #187's general case, which the word `list` was only one instance of:
/// an English request that names the recipe's own subject is still not this
/// recipe's job, because the recipe's copy is Russian and the answer comes
/// back in the recipe's language.
#[test]
fn an_english_request_is_not_answered_with_a_cyrillic_recipe() {
    for prompt in [
        "Create an inventory screen listing every server, switch and rack in a table",
        "Build an equipment registry with a search field and a device table",
    ] {
        assert!(
            !selects(prompt),
            "the recipe's copy is Russian; this request is not: {prompt}"
        );
    }
    assert_eq!(
        recipe_decision(
            "Create an inventory screen listing every server and switch",
            kit()
        ),
        RecipeDecision::Refuse(MatchRefusal::ScriptDisagreed)
    );
}

/// The counterweight: the Russian request for the same screen keeps it, so the
/// script gate is a mismatch test and not a ban on the recipe.
#[test]
fn a_russian_request_keeps_the_russian_recipe() {
    assert!(selects(
        "Сделай экран инвентаря: список серверов и коммутаторов с таблицей"
    ));
}

// ------------------------------------------------------------------ words ---

#[test]
fn a_needle_matches_words_not_fragments() {
    assert!(needle_matches("коммутатор", "коммутаторов"));
    assert!(needle_matches("список", "список"));
    assert!(!needle_matches("лог", "логина"));
    assert!(!needle_matches("лог", "каталога"));
    assert!(!needle_matches("log", "login"));
    assert!(!needle_matches("list", "listen"));
    assert!(!needle_matches("user", "superuser"));
    assert!(!needle_matches("", "anything"));
    // Below the stem length only the whole word counts, so an inflected form
    // of a short needle is simply not evidence.
    assert!(!needle_matches("роль", "ролей"));
    assert!(!needle_matches("user", "users"));
}

#[test]
fn words_split_on_everything_that_is_not_a_letter_or_a_digit() {
    assert_eq!(
        words("Список коммутаторов: 375x812, #4F46E5 — да/нет"),
        vec!["список", "коммутаторов", "375x812", "4f46e5", "да", "нет"]
    );
    // `ё` and `е` are the same letter to a reader and to this matcher.
    assert_eq!(words("Отчёт"), words("отчет"));
}

#[test]
fn the_script_of_a_request_is_read_from_its_own_letters() {
    assert_eq!(script_of("Сделай экран"), Some(KitScript::Cyrillic));
    assert_eq!(script_of("Build a screen"), Some(KitScript::Latin));
    // No letters at all, or a balance between scripts: the request claims
    // nothing, and nothing may be refused on the strength of it.
    assert_eq!(script_of("123 — 456"), None);
    assert_eq!(script_of("да ok"), None);
    // One Latin word in a Cyrillic request does not make it an English one —
    // the comparison is between counts, not between presences.
    assert_eq!(
        script_of("Сделай экран логина с кнопкой Sign in"),
        Some(KitScript::Cyrillic)
    );
}

#[test]
fn an_explicit_artboard_pair_is_read_as_a_width() {
    assert_eq!(measured_width("сделай экран 375x812"), Some(375.0));
    assert_eq!(measured_width("canvas 1440 × 900"), Some(1440.0));
    assert_eq!(measured_width("экран 375х812"), Some(375.0));
    // Anything that is not a pair of artboard-sized numbers is not a width.
    assert_eq!(measured_width("сетка 3x3"), None);
    assert_eq!(measured_width("#4F46E5"), None);
    assert_eq!(measured_width("экран 375 на 812"), None);
}

// ------------------------------------------------ generated test manifests ---

/// A kit built in the test, so the rules can be exercised on shapes and
/// scripts the shipped kit does not carry — the point being that the rules
/// generalise past the one recipe they were measured against.
fn manifest(recipes: &str, canvas_width: f64) -> KitManifest {
    let json = format!(
        r##"{{
            "id": "test-kit",
            "name": "Test kit",
            "library": "test.lib.op",
            "sentinelMasterId": "tpl-layout-default",
            "canvas": {{ "width": {canvas_width}, "height": 850, "fill": "#FFFFFF" }},
            "types": [],
            "recipes": [{recipes}]
        }}"##
    );
    serde_json::from_str(&json).expect("the test manifest parses")
}

fn recipe(id: &str, matches: &str, extra: &str) -> String {
    format!(
        r#"{{ "id": "{id}", "name": "{id}", "template": "tpl-{id}", "notes": "", "matches": [{matches}] {extra} }}"#
    )
}

/// A second recipe changes which one wins: the request's own words decide, and
/// the older recipe is not privileged for being first in the file.
#[test]
fn the_recipe_whose_vocabulary_the_request_uses_wins() {
    let recipes = format!(
        "{}, {}",
        recipe("ops-servers-screen", r#""server","коммутатор""#, ""),
        recipe("billing-screen", r#""invoice","счёт","оплат""#, "")
    );
    let kit = manifest(&recipes, 1440.0);
    assert_eq!(
        recipe_decision("Сделай экран счетов с кнопкой оплаты", &kit),
        RecipeDecision::Place(&kit.recipes[1])
    );
    assert_eq!(
        recipe_decision("Список коммутаторов с фильтром", &kit),
        RecipeDecision::Place(&kit.recipes[0])
    );
}

/// A recipe that is the other shape declares it, and that declaration beats
/// the kit canvas — the exception the canvas fallback must not swallow.
#[test]
fn a_declared_shape_beats_the_kit_canvas() {
    // A phone recipe in a desktop kit: the mobile request is exactly what it
    // is for, and the desktop request is not.
    let recipes = recipe(
        "phone-servers",
        r#""server","сервер""#,
        r#", "formFactor": "mobile", "copyScript": "cyrillic""#,
    );
    let kit = manifest(&recipes, 1440.0);
    assert!(matches!(
        recipe_decision("Сделай мобильный экран со списком серверов", &kit),
        RecipeDecision::Place(_)
    ));
    assert!(matches!(
        recipe_decision("Сделай десктопный экран со списком серверов", &kit),
        RecipeDecision::Refuse(MatchRefusal::ShapeContradicted)
    ));

    // With nothing declared, the kit canvas is the shape: the same mobile
    // request is refused by a 1440-wide kit, and accepted by a 390-wide one.
    let undeclared = recipe("ops-servers", r#""server","сервер""#, "");
    let desktop_kit = manifest(&undeclared, 1440.0);
    assert!(matches!(
        recipe_decision("Сделай мобильный экран со списком серверов", &desktop_kit),
        RecipeDecision::Refuse(MatchRefusal::ShapeContradicted)
    ));
    let phone_kit = manifest(&undeclared, 390.0);
    assert!(matches!(
        recipe_decision("Сделай мобильный экран со списком серверов", &phone_kit),
        RecipeDecision::Place(_)
    ));
}

/// A recipe in the request's own script is placed for it; the same words in
/// the other script are not. The rule is symmetric, so neither language is
/// the special case — and the needle list stays bilingual either way, because
/// how a request is *matched* is a separate question from what language its
/// answer comes back in.
#[test]
fn the_script_gate_is_symmetric() {
    let latin = recipe(
        "inventory-en",
        r#""server","switch","сервер","коммутатор""#,
        r#", "copyScript": "latin""#,
    );
    let kit = manifest(&latin, 1440.0);
    assert!(matches!(
        recipe_decision("Create an inventory screen listing each server", &kit),
        RecipeDecision::Place(_)
    ));
    assert!(matches!(
        recipe_decision("Сделай экран инвентаря со списком серверов", &kit),
        RecipeDecision::Refuse(MatchRefusal::ScriptDisagreed)
    ));
}

/// A request that names nothing gives this reason, and it is the reason for a
/// kit whose recipes are all about something else.
#[test]
fn a_request_that_names_nothing_is_refused_as_unmentioned() {
    let recipes = recipe("ops-servers", r#""server""#, "");
    let kit = manifest(&recipes, 1440.0);
    assert_eq!(
        recipe_decision("нарисуй кота в шляпе", &kit),
        RecipeDecision::Refuse(MatchRefusal::NotMentioned)
    );
}

/// The shipped kit on requests it has no screen for — the negative half of
/// the selection, kept against the real manifest.
#[test]
fn a_request_the_kit_has_no_screen_for_selects_nothing() {
    for prompt in [
        "нарисуй кота в шляпе",
        "Create a pricing page with three plan cards, a monthly and yearly toggle, and an FAQ section",
        "Сделай дашборд аналитики в тёмной теме, акцент #4F46E5, с графиком",
        "Сделай сразу два экрана: экран регистрации и экран восстановления пароля",
    ] {
        assert!(!selects(prompt), "the kit has no screen for this: {prompt}");
    }
}

/// The decisions behind this fix, printed on demand — the table that went into
/// the issue comments. Four of these prompts are the measured baseline; the
/// rest are not in it.
///
/// ```sh
/// cargo test -p op-editor-core --lib printed_decisions -- --ignored --nocapture
/// ```
#[test]
#[ignore = "diagnostic: prints the decision for a fixed prompt corpus"]
fn printed_decisions_for_a_prompt_corpus() {
    for prompt in [
        "Нарисуй мобильный экран профиля пользователя: аватар, имя, список из четырёх пунктов настроек и нижняя навигация",
        "Нарисуй экран логина: карточка по центру, поля email и password, кнопка Sign in",
        "Сделай экран списка серверов: слева сайдбар с разделами, справа таблица с колонками",
        "Design a settings page with sections Profile, Notifications, Security and a Save button, plus a left navigation list",
        "Собери экран: список коммутаторов с фильтром",
        "Сделай экран каталога товаров с карточками",
        "Create an inventory screen listing every server, switch and rack in a table",
        "Build an equipment registry with a search field and a device table",
        "Сделай экран инвентаря: список серверов, коммутаторов и стоек в таблице",
        "Сделай мобильный экран со списком коммутаторов",
        "Нарисуй экран списка серверов для телефона, 375x812",
        "Сделай журнал событий: список записей с датой и автором",
        "Make a team management page with a member list and role dropdowns",
        "Create a pricing page with three plan cards, a monthly and yearly toggle, and an FAQ section",
    ] {
        let decision = match recipe_decision(prompt, kit()) {
            RecipeDecision::Place(recipe) => format!("PLACE {}", recipe.id),
            RecipeDecision::Refuse(reason) => format!("REFUSE {reason:?}"),
        };
        println!("{decision:30} {prompt}");
    }
}
