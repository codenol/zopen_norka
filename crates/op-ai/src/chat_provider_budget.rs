//! The output budget for one turn — split out of `chat_provider.rs` to keep
//! that file under the workspace's 800-line ceiling, and re-exported from it
//! so every existing import path is unchanged.

/// Output budget for a turn whose reasoning shares it with the answer.
///
/// A reasoning model spends `max_output_tokens` on its hidden thinking before
/// it writes one visible character, and the two are comparable in size. On
/// DeepSeek V4 the wire default is thinking ON at effort high, so a budget
/// sized for the answer alone is a budget the model exhausts inside `<think>`
/// and returns nothing at all.
///
/// Measured against the configured model (lived through issue #179), same
/// prompt, 4000 tokens: 19 s, 15 582 characters of thinking streamed,
/// **0 characters of answer**, then `{"done":true}` — the turn looked
/// finished and the canvas stayed empty. 16 384 leaves the answer room beside
/// the reasoning, and matches the budget the orchestrator's own LLM client
/// already uses (`op-host-services::chat_provider_llm`) after 8192 truncated a
/// rich plan mid-JSON.
///
/// Use it wherever a caller has no reason to pick a smaller number; callers
/// that genuinely know their reply is short (intent classification, a verdict
/// JSON) may keep their own.
pub const DEFAULT_TURN_MAX_OUTPUT_TOKENS: u32 = 16_384;
