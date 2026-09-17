//! Single-shot subprocess bridge for Claude Code (fallback), Codex,
//! Antigravity, Grok Build, and DeepSeek Harness. OpenCode uses its HTTP
//! transport; Copilot uses its SDK transport.
//!
//! Codex runs `codex exec [--ephemeral] --json ... -` with an allowlisted
//! environment and the prompt on stdin. Antigravity and Grok use isolated, fail-closed turns;
//! DSH runs `dsh --profile headless <prompt>` with its narrow child
//! environment. Their exact argv and parsers live in the corresponding
//! `chat_subprocess_*` siblings.
//!
//! Multi-turn context rides an in-band history digest. Codex parse misses are
//! skipped; custom binaries fall back to plain text. Every child has stderr
//! drained, exit status interpreted, and its full process tree terminated on
//! cancellation or deadline. Binary discovery lives in [`chat_spawn`].

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use op_ai::chat_provider::{
    AttachmentTransport, ChatDelta, ChatProvider, ChatRequest, CliName, EffortLevel, StopReason,
};
use op_process_io::LineStreamChild;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use crate::chat_runtime::{prompt_with_system_prompt, shared_runtime, BlockingRecvIter};
use crate::chat_spawn::{build_command, find_binary, runtime_path_for_binary};
use crate::chat_subprocess_lifecycle::{child_env_for_cli, wait_for_terminal_exit};
use crate::chat_subprocess_quirks as quirks;
use crate::chat_subprocess_quirks::codex_reasoning_effort;
use crate::chat_subprocess_safety as safety;

pub use crate::chat_subprocess_parse::parse_line;

