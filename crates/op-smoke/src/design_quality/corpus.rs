//! The corpus file: what to ask, and what the previous runs measured.
//!
//! `crates/op-smoke/corpus/design-quality.json` is the reviewable record of the
//! 8-prompt corpus both hand measurements of 2026-09-16 used. It is data, not
//! code, and it carries three things a scorecard needs and a prompt list alone
//! cannot supply:
//!
//! - the prompts **verbatim** (from `.openpencil-tmp/gq2/manifest.json`), so a
//!   run today is comparable with a run then;
//! - one line of **intent** per prompt, so a reader knows what the run was
//!   asking for without inferring it from the Russian or English wording;
//! - the **recorded expectation** — what the baseline pass (`gq/`) and the last
//!   pass (`gq2/`) measured, which is what the gate compares against.
//!
//! A prompt whose verdict is a judgement call says so in `unmeasurable`; the
//! scorecard then prints `n/a` with that reason instead of a guess. Prompt 5's
//! dark theme and prompt 6's "сделай красиво" are the two the hand passes hit.

use serde::{Deserialize, Serialize};

/// The whole corpus file.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Corpus {
    pub(crate) corpus: String,
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) description: String,
    /// What counts as "a screen is drawn": the fresh document starts as a
    /// single 1-node starter frame, so this is the weakest honest threshold for
    /// "something was drawn".
    pub(crate) screen_min_nodes: u32,
    pub(crate) routes: Routes,
    pub(crate) body: TurnBody,
    pub(crate) prompts: Vec<Prompt>,
}

/// The daemon's route paths, from the corpus so a route change is a data edit.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Routes {
    pub(crate) new_document: String,
    pub(crate) document: String,
    pub(crate) turn: String,
}

/// The browser's own `/api/ai/standard` body.
///
/// Field names are spelled out one by one because the real body mixes cases
/// (`builtinProviderId` / `selectedIds` are camelCase, `max_output_tokens` /
/// `agent_team_size` are snake_case) — a blanket rename rule would post a body
/// the daemon was never measured against.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct TurnBody {
    pub(crate) provider: String,
    #[serde(rename = "builtinProviderId")]
    pub(crate) builtin_provider_id: String,
    pub(crate) model: String,
    pub(crate) credential: Option<String>,
    pub(crate) skills: Vec<String>,
    pub(crate) user: String,
    #[serde(rename = "max_output_tokens")]
    pub(crate) max_output_tokens: u32,
    pub(crate) thinking: String,
    pub(crate) effort: String,
    #[serde(rename = "agent_team_size")]
    pub(crate) agent_team_size: u32,
    pub(crate) history: Vec<serde_json::Value>,
    pub(crate) attachments: Vec<serde_json::Value>,
    #[serde(rename = "selectedIds")]
    pub(crate) selected_ids: Vec<String>,
}

/// One corpus prompt plus its recorded history and expectation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Prompt {
    /// Stable id — used for artifact file names and every report line.
    pub(crate) id: String,
    pub(crate) index: u32,
    pub(crate) language: String,
    /// One line saying what the run was asking for.
    pub(crate) intent: String,
    /// Verbatim from the manifest the two hand passes used.
    pub(crate) prompt: String,
    /// Whether the prompt asks for a screen at all (prompt 6 asks for nothing
    /// concrete, so it is excluded from the "drew the requested screen" count).
    pub(crate) screen_requested: bool,
    pub(crate) verdict_rule: VerdictRule,
    /// Judgement calls this measurement cannot make; printed as `n/a`.
    #[serde(default)]
    pub(crate) unmeasurable: Vec<String>,
    /// The first measured pass (`gq/`).
    pub(crate) baseline: PassRecord,
    /// The most recent measured pass (`gq2/`) — what a `known_broken`
    /// expectation compares against.
    pub(crate) last: PassRecord,
    pub(crate) expectation: Expectation,
}

/// How the scorecard judges this prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerdictRule {
    /// A screen was asked for and drawn.
    Screen,
    /// Nothing concrete was asked for; the only measurable claim is that the
    /// turn changed the document at all.
    DocumentMoved,
}

/// What one pass measured for one prompt.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PassRecord {
    pub(crate) pass: String,
    /// Nodes under `pages[0]` after the turn.
    pub(crate) page0_nodes: u32,
    pub(crate) screen: bool,
    pub(crate) audit_issues: u32,
    pub(crate) verdict: String,
    #[serde(default)]
    pub(crate) seconds: Option<f64>,
    #[serde(default)]
    pub(crate) thinking_chars: Option<usize>,
    #[serde(default)]
    pub(crate) delta_chars: Option<usize>,
    #[serde(default)]
    pub(crate) terminal: Option<String>,
    #[serde(default)]
    pub(crate) frames: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) truncated: Option<String>,
}

