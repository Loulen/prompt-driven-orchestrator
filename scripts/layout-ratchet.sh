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
# frontend/src/components: 157 admits the declarative provisioning editor,
#   persisted-scope host, and component test. Provisioning is a new concern shared
#   by four existing surfaces, so one reusable sibling avoids divergent editors.
# crates/pdo-daemon/src: 75 admits the provisioning resolver/provisioner sibling;
#   it owns matching, persistence, preview, and filesystem effects at one seam.
#   71 admitted three pure Performance modules from #585:
#   context_peak.rs parses harness telemetry, distribution.rs owns R-7 summary
#   statistics, and stats_performance.rs aggregates the HTTP response. Keeping
#   these concerns separate follows the sibling-module rule. 68 previously
#   admitted the three pure modules from the `copilot` spec (#612).
BASELINES='
frontend/src/components 157
crates/pdo-daemon/src 75
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
