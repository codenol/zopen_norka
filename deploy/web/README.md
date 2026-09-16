# Deploying the web editor onto a host that already runs it

This is the recipe for **this** deployment: one Ubuntu host, nginx for TLS, and
the daemon under systemd. No containers — the operator does not want them, and
nothing here needs one: the product is a binary, a wasm bundle and a directory of
assets.

An earlier version of this directory described a compose file with Caddy. It was
removed: it was written before anyone had looked at the server, and the server
already had the shape this recipe follows. Kept here as a sentence so the next
reader does not go looking for it.

## What the deployment is made of

| Piece | Where | Notes |
| --- | --- | --- |
| daemon binary | `/opt/norka/src/target/release/op-host-web-server` | Release build, x86_64-unknown-linux-gnu |
| wasm bundle | `/opt/norka/src/crates/op-host-web/pkg/` | The page, the shim and the staged assets (fonts, previews, templates, icon catalog) |
| CanvasKit | `/opt/norka/src/crates/op-host-web/assets/canvaskit/` | Loaded by the bundle at run time; not part of a deploy |
| account store | `/var/lib/norka/online/accounts.db` | **Never** touched by a deploy |
| documents | under the same data directory | **Never** touched by a deploy |
| service | `norka-op.service` | `--serve-web 3100 --host 127.0.0.1 --online` |
| TLS + proxy | nginx site `norka-op` | `proxy_pass 127.0.0.1:3100`, SSE-friendly (`proxy_buffering off`), 32 MB body |

The origin matters: the unit sets
`OPENPENCIL_WEB_ALLOWED_ORIGINS=https://html.norka.cc`, and without it the CSRF
gate refuses cookie writes.

## The model every account gets, and the key a person may bring

A deployment has two ways to reach a model, and `--online` supports both at
once:

| | Where it lives | Who can change it |
| --- | --- | --- |
| **The shared model** | `~/.config/openpencil/settings.json` **of the service account** — here `/var/lib/norka/.config/openpencil/settings.json`, mode 0600, owner `norka` | The operator, by editing the file and restarting the unit. No account can: `--online` refuses every settings write (`online_policy::allows_settings_persistence`) |
| **A person's own key** | The browser, per turn — the `credential` field of the turn's own request | Whoever holds the key. It is spent per request, is never written to the settings file, and dies with the account's in-memory tenant |

The daemon reads the shared file **once, at start-up**, and installs its
operator-owned providers into every account as that account's editor is created
(`web_canvas_server/deployment_providers.rs`). Two consequences worth knowing:

- **A model change is a restart, not a deploy.** Nothing re-reads the file while
  the daemon runs; `systemctl restart norka-op` is the whole update.
- **The first thing to read in the log is one line.** Every start prints either
  `offering N shared model(s) from M provider(s) to every signed-in account` or
  `no shared model is offered …`, which answers "why is the model list empty?"
  without waiting for a turn to fail.

`OPENPENCIL_PERSIST_WEB_CREDENTIALS_SERVER` is **not** the switch for this, and
on an `--online` daemon it does nothing at all: a tenant's credential policy is
browser-only by construction (`WebCanvasState::new_for_tenant`) and the route
that would write the process settings file is gated on the serve mode
(`persist_api_settings`), not on that variable. It matters only to the
single-document daemon (desktop's `--serve-web`), where it lets a browser persist
a credential into that process's own settings file — which is exactly why its
shipped default is fail-closed.

## Updating it

```sh
# 1. Get the built pair onto this machine. The workflow publishes the pair on a
#    `v*` tag push and on a manual dispatch — a plain push to main publishes
#    NOTHING (its pull_request runs deliberately upload no artifacts), so a run
#    id from `gh run list` may have nothing to download:
gh run list --workflow=web-deploy-build.yml --limit 5
gh workflow run web-deploy-build.yml --ref <branch-or-tag>   # when there is no tag run
gh run download <run-id> --dir dist/web
#    → dist/web/op-host-web-server-x86_64-unknown-linux-gnu/op-host-web-server
#      dist/web/op-web-bundle/…

# 2. Put them where the script expects. It wants `op-host-web-server` and `pkg/`
#    at the top of the payload directory, and `gh` nests each artifact under a
#    directory named after it:
mkdir -p dist/web/pkg
cp -a dist/web/op-web-bundle/. dist/web/pkg/
cp -a dist/web/op-host-web-server-x86_64-unknown-linux-gnu/op-host-web-server dist/web/

# 3. Deploy. It stages, backs up, swaps, restarts and health-checks, and prints
#    a rollback command if the daemon does not come up.
bash deploy/web/deploy.sh --host root@html.norka.cc --dry-run
bash deploy/web/deploy.sh --host root@html.norka.cc
```

The script keeps the two most recent backups under `/opt/norka/src/.backups/` and
records the deployed revision in `/opt/norka/src/.deployed-revision`.

## Verifying a deployment, in this order

1. **`https://html.norka.cc` loads over https.** A certificate warning here is
   nginx or Let's Encrypt, not the editor.
2. **Sign in** with the admin account. A refusal that says the account does not
   exist means the daemon is not reading the store you think it is — check
   `OPENPENCIL_ONLINE_DATA_DIR` in the unit.
3. **Create a document, reload, and find it there.** This is the documents
   directory, and it is worth checking before inviting anyone.
4. **The model list is not empty.** With a signed-in session cookie,
   `GET /api/ai/models` lists the shared model with its `builtinProviderId` and
   value. `[]` here is the deployment offering nobody a model — check the
   start-up line and the settings file above before blaming the AI path.
5. **One design turn**, e.g. `нарисуй экран логина: карточка по центру, поля
   email и пароль, кнопка войти`. A turn that ends with the canvas unchanged is a
   defect to report, not a slow start.
6. **Invite one person and have them sign in.** The first end-to-end proof that
   accounts, sharing and the store agree.

## When something is wrong

```sh
systemctl status norka-op --no-pager
journalctl -u norka-op -n 80 --no-pager      # the daemon's own words, including a failed turn
nginx -t && tail -20 /var/log/nginx/error.log
```

| Symptom | First thing to check |
| --- | --- |
| 502 from nginx | `systemctl is-active norka-op`; the daemon binds 127.0.0.1:3100 |
| editor loads, AI turns fail | `journalctl -u norka-op` — a missing model credential says so there |
| `GET /api/ai/models` answers `[]` | the start-up line: `offering N shared model(s)…` means the deployment has one and the account is asking with the wrong session; `no shared model is offered` means `/var/lib/norka/.config/openpencil/settings.json` (the SERVICE account's `~/.config`) holds no enabled provider with a key and a model. On a workstation that path is `~/Library/Application Support/openpencil/settings.json` on macOS — a hand-written `~/.config` copy is read by nobody there |
| sign-in refused for the admin | the store in `OPENPENCIL_ONLINE_DATA_DIR`, and whether it was ever seeded |
| documents gone after a deploy | they should not be: they are outside the install root. Check the volume, not the deploy |
| stale page after an update | hard-reload: the bundle route is revalidated, but a tab can hold the old wasm in memory |
