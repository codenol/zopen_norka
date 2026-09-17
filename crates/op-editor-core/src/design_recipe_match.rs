//! Which recipe a request asks for, and when none may be placed.
//!
//! Selection is a decision the product makes, not a hint it hopes the model
//! takes: a placed recipe becomes the page, arrives with a Require rule
//! telling the model to keep it, and turns the turn into a rewrite of a
//! canned screen. So the bar is not "some word looked related" — it is "this
//! request is asking for this recipe".
//!
//! A measured baseline of the browser path (eight prompts, one fresh document
//! each) found the bar was far below that, and that the failures were all in
//! *what* the score was made of rather than how large it was:
//!
//! * a mobile profile prompt scored 17 — higher than the genuine switch-list
//!   prompt's 16 — off the needles `пользовател` and `список`, and was
//!   answered with a 1440x850 desktop equipment table;
//! * `лог` matched inside “**лог**ина” and inside “ката**лог**а”, `log` inside
//!   “**log**in”, so a login prompt selected an ops screen;
//! * `list` in "a left navigation list" was the entire evidence for an
//!   English settings prompt, which came back with a Russian ops screen.
//!
//! No score floor can separate 17 from 16, so this module does not try. It
//! asks five questions instead, and a recipe has to pass all of them:
//!
//! 1. a needle matches a **word** of the request, not a fragment of one;
//! 2. a needle shorter than [`STEM_MIN_CHARS`] matches only as a whole word,
//!    because at three or four characters a shared opening is as likely to be
//!    a different word (`лог`/`логина`) as an inflection of its own;
//! 3. the request names at least one of the recipe's **subject** words, not
//!    only words that describe any screen with rows (`список`, `list`);
//! 4. the request does not ask for a shape the recipe is not
//!    (`мобильный экран` against a desktop master);
//! 5. the request is written in the script the recipe's copy is written in.
//!
//! Each of the four measured failures above is caught by one of these. The
//! direction is deliberate: refusing a recipe costs the kit's help and leaves
//! the prompt in the model's hands, while placing a wrong one replaces the
//! requested screen with a stored one — measured, the login prompt that took
//! the no-recipe route produced the screen that was asked for.

use crate::kit_manifest::{KitFormFactor, KitManifest, KitRecipe, KitScript};

/// A needle this long or longer may stand for its inflected forms; anything
/// shorter matches only the whole word.
///
/// The kit writes its Russian needles as stems (`коммутатор` → “коммутаторов”,
/// `таблиц` → “таблица”), so a stem has to reach past its own word. Five
/// characters is where that stops being safe: `лог` opens “логина”, `log`
/// opens “login”, `list` opens “listen”, `роль` opens “рольганг” — all
/// measured or one step from a measured misfire. Their cost is small and
/// one-directional: a three- or four-character word only ever counts as
/// itself, and every one of them is supporting vocabulary, which cannot select
/// a recipe on its own.
const STEM_MIN_CHARS: usize = 5;

/// Words that ask for a phone-shaped screen.
const MOBILE_WORDS: &[&str] = &[
    "mobile",
    "phone",
    "smartphone",
    "iphone",
    "ipad",
    "android",
    "tablet",
    "мобильн",
    "телефон",
    "смартфон",
    "планшет",
    "айфон",
];

/// Words that ask for a desktop-shaped screen.
const DESKTOP_WORDS: &[&str] = &["desktop", "laptop", "десктоп", "ноутбук"];

/// The decision, and — when nothing may be placed — which gate refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeDecision<'a> {
    /// The request asks for this recipe's screen and contradicts nothing
    /// about it.
    Place(&'a KitRecipe),
    /// No recipe may be placed this turn.
    Refuse(MatchRefusal),
}

impl<'a> RecipeDecision<'a> {
    /// The recipe to place, or `None` when the request must be answered
    /// without one.
    pub fn recipe(self) -> Option<&'a KitRecipe> {
        match self {
            Self::Place(recipe) => Some(recipe),
            Self::Refuse(_) => None,
        }
    }
}

/// The gate that refused a placement.
///
/// Reported rather than swallowed so a "why did I get the plain route" report
/// can be answered from the decision instead of from a rerun.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchRefusal {
    /// No recipe's needles appear in the request at all.
    NotMentioned,
    /// Only words that fit any screen with rows (`список`, `list`,
    /// `пользовател`) — none of the recipe's own subjects.
    NoSubjectWord,
    /// The request asks for a shape this recipe's master is not.
    ShapeContradicted,
    /// The request is written in a script this recipe's copy is not.
    ScriptDisagreed,
}

