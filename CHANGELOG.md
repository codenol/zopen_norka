# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Each entry names the version that ships it; the app's top bar shows the same
version next to its build time, so a running build can be matched to a section
here at a glance.

## [Unreleased]

### Added

- **The daemon's document list lives in SQLite.** The accounting behind the
  file list — name, timestamps, size, whether a preview exists — was one
  `index.json` rewritten whole on every change, so a save cost more as the
  folder filled, a crash between the temp file and the rename lost everything
  since the previous good write, and a flat array had nowhere to put the
  relations the next steps ask for. It is now one row per document in
  `documents.db`, beside the `.op` files. From the outside nothing moves: same
  routes, same answers, same order.

  The old files are not touched. `index.json` is read once at the first open —
  its rows land in the database with no owner — and kept, because it is the
  only record of what the folder held before; `last.json` is read the same way,
  so an upgrade still reopens the document that was open. A file that will not
  parse leaves that import unfinished and records why, instead of failing the
  start or throwing the history away.

- **The daemon asks who is calling before it touches a stored document.** A
  request now carries what the answer needs — the deployment mode, the
  document's owner, the caller's verified identity with its roles, and whether
  the owner's access list admits them — and one pure function decides. Two
  questions, in this order: *may this caller see this document at all* (owner,
  or named in the access list), then *does a role they hold grant the write*.
  Local and managed daemons answer "the operator" and behave exactly as
  before; nothing about working on your own machine changed.

  An owner works on their own document whatever roles the hub sends: the file
  is theirs, and a deployment whose hub sends no roles would otherwise be
  read-only for the very people the documents belong to. Someone else's
  document is a different matter — there the roles decide, so a visitor given
  access without an editing role reads, and an unknown role grants nothing.

  The online refusal that fronts these routes (#20) stays for now: it answers
  a different question — *whose files may be addressed at all* — and
  `documents_dir()` is still one flat directory for the process. A test pins
  the refusal so lifting it has to be deliberate.
- **Every role has a colour.** Golden for Admin, violet for UX/UI, sky for
  Software, yellow for Analyst, green for Frontend, grey for Backend, brown for
  QA — taken from the operator's own naming and resolved to values that stay
  apart on the dark chrome, gold and yellow included. Roles need to be
  recognisable before they are useful: the first thing a team asks of them is
  who said or did something, and that has to read without a label. The colours
  land now so that commenting, which reads them, arrives into a system that
  already knows what an author looks like.
- **The draft is offered back (#26).** The next launch asks the daemon once,
  and a bar over the canvas offers the recovered work: "Unsaved work found:
  10m ago" with **Restore** and **Discard**. Restore adopts the draft as the
  open document and pulls it into the tab; Discard drops it. Answering is
  final for the page, whether or not the daemon agrees — a request it rejects
  leaves the draft in its slot, so the offer returns on the next launch
  instead of looping on someone who already decided. The bar stays off the
  screen whenever a modal owns it, so it can never look pressable under a
  scrim.
- **Unsaved work with no home is kept.** A document with no server key — an
  untitled screen — now writes itself into the daemon's draft slot on the same
  autosave schedule, so work that used to exist only in a tab survives, and the
  next launch offers it back.
- **A draft slot for work with no home (server side).** A document with no
  server key and no path now has somewhere to write itself: one draft beside
  the documents, addressed by `/api/recovery` (write, ask about, restore,
  drop). It is deliberately not a document in the store — no key, never
  listed, and dropped the moment it is restored or refused.
- **The saved state is stated, not implied.** The title bar now says "Saved"
  when the document matches its file and "Edited" when it does not; before, a
  saved document showed nothing, which is indistinguishable from a label that
  failed to draw. The marker follows the localised label in all fifteen
  languages.
- **Desktop autosave.** The window saves a stored document on the same
  schedule as the browser (3 s of quiet, floor 15 s) through the existing
  `SaveSession`, and reports its deadline to the event loop — without that, a
  window sitting idle after an edit would never wake to save.
- **A restart returns to your work.** The daemon records which document was
  open and reopens it on startup, instead of handing back the kit. An explicit
  document argument still wins, and a document that has since been deleted
  falls back to a fresh one rather than failing to start.
- **Autosave.** A stored document now reaches disk on its own: after edits
  settle (3 s of quiet, and never more often than every 15 s) the browser
  writes it through a quiet route that skips the preview render. The
  acknowledgement is the one the manual save already uses, so a late reply can
  never mark a different document saved. A document with no server key is left
  alone — where a draft for it should live is a product decision (#16), not
  something a background task should invent.
- **Cmd/Ctrl+S saves in the browser.** It stopped the browser's own dialog and
  then did nothing — the first shortcut everybody tries did not save.

### Added

- **Product roles arrive from the hub (#10, first step).** The hub already sends
  a user's roles; the verifier parsed them and dropped them one function later,
  so nothing downstream could act on a role. They now travel with the resolved
  identity, and `op_editor_core::access` models the seven product roles
  (Admin / UX/UI / Software / Analyst / Frontend / Backend / QA) against four
  rights levels: admin manages everything, UX/UI edits everything except users,
  the five contributor roles read, comment and invite, and a guest with a link
  only reads. An account may hold several roles — rights compose by union — and
  an unknown role from the wire is refused rather than trusted. Nothing enforces
  these yet on the routes: that needs a document owner, which is the next step.
- **Roles decide the stored-document and recovery routes (#10, second step).**
  Who owns a document, and what the caller's roles grant, now reach
  `/api/files*` and `/api/recovery*`, and one function decides every route
  there. Local and managed deployments answer exactly as before — no accounts,
  no login, nothing refused. Online, a caller must own the document or be on
  its access list, and then hold a role that grants an edit before anything is
  saved, renamed, deleted, restored or written as a draft; a caller with no
  roles — or with role names this build does not know, which fail closed — may
  read and is refused every write. Both families are still refused wholesale
  online, in front of that gate: `documents_dir()` has no owner dimension yet,
  and a role check says who may edit, never whose file it is.

### Fixed

- **The three red tests (#19).** Two of them pinned behaviour from before the
  kit owned the chrome — the sidebar they expected the scaffold to author is now
  cloned from the kit — and were retargeted at what actually holds today. The
  third was a real bug: the replace branch of a component upsert skipped the
  gallery pass the append branch runs, so conversion was not idempotent and a
  second run produced a different document.
- **`op-host-web` compiles without `canvaskit` again.** Five modules were added
  without the feature gate their neighbours carry, so the wasm-clean compile
  check the CI runs had been red for a while.

- **The stored-document routes now clear the same gates as the local ones.**
  `POST /api/files/<key>/save` (the route the browser and autosave actually
  use) installs the document it wrote — a whole-document swap — but never
  asked the collaboration policy, so a guest could write through it while the
  same write through `/api/file/save` was refused. It now clears the same gate.
- **Online deployments refuse the document routes.** `documents_dir()` has no
  tenant dimension, so in a shared process one account's list, preview, save or
  delete addresses every other account's files. `/api/files/*` and
  `/api/recovery*` are refused in online mode exactly as the local-path routes
  already were; a per-tenant store is the real fix and belongs with #10.

- **Edits reached the daemon again.** The sync channel refused documents over
  2 MiB, and an ordinary kit-backed screen is ~3.4 MiB, so *every* real
  document silently stopped syncing: the daemon kept an old copy and Save wrote
  that old copy. The ceiling is now above the sizes the product produces (the
  honest fix is an incremental push — #16), and Save carries the document from
  the browser rather than trusting the daemon's echo.

- **Links work in both builds.** The desktop window now has what the browser
  got from the address bar: its title carries the page, `Cmd/Ctrl+Alt+←/→`
  walks back and forward over documents, pages and nodes, and
  `openpencil-desktop <file> --node <id>` opens focused on a node. "Copy link"
  is one command (layer context menu, or `Cmd/Ctrl+Alt+C`) that copies exactly
  the address the browser shows for the same selection, and says so with the
  editor's existing banner. The rule behind all of it — state to route, route
  to link — lives once in `op_editor_core::route`, shared by both hosts; the
  browser supplies its origin, the desktop the daemon it talks to.

- **"Copy link" for the selection (web).** The layer context menu gained a row
  and the editor a `Cmd/Ctrl+Alt+C` chord; both copy `<origin>/f/<key>/<slug>`
  with the current page and the selected node — exactly the address the tab is
  already showing — and raise the editor's transient banner to confirm it. One
  platform-free rule builds the link for both entry points, so a copied link
  cannot drift from the visible address.
- **Preview cards on the file screen.** The card grid shows each document's
  rendered preview instead of a placeholder band. Bytes are fetched by the host
  (the route answers a base64 envelope), installed into the image cache the
  canvas already uses, and drawn with the canvas's own decode handshake — so a
  preview is crisp on retina, decoded off the paint path, and evicted like any
  other image. A missing or failed preview costs the picture and nothing else.
- **Document previews, server side.** Saving (or creating) a document renders
  a preview through the same raster exporter the Export button uses — scaled to
  the card width from the page's own bounds — stores it beside the document,
  and serves it from \`GET /api/files/<key>/thumb\`. The index carries a
  \`hasThumbnail\` flag, and deleting a document deletes its preview. The cards
  paint it in a following change.
- **"All files" in the top bar.** A button left of the file menu (and of
  Import) opens the file browser from inside the editor, on both hosts — the
  browser goes to `/files` and the desktop shows the same screen, since it has
  no address bar to type into.
- **Rename and delete from the file screen.** Right-press a card for its
  menu: Rename opens a field over the card (Enter commits, Escape abandons,
  an empty name is treated as no change) and Delete removes the stored
  document. The card updates immediately and the fresh list is the authority,
  so a failed request corrects itself.
- **Search on the file screen.** The field takes the keyboard when clicked,
  filters the cards by name as you type (case-insensitive substring, Cyrillic
  included) and paints a caret; it swallows keystrokes while focused, so a
  letter cannot reach the canvas shortcuts behind the screen.
- **File browser actions.** A card opens its document (the daemon loads it,
  the address becomes `/f/<key>` and the tab takes the file's name), and
  "New file" creates a stored document and opens it. Presses are recorded on
  the state and performed by the host on the next frame — the widget layer has
  no transport of its own by design.
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
