# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

> **The TypeScript OpenPencil has been retired.** `apps/web`, `apps/desktop`, `apps/cli`, and the `pen-*` packages are gone. The product is **Rust** (`crates/`) + a **wasm-backed web SDK** (`packages/op-web-sdk*`). See git history (last TS tag `v0.7.5`) for the retired code.

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
  `packages/*/package.json`, `packages/package.json`,
  `packages/op-chrome-extension/manifest.json` and `packages/bun.lock`.
- **Build stamp** — nothing to do by hand. `crates/op-editor-ui/build.rs`
  stamps the version and the build time into the top bar, next to
  "Agents & MCP", and colours it by freshness: green under three minutes,
  amber to six (blinking every three seconds), red beyond that (blinking every
  second). Read that stamp before debugging "my change does nothing" — the kit
  manifest is `include_str!`-embedded, so config edits need a rebuild, and a
  browser tab can hold an older bundle.

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