/// The recipe this request is asking for, or why none may be placed.
pub fn recipe_decision<'a>(prompt: &str, kit: &'a KitManifest) -> RecipeDecision<'a> {
    let words = words(prompt);
    let script = script_of(prompt);
    let shape = shape_intent(prompt, &words);

    let mut candidates: Vec<Candidate<'a>> = kit
        .recipes
        .iter()
        .filter_map(|recipe| {
            let subjects = matched(&words, &recipe.matches);
            let supporting = matched(&words, &recipe.supporting);
            // A recipe whose vocabulary is nowhere in the request is not this
            // request's business and its refusal is not worth reporting.
            if subjects.is_empty() && supporting.is_empty() {
                return None;
            }
            let blocked = if subjects.is_empty() {
                Some(MatchRefusal::NoSubjectWord)
            } else if shape_contradicted(shape, recipe.form_factor(kit)) {
                Some(MatchRefusal::ShapeContradicted)
            } else if script_contradicted(script, recipe.copy_script) {
                Some(MatchRefusal::ScriptDisagreed)
            } else {
                None
            };
            Some(Candidate {
                recipe,
                score: subjects.chars + supporting.chars,
                blocked,
            })
        })
        .collect();

    if candidates.is_empty() {
        return RecipeDecision::Refuse(MatchRefusal::NotMentioned);
    }
    // Most matched vocabulary first, then id, so the choice is the same on
    // every run and in every process.
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.recipe.id.cmp(&b.recipe.id))
    });
    if let Some(found) = candidates
        .iter()
        .find(|candidate| candidate.blocked.is_none())
    {
        return RecipeDecision::Place(found.recipe);
    }
    RecipeDecision::Refuse(candidates[0].blocked.unwrap_or(MatchRefusal::NotMentioned))
}

/// One recipe that the request's words reached, and whether it may be placed.
struct Candidate<'a> {
    recipe: &'a KitRecipe,
    score: usize,
    blocked: Option<MatchRefusal>,
}

/// How much of a needle list a request's words matched.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Matched {
    /// Distinct needles that matched at least one word.
    needles: usize,
    /// Their total length — the old ranking, kept for ordering only.
    chars: usize,
}

impl Matched {
    fn is_empty(&self) -> bool {
        self.needles == 0
    }
}

fn matched(words: &[String], needles: &[String]) -> Matched {
    let mut found = Matched::default();
    for needle in needles {
        let needle = normalize(needle);
        if needle.is_empty() {
            continue;
        }
        if words.iter().any(|word| needle_matches(&needle, word)) {
            found.needles += 1;
            found.chars += needle.chars().count();
        }
    }
    found
}

/// Whether one word of a request carries this needle.
///
/// A word is matched whole, or — for a needle at least [`STEM_MIN_CHARS`]
/// long — as the stem it starts with. Nothing else: a needle is never looked
/// for inside a word.
fn needle_matches(needle: &str, word: &str) -> bool {
    if word == needle {
        return true;
    }
    needle.chars().count() >= STEM_MIN_CHARS && word.starts_with(needle)
}

/// The words of a request, lowercased, in the spelling Russian keyboards
/// disagree about (`ё` written as `е`).
fn words(prompt: &str) -> Vec<String> {
    prompt
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(normalize)
        .collect()
}

fn normalize(text: &str) -> String {
    text.to_lowercase().replace('ё', "е")
}

/// The script a request is written in, when its own letters say so.
///
/// Counted rather than guessed from a locale: a request carries its letters,
/// and "is this mostly Cyrillic" is a question the text answers. A request
/// with no letters at all, or one balanced between scripts, claims nothing
/// and is not refused for language.
fn script_of(prompt: &str) -> Option<KitScript> {
    let mut cyrillic = 0usize;
    let mut other = 0usize;
    for c in prompt.chars().filter(|c| c.is_alphabetic()) {
        if is_cyrillic(c) {
            cyrillic += 1;
        } else {
            other += 1;
        }
    }
    match cyrillic.cmp(&other) {
        std::cmp::Ordering::Greater => Some(KitScript::Cyrillic),
        std::cmp::Ordering::Less => Some(KitScript::Latin),
        std::cmp::Ordering::Equal => None,
    }
}

fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{052F}').contains(&c)
}

/// The shape a request asks for, when it asks for one.
///
/// A named phone wins over everything else: a phone screen is a physical
/// constraint, while "site" or "app" is a delivery channel, so "мобильная
/// версия сайта" is a request for a phone screen and refusing it costs less
/// than placing a desktop master over it. With no word to go on, an explicit
/// `375x812` in the request is read as the width it is.
fn shape_intent(prompt: &str, words: &[String]) -> Option<KitFormFactor> {
    if names_any(words, MOBILE_WORDS) {
        return Some(KitFormFactor::Mobile);
    }
    if names_any(words, DESKTOP_WORDS) {
        return Some(KitFormFactor::Desktop);
    }
    measured_width(prompt).and_then(KitFormFactor::of_width)
}

fn names_any(words: &[String], needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| words.iter().any(|word| needle_matches(needle, word)))
}

/// The first `1024x768`-shaped pair in a request, read as an artboard width.
fn measured_width(prompt: &str) -> Option<f64> {
    let chars: Vec<char> = prompt.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if !(3..=4).contains(&(i - start)) {
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        if j >= chars.len() || !matches!(chars[j], 'x' | 'X' | '×' | 'х' | 'Х') {
            continue;
        }
        j += 1;
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        let height = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if (3..=4).contains(&(j - height)) {
            let width: String = chars[start..i].iter().collect();
            return width.parse().ok();
        }
    }
    None
}

/// Whether the request asks for a shape the recipe is not.
///
/// Only a contradiction refuses: a request that names no shape fits any
/// recipe, and a recipe that declares no shape fits any request.
fn shape_contradicted(asked: Option<KitFormFactor>, recipe: Option<KitFormFactor>) -> bool {
    matches!((asked, recipe), (Some(a), Some(b)) if a != b)
}

/// Whether the request is written in a script the recipe's copy is not.
fn script_contradicted(asked: Option<KitScript>, copy: Option<KitScript>) -> bool {
    matches!((asked, copy), (Some(a), Some(b)) if a != b)
}

#[cfg(test)]
#[path = "design_recipe_match_tests.rs"]
mod tests;
