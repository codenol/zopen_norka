# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Each entry names the version that ships it; the app's top bar shows the same
version next to its build time, so a running build can be matched to a section
here at a glance.

## [Unreleased]

### Added

- **Share, not Collaborate — the dialog a designer actually reaches for.** The
  top bar's chip now opens a Figma-shaped access dialog instead of the
  live-session panel: copy the document link, add comma-separated emails or
  account names to invite, see who has access and at what level, switch
  "anyone with the link" on or off — and every press ends in a sentence: the
  grant, the links an invitation produced, or the reason nothing happened. The
  dialog's footer keeps the live collaboration session reachable, because the
  chip was its only entrance. The four levels are the ones the daemon already
  enforces (admin / editor / commenter / viewer), and the dialog shows that
  mapping rather than assuming Figma's two.

  The browser host drains the dialog's queued work to
  `POST /api/share/{grant,revoke,link}` and `GET /api/share/list`, and issues an
  invitation through `POST /api/auth/admin/invites` — which hands back a link
  and sends nothing, because this deployment has no mail. What the caller may do
  is read from the address (`?tenant=` means somebody else's document), which is
  the same answer the routes give, so the button cannot offer what the server
  would refuse. The desktop host answers the questions it can answer locally —
  the clipboard, the collaboration panel — and says plainly that the rest needs
  an online deployment.

- **A section says what it was built from.** The toolbar has a **Section** tool:
  draw one and it is a frame, marked, that groups the screens of one feature or
  scenario. Select it and the property panel opens with the section's own block
  above everything else — because what a section came from is the first question
  a reader of somebody else's design asks, and the one thing the rest of the
  inspector cannot answer:

  - **Built from** names the analytics document the section was written against
    and says whether it still matches: in sync, the analytics has changed since,
    the screens have changed since, both did, or the document is gone. A link
    that no longer holds is painted in the warning tone, because it is the one
    claim in the block that quietly becomes false.
  - **The summary** — what this is, where to look, use cases, what to check —
    with the unanswered questions left out rather than padded with dashes.
  - **Flows**, with a step count each, because a list of names alone cannot say
    whether a flow was ever finished.

  Three empty states are kept apart rather than collapsed into one: "nobody has
  written anything about this section", "we have not read it yet" and "the read
  failed" are different facts, and a block that painted the same nothing for all
  three would tell a designer their summary had been erased. A section nobody
  has read says so and stops, rather than showing four empty fields that look
  like answers.

- **The summary is written where it is read.** Click a question in the section's
  block, type, and Enter saves it — the answer goes to the daemon with the whole
  properties object, so the route can see that only the summary moved and ask
  for the summary right rather than for an edit of the document. Escape gives the
  keyboard back without saving, and a write that fails keeps the sentence, so the
  retry is a keystroke rather than retyping it.

- **The canvas marks a section that no longer matches what it was built
  from.** A section whose analytics has changed since — or whose screens have —
  wears a warning glyph in its top-right corner, visible without selecting
  anything. An octagon rather than a triangle when the document it was built
  from is gone, because "the reasoning moved" and "the reasoning is missing" are
  different repairs. Nothing is marked before the store has been asked, so a
  document that has never been opened does not accuse its own sections.

- **A section can be attached to the analytics it came from.** "Attach
  analytics…" opens a file dialog; pick a markdown document and it is loaded into
  the store as an asset and linked to the section in one step, with both
  fingerprints recorded — the analytics' own digest as the store hashes it, and
  the section's screens as the document holds them. The name a person reads is
  the file's, without its extension. The link is written in the same gesture
  rather than living in the panel, because a link that was never saved would be
  gone the moment the selection moved.

- **Analytics is an asset with an address.** `POST /api/analytics` loads a
  markdown document, `GET /api/analytics/<key>` returns it with the digest of
  what it says now, and a rename keeps the address a section's link points at.
  The markdown lives as a real file beside the `.op` files, so a person who is
  not in the app can read the reasoning — which is the point of it being
  markdown at all.

- **The release binary is built in CI, not on the production server.** A new
  `Web deploy build` workflow (on demand, on a `v*` tag, and on pull requests
  without publishing) builds `op-host-web-server` for
  `x86_64-unknown-linux-gnu` together with the CanvasKit web bundle, and hands
  both over as run artifacts. Nothing is published: how an artifact reaches a
  server stays a human decision.

- **Signing in to a deployment, with the deployment's own accounts.** The
  online daemon resolves every request against the account store that landed in
  the previous change: a session cookie (`norka_session`, `HttpOnly`,
  `SameSite=Lax`, `Secure` everywhere except a browser on this machine) names an
  account, and the roles on that account reach the route checks that already
  existed. Nothing outside the daemon asserts who anybody is any more.

  The routes are `POST /api/auth/login`, `POST /api/auth/logout` (with
  `{"all":true}` for "sign out everywhere"), `GET /api/auth/status` — which now
  answers an anonymous caller `200 {signed_in:false}` instead of refusing, so
  the shell can tell "signed out" from "unreachable" — and
  `POST /api/auth/invite/accept`, where an invitation becomes an active account
  with the roles the invite carried and a session. A `disabled` account cannot
  sign in, and an invitation cannot be accepted twice.

- **The first administrator is a deliberate act.** Either `NORKA_ADMIN_USERNAME`
  and `NORKA_ADMIN_PASSWORD` on the deployment's first start — obeyed only when
  the store holds **no accounts at all**, so a deployment that already has one
  is never given a second administrator by a leftover variable — or
  `op admin create`, which asks for a name and a password twice, refuses a weak
  one, and prints neither. A deployment with no administrator still serves, and
  its status route says the administrator is missing.

- **`op admin create [--data-dir DIR]`** — the first administrator, from the
  host an operator is sitting at. Writes the same `accounts.db` the daemon
  writes, in `OPENPENCIL_ONLINE_DATA_DIR` unless `--data-dir` says otherwise.

- **An invitation can now actually be issued.** Accepting one worked and
  issuing one had no front door at all, so a deployment could describe an
  account it had no way to make. `POST /api/auth/admin/invites` now hands out
  the link — returned once and stored nowhere but as a SHA-256, exactly as the
  store already held it — and `GET /api/auth/admin/invites` is the list an
  operator needs when somebody says the link does not work: who issued it, when
  it was issued, when it stops being accepted, and whether anybody used it.
  `POST /api/auth/admin/invites/revoke` withdraws one by the id the listing
  gives it. `GET /api/auth/admin/users` is who is in the deployment, with their
  roles, status and last sighting, and `POST /api/auth/admin/users/roles` +
  `POST /api/auth/admin/users/status` change it. All of it is for an account
  whose roles carry the account list — a guest or an ordinary contributor gets
  the daemon's usual `403 {error:"admin-role-required"}`, and a caller with no
  session gets `401`.

  **`op admin invite [--roles a,b] [--email ADDR] [--origin URL]`** does the
  same from a shell and prints the link, for the deployment whose operator has
  no browser and for the script that makes one link per person. Roles are this
  build's own vocabulary and are folded onto their wire spellings; a role the
  build does not have is refused before anything is written, rather than stored
  as a role that would silently grant nothing.

- **The foundation of our own accounts.** A store of its own — users, sessions,
  invitations and one-time tokens — in a database beside the deployment's data
  rather than beside its documents, because accounts belong to a deployment and
  documents to a folder. Passwords are Argon2id and only the hash is kept;
  tokens are 32 random bytes of which the database holds **only a SHA-256**, so
  a copy of the file is not a set of working credentials. The tests assert that
  by scanning the database and its write-ahead log for the plaintext.

  Nothing reads it yet: no route, no verifier, no daemon. This is the floor the
  sign-in work stands on, landed first so that the visible half is a change to
  behaviour rather than a change to storage.

- **Signing in from the browser.** The daemon could sign people in; the browser
  had no way to ask. It does now: the web shell reads `GET /api/auth/status`,
  and — only where that answer says this deployment has accounts and nobody is
  signed in — shows a name-and-password form over the editor. A wrong name and a
  wrong password get one and the same sentence (the store refuses to say which,
  so the form must not guess), a disabled account says so, and the typed
  password is dropped from editor state the moment the request carrying it is
  built. A deployment with no accounts at all shows how to create one
  (`NORKA_ADMIN_USERNAME`/`NORKA_ADMIN_PASSWORD`, or `op admin create`) instead
  of a form that could only refuse. Signing out is the account menu's own row,
  and it re-reads the status rather than assuming either outcome.

- **Invitations open where the link points.** `GET /invite/<token>` serves the
  editor page and shows an acceptance form — account name, password twice, an
  optional display name — that posts to `POST /api/auth/invite/accept` and, on
  success, leaves the visitor signed in. A spent, expired or unknown link says
  which of the three it is.

### Changed

- **A comment is placed by coordinates instead of being pinned to an element.**
  A pin hung on a node, so a comment about a small element — an icon, a label,
  the space between two frames — could only be placed by hitting that element
  exactly, and it moved with the element afterwards. A comment is now a point
  on a page: `pageId`, `x`, `y` in the document's own coordinates, so zooming
  and panning never move it and the pin stays where the reviewer pointed.

  Comments written before this change are kept, with their replies: they have
  no coordinates, so they are listed without a pin, and the element each one
  was anchored to is carried along as a hint. The comment routes answer the new
  fields, and a body that still carries `nodeId` is refused with a reason
  rather than accepted and placed somewhere else.

- **The comment tool moved into the toolbar, and its thread list into the right
  rail.** The mode was a pill in the corner of the canvas and its list was a box
  floating over the design; both were places a reviewer looks for something
  else. Comments are now an entry in the tool column beside the other modes —
  it carries the number of open threads on the page, and picking another tool
  leaves it, as with every other tool — and the list takes the rail the
  inspector uses, so the canvas keeps its width and nothing sits on the design.
  Threads are listed for the page being edited, with a count of the rest.

### Removed

- **The browser extension, and the Rust crate behind it.** The Manifest V3
  Chrome extension is gone from the tree — `packages/op-chrome-extension/` and
  everything in it (manifest, locales, popup, capture scripts, store
  packaging) — together with the `crates/op-chrome-extension-core/` wasm core it
  loaded. We do not use it and do not plan to, and it carried a client for a
  third-party service, whose domains its manifest asked the browser for.

  What only served it goes too: the version-sync guard that pinned the
  extension manifest to the workspace version, the CI wasm32 check that
  compiled the crate, the `packages/` lint guards and build scripts that all
  pointed into the extension directory, and the ignore rules that existed to
  keep its generated wasm and its vendored extractor copy out of the linters.

  Nothing the product uses lived in that crate. `design.md` extraction and the
  browser-snapshot ingress stay where they already were —
  `crates/op-host-services` (`design_md_*`, `mcp_live/snapshot_ingest.rs`) — and
  are unchanged.

- **The browser shell's device-login proxy.** The daemon stopped serving
  `/api/auth/login/begin`, `/api/auth/login/status`, `/api/auth/login/cancel`,
  `/api/auth/avatar` and `/auth/loading` in the previous change; the browser
  shell kept calling them, so signing in from a browser opened a popup that
  could only 404. Those calls, the popup they navigated, the login-status poll
  and the account-avatar fetch are gone, and the sign-in surface is the password
  form. The route spellings stay in `op_editor_core::auth_routes` because the
  daemon's tests name them to prove the 404.

- **The hub, and the device-login proxy that fronted it.** Identities are this
  deployment's own, so the hub client, its error type and its verifier are
  gone, along with `OPENPENCIL_HUB_BASE_URL` and the `op_hub_session` cookie.
  The daemon's device-login routes (`/api/auth/login/*`, `/api/auth/avatar`,
  `/auth/loading`) are gone with the SSO they proxied; a request for one is not
  found. The `op-auth-bridge` library itself stays until the mobile hosts are
  ported off it.

  A deployment that previously set `OPENPENCIL_HUB_BASE_URL` now needs
  `OPENPENCIL_ONLINE_DATA_DIR` (which `--online` already required) and either
  the `NORKA_ADMIN_*` pair or `op admin create`.

