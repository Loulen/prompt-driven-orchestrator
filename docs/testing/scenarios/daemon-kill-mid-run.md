# Scenario — `daemon-kill-mid-run`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format at the bottom. This is a **transverse** scenario
> for the pérennisation sprint (#214 / #209): no single issue proves it. It
> asserts the sprint invariant across a full daemon restart:
>
> > A valid pipeline on a healthy daemon always reaches a clean terminal state
> > (Completed, or Failed with a visible cause) without manual intervention —
> > including across a daemon restart. Never a silent stall.
>
> Two behaviours are proven on the **same** restart path:
> - **Part A — nominal resume**: a run whose node session survives the restart
>   resumes and completes normally (no false failure, no double-spawn).
> - **Part B — fail-fast reconciliation**: a run whose node session died during
>   the downtime is reconciled to a clean terminal state at boot (the orphaned
>   node Failed with a session-named cause, and — when nothing remains
>   schedulable — the **run itself** Failed with a run-level cause, #214). The
>   run never sits `running` forever.

## Setup

- Maestro daemon running on the user's repo. The agent controls the daemon
  process lifecycle for this scenario (it kills and restarts it), so prefer a
  **dedicated daemon on a free port** rather than the user's live dev daemon.
  Launch it with `maestro daemon` and record its PID and URL.
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- `claude` available on `PATH`.
- A two-worker pipeline `daemon-kill-scenario.yaml` in `.maestro/pipelines/`.
  If absent, the agent creates it before driving the UI:

  ```yaml
  name: daemon-kill-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: survivor
      name: survivor
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 200, y: 0 }
    - id: victim
      name: victim
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 200, y: 200 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 500, y: 100 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: survivor, port: in }
    - source: { node: start, port: user_prompt }
      target: { node: victim, port: in }
    - source: { node: survivor, port: out }
      target: { node: end, port: result }
  ```

  And both prompts under `.maestro/pipelines/daemon-kill-scenario.prompts/`:

  - `survivor.md`:

    ```
    Sleep for 5 minutes (run the shell command `sleep 300`) and only THEN write
    `# Out` to `.maestro/artifacts/survivor/iter-1/out/output.md` and call
    `maestro complete`. Do nothing else in the meantime.
    ```

  - `victim.md`:

    ```
    Sleep for 5 minutes (run the shell command `sleep 300`). Do nothing else.
    ```

## Part A — nominal resume after a daemon restart

1. Open the UI; confirm **`Daemon: connected`**.
2. Launch a run of `daemon-kill-scenario` with input `daemon-kill`. Capture the
   `run_id`. Within ~2 s both `survivor` and `victim` animate to **`running`**.
3. Confirm both sessions exist (tmux runs on the daemon's per-port socket
   `maestro-<port>`):

   ```bash
   tmux -L maestro-<port> ls | grep "maestro-<run_id>-survivor-iter-1"
   tmux -L maestro-<port> ls | grep "maestro-<run_id>-victim-iter-1"
   ```

4. **Kill the daemon process** (models a crash / restart):
   `kill <daemon_pid>`. Do **not** touch the tmux sessions — they are detached
   and survive the daemon's death. Confirm the daemon is gone (HTTP to its URL
   fails / `kill -0 <pid>` reports no process).
5. **Restart** the daemon (`maestro daemon` on the same port and repo). Boot
   recovery runs once at startup.
6. Wait for `Daemon: connected` in the UI, reload the run view, and assert for
   the `survivor` node:
   - Its session is still alive (step 3 command still finds it).
   - The node is **still `running`** — boot recovery did **not** falsely fail a
     node whose session survived (no false positive), and it was **not**
     re-spawned a second time (a single `maestro-<run_id>-survivor-iter-1`
     session, no `iter-2`).
7. Unblock `survivor`: from its pane interrupt the sleep and let it write the
   artifact + `maestro complete`, OR write the output and Mark complete:

   ```bash
   mkdir -p .maestro/runs/<run_id>/worktree/.maestro/artifacts/survivor/iter-1/out
   echo '# Out' > .maestro/runs/<run_id>/worktree/.maestro/artifacts/survivor/iter-1/out/output.md
   ```

   (kill the `victim` session first so the run can settle on the `survivor`
   path; `victim` failing is exercised in Part B — for Part A just stop it).
   Assert the `survivor` node reads **`completed`** and the run reaches a clean
   terminal state (the End `result` port fires; the run is **`completed`**, not
   stuck `running`).

## Part B — fail-fast reconciliation when a session dies during downtime

8. Launch a **second** run of `daemon-kill-scenario` with input
   `daemon-kill-fail`. Capture `run_id_2`. Both nodes reach **`running`**.
9. **Kill the daemon process** (`kill <daemon_pid>`).
10. While the daemon is down, **kill both node sessions** (models the tmux
    server collapsing during the outage, #202):

    ```bash
    tmux -L maestro-<port> kill-session -t "maestro-<run_id_2>-survivor-iter-1"
    tmux -L maestro-<port> kill-session -t "maestro-<run_id_2>-victim-iter-1"
    ```

11. **Restart** the daemon. Boot recovery runs.
12. Reload the run view for `run_id_2` and assert:
    - Both `survivor` and `victim` are **`failed`** (red), **not** stuck
      `running`. Selecting each, the inspector shows a failure cause containing
      `session` and the dead session name
      (`maestro-<run_id_2>-survivor-iter-1` / `…-victim-iter-1`).
    - The **run** is reconciled to a clean terminal state: it is **`failed`**
      (not perpetually `running`). The run-level cause is visible and explains
      the stall — it contains `run_stalled` and names the blocking node(s)
      (#214). The End `result` port is **not** marked received (the run did not
      falsely complete).
13. Cross-check the run never silently stalled: there is no run left `running`
    with zero live nodes. Over HTTP, `GET /runs/<run_id_2>` reports
    `"status": "failed"` and a failure reason.

## Cleanup

- Kill any sessions still alive for both runs:
  `tmux -L maestro-<port> kill-session -t "maestro-<run_id>-survivor-iter-1"`
  (and the `victim` / `run_id_2` variants).
- Remove worktrees: `git worktree remove --force .maestro/runs/<run_id>/worktree`
  per run.
- Stop the dedicated daemon started for this scenario.
- Delete `.maestro/pipelines/daemon-kill-scenario.yaml` and its `.prompts/` dir
  if the agent created them in Setup.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "A4: daemon process killed mid-run with both node sessions left alive",
    "A6: after restart, survivor's surviving session still running, not falsely failed, not re-spawned",
    "A7: survivor completed and the run reached a clean terminal Completed state",
    "B10: both node sessions killed while the daemon was down",
    "B12: after restart, both nodes Failed with session-named causes; the RUN was reconciled to Failed with a visible run_stalled cause, not left running forever",
    "B13: GET /runs reports status=failed with a reason; no run left running with zero live nodes"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