/// The gate's recorded expectation for one prompt.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Expectation {
    pub(crate) state: GateState,
    /// `required` only: a turn that reports `done` but does not write to the
    /// document is a failure ("reported done, moved nothing").
    #[serde(default)]
    pub(crate) require_version_move: bool,
    /// `known_broken` only: the last pass's own `page0Nodes`. Reaching it is not
    /// a regression; falling below it is.
    #[serde(default)]
    pub(crate) floor_page0_nodes: u32,
    /// Why this expectation, in the corpus's own words — printed with a failure.
    pub(crate) note: String,
}

/// Whether the last measurement delivered this prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GateState {
    /// The last pass delivered; anything below `screenMinNodes` is a regression.
    Required,
    /// The last pass already failed; reproducing that is reported as
    /// `known-broken`, not as a regression.
    KnownBroken,
}

impl Corpus {
    /// Reads and validates the corpus at `path`.
    pub(crate) fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("[QUALITY] read corpus {path}: {e}"))?;
        let corpus: Corpus = serde_json::from_str(&text)
            .map_err(|e| format!("[QUALITY] parse corpus {path}: {e}"))?;
        corpus.validate(path)?;
        Ok(corpus)
    }

    /// Rejects a corpus that would make the scorecard lie about itself.
    pub(crate) fn validate(&self, path: &str) -> Result<(), String> {
        if self.prompts.is_empty() {
            return Err(format!("[QUALITY] corpus {path} holds no prompts"));
        }
        if self.screen_min_nodes == 0 {
            return Err(format!(
                "[QUALITY] corpus {path}: screenMinNodes must be > 0 (it is what \"a screen \
                 is drawn\" means)"
            ));
        }
        for prompt in &self.prompts {
            if prompt.prompt.trim().is_empty() {
                return Err(format!(
                    "[QUALITY] corpus {path}: {} has an empty prompt",
                    prompt.id
                ));
            }
            if prompt.intent.trim().is_empty() {
                return Err(format!(
                    "[QUALITY] corpus {path}: {} has no `intent` — a reader must be able to see \
                     what the run was asking for",
                    prompt.id
                ));
            }
            if prompt.expectation.note.trim().is_empty() {
                return Err(format!(
                    "[QUALITY] corpus {path}: {} has no expectation note; the gate has to be \
                     readable",
                    prompt.id
                ));
            }
            let duplicates = self
                .prompts
                .iter()
                .filter(|other| other.id == prompt.id)
                .count();
            if duplicates > 1 {
                return Err(format!(
                    "[QUALITY] corpus {path}: duplicate prompt id {:?}",
                    prompt.id
                ));
            }
        }
        Ok(())
    }

    /// The prompts a run should execute, honouring `--only`.
    ///
    /// `only` matches an id exactly or by prefix (`--only 04` selects
    /// `04-settings-en`).
    pub(crate) fn selected(&self, only: &[String]) -> Result<Vec<Prompt>, String> {
        if only.is_empty() {
            return Ok(self.prompts.clone());
        }
        let mut selected = Vec::new();
        for wanted in only {
            let matches: Vec<&Prompt> = self
                .prompts
                .iter()
                .filter(|p| p.id == *wanted || p.id.starts_with(wanted.as_str()))
                .collect();
            match matches.len() {
                0 => {
                    return Err(format!(
                        "[QUALITY] --only {wanted:?} matches no corpus prompt (ids: {})",
                        self.prompts
                            .iter()
                            .map(|p| p.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
                _ => {
                    for prompt in matches {
                        if !selected.iter().any(|p: &Prompt| p.id == prompt.id) {
                            selected.push(prompt.clone());
                        }
                    }
                }
            }
        }
        Ok(selected)
    }

    /// The `/api/ai/standard` body for one prompt, with the run's overrides.
    pub(crate) fn body_for(&self, prompt: &Prompt, overrides: &BodyOverrides) -> TurnBody {
        let mut body = self.body.clone();
        body.user = prompt.prompt.clone();
        if let Some(provider) = &overrides.provider {
            body.provider = provider.clone();
        }
        if let Some(id) = &overrides.builtin_provider_id {
            body.builtin_provider_id = id.clone();
        }
        if let Some(model) = &overrides.model {
            body.model = model.clone();
        }
        body
    }
}

/// CLI/env overrides for the turn body.
#[derive(Debug, Clone, Default)]
pub(crate) struct BodyOverrides {
    pub(crate) provider: Option<String>,
    pub(crate) builtin_provider_id: Option<String>,
    pub(crate) model: Option<String>,
}
