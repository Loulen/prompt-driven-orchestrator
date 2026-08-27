#!/usr/bin/env bash
# Layout ratchet (#494): the number of direct tracked files in each watched
# directory must never grow past its baseline. Rule: docs/agents/module-layout.md.
# When you tidy a directory below its baseline, LOWER the number here in the
# same commit (ratchet down).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# <directory>  <max direct tracked files>
# frontend/src/components: 146 reconciles a preexisting drift (the tree already
#   held 146 direct files before #598; the baseline had lagged at 144). Ratchet
#   down when the flat list is genuinely tidied.
# crates/pdo-daemon/src: 68 admits the three pure modules the `copilot` spec
#   (#612) added, each a distinct concern with its own parser or renderer rather
#   than drift: copilot_journal.rs (#615, the event-journal reader),
#   harness_catalogue.rs (#616, the `--help` catalogue parser), and
#   harness_support.rs (#617, the published capability × harness matrix). They
#   mirror their standalone-module siblings recovery.rs / auto_fail.rs /
#   retry_verdict.rs / restart_verdict.rs. 65 previously admitted recovery.rs
#   (#599) on the same rationale, and 64 auto_fail.rs (#598) before it.
BASELINES='
frontend/src/components 152
crates/pdo-daemon/src 68
'

fail=0
while read -r dir max; do
  [ -n "$dir" ] || continue
  count=$(git ls-files -- "$dir" | sed "s|^$dir/||" | grep -cv '/' || true)
  if [ "$count" -gt "$max" ]; then
    echo "FAIL: $dir has $count direct files (baseline: $max)." >&2
    echo "  A new top-level file widens the flat list. Fold the concern into an" >&2
    echo "  existing sibling module instead — see docs/agents/module-layout.md." >&2
    echo "  If a new direct file is genuinely the right shape, raise the baseline" >&2
    echo "  in scripts/layout-ratchet.sh and say why in the PR." >&2
    fail=1
  elif [ "$count" -lt "$max" ]; then
    echo "note: $dir at $count < baseline $max — ratchet down: lower it in scripts/layout-ratchet.sh."
  else
    echo "ok: $dir ($count/$max)"
  fi
done <<EOF
$BASELINES
EOF
exit "$fail"
