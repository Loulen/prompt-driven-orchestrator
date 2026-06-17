# Adversity suite — pérennisation sprint (#209)

> The non-regression suite that proves the sprint invariant end-to-end. Layer 5
> (agentic) per ADR 0004: each scenario file below is executed manually by an
> agent that drives the running app and emits the file's JSON verdict. This
> index is the single place that lists the full suite, maps each scenario to the
> bugs/issues it covers, and states the invariant the suite proves.

## The invariant

> A valid pipeline, launched on a healthy daemon, always reaches a clean
> terminal state (Completed, or Failed with a visible cause) without manual
> intervention — including if the daemon restarts, if a tmux session dies, or if
> an event arrives twice. Never a silent stall.

Default posture: **explicit fail-fast**. When the daemon detects an
unrecoverable situation, the node/run goes Failed with a readable cause; no
magical auto-repair. The only user-visible behaviour change is explicit errors
where there used to be silent blocks.

## Per-issue adversity scenarios (A–D)

| Scenario file | Pérennisation chantier / issue | Adversity proven |
|---|---|---|
| [`loop-entry-join-termination.md`](loop-entry-join-termination.md) | A — #210 (fixes #194, #199) | A bounded loop region closes when exhausted instead of starting a phantom lap; entry nodes are never re-spawned past `max_iter`; failed-iteration artifacts are never consumed downstream. |
| [`mid-run-edit-policy.md`](mid-run-edit-policy.md) | B/D — #211, #212 (fixes #206) | Launching a pipeline with a dangling edge port is refused with an explicit message and no run is created; a dangerous mid-run edit (changing the type of a running node) is rejected with a visible message and not persisted, while a safe edit in the same run still applies (ADR-0007 enforced at runtime). |
| [`manager-unstick-loop.md`](manager-unstick-loop.md) | D — #211 (and #152) | A stalled bounded loop region is surfaced (amber) and the Pipeline Manager can route it (bump / end) by id to drive the run to a clean terminal state without restarting the daemon. |
| [`process-lifecycle-resilience.md`](process-lifecycle-resilience.md) | C — #213 (fixes #202, #205) | A Running node whose tmux session dies is marked Failed with a session-named cause within one detector cycle; a nominal live-session node is never disturbed (no false positive); a node orphaned across a daemon restart is reconciled Failed at boot; a terminal node's session is reaped promptly and its pane snapshot keeps serving `/pane`; the freed admission slot is reusable. |

## Transverse scenarios (#214)

These prove behaviours no single issue can: they cross a full daemon restart or
combine several adversities in one run.

| Scenario file | Issue | Adversity proven |
|---|---|---|
| [`daemon-kill-mid-run.md`](daemon-kill-mid-run.md) | #214 | Killing the daemon mid-run and restarting it: a run whose node session survived resumes and completes normally (no false failure, no double-spawn); a run whose session died during the outage is reconciled terminal at boot — orphaned node Failed with a session-named cause **and**, when nothing remains schedulable, the **run itself** Failed with a visible run-level `run_stalled` cause. Never left `running` forever. |
| [`run-of-hell.md`](run-of-hell.md) | #214 | A single run takes every adversity at once: a duplicate completion is a no-op (#198), a forbidden mid-run edit is rejected with a message (#211), and a killed session makes the node Failed with a cause (#202). The run still reaches a clean terminal state, each incident having left a visible trace. |

## Run-level stall reconciliation (#214 added scope)

Boot recovery (#213) reconciled orphaned **nodes** but not the **run** level: a
run left `Running` with no live node and nothing schedulable would sit Running
forever — a silent run-level stall, a direct violation of the invariant. The
daemon now reconciles such a run to a terminal `Failed` state with a
`run_stalled` cause, both at boot **and** during the periodic stale sweep.
Covered by:

- `daemon-kill-mid-run.md` Part B (boot path) and `run-of-hell.md` step 10
  (periodic-sweep path) at Layer 5.
- `tests/process_lifecycle.rs::boot_recovery_reconciles_a_run_level_stall` and
  `::run_with_no_live_node_and_nothing_schedulable_is_reconciled_terminal` at
  the integration layer, plus the `run_stall_reason` unit tests in
  `crates/pdo-daemon/src/lib.rs` (pure decision logic).

## Running the suite

1. Run the full automated layer first — it is the green gate the L5 scenarios
   sit on top of:

   ```bash
   cargo test --workspace
   ```

2. Then execute each scenario file above against the running app (frontend +
   daemon) in the order: A–D scenarios, then the two transverse scenarios. Each
   emits its own `verdict` JSON. A single failed assertion in any file ⇒ the
   suite fails — do not half-pass.

## Bug coverage map

Every bug consolidated by #209 is covered by an automated test **and** an
adversity scenario:

| Bug | Where proven |
|---|---|
| #194 (failed-iter artifacts consumed) | `loop-entry-join-termination.md` + input-resolution unit tests |
| #195 (resume re-runs completed non-loop nodes) | resume integration tests |
| #196 (restart_node duplicate iteration) | restart integration tests |
| #197 (run status vs scheduler disagree) | `process-lifecycle-resilience.md` + scheduler/status unit tests |
| #198 (duplicate completion re-spawns downstream) | `run-of-hell.md` step 4 + transition-guard unit tests |
| #199 (phantom lap past max_iter) | `loop-entry-join-termination.md` |
| #201 (iter N+1 spawned while iter N running) | resume guard tests |
| #202 (dead-session node burns a slot) | `process-lifecycle-resilience.md` + `run-of-hell.md` |
| #203 / #207 (dead scheduler code / single input path) | input-resolution unit tests |
| #205 (terminal-node session not reaped) | `process-lifecycle-resilience.md` reap spot-check |
| #206 (dangling port discovered mid-run) | `mid-run-edit-policy.md` Part 1 |
| run-level silent stall (#214 added scope) | `daemon-kill-mid-run.md` Part B + `run-of-hell.md` step 10 |