### Fixed

- **Save no longer writes a local document into the file the daemon is
  holding.** A document opened from disk, imported, or started with File → New
  has no key in the store — and neither does the document a daemon shows on `/`,
  so Save treated the two the same and wrote the browser's work into the daemon's
  bound file path: a different document, told it had saved (issue #98). The tab
  knows where its document came from; Save honours it now, and a local document
  goes to the browser's own download instead of over somebody else's file.

- **A visitor cannot change who else may open a document they were given.** The
  share routes administer the CALLER's own access list, so a visitor's grant
  used to land on their own list and be answered `200 changed:true` — a success
  reported for a document they have no authority over, and a row that appeared
  in the wrong "Who has access". It is refused with `cannot-reshare` now, and
  reading the list stays allowed.

- **A guest is told what level they hold.** `GET /api/share/list` answered
  `sharedWithMe` as bare owner ids, so a colleague given "can comment" was told
  nothing about it and the Share dialog drew them as view-only whatever the
  grant said. The list carries the level now, and the dialog's own row shows it.

- **A copied share link finally reaches the colleague it is sent to.** The link
  named the document but not its owner, and the daemon serves another account's
  document only when the address says whose it is — so a colleague who pasted
  the link was refused with `tenant-not-shared`. The copy now carries the
  owner's account key, and the dialog's own identity in "Who has access" comes
  from that key rather than from a username the access list has never heard of.

- **A fresh online deployment is no longer read-only in a browser.** A
  cookie-authenticated write was admitted only when the deployment's own origin
  appeared in `OPENPENCIL_WEB_ALLOWED_ORIGINS`, so creating a document, saving,
  commenting or sharing from the deployment's own editor was refused with
  `cross-origin-write-forbidden` until an operator named their own page. The
  request's own host is now trusted for it — a page cannot forge that header —
  and the allowlist keeps its meaning for everybody else.

- **The link-access switch no longer reports the state from one click ago.**
  `POST /api/share/link` answered with the level the document handed out BEFORE
  the change, and the dialog stores that answer.

- **A share grant naming an account that does not exist is refused** rather than
  recorded: the invite field is a text box that accepts a name, and the route
  used to answer `200 changed:true` for one, adding a row to "Who has access"
  that grants nobody anything and looks exactly like a person.

- **A letter that is also a canvas tool shortcut no longer switches the tool
  while the Share dialog's invite field has focus.** Typing `userB` left the
  field holding `useB` and selected the rectangle tool behind the card, because
  the bare-letter tool router ran before the invite field's own arm. The dialog
  is modal and now takes the keyboard the way the comment composer (#49) and the
  sign-in form (#87) already do. Found by clicking through the dialog in a
  browser, not by a test.

- **On the desktop, an attached image no longer disappears.** If a turn ran as a
  chat or a modify request rather than a design request, the attachment had been
  taken into the design request and was simply gone: the assistant answered as
  if nothing had been attached, and the person was told nothing. The attachment
  now goes to whichever route actually runs, and a turn that cannot start says
  so — including that the attachment was not sent.

  Where a route genuinely cannot carry a file, the model is told in the prompt
  and the person is told in the transcript, instead of either being left to
  assume it arrived.


- **A local file can no longer be saved into somebody else's stored document.**
  The tab kept the server key of the document it had open before, so opening a
  `.op` file from disk left it holding the previous document's identity: Save
  and autosave posted the local file's contents to `/api/files/<old key>/save`,
  and the comment tool read and wrote the wrong document's conversation. The key
  now belongs to the document and moves with it — every seam that installs a
  document the server does not hold (open from disk, drag-and-drop, Figma/HTML
  import, Open Recent, File → New, restoring a recovery draft) drops it, and the
  seams that adopt a stored document's key set it. With no key, autosave goes to
  the daemon's draft slot and the comment tool sends nothing at all.

- **An attached screenshot now reaches the model — and the brief stops being
  invented when it does not.** The built-in API-key providers posted the turn's
  text and nothing else, so a reference image arrived as the line
  `[attached image: /tmp/…]`: the model answered about the file name, and that
  answer was handed to the planner as an authoritative inventory of a screen it
  had never seen. Images now ride the request body as real image blocks —
  OpenAI `image_url` data URLs, Anthropic base64 `image` blocks — with the
  media type read from the bytes rather than the browser's label. A transport
  that cannot deliver an attachment says so in the prompt instead of naming a
  path it cannot open, and the vision caller refuses to spend a call it cannot
  ground, so the planner gets the conservative brief that forbids inventing KPI
  cards, charts and analytics tiles. Transports that genuinely read the file
  themselves (Claude Code, GitHub Copilot, local ACP, the CLI agents) keep
  doing exactly that.

- **A comment field opened in the corner of the canvas instead of where the
  comment was.** Clicking an element in the middle of the page opened the
  composer at the canvas' top-left corner, and the pin then appeared somewhere
  the reviewer never clicked. The field now opens at the point that was
  clicked — the marker's own place — so what is typed and what is pinned are
  the same spot, and the box stays inside the window at the edges.

- **A document with open comments no longer looks like one nobody ever
  discussed.** The conversation was read only once the comment tool was armed,
  so the page list, the layer rows and the badge on the tool stayed empty until
  the reviewer went looking — asserting "no comments" when the truth was "not
  asked". Opening a document now reads its conversation once, up front, through
  the same request the tool used to send; a different document, or a different
  account, reads again. Saving and autosaving deliberately do not, because a
  save cannot change a conversation and autosave would turn it into a stream of
  requests.

## [0.9.0] — 2026-09-14

### Added

- **Comments on elements, with threads and resolve.** A pin hangs on an
  element rather than a point, so it stays with what it is about when the
  layout moves; a thread carries its replies and is resolved rather than
  deleted, because a resolved thread is exactly the record of what was asked
  and what was answered. The author is recorded as they were — name and role at
  the time — and drawn in their role's colour, so the panel says who was
  speaking *then* rather than who they are now.

  Nobody needs the edit right to take part: five of the seven roles comment and
  do not edit, so commenting is a right of its own. Resolving is the thread's
  author, or anyone who may edit the document. A conversation is deliberately
  **not** part of the document: the routes cannot reach the editor state at
  all, so commenting never bumps a version, never enters undo and never makes
  a file dirty.

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

  The online refusal that fronted these routes (#20) is gone: a document now
  records the account that made it, the list is asked for by owner, and a
  document addressed by key is reached only by the account that owns it or by a
  visitor the owner admitted. See the entry below.

- **A shared deployment serves the file list, and only your own rows.** Every
  document the daemon makes now records the account that made it, and the file
  list asks for that account's rows: one directory holds everyone's files, so
  "list everything" would hand over names, sizes and timestamps that belong to
  somebody else. A document addressed by key is a different question — the key
  says nothing about whose row it is — so every per-key route resolves the row
  and checks its owner before it touches a file: your own document, or one of
  the workspace you were admitted to; anything else answers `tenant-not-shared`
  rather than an empty list or somebody else's file.

  What this replaced was a wholesale refusal of `/api/files*` and
  `/api/recovery*` in a public deployment (#20), because a role check could say
  who may edit and never whose file it was. Working online is now the same as
  working locally: create, list, open, save, rename and delete, for the account
  the documents belong to. A document shared with you is opened by the key its
  link carried, not listed.

  The unsaved-work draft moved with it: it is one slot per workspace rather than
  one file for the whole process, keyed by a digest of the workspace's account
  so an opaque hub id can never become a path. Before that, `restore` would have
  adopted one account's unsaved work into another's editor — the per-caller role
  gate cannot close that, because it decides what a caller may do and never
  whose file this is. Rows with no owner at all (everything the legacy
  `index.json` import brought over) belong to no account and are therefore in
  nobody's list online; #46 tracks the adoption path that does not exist yet.
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

- **The comment field takes what you type into it.** A comment composer is an
  in-canvas text field like the chat input and the property inputs, but the
  host's "a text input owns the keyboard" rule did not know about it. So while
  a review was being written, the browser's hidden IME capture input was never
  focused — and anything the browser delivers that is not a plain
  single-character `keydown` (an IME commit, a dead key, an emoji insertion)
  had nowhere to land, leaving the field empty and Send switched off. The
  letters that name canvas tools (`r`, `t`, `v`, …) also switched the tool
  instead of being typed. The composer now reports itself as the active text
  input, which is the same rule every other field follows (#49).
- **A shared document no longer hands over the account behind it.** Two routes
  write the *workspace* rather than the document — the AI provider credentials
  and the MCP server card — and both are reachable with a `?tenant=` lease,
  which is how a browser addresses a document shared with it. A visitor whose
  roles grant no more than reading could rewrite the owner's provider keys and
  the owner's MCP port: the document was protected and the account behind it
  was not.

  The right here is not the document's. Answering with the document's "edit"
  would have made the hole worse rather than smaller — an editing role is
  handed out to work on a shared document and must not carry the owner's
  credentials with it. So the question is *whose workspace is this*: the owner,
  or an admin.
- **The rights reached the routes that actually edit the document.** The gates
  added for the file routes covered saving a document; they did not cover
  *editing* one, and the browser edits through `POST /api/mcp/document`. An
  account with no editing role could therefore still change a document by
  editing it — the gate guarded the door next to the open window.

  One table now answers "does this request change the document" for every tier,
  and one gate checks it in front of all of them: the document push, the MCP
  tools by what the call does rather than by its path, `sync-reset`, selection,
  and two routes found on the way that changed the document in the same way —
  `POST /api/ai/standard` (an AI turn applies commands) and `POST /api/file/new`
  (which replaced the document with a fresh starter, and in online mode was not
  even behind the local-file refusal).

  The gate sits ahead of every branch, the way the token-scope gate already
  does and for the same reason: a check inside one branch leaves the others as
  a way round with the same effect. A read-only visitor keeps everything that
  reads — the tool list and read-only tools included — and is refused every
  write. Local and managed daemons answer as before, proven by a unit test and
  by a run through the real connection loop.

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
