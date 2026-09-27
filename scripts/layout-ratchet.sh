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
# frontend/src/components: 203 (#890, story #888) admits StatsAbsorption.tsx and its test —
#   the multi-select bar, the Combine modal, the `[⧉ N]` pill and the members modal of a
#   Stats Absorption (ADR-0077). StatsModal/StatsCharts render it; the grouping rule lives in
#   lib/statsAbsorption and the persistence in hooks/useStatsAbsorption.
# crates/pdo-daemon/src: 93 (#890, story #888) admits stats_absorption.rs — storing
#   absorptions and folding absorbed Pipelines into their absorber across every Stats tab. It
#   is ONE concern that stats.rs and stats_performance.rs consume, as stats_performance.rs
#   already sits beside stats.rs; lib.rs only wires the routes.
# frontend/src/components: 205 (#891, story #888; 204 + #899's AgentProfileModal.tsx once
#   main merged in) admits StatsAbsorptionsPanel.tsx — the
#   Settings › General › Stats absorptions list (every absorption, its origin, uncombine as
#   you go). SettingsSurface renders it; it reuses StatsAbsorption's UncombineButton and
#   lib/statsAbsorption, so only the Settings layout is new.
# frontend/src/components: 197 and crates/pdo-daemon/src: 89 (#916) — the platform
#   context moved out: its components to frontend/src/platform/, its modules to
#   crates/pdo-platform. contexts.rs is the one file admitted in the legacy crate: the
#   seam the composition root wires the extracted contexts through.
# frontend/src/components: 199 and crates/pdo-daemon/src: 81 (#919) ratchet down: the
#   harness context moved out — HarnessSelect, ModelPicker, EffortPicker and their tests to
#   frontend/src/harness/components/, the twelve harness modules to crates/pdo-harness.
# frontend/src/components: 198 and crates/pdo-daemon/src: 77 (#921) ratchet down: the
#   sandbox context moved out — StagingProfilesPanel to frontend/src/sandbox/components/, the
#   five sandbox modules to crates/pdo-sandbox; run_sandbox.rs keeps the Run's side of it
#   (building a Run's sandbox context from its state) until the run context moves.
# frontend/src/components: 191 and crates/pdo-daemon/src: 79 — #916 and #919 combined.
# frontend/src/components: 190 and crates/pdo-daemon/src: 73 — #916, #919 and #921 combined.
# frontend/src/components: 194 and crates/pdo-daemon/src: 89 (#917, spec #913) ratchet down:
#   the workspace context moved out — project_store.rs, provisioning.rs, worktree_ops.rs and
#   repo_scaffold.rs to crates/pdo-workspace; ProjectEditModal, SourceBranchField,
#   ProvisioningRulesEditor, PersistedProvisioningEditor, FsExplorerModal, RepoCombobox and
#   their tests to frontend/src/workspace/components.
# frontend/src/components: 180 and crates/pdo-daemon/src: 73 — #917, #916 and #919 combined.
# frontend/src/components: 179 and crates/pdo-daemon/src: 69 — #916, #917, #919 and #921 combined.
# frontend/src/components: 195 and crates/pdo-daemon/src: 78 (#920) ratchet down: the
#   session context moved out — TmuxTerminal, RunShellModal and their tests to
#   frontend/src/session/components/, tmux_session_manager, pty_bridge, shared_terminal and
#   declared_wait to crates/pdo-session. The legacy side of its ports is one new module,
#   contexts.rs, plus the session_ports/ directory (not a direct file).
# frontend/src/components: 176 and crates/pdo-daemon/src: 69 — #920, #917, #916 and #919
#   combined.
# frontend/src/components: 175 and crates/pdo-daemon/src: 65 — #916, #917, #919, #920 and
#   #921 combined.
# frontend/src/components: 168 and crates/pdo-daemon/src: 68 (#918, spec #913) ratchet down:
#   the skill context moved out — skill_bank, skill_import, skill_selection, skill_sidecar and
#   skill_delivery to crates/pdo-skill; the skill bank UI to frontend/src/skill/.
# frontend/src/components: 163 and crates/pdo-daemon/src: 60 — #916, #917, #918, #919, #920
#   and #921 combined.
# frontend/src/components: 154 and crates/pdo-daemon/src: 52 (#922, spec #913) ratchet down:
#   the pipeline context moved out — pipeline, pipeline_semantics (deleted: test-only),
#   pipeline_migrator, pipeline_watcher, portable_pipeline, library_store,
#   workflow_importer, mutation_validator and frontmatter_parser to crates/pdo-pipeline;
#   EditCanvas, PipelineInfoPanel and their tests to frontend/src/pipeline/components/.
#   pipeline_ports.rs is admitted: the legacy side of the context's ports (the Runs, Stats).
# frontend/src/components: 152 and crates/pdo-daemon/src: 36 (#923, spec #913) ratchet down:
#   the run context's foundation moved out — event_log, graph_resolver, loop_region,
#   edge_router, switch_router, condition, variable_resolver, transition_guard, admission,
#   blackboard, input_resolution, retry_verdict, restart_verdict, reap_policy, auto_fail,
#   completion_refusal and child_awaiting to crates/pdo-run; SessionCounter and its test to
#   frontend/src/run/components/. run_ports.rs is admitted: the legacy side of the context's
#   ports (what a Run's detail reads from disk).
# crates/pdo-daemon/src: 21 (#924, spec #913) ratchet down: the run context's scheduler and
#   node lifecycle moved out — scheduler, scheduler_dispatcher, scheduler_interpreter,
#   run_advance, run_command, node_primitives, node_spawn, node_io_resolver, prompt_augmenter,
#   outputs_validator, merge_action, recovery, boot_recovery, stale_detector and
#   repo_edit_refusal to crates/pdo-run; run_sandbox.rs keeps only the sandbox context's routes
#   and profile referents, renamed sandbox_ports.rs.
# frontend/src/components: 131 and crates/pdo-daemon/src: 19 (#925, spec #913) ratchet down:
#   the Run's surfaces moved out — run_cost and run_refs to crates/pdo-run (price_table keeps
#   only the price sync); NewRunModal, NodeDetailPanel, MarkdownArtifactModal,
#   OrchestrationTab, RunFilters, runFilter, ForgetRunModal, CleanupConfirmModal,
#   RepositoriesSection, SecondaryRepoRow and their tests to frontend/src/run/.
# frontend/src/components: 124 and crates/pdo-daemon/src: 15 (#928, spec #913) ratchet down:
#   the stats context moved out — stats, stats_performance, stats_absorption, distribution
#   and price_table to crates/pdo-stats; StatsCharts (split by tab), StatsAbsorption,
#   StatsAbsorptionsPanel, StatsModal and their tests to frontend/src/stats/. stats_ports.rs
#   is admitted: the legacy side of the context's ports (the Triggers' fires).
BASELINES='
frontend/src/components 124
crates/pdo-daemon/src 15
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
