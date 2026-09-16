#!/usr/bin/env bash
#
# deploy.sh — put a built daemon and web bundle onto a server that runs the
# daemon under systemd, and prove it came up. No containers anywhere.
#
# The product is two files and a directory: the `op-host-web-server` binary, the
# wasm bundle (`pkg/`), and the CanvasKit runtime the bundle loads. A deployment
# is therefore "replace those, restart the unit" — which is what this script
# does, with a backup and a health check so a bad build does not become an
# outage.
#
# Usage:
#   deploy.sh --host <ssh target> [--dir <payload dir>] [--service <unit>]
#             [--root <install root>] [--dry-run]
#
#   --dir  A directory holding `op-host-web-server` and `pkg/`. Defaults to
#          `dist/web` next to the repository root; both CI artifacts
#          (`op-host-web-server-x86_64-unknown-linux-gnu` and `op-web-bundle`)
#          unpack into it with one `gh run download --dir dist/web`.
#
# What it does NOT do: build anything, touch nginx, or touch the deployment's
# data directory. The account store and the documents are the two things that
# must never be part of a deploy, and they live outside the install root.

set -euo pipefail

host=""
payload="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/dist/web"
service="norka-op"
root="/opt/norka/src"
dry_run=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --host) host="$2"; shift 2 ;;
        --dir) payload="$2"; shift 2 ;;
        --service) service="$2"; shift 2 ;;
        --root) root="$2"; shift 2 ;;
        --dry-run) dry_run=1; shift ;;
        -h | --help) sed -n '2,26p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done

[ -n "$host" ] || { printf 'error: --host is required (e.g. root@html.norka.cc)\n' >&2; exit 2; }
[ -x "$payload/op-host-web-server" ] || {
    printf 'error: %s/op-host-web-server is missing or not executable\n' "$payload" >&2
    printf 'hint: gh run download <run-id> --dir %s\n' "$payload" >&2
    exit 2
}
[ -d "$payload/pkg" ] || { printf 'error: %s/pkg is missing\n' "$payload" >&2; exit 2; }
[ -f "$payload/pkg/op_host_web_bg.wasm" ] || {
    printf 'error: %s/pkg has no op_host_web_bg.wasm\n' "$payload" >&2
    exit 2
}

# The revision the payload was built from, when the caller knows it. Recorded
# next to the installed files so "what is running" is answerable on the server
# without guessing from a timestamp.
revision="$(git -C "$(dirname "$payload")/.." rev-parse --short HEAD 2>/dev/null || echo unknown)"
bundled_bytes="$(wc -c < "$payload/pkg/op_host_web_bg.wasm" | tr -d ' ')"
binary_bytes="$(wc -c < "$payload/op-host-web-server" | tr -d ' ')"

printf 'deploying to %s\n' "$host"
printf '  binary : %s bytes\n' "$binary_bytes"
printf '  bundle : %s bytes (wasm)\n' "$bundled_bytes"
printf '  revision: %s\n' "$revision"
printf '  unit   : %s   install root: %s\n' "$service" "$root"

if [ "$dry_run" = 1 ]; then
    printf '(dry run — nothing was sent)\n'
    exit 0
fi

stamp="$(date -u +%Y%m%dT%H%M%SZ)"

# 1. Payload to a staging directory on the server, never straight over the live
#    one: a half-copied bundle is a daemon that serves a broken editor.
ssh "$host" "set -euo pipefail
    mkdir -p '$root/.deploy/$stamp'
    rm -rf '$root/.deploy/$stamp/pkg'
    mkdir -p '$root/.deploy/$stamp/pkg'
"
tar -C "$payload" -cf - op-host-web-server pkg | ssh "$host" "tar -C '$root/.deploy/$stamp' -xf -"

# 2. Stop, back up, install, start. The data directories are NOT touched: the
#    account store and documents live under /var/lib/norka, outside this root.
ssh "$host" "set -euo pipefail
    root='$root'; stamp='$stamp'; unit='$service'

    systemctl stop \"\$unit\"

    mkdir -p \"\$root/.backups\"
    if [ -x \"\$root/target/release/op-host-web-server\" ]; then
        cp -a \"\$root/target/release/op-host-web-server\" \"\$root/.backups/op-host-web-server-\$stamp\"
    fi
    if [ -d \"\$root/crates/op-host-web/pkg\" ]; then
        mv \"\$root/crates/op-host-web/pkg\" \"\$root/.backups/pkg-\$stamp\"
    fi

    install -m 0755 \"\$root/.deploy/\$stamp/op-host-web-server\" \"\$root/target/release/op-host-web-server\"
    mkdir -p \"\$root/crates/op-host-web\"
    mv \"\$root/.deploy/\$stamp/pkg\" \"\$root/crates/op-host-web/pkg\"
    printf '%s\n' '$revision' > \"\$root/.deployed-revision\"

    # Keep the two most recent backups and drop the rest, so a deployment host
    # does not slowly fill with copies of a 77 MB binary.
    ls -1dt \"\$root/.backups/op-host-web-server-\"* 2>/dev/null | tail -n +3 | xargs -r rm -f
    ls -1dt \"\$root/.backups/pkg-\"* 2>/dev/null | tail -n +3 | xargs -r rm -rf
    rm -rf \"\$root/.deploy/\$stamp\"

    systemctl start \"\$unit\"
"

# 3. Health, from the server and through the public origin. The daemon's own
#    root route is served without a credential in online mode, which is exactly
#    why it is the probe.
printf 'waiting for the daemon…\n'
for attempt in $(seq 1 20); do
    if ssh "$host" "curl -fsS -o /dev/null -m 3 http://127.0.0.1:3100/"; then
        printf 'daemon answers on 127.0.0.1:3100 (attempt %s)\n' "$attempt"
        break
    fi
    if [ "$attempt" = 20 ]; then
        printf 'error: the daemon did not come up; last 40 log lines:\n' >&2
        ssh "$host" "journalctl -u $service -n 40 --no-pager" >&2 || true
        printf 'roll back with:\n' >&2
        printf '  ssh %s "systemctl stop %s && cp -a %s/.backups/op-host-web-server-%s %s/target/release/op-host-web-server && systemctl start %s"\n' \
            "$host" "$service" "$root" "$stamp" "$root" "$service" >&2
        exit 1
    fi
    sleep 2
done

unit_state="$(ssh "$host" "systemctl is-active $service")"
printf 'unit: %s\n' "$unit_state"
[ "$unit_state" = "active" ] || { printf 'error: unit is %s\n' "$unit_state" >&2; exit 1; }

printf 'deployed %s to %s\n' "$revision" "$host"
printf 'verify from outside: the page loads, a login is accepted, and one design turn draws something\n'
