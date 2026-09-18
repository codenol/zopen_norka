//! How many screens a request is asking for.
//!
//! One decision, shared by the two places that need the same answer: the
//! scorer's screen rule (`op-design-lint`'s `S-01`) and the plan normaliser
//! (`op-orchestrator`'s `plan_normalize`), which decides whether a plan's
//! per-screen labels become several roots or one.
//!
//! Why it is shared rather than written twice: when the two disagree, the
//! product flags its own output. Measured on a live turn — the plan split six
//! subtasks across six screen labels, the scaffold built six roots, and the
//! screen rule then reported two of them as violations of the product's own
//! rule. One predicate means one answer.
//!
//! The rule is "one request, one screen" — NOT "one screen, always": a request
//! that names a flow, several pages or two screens is asking for exactly that,
//! and treating it as one would be wrong rather than strict.

/// The phrasings that mean "more than one screen", in the languages the
/// product's users actually type.
const MANY_SCREEN_MARKERS: [&str; 11] = [
    "два экран",
    "две страниц",
    "три экран",
    "три страниц",
    "несколько экран",
    "несколько страниц",
    "экраны",
    "страницы",
    "screens",
    "pages",
    "flow",
];

/// Whether the request itself asks for more than one screen.
pub fn request_asks_for_many_screens(prompt: &str) -> bool {
    let prompt = prompt.to_lowercase();
    MANY_SCREEN_MARKERS
        .iter()
        .any(|marker| prompt.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_singular_request_is_one_screen() {
        for prompt in [
            "Собери экран с таблицей ПАКов: узлы, кластеры, ВМ",
            "Create a pricing page with three plan cards",
            "сделай красиво",
            "нарисуй дашборд оборудования",
        ] {
            assert!(
                !request_asks_for_many_screens(prompt),
                "{prompt} asks for one screen"
            );
        }
    }

    #[test]
    fn a_plural_request_is_many_screens() {
        for prompt in [
            "Сделай два экрана: вход и регистрация",
            "сделай несколько страниц приложения",
            "Build the remaining 3 pages of the app",
            "нарисуй экраны онбординга",
            "собери flow регистрации",
        ] {
            assert!(
                request_asks_for_many_screens(prompt),
                "{prompt} asks for several screens"
            );
        }
    }
}
