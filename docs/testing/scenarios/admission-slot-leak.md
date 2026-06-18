# Scenario — `admission-slot-leak`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format at the bottom. This scenario proves the fix for
> **#215** (terminal run with a session-holding node leaks an admission slot
> forever) end-to-end, the way a human would observe it: through the UI status
> bar, the live tmux sessions, and the daemon's `/sessions` + `/runs` HTTP
> surface. It asserts the invariant:
>
> > A node that stays `Running`/`AwaitingUser` after its **Run** has reached a
> > terminal status (`Completed`/`Failed`/`Halted`) must never count as a live
> > NodeRun session, and must be reconciled to a terminal node status at boot.
> > A terminal run can never permanently subtract from the global session cap.
>
> Two behaviours are proven on the **same** repro:
> - **Part A — accounting fix (required #215.1)**: when a run fails while a
>   sibling node is still `running`, `/sessions` `live` must *not* count that
>   sibling. The phantom slot is gone the instant the run goes terminal — no
>   reboot needed.
> - **Part B — boot-recovery state-consistency (recommended #215.2)**: after a
>   daemon restart, no terminal run is left with a session-holding node; the
>   leftover node is reconciled to `Failed` with a visible cause, and `live` is
>   0 with no live tmux sessions.

## Background (so the agent reads the evidence correctly)

- **`RunStatus` has no `Stopped` variant.** The run-level terminal set is
  `Completed` / `Failed` / `Halted` (plus `Archived`). `Stopped` is a *node*
  status. "Live run" = `Running` / `AwaitingUser` / `Paused`.
- **How the phantom arises (dominant real path):** when one node fails,
  fail-fast fails the **whole run** (`RunFailed`), but only the *failing* node's
  tmux session is reaped — sibling nodes that were still `running` are left
  projected as `Running` with their sessions alive. That sibling is the
  session-holding node inside a now-terminal run.
- **The accounting fix is deliberately a projection filter, not a reap.** Right
  after the run fails, the sibling's tmux session may still be *physically
  alive* for a short window (it is reaped at next boot / by the orphan sweep).
  The fix excludes it from the **count** because a terminal run will never spawn
  more work, so its lingering session must not block *new* runs from admission.
  Surfacing a still-alive sibling session in `anomalies` is expected, not a
  failure (see Part A step A6).

## Setup

- PDO daemon running. The agent controls the daemon process lifecycle for
  this scenario (it kills and restarts it in Part B), so use a **dedicated
  daemon on a free port** — NOT the user's live dev daemon. Launch it with
  `pdo daemon --port <free_port>` (or `PDO_PORT=<free_port> pdo
  daemon`) against a working repo, and record its PID, port, and URL
  (`http://127.0.0.1:<free_port>`). All tmux calls use the per-port socket
  `pdo-<free_port>`.
- Confirm a clean baseline: `curl -fs http://127.0.0.1:<free_port>/sessions`
  returns `{"cap":N,"live":0,...}` with `live` == 0 (no other live work on this
  dedicated daemon). Record `cap` (it comes from `PDO_SESSION_CAP` or
  defaults to 10 — its value does not matter to this scenario, only `live`
  does).
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright MCP
  is a fallback.
- `claude` and `pdo` available on `PATH` (the node sessions call
  `pdo fail`).
- A fan-out pipeline `admission-slot-leak.yaml` in `.pdo/pipelines/`. If
  absent, the agent creates it before driving the UI:

  ```yaml
  name: admission-slot-leak
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: faulter
      name: faulter
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 220, y: 0 }
    - id: sleeper
      name: sleeper
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 220, y: 200 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 520, y: 100 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: faulter, port: in }
    - source: { node: start, port: user_prompt }
      target: { node: sleeper, port: in }
    - source: { node: sleeper, port: out }
      target: { node: end, port: result }
  ```

  And both prompts under `.pdo/pipelines/admission-slot-leak.prompts/`:

  - `faulter.md`:

    ```
    Immediately run the shell command:
    `pdo fail --reason "intentional fault: fail the run while sibling sleeper is still running"`
    Do nothing else — do not write any artifact, do not call `pdo complete`.
    ```

  - `sleeper.md`:

    ```
    Run the shell command `sleep 300`. Do nothing else in the meantime.
    ```

## Part A — the phantom slot is freed the instant the run goes terminal (#215.1)

1. Open the UI; confirm **`Daemon: connected`**. Read the bottom status-bar
   session counter (`[data-testid="session-counter"]`) — it should read
   **`0 / <cap> sessions`**. Screenshot.
2. Launch a run of `admission-slot-leak` with input `leak-repro`. Capture
   `run_id`. Within ~2 s both `faulter` and `sleeper` animate to **`running`**.
3. Confirm both node sessions exist:

   ```bash
   tmux -L pdo-<port> ls | grep "pdo-<run_id>-faulter-iter-1"
   tmux -L pdo-<port> ls | grep "pdo-<run_id>-sleeper-iter-1"
   ```

4. Within a few seconds `faulter` runs `pdo fail`, which fails the **whole
   run** (fail-fast). Assert via HTTP `GET /runs/<run_id>`:
   - run `"status": "failed"`,
   - `faulter` node **`failed`**,
   - `sleeper` node **still `running`** — this is the session-holding node inside
     a now-terminal run (the phantom). In the UI, `faulter` is red and `sleeper`
     is still animating `running`. Screenshot.
5. **Accounting assertion (the #215.1 fix).** `GET /sessions` → `live` is back to
   the **baseline `0`** — the `running` `sleeper` in the `failed` run is **not**
   counted. The UI status-bar counter reads **`0 / <cap> sessions`**, not
   `1 / <cap>`. Screenshot. (Pre-fix this read `1` and would stay `1` forever,
   even after the run is long dead — the leak.)
6. Cross-check physical reality (expected `anomaly`, not a failure):

   ```bash
   tmux -L pdo-<port> ls
   ```

   `faulter`'s session is **gone** (reaped on its own failure); `sleeper`'s
   session is **still alive** (genuinely `sleep 300`). So one real NodeRun tmux
   session physically exists, yet `/sessions` correctly reports `live: 0` because
   that session belongs to a terminal run. Record this in `anomalies`: the
   sibling session lingers until reboot (it is reaped in Part B); closing that
   live-window residual is the runtime cascade-reap follow-up, out of scope for
   #215.

## Part B — boot recovery reconciles the terminal-run phantom (#215.2)

7. **Kill the daemon process** (`kill <daemon_pid>`). Confirm it is gone
   (HTTP to its URL fails / `kill -0 <pid>` reports no process).
8. While the daemon is down, **kill `sleeper`'s session** (models the tmux
   server collapsing during the outage, and matches the issue's real fixture
   where zero tmux sessions remain at boot):

   ```bash
   tmux -L pdo-<port> kill-session -t "pdo-<run_id>-sleeper-iter-1"
   ```

9. **Restart** the daemon (`pdo daemon --port <port>` on the same repo).
   Boot recovery runs once at startup.
10. Wait for `Daemon: connected`, reload the run view for `run_id`, and assert:
    - `sleeper` is now **`failed`** (red), **not** stuck `running`. Selecting it,
      the inspector shows a failure cause that names the situation — it contains
      `run terminal` and/or `session-holding` (and may name the session
      `pdo-<run_id>-sleeper-iter-1`). Screenshot.
    - The **run** is still **`failed`** (boot recovery did not resurrect it or
      flip it to any other status).
    - The End `result` port is **not** marked received (the run did not falsely
      complete).
11. **Accounting after boot.** `GET /sessions` → `live: 0`. `tmux -L
    pdo-<port> ls` shows **no** `pdo-<run_id>-*` NodeRun session. No
    terminal run is left with a `running`/`awaiting_user` node: scan `GET /runs`
    and, for any terminal run, `GET /runs/<id>` shows zero session-holding nodes.
12. **Idempotency.** Re-read `GET /runs/<run_id>` after a few seconds — the
    `sleeper` node stays `failed` with a single, stable cause (boot recovery
    re-running must not double-fail it or churn the status). If the harness
    exposes a boot-recovery re-tick, invoking it leaves state unchanged.

## Cleanup

- Kill any sessions still alive for the run:
  `tmux -L pdo-<port> kill-session -t "pdo-<run_id>-sleeper-iter-1"`
  (and the `faulter` variant) — ignore "session not found".
- Archive the run:
  `curl -X POST http://127.0.0.1:<port>/runs/<run_id>/commands -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'`
  (or `git worktree remove --force .pdo/runs/<run_id>/worktree`).
- Stop the dedicated daemon started for this scenario (`kill <daemon_pid>`).
- Delete `.pdo/pipelines/admission-slot-leak.yaml` and its `.prompts/` dir if
  the agent created them in Setup.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "A1: fresh dedicated daemon, /sessions live=0, status-bar reads '0 / <cap> sessions' (screenshot)",
    "A2-A3: run launched, faulter+sleeper reached running, both tmux sessions present",
    "A4: faulter ran `pdo fail` -> GET /runs shows run=failed, faulter=failed, sleeper STILL running (the phantom session-holder) (screenshot)",
    "A5: GET /sessions live=0 and status-bar reads '0 / <cap>' — the running sleeper in the failed run is NOT counted (the #215.1 accounting fix) (screenshot)",
    "B8-B10: daemon killed, sleeper session killed during downtime, daemon restarted; after boot recovery sleeper=failed with a cause containing 'run terminal'/'session-holding', run still failed, End result not received (screenshot)",
    "B11: GET /sessions live=0, no pdo-<run_id>-* tmux session remains, no terminal run has a running/awaiting_user node",
    "B12: re-reading GET /runs is stable — sleeper stays failed with one cause (boot recovery is idempotent)"
  ],
  "anomalies": [
    "A6: sleeper's tmux session physically lingered after the run went terminal (run-terminal reaps only the failing node, not siblings) — correctly excluded from the count and reaped at the Part B reboot; closing this live-window residual is the runtime cascade-reap follow-up, out of #215 scope"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. If `faulter`
failing does not transition the **run** to `failed` (so no phantom is produced),
report it in `anomalies` and `FAIL` Part A's precondition rather than fabricating
the state.
