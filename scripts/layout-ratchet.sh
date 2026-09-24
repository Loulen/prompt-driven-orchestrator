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
# frontend/src/components: 185 and crates/pdo-daemon/src: 83 reconcile a drift
#   already present on main (the CI ratchet is red there since the run behind
#   #718); #722 adds no top-level file to either directory, the flat counts are
#   re-admitted as-is to green the gate again. Ratchet down when tidied.
# frontend/src/components: 188 and crates/pdo-daemon/src: 87 (#750) — 188 re-admits
#   three top-level components that landed on main after the previous reconcile
#   (the review UI itself lives under components/review/, not counted); 87 admits
#   review_comments.rs, the review-comment concern (ids, excerpt, batch message,
#   projection fold — ADR-0067 §2). Ratchet down when tidied.
# frontend/src/components: 190 re-admits ThemeSelect.tsx and its test (#767, light
#   theme), which landed on main past the 188 baseline and left the gate red. Ratchet
#   down when tidied.
# frontend/src/components: 191 admits OrchestrationTab.test.tsx (#783) — the tab
#   and its shared pastilles had no component test; the stale pill needed one.
# frontend/src/components: 194 (#802) admits SourceBranchField.tsx and its test, and
#   re-admits one file that landed on main past the previous baseline. The field is
#   ONE new concern — choosing a source branch and seeing its freshness — and it is
#   deliberately one sibling rather than two (quick pick + sync button): the primary
#   field and every secondary row use the same component, which is the only thing
#   that keeps them from drifting the way #571 documented. Its pure logic went into
#   the EXISTING lib/branchSelect.ts, and its fetch/race rules into hooks/ (neither
#   is a watched directory), so nothing that could live in a sibling opened a file.
# crates/pdo-daemon/src: 90 reconciles a drift already present on main (the CI
#   ratchet was red there before #802). #802 itself adds NO daemon file: the fetch
#   verb, the enriched branch list and their tests all folded into lib.rs beside
#   `list_branches`, which already owned the concern. Ratchet down when tidied.
# crates/pdo-daemon/src: 91 (#824, release 1.97.0) admits repo_scaffold.rs — the generic
#   "create a git repository from scratch" verb the *First run* tour calls for its
#   training repo. It is ONE new concern with three outcomes and no existing sibling
#   owns it (worktree/branch modules operate on an existing repo, never create one);
#   folding it into lib.rs would only hide 400 lines of git plumbing there. The
#   integration PRs landed past the 90 baseline and left main red.
# frontend/src/components: 200 (#840 story, integration/840-canvas-edges). Re-admits the
#   three per-ticket component tests #843/#845 landed past 194 (EdgeDetailPanel.outputs843,
#   EdgeDetailPanel.display845, OrthogonalEdge.labels845) and admits #844's net +3:
#   NodeRimHandles.tsx replaces OutputPortDot.tsx (the rim IS the connection source now,
#   ADR-0072), WiringGridOverlay.tsx + its test draw the wiring lattice, and
#   OrthogonalEdge.wiring844 / EditCanvas.contextMenu are the FP-backing tests. The pure
#   geometry went into lib/ (anchorSide, wiringGrid), not a watched directory.
# frontend/src/components: 201 (#877, release 1.103.0) admits GridSizePicker.tsx — the S/M/L
#   wiring grid size radio group, rendered by BOTH SettingsSurface (global default) and
#   PipelineInspector (per-pipeline override, ADR-0076). One shared control rather than two
#   copies; the step arithmetic lives in lib/wiringGrid and the resolution in hooks/useWiringGrid.
# crates/pdo-daemon/src: 92 (#869, release 1.98.0) admits shared_terminal.rs — the shared
#   terminal presence registry (one pilot per tmux session, read-only spectators, role
#   messages, `ignore-size` switching; ADR-0075). It is ONE new concern that #870 (take
#   control) extends; pty_bridge.rs only moves bytes and keeps its size, lib.rs only wires it.
# frontend/src/components: 202 (#899, release 1.103.2) admits AgentProfileModal.tsx — the
#   agent profile editor moved out of AgentProfilesPanel's inline form into ONE modal shared
#   by the row's Edit button and New profile (like ProjectEditModal). The panel keeps the list.
BASELINES='
frontend/src/components 202
crates/pdo-daemon/src 92
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
