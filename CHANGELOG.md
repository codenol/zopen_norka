# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Each entry names the version that ships it; the app's top bar shows the same
version next to its build time, so a running build can be matched to a section
here at a glance.

## [Unreleased]

### Added

- **File browser screen (`/files`).** A screen of its own, not a panel: title,
  search field, "New file" action and a grid of document cards with names and
  edit times, plus honest empty, loading and error states. Reached by address;
  the editor stays open behind it. (The list is fetched but not yet rendered —
  see the plan's progress log.)
- **Server-side documents.** The daemon now stores documents itself — a
  directory (`$NORKA_DOCUMENTS_DIR`, else `~/.norka/files`), an `index.json`
  of names and timestamps, and short opaque keys — exposed as
  `/api/files` (list, create), `/api/files/<key>/open`, `/save`, `/rename`
  and `DELETE`. Keys are validated before they touch a path: `../` and
  malformed keys are refused, so a pasted URL is never a filesystem request.
  Opening a stored document binds it to its key, and Save writes back through
  the file route instead of pushing the whole document from the browser.
- **Address-bar routing (first step).** The editor's address now names the
  document, the page and the selected node — `/f/<key>/<slug>?node=<id>` — with
  the vocabulary in `op_editor_core::route` so the browser and the desktop app
  can share it. Selecting a node or switching a page **replaces** the address
  (history does not grow with every click); opening a link applies it, waits
  for the document to arrive, and reveals the node. The tab title is the file
  name. `/files` and `/f/<key>` are served the editor page by the daemon
  instead of a 404, which is what makes a shared link work at all.

## [0.8.6] — 2026-09-11

### Added

- **Component rules system.** The rules panel replaces the markdown brief: one
  document per kit component, one for the session's AI working agreement, and
  one per shipped recipe. Rules reach every prompt path (chat, planning,
  sub-agents, MCP) as `WORKING AGREEMENT`, `GLOBAL RULES`, `RECIPE RULES` and
  `COMPONENT RULES`, and a document that has never saved an instruction still
  gets the shipped default.
- **Recipes as first-class kit citizens.** A recipe is a component: it is a
  master in the library, listed in its own rail section, and clicking a row
  opens the master's page the way a component row does. `list_recipes` and
  `use_recipe` expose them to the AI.
- **Deterministic recipe selection.** `KitRecipe::matches` declares the words
  that mark a request as a recipe's job; the product picks and places the
  recipe before the model runs, so the turn becomes an edit of a real screen
  instead of a request to invent one.
- **Optional recipe blocks.** `KitRecipe::optional` names blocks a request may
  dismiss ("the filter and pagination are not needed"); the product hides them
  before the model runs, matched by subject word plus negation nearby.
- **Chat transcript persistence.** The browser mirrors the transcript into
  `localStorage` and restores it on mount, so a reload no longer empties the
  panel.
- **Build stamp in the top bar** — version and build time, coloured by
  freshness: green under three minutes, amber to six (blinking every three
  seconds), red beyond that (blinking every second).
- **Optional prompt dumping** behind `OPENPENCIL_DUMP_PROMPTS`, for verifying
  what the model actually receives.

### Changed

- The markdown `design.md` brief is gone from the product: it no longer feeds
  prompts, no longer appears in the chat chip, and its import/export drivers
  are retired.
- Rules no longer compete for the skill budget; their token cost is added on
  top, so an always-on rules block cannot evict skills like `mobile-app`.
- A reference image outranks the automatic recipe: a turn that points at a
  picture skips recipe placement, drops recipe rules from its prompts, and goes
  to the build path instead of being classified as conversation.
- Web repaint no longer depends on `requestAnimationFrame` alone; a timer
  fallback paints the frame a throttled tab would drop, so an AI turn's result
  shows up without a reload.

### Fixed

- A generated screen is no longer invisible until reload when the tab is
  backgrounded.
- Recipe rows no longer open the recipe's rules; they open the recipe.
- The build stamp no longer reports every build as stale (milliseconds were
  compared against seconds) and no longer quantizes its blink phase to whole
  seconds.

[Unreleased]: https://github.com/codenol/zopen_norka/compare/v0.8.6...HEAD
[0.8.6]: https://github.com/codenol/zopen_norka/releases/tag/v0.8.6