/// How the user's prompt reaches the CLI. Claude Code's `--print`
/// mode requires the prompt as a positional argv after `--` and
/// closes stdin immediately. Codex (`-` prompt arg) reads the
/// message off piped stdin. Generic `with_binary` callers can
/// pick either via [`SubprocessProvider::with_binary_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// Append `-- <prompt>` to argv; stdin gets closed (no input).
    PositionalArg,
    /// Argv is passed verbatim; user_message is written to stdin
    /// followed by EOF.
    Stdin,
    /// Append `<flag> <prompt>` to argv; stdin gets closed.
    FlagArg(&'static str),
    /// Append `<flag> <private-file>`; stdin gets closed.
    PromptFile(&'static str),
    /// Append `<prompt>` itself as the final argv element — the CLI's
    /// interface takes the prompt as a bare trailing argument
    /// (`dsh --profile headless "<prompt>"`). Stdin gets closed so the
    /// one-shot CLI cannot fall back into interactive mode. The prompt
    /// stays visible in argv while the child is alive (same documented
    /// tradeoff as Antigravity's `-p`).
    BareArg,
}

/// `ChatProvider` impl that bridges to a CLI binary via stdio.
/// Construct via [`SubprocessProvider::for_cli`] or
/// [`SubprocessProvider::with_binary`] for a custom binary path.
pub struct SubprocessProvider {
    binary: String,
    args: Vec<String>,
    label: String,
    prompt_mode: PromptMode,
    /// Argv flag that selects a model for this CLI (`--model` for
    /// Codex and Antigravity, `-m` for Grok Build). `None` = the transport has no model
    /// selector; `ChatRequest::model` is ignored and the CLI keeps
    /// its own default.
    model_flag: Option<&'static str>,
    /// True when the CLI accepts Codex's native reasoning knob
    /// (`--config model_reasoning_effort=<level>`). When set, the
    /// thinking + effort knobs ride that flag instead of the in-band
    /// directive line (TS parity: `codex-client.ts` never prepends a
    /// prose directive).
    native_effort_config: bool,
    /// Trailing argv appended after the per-turn flags — Codex's `-`
    /// stdin marker. TS keeps the same flag-then-marker order.
    tail_args: Vec<String>,
    /// Which known CLI this provider bridges (drives per-CLI env
    /// filtering, line parsing, stderr capture, and timeout quirks).
    /// `None` for custom `with_binary` providers — generic behavior.
    cli: Option<CliName>,
    turn_purpose: safety::TurnPurpose,
}

impl SubprocessProvider {
    /// Build a subprocess provider for a known [`CliName`]. Each CLI
    /// has its own argv template + prompt-routing mode (see module
    /// docs). Returns `None` for OpenCode (HTTP-server transport
    /// in `chat_http_server.rs`) and Copilot (official SDK transport
    /// in `chat_copilot.rs`) — neither has a stdio wire.
    pub fn for_cli(cli: CliName) -> Option<Self> {
        Self::for_cli_with_purpose(cli, safety::TurnPurpose::CanvasAgent)
    }

    /// Build a tool-free provider for orchestrator, subtask, and codegen turns.
    pub fn for_cli_generation(cli: CliName) -> Option<Self> {
        Self::for_cli_with_purpose(cli, safety::TurnPurpose::Generation)
    }

    fn for_cli_with_purpose(cli: CliName, turn_purpose: safety::TurnPurpose) -> Option<Self> {
        // Per-CLI model selector (third tuple slot): Codex takes
        // `--model <id>` — matching the TS reference. Claude Code's
        // model rides the SDK adapter (`chat_claude.rs`), so
        // its subprocess template carries no flag here.
        type Template = (Vec<String>, PromptMode, Option<&'static str>, Vec<String>);
        let (mut args, prompt_mode, model_flag, tail_args): Template = match cli {
            CliName::ClaudeCode => (
                vec![
                    "--print".into(),
                    "--verbose".into(),
                    "--output-format".into(),
                    "stream-json".into(),
                ],
                PromptMode::PositionalArg,
                None,
                Vec::new(),
            ),
            // TS `codex-client.ts` argv plus capability-gated non-persistence:
            // `exec [--ephemeral] --json
            // --skip-git-repo-check --sandbox read-only [--model]
            // [--config model_reasoning_effort=…] -` with the prompt
            // piped via stdin (the `-` marker). `--output-last-message`
            // is intentionally not ported — Rust streams the agent
            // message instead of re-reading it from a temp file.
            CliName::Codex => (
                vec![
                    "exec".into(),
                    "--json".into(),
                    "--skip-git-repo-check".into(),
                    "--sandbox".into(),
                    "read-only".into(),
                ],
                PromptMode::Stdin,
                Some("--model"),
                vec!["-".into()],
            ),
            CliName::Antigravity => (
                safety::antigravity_args(turn_purpose),
                PromptMode::FlagArg("-p"),
                Some("--model"),
                Vec::new(),
            ),
            CliName::GrokBuild => (
                safety::grok_args(turn_purpose),
                PromptMode::PromptFile("--prompt-file"),
                Some("-m"),
                Vec::new(),
            ),
            // DeepSeek Harness: one-shot subprocess, prompt as a bare
            // trailing argv element (its only verified interface), no
            // model selector. Args / parser / timeout live in the
            // `chat_subprocess_dsh` sibling (this file sits at the
            // 800-line cap).
            CliName::Dsh => (
                crate::chat_subprocess_dsh::dsh_args(),
                PromptMode::BareArg,
                None,
                Vec::new(),
            ),
            // Copilot's routed transport is the official SDK
            // (`chat_copilot.rs`); the old `gh-copilot suggest`
            // template was a stale dead end. OpenCode chats over its
            // local HTTP server (`chat_http_server.rs`).
            CliName::Copilot | CliName::OpenCode => return None,
        };
        let binary = find_binary(cli.default_binary());
        if cli == CliName::Codex {
            quirks::append_codex_ephemeral_arg(std::path::Path::new(&binary), &mut args);
        }
        Some(Self {
            binary,
            args,
            label: cli.label().into(),
            prompt_mode,
            model_flag,
            // Only Codex has the native reasoning-effort config knob.
            native_effort_config: cli == CliName::Codex,
            tail_args,
            cli: Some(cli),
            turn_purpose,
        })
    }

    /// Build a subprocess provider with a user-supplied binary path
    /// and argv (defaults to stdin prompt). Used when the settings
    /// modal needs to point at a non-PATH install.
    pub fn with_binary(
        binary: impl Into<String>,
        args: Vec<String>,
        label: impl Into<String>,
    ) -> Self {
        Self::with_binary_mode(binary, args, label, PromptMode::Stdin)
    }

    /// Build a subprocess provider with an explicit prompt-routing
    /// mode. Required for CLIs like Claude Code that want the prompt
    /// as a positional argv rather than via stdin.
    pub fn with_binary_mode(
        binary: impl Into<String>,
        args: Vec<String>,
        label: impl Into<String>,
        prompt_mode: PromptMode,
    ) -> Self {
        Self {
            binary: binary.into(),
            args,
            label: label.into(),
            prompt_mode,
            // Custom binaries carry no known model / effort flags;
            // the request's model is ignored (CLI default applies).
            model_flag: None,
            native_effort_config: false,
            tail_args: Vec::new(),
            cli: None,
            turn_purpose: safety::TurnPurpose::Generation,
        }
    }

    /// Point a known-CLI provider at a stand-in binary so the exit /
    /// stderr / stdout handling can be exercised without the real CLI.
    /// Its only callers are the unix-gated exit tests (the stand-ins are
    /// `/bin/sh` scripts), so it carries the same gate to stay live-code
    /// on Windows.
    #[cfg(all(test, unix))]
    pub(crate) fn with_test_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }

    /// Argv for one turn: the configured base args plus the model
    /// selector and (Codex) the native reasoning-effort config when
    /// the request carries them, then the trailing prompt marker
    /// (`-` / `-p ' '`). Empty / blank model ids emit no flag at all —
    /// the CLI keeps its own default.
    fn turn_args(&self, request: &ChatRequest) -> Vec<String> {
        let mut args = self.args.clone();
        if let (Some(flag), Some(model)) = (self.model_flag, request.model_id()) {
            let provider_default = model == "default"
                && matches!(self.cli, Some(CliName::Antigravity | CliName::GrokBuild));
            if !provider_default {
                args.push(flag.into());
                args.push(model.into());
            }
        }
        if self.native_effort_config {
            if let Some(level) = codex_reasoning_effort(request.thinking, request.effort) {
                args.push("--config".into());
                args.push(format!("model_reasoning_effort={level}"));
            }
        }
        args.extend(self.tail_args.iter().cloned());
        args
    }
}

// The turn path — spawn, the stream read loop, and the teardown that
// interprets the exit status — lives in the child module below: the
// 800-line cap's spine + sibling shape. A child rather than a sibling of
// `chat_subprocess` because that path reads this provider's private
// fields and calls its private `turn_args`.
#[path = "chat_subprocess_send.rs"]
mod send;

#[cfg(test)]
#[path = "chat_subprocess_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "chat_subprocess_exit_tests.rs"]
mod exit_tests;
