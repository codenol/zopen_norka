#!/usr/bin/env bash
# CLAUDE.md's 800-line-per-file convention, enforced.
#
# The cap had drifted to eight violations by 2026-08-27 (paint.rs 911,
# tests.rs 894, lib.rs 868, …) precisely because nothing checked it — the
# doc claimed "zero violations" while the tree said otherwise. Splitting is
# a spine plus sibling files with re-exports keeping import paths stable;
# see CLAUDE.md's "Code Style" section.
#
# ## The ceiling table, and why a gate needs one
#
# One file in this tree cannot come under the cap by moving code:
# `op-editor-core/src/editor_ui_state.rs` is 245 field declarations and 568
# lines of documentation for them, with no function left to move — every `impl`
# is already a sibling. Under 800 lines means either grouping those fields into
# sub-structs, which rewrites 7 339 `editor_ui.<field>` accesses across 720
# files in 15 crates (measured, issue #158), or pulling the field docs into
# `include_str!`, which is documentation extraction rather than a split.
#
# An all-or-nothing gate then has two bad outcomes: it stays red forever, so
# `rust-check` never reaches `cargo build` and `cargo test` and the whole job's
# signal is lost (issue #223 — which is what happened), or somebody adds
# `--allow` and the tripwire stops existing. So a file may sit over the cap ONLY
# by appearing in the table below, with the number it may reach and the issue
# that owns the reason. The gate still fails when it grows past that ceiling,
# and still fails for every file that is not in the table.
#
# Adding a row is a decision, not a workaround: a row without an issue number,
# or for a file that could simply be split, is the drift this script exists to
# catch. Removing a row is what a split does.
set -euo pipefail

cap=800
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# path|ceiling|issue. The ceiling is the file's size rounded up to the next
# multiple of ten: ordinary editing does not trip it, growth does.
ceiling_table() {
  cat <<'TABLE'
crates/op-editor-core/src/editor_ui_state.rs|960|158
TABLE
}

sizes="$(find crates -name '*.rs' -not -path '*/target/*' -exec wc -l {} + \
  | awk '$2 != "total" { print $1 " " $2 }')"

failures=""
while read -r lines path; do
  [[ -n "${path:-}" ]] || continue
  [[ "$lines" -gt "$cap" ]] || continue

  row="$(ceiling_table | awk -F'|' -v p="$path" '$1 == p { print $2 " " $3 }')"
  if [[ -z "$row" ]]; then
    failures+="  ${lines} ${path}"$'\n'
    continue
  fi
  allowed="${row%% *}"
  issue="${row##* }"
  if [[ "$lines" -gt "$allowed" ]]; then
    failures+="  ${lines} ${path} — ceiling ${allowed} (issue #${issue}); split it or raise the ceiling deliberately"$'\n'
  fi
done <<< "$sizes"

if [[ -n "$failures" ]]; then
  echo "::error::files exceed the ${cap}-line cap (CLAUDE.md Code Style):"
  printf '%s' "$failures"
  echo "  (a file that cannot be split carries a ceiling row in $0; every row needs an issue)"
  exit 1
fi

echo "file line cap OK (no crates/**/*.rs over ${cap} lines without a recorded ceiling)"
recorded="$(ceiling_table | awk -F'|' '/^[a-z]/ { print "  " $1 " (ceiling " $2 ", issue #" $3 ")" }')"
if [[ -n "$recorded" ]]; then
  echo "recorded ceilings:"
  printf '%s\n' "$recorded"
fi
