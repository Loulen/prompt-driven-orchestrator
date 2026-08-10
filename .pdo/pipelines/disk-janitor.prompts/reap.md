#!/usr/bin/env bash
# disk-janitor · reap node (#480, #128 Track A)
#
# This is a `script` node body (ADR-0017): the runtime runs it as
# `timeout 60s bash <this-file>`, then calls `pdo complete` on exit 0 or
# `pdo fail` otherwise. Do NOT call `pdo complete` here — the tail does it.
#
# All the policy + API work lives in the deterministic `pdo reap` subcommand
# (pure `reap_policy` + reqwest). `pdo reap` exits 0 even on partial progress
# (a wall-clock budget defers the rest to the next fire) and only fails when it
# cannot reach the daemon at all — so a genuine failure surfaces, but a slow
# batch never fails this Run and leaks its own worktree.
#
# PDO_DAEMON_URL is injected into every node session; PDO_OUTPUT_OUT is the
# absolute path this node writes its `out` artifact to (dir pre-created at spawn).
set -uo pipefail

report="$(pdo reap 2>&1)"
rc=$?

printf '%s\n' "$report"

{
  printf '# disk-janitor report\n\n'
  printf 'Exit code: %s\n\n' "$rc"
  printf '```\n%s\n```\n' "$report"
} > "$PDO_OUTPUT_OUT"

exit "$rc"
