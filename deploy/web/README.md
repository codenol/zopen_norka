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

## Updating it

```sh
# 1. Get the built pair onto this machine. CI produces both on a push to main:
gh run list --workflow=web-deploy-build.yml --limit 3
gh run download <run-id> --dir dist/web
#    → dist/web/op-host-web-server  dist/web/op-web-bundle/…

# 2. Put them where the script expects (it wants `pkg/`, the bundle's own name):
mkdir -p dist/web/pkg && cp -a dist/web/op-web-bundle/. dist/web/pkg/

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
4. **One design turn**, e.g. `нарисуй экран логина: карточка по центру, поля
   email и пароль, кнопка войти`. A turn that ends with the canvas unchanged is a
   defect to report, not a slow start.
5. **Invite one person and have them sign in.** The first end-to-end proof that
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
| sign-in refused for the admin | the store in `OPENPENCIL_ONLINE_DATA_DIR`, and whether it was ever seeded |
| documents gone after a deploy | they should not be: they are outside the install root. Check the volume, not the deploy |
| stale page after an update | hard-reload: the bundle route is revalidated, but a tab can hold the old wasm in memory |
