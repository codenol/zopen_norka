#!/usr/bin/env bash
#
# build-bundle.sh — build the CanvasKit web bundle HERE, so a deploy does not
# have to wait for CI.
#
# Why this exists (measured, 2026-09-17):
#
#   the CI lane       queue 10-20 min + build 6m46 + browser smoke 1 min
#                     + 100 MB of artifacts to download on this link ≈ 22-30 min
#   this script       build 2m15 + 15 MB to upload (35 s) ≈ 3 min
#
# The CI lane is not slow at what it does — its cache works and the build is
# under seven minutes. What costs the time is the queue in front of it and the
# round trip, and both disappear when the bundle is built on the machine the
# change was made on.
#
# The one thing this does NOT do is `wasm-opt -Oz` (the CI gate runs it). That
# turns out not to matter for what a visitor downloads: measured on the same
# commit, the optimised bundle is 18 256 602 bytes of wasm and this one is
# 20 558 138 — but gzipped they are 5 719 075 and 5 719 643. `-Oz` removes the
# redundancy gzip already removes, so the page a browser fetches is the same
# size either way. When `wasm-opt` IS on the PATH it is applied, and the script
# says which of the two it produced.
#
# Usage:
#   build-bundle.sh [--out <dir>] [--quiet]
#
# Writes <dir>/pkg (default `dist/web-local/pkg` next to the repository root),
# the directory `deploy.sh --dir <dir> --bundle-only` expects.
#
# What it does NOT do: the daemon binary. That one is Linux x86_64, so it is
# still built by CI — but it changes far less often than the interface, and a
# bundle-only deploy does not touch it.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
out="$root/dist/web-local"
quiet=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) out="$2"; shift 2 ;;
        --quiet) quiet=1; shift ;;
        -h | --help) sed -n '2,40p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done

cd "$root"

say() { [ "$quiet" = 1 ] || printf '%s\n' "$*"; }

for tool in cargo wasm-bindgen; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'error: %s is required and is not on the PATH\n' "$tool" >&2
        printf 'hint: cargo install wasm-bindgen-cli --version 0.2.117\n' >&2
        exit 2
    }
done

# The free-space rule from AGENTS.md: a wasm release build writes a lot into
# target/, and a full disk does not slow a build down, it stops it.
free_kb=$(df -Pk "$root" | awk 'NR == 2 { print $4 }')
if [ "${free_kb:-0}" -lt 5000000 ]; then
    say "cleaning target/debug/incremental first: ${free_kb} KB free is under the 5 GB floor"
    rm -rf target/debug/incremental
fi

say "building the bundle (cargo, wasm32, release)…"
started=$(date +%s)
cargo build -p op-host-web \
    --target wasm32-unknown-unknown --no-default-features --features canvaskit --release --locked

pkg="$out/pkg"
rm -rf "$pkg"
mkdir -p "$pkg"
wasm-bindgen --target web --out-dir "$pkg" \
    target/wasm32-unknown-unknown/release/op_host_web.wasm >/dev/null

# The runtime assets (fonts, previews, templates, icon catalogue) live in the
# tree and are staged by the same script CI uses, so the two cannot drift.
bash tools/stage-web-assets.sh "$pkg/assets" >/dev/null

# CanvasKit itself: the daemon resolves `canvaskit/` beside the bundle on this
# deployment, and a stale copy here is the classic "the page loaded an old
# renderer" afternoon.
if [ -d crates/op-host-web/assets/canvaskit ]; then
    cp -a crates/op-host-web/assets/canvaskit "$pkg/canvaskit"
fi

optimised=no
if command -v wasm-opt >/dev/null 2>&1; then
    say "optimising with wasm-opt -Oz…"
    # The same candidate flags the CI gate filters against, so an older binaryen
    # does not hard-fail on a feature name it does not know.
    flags=()
    help="$(wasm-opt --help 2>&1 || true)"
    for flag in --enable-bulk-memory --enable-bulk-memory-opt --enable-nontrapping-float-to-int; do
        printf '%s\n' "$help" | grep -qF -- "$flag" && flags+=("$flag")
    done
    wasm-opt "${flags[@]}" -Oz "$pkg/op_host_web_bg.wasm" -o "$pkg/op_host_web_bg.opt.wasm"
    cp "$pkg/op_host_web_bg.opt.wasm" "$pkg/op_host_web_bg.wasm"
    optimised=yes
fi

wasm_bytes=$(wc -c < "$pkg/op_host_web_bg.wasm" | tr -d ' ')
gz_bytes=$(gzip -c "$pkg/op_host_web_bg.wasm" | wc -c | tr -d ' ')
elapsed=$(( $(date +%s) - started ))

say "built in ${elapsed}s: $pkg"
say "  wasm   : $wasm_bytes bytes ($gz_bytes gzipped)"
say "  wasm-opt: $optimised$([ "$optimised" = no ] && printf ' (not on the PATH; -Oz removes what gzip already removes, so the download is the same size)')"
say ""
say "deploy it with:"
say "  bash deploy/web/deploy.sh --host root@html.norka.cc --dir $out --bundle-only"
