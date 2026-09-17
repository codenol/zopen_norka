# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

> **The TypeScript OpenPencil has been retired.** `apps/web`, `apps/desktop`, `apps/cli`, and the `pen-*` packages are gone. The product is **Rust** (`crates/`) + a **wasm-backed web SDK** (`packages/op-web-sdk*`). See git history (last TS tag `v0.7.5`) for the retired code.

## The product is the generation. Everything else is in service of it.

Written down by the operator on 2026-09-17, and it governs how work is chosen and
ordered here:

> У этого продукта основная задача — это генерация через ИИ. Всё. Если ИИ не
> генерирует из текущих компонентов по текущим правилам, этот продукт в целом не
> нужен.

- **The one job.** A person types what they want and the canvas ends up holding
  that screen, assembled from the components the kit actually has, following the
  rules the product actually ships. Nothing else this repository does matters if
  that does not happen.
- **"Из текущих компонентов по текущим правилам" is the acceptance test**, not
  a preference: a turn that invents components the kit does not have, or ignores
  the rules it was given, has failed even if the canvas is full.
- **Order of work.** Generation first; deploys, packaging, tooling and the rest
  follow it. A fix that makes a deploy faster while a prompt still draws nothing
  is the wrong fix to be making.
- **The shape generation takes** (operator, 2026-09-17, issue #249): models are
  configured **once in Settings** — not chosen per turn in the chat — and one of
  them builds while a second checks the first. The loop is:

  1. take the request;
  2. open the rules;
  3. follow the rules;
  4. find the right recipes — and when no recipe fits, the right **components**
     for the job;
  5. lay the mockup out on the canvas from the recipe and/or those components;
  6. start the checker;
  7. the checker looks and presses — by eye and by hand — that it is all correct,
     all by the rules and the recipes, and that there is nothing extra;
  8. anything wrong comes back as instructions, and the turn goes round again.

  Today only step 6's deterministic half exists (a self-check over the generated
  nodes and the layout rules); there is no second model and no role assignment.
- **Every task is read through this.** Before picking up a task, ask what it does
  for the generation path — and if the answer is "nothing", it waits.

For full guidance see **`CLAUDE.md`** (this directory). Authoritative Rust architecture lives in **`crates/CLAUDE.md`**; remaining packages in **`packages/CLAUDE.md`**.

## Commands

Tooling is **Cargo** (Rust — the product). The root has **no `package.json`**; the JS/**Bun** tooling for the web SDK lives under `packages/` — run SDK/JS scripts from there.

- **Web dev server (Rust):** `bash scripts/start-web-rust.sh`
- **Build (Rust):** `cargo build --workspace --release`
- **Tests (Rust):** `cargo test --workspace`; single crate: `cargo test -p <crate>`
- **Type check:** `cargo check --workspace`; wasm: `cargo check --target wasm32-unknown-unknown -p op-host-web --no-default-features --features web`
- **Lint / format (Rust):** `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all`
- **Lint / format (TS SDK):** from `packages/`: `bun run lint` (oxlint) / `bun run format` (oxfmt)
- **Desktop app:** `cargo build -p op-host-desktop` → binary `openpencil-desktop`
- **CLI:** `cargo build -p op-cli` → binary `op`
- **Iconify catalog (Rust assets):** from `packages/`: `bun run generate-iconify-catalog`

## Conventions

- Single files ≤ **800 lines**; one component/widget per file.
- `.rs` snake_case, `.ts`/`.tsx` kebab-case; source comments in English.
- Conventional Commits: `<type>(<scope>): <subject>` — scopes: `editor`, `canvas`, `panels`, `ai`, `codegen`, `variables`, `figma`, `mcp`, `desktop`, `web`, `renderer`, `sdk`, `cli`, `agent`, `i18n`.

## Releases: changelog, version, build stamp

Every finished piece of work updates the release records in the same change —
a feature that ships without them is an unfinished feature.

- **CHANGELOG.md** — add the entry under `## [Unreleased]`, Keep a Changelog
  sections (`Added` / `Changed` / `Fixed` / `Removed`), one line per
  user-visible change. Move the block under the new version heading when you
  bump.
- **Version** — bump `[workspace.package].version` in the root `Cargo.toml`,
  then run `scripts/sync-version.sh`. That script needs the JS workspace
  installed; when it cannot run, do its work by hand: `cargo update --workspace
  --offline` for `Cargo.lock` and the same version string in
  `packages/*/package.json`, `packages/package.json` and `packages/bun.lock`.
- **Build stamp** — nothing to do by hand. `crates/op-editor-ui/build.rs`
  stamps the version and the build time into the top bar, next to
  "Agents & MCP", and colours it by freshness: green under three minutes,
  amber to six (blinking every three seconds), red beyond that (blinking every
  second). Read that stamp before debugging "my change does nothing" — the kit
  manifest is `include_str!`-embedded, so config edits need a rebuild, and a
  browser tab can hold an older bundle.

## Disk: a new debug artifact retires the old one

Cargo never deletes what it supersedes. Every edit to a crate's source or to its
dependencies lands new hashed artifacts in `target/debug/deps`, and every debug
build opens another incremental session in `target/debug/incremental`; both are
kept forever, so the directory only grows. Measured in this checkout on
2026-09-14: `target/` was **95 GB** — 90 GB of it `target/debug` (`deps` 76 GB,
`incremental` 18 GB) — against **5.7 GB free** on the data volume. A full disk
does not slow a build down, it stops it.

- When a debug build leaves a new artifact, the one it replaced goes in the same
  step: `rm -rf target/debug/incremental` (a rebuild cache, safe to drop) or
  `cargo clean --profile dev` (all of it, then rebuild).
- Read `df -h /System/Volumes/Data` **before** a workspace-wide build, not after
  it fails. Under ~20 GB free, clean first.
- `target/debug/deps` is the bulk and cannot be pruned selectively: only cargo's
  fingerprint knows which hashed artifact is stale, and it offers no delete. The
  real choices are `cargo clean --profile dev` plus a rebuild, or accepting the
  growth knowingly.
- `cargo check` and `cargo test` write there too. A workspace check run "just to
  look at something" counts, and a cancelled one leaves its partial artifacts
  behind — which is how the numbers above were produced.
- Stray files agents leave in `target/` (`Cargo.lock.after-add`, `target/doc`,
  `target/issue-bodies`) are not build output; delete them when you see them.

## Findings become issues, immediately

Anything discovered while working — a defect, a design flaw, a security
asymmetry, a piece of debt, an idea worth keeping — is filed as a GitHub issue
**in the same turn it is found**, on `codenol/zopen_norka`. Not in a report, not
in a commit message, not in a comment on another issue, and not "later".

The reason is not bookkeeping. Findings in prose get lost the moment the
conversation moves on, and this project has already paid for that twice: two
silent failures (edits above the sync ceiling never reaching the daemon, and
writes that reported success while saving a stale document) were each noticed,
described, and then forgotten for days because nothing tracked them.

- One issue per finding, titled so it can be understood without reading the
  conversation.
- Say how it was found and what is known, enough that someone else could pick
  it up cold. Note what is *not* known as plainly as what is.
- Note what it blocks or relates to, and link the tracking issue from the PR
  that touches the same area.
- Close it only when it is actually done — a workaround is a comment on the
  issue, not a close.
