# The web editor on one host

Two containers: the daemon that serves the editor, the bundle and the documents,
and Caddy in front of it for TLS. Everything that must survive a redeploy lives
in two named volumes.

What a person gets: a URL, a login, and a design editor with AI generation.
What is **not** in this release, so nobody goes looking for it: offline editing
(the browser needs the daemon), live collaboration beyond what the panel already
offers, and the desktop build. Say that in whatever message goes out with the
link.

## Before you start

| What | Why it is needed |
| --- | --- |
| A host with Docker and the compose plugin | `docker compose version` should print something |
| A domain whose A record points at that host | Caddy requests the certificate on first start and fails without it |
| Ports 80 and 443 reachable from the internet | the certificate challenge needs 80; people need 443 |
| `deploy/web/.env` filled in | `cp .env.example .env`, then set `DOMAIN`, `ACME_EMAIL`, `ADMIN_USERNAME`, `ADMIN_PASSWORD` |
| `chmod 600 .env` | it holds the first administrator's password |

## Bring it up

```sh
cd deploy/web
cp .env.example .env && $EDITOR .env          # DOMAIN, ACME_EMAIL, ADMIN_*
chmod 600 .env

# Two ways to get the image onto the host:
#   a) build it here (needs ~15 GB free and 20-30 minutes: it compiles the wasm
#      bundle and the daemon inside the image)
docker compose build
#   b) or build it anywhere and move it:
#      docker build -f Dockerfile.web-rust -t openpencil-web-rust:<tag> .
#      docker save openpencil-web-rust:<tag> | gzip > openpencil.tgz
#      scp openpencil.tgz host: && ssh host 'gunzip -c openpencil.tgz | docker load'

docker compose up -d
docker compose logs -f openpencil caddy       # watch the certificate and the admin creation
```

The daemon logs one line when it creates the first administrator, and Caddy logs
the certificate it obtained. When both are there, open `https://$DOMAIN`.

## Check it, in this order

1. **The page loads over https** — a certificate warning here means the A record
   or the challenge, not the editor.
2. **Log in** with the administrator from `.env`. A refusal that says "no such
   account" means the account store is not the one you think it is: check that
   `OPENPENCIL_ONLINE_DATA_DIR` is on the mounted volume.
3. **Create a document** and reload the page: it must still be there. This is
   the check that the documents volume is mounted, and it is worth doing before
   anyone else is invited.
4. **Run one design turn** ("нарисуй экран логина: карточка по центру, поля
   email и пароль, кнопка войти") and watch the canvas. A turn that finishes with
   nothing on the canvas is a real defect to report, not a slow start.
5. **Invite one person** and have them log in, open the document and edit it.
   This is the first end-to-end proof that accounts, sharing and the store agree.

## After it is up

- **Back up the two volumes.** `openpencil-data` is every account and session;
  `openpencil-documents` is every document. A snapshot of both is a consistent
  pair; one without the other is a store whose index names documents it cannot
  open.
  ```sh
  docker run --rm -v openpencil_openpencil-data:/data -v "$PWD:/backup" \
    alpine tar czf /backup/openpencil-data-$(date +%F).tgz -C /data .
  docker run --rm -v openpencil_openpencil-documents:/documents -v "$PWD:/backup" \
    alpine tar czf /backup/openpencil-documents-$(date +%F).tgz -C /documents .
  ```
- **A forgotten password** is not fixed through `.env` — that pair only ever
  creates a *fresh* store's first administrator, and changing it later changes
  nothing. The account store is a file inside the `openpencil-data` volume, so
  the repair is: stop nothing, run the CLI **on a host that has `op`**, pointed
  at that directory.
  ```sh
  # on the host that can see the volume (a bind mount, or docker cp in/out):
  OPENPENCIL_ONLINE_DATA_DIR=/path/to/openpencil-data op admin create
  ```
  The daemon image deliberately carries the daemon only, not the CLI, so there
  is no `op` inside the container to run.
- **Logs**: `docker compose logs -f openpencil` is the API side (and the only
  place a failed turn says why); `docker compose logs -f caddy` is access and
  certificate. Both rot with the container unless a logging driver is configured.
- **Updating**: `docker compose pull` (or rebuild), `docker compose up -d`. The
  volumes are untouched; the first request after a restart may take a moment
  while the wasm bundle is revalidated.

## When something is wrong

| Symptom | First thing to check |
| --- | --- |
| Certificate warning in the browser | the A record, then `docker compose logs caddy` |
| "no such account" for the admin you just set | the account store is not on the volume, or the store was already non-empty when `.env` was set |
| The editor loads but AI turns fail | `docker compose logs openpencil` — a turn without a model credential says so there, not in the UI |
| A document disappeared after a redeploy | the documents volume is not mounted at `NORKA_DOCUMENTS_DIR`, which is the one mistake that loses data |
| The page is stale after an update | hard-reload; the bundle route is `no-cache`, but a browser can still hold the old wasm in memory |
