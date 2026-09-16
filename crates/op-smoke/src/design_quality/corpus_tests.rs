//! The committed corpus is an input a reader has to be able to trust: these
//! tests pin its shape, its provenance and the parts of it the scorecard
//! depends on.

use super::corpus::{Corpus, GateState, VerdictRule};

#[test]
fn the_committed_corpus_loads_and_validates() {
    let corpus = Corpus::load(super::DEFAULT_CORPUS).expect("load");
    assert_eq!(corpus.corpus, "design-quality");
    assert_eq!(corpus.prompts.len(), 8, "the 8-prompt corpus");
    assert_eq!(corpus.screen_min_nodes, 5);
    assert_eq!(corpus.routes.turn, "/api/ai/standard");
    assert_eq!(corpus.routes.document, "/api/mcp/document");
    assert_eq!(corpus.routes.new_document, "/api/file/new");
    // The browser's own body, verbatim — the two hand passes posted exactly it.
    assert_eq!(corpus.body.provider, "codex-cli");
    assert_eq!(corpus.body.builtin_provider_id, "builtin-1");
    assert_eq!(corpus.body.max_output_tokens, 16384);
    assert_eq!(corpus.body.thinking, "adaptive");
    assert_eq!(corpus.body.effort, "low");
    assert_eq!(corpus.body.agent_team_size, 1);
    assert!(corpus.body.credential.is_none());
}

#[test]
fn prompts_are_verbatim_and_every_prompt_explains_itself() {
    let corpus = Corpus::load(super::DEFAULT_CORPUS).expect("load");
    let by_id = |id: &str| {
        corpus
            .prompts
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    // Two spot checks against `.openpencil-tmp/gq2/manifest.json`; the numbers
    // this tool prints are only comparable if the wording did not drift.
    assert_eq!(
        by_id("01-login-ru").prompt,
        "Нарисуй экран логина: карточка по центру, поля email и password, кнопка Sign in"
    );
    assert_eq!(by_id("06-vague-ru").prompt, "сделай красиво");
    assert_eq!(
        by_id("08-pricing-en").prompt,
        "Create a pricing page with three plan cards, monthly and yearly toggle, and a FAQ section"
    );
    for prompt in &corpus.prompts {
        assert!(!prompt.intent.trim().is_empty(), "{} intent", prompt.id);
        assert!(
            !prompt.expectation.note.trim().is_empty(),
            "{} expectation note",
            prompt.id
        );
        assert!(
            prompt.baseline.pass == "gq" && prompt.last.pass == "gq2",
            "{} record provenance",
            prompt.id
        );
    }
    // Indices are 1..=8, in order — they number the scorecard rows.
    let indices: Vec<u32> = corpus.prompts.iter().map(|p| p.index).collect();
    assert_eq!(indices, (1..=8).collect::<Vec<u32>>());
}

#[test]
fn the_gate_states_match_what_the_last_pass_measured() {
    let corpus = Corpus::load(super::DEFAULT_CORPUS).expect("load");
    let state = |id: &str| {
        corpus
            .prompts
            .iter()
            .find(|p| p.id == id)
            .expect("prompt")
            .expectation
            .state
    };
    // Required: the last pass delivered a screen (1, 2, 3, 5, 7).
    for id in [
        "01-login-ru",
        "02-servers-ru",
        "03-mobile-ru",
        "05-dark-theme-ru",
        "07-two-screens-ru",
    ] {
        assert_eq!(state(id), GateState::Required, "{id}");
    }
    // Known-broken: the last pass delivered nothing (4, 6, 8) — the recorded
    // #202 regression and the no-op turn.
    for id in ["04-settings-en", "06-vague-ru", "08-pricing-en"] {
        assert_eq!(state(id), GateState::KnownBroken, "{id}");
    }
    // The one prompt whose verdict is a judgement call says so instead of
    // guessing: prompt 5's dark theme cannot be read out of the document.
    let dark = corpus
        .prompts
        .iter()
        .find(|p| p.id == "05-dark-theme-ru")
        .expect("prompt 5");
    assert!(!dark.unmeasurable.is_empty());
    assert!(dark.unmeasurable[0].contains("DARK"));
    assert_eq!(dark.verdict_rule, VerdictRule::Screen);
    // Prompt 6 asks for nothing concrete, so it is excluded from the
    // "asked for a screen" count and judged on movement.
    let vague = corpus
        .prompts
        .iter()
        .find(|p| p.id == "06-vague-ru")
        .expect("prompt 6");
    assert!(!vague.screen_requested);
    assert_eq!(vague.verdict_rule, VerdictRule::DocumentMoved);
}

#[test]
fn only_selects_by_id_or_prefix() {
    let corpus = Corpus::load(super::DEFAULT_CORPUS).expect("load");
    let one = corpus.selected(&["04".to_string()]).expect("prefix match");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].id, "04-settings-en");
    let two = corpus
        .selected(&["01-login-ru".to_string(), "08".to_string()])
        .expect("mixed");
    assert_eq!(two.len(), 2);
    assert!(corpus.selected(&["99".to_string()]).is_err());
    assert_eq!(corpus.selected(&[]).expect("all").len(), 8);
}

#[test]
fn a_corpus_with_a_duplicate_id_or_no_intent_is_rejected() {
    let corpus = Corpus::load(super::DEFAULT_CORPUS).expect("load");
    let mut duplicated = corpus.clone();
    duplicated.prompts[1].id = duplicated.prompts[0].id.clone();
    let error = duplicated.validate("test.json").expect_err("duplicate id");
    assert!(error.contains("duplicate prompt id"), "{error}");

    let mut unexplained = corpus.clone();
    unexplained.prompts[0].intent = "  ".to_string();
    let error = unexplained.validate("test.json").expect_err("no intent");
    assert!(error.contains("no `intent`"), "{error}");

    let mut unexplained_gate = corpus.clone();
    unexplained_gate.prompts[0].expectation.note = String::new();
    let error = unexplained_gate
        .validate("test.json")
        .expect_err("no expectation note");
    assert!(error.contains("readable"), "{error}");
}

#[test]
fn the_turn_body_mixes_cases_exactly_as_the_browser_does() {
    let corpus = Corpus::load(super::DEFAULT_CORPUS).expect("load");
    let prompt = corpus.prompts[0].clone();
    let body = corpus.body_for(&prompt, &super::corpus::BodyOverrides::default());
    assert_eq!(body.user, prompt.prompt);
    let json = serde_json::to_value(&body).expect("serialize");
    let mut keys: Vec<&str> = json
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    // `serde_json`'s map is sorted, so this asserts the key SET: the four
    // camelCase/snake_case spellings the browser really posts are all here.
    keys.sort_unstable();
    let mut expected = vec![
        "provider",
        "builtinProviderId",
        "model",
        "credential",
        "skills",
        "user",
        "max_output_tokens",
        "thinking",
        "effort",
        "agent_team_size",
        "history",
        "attachments",
        "selectedIds",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected);
}
