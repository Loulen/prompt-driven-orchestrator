# Scenario — `run-of-hell`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format at the bottom. This is the **transverse stress**
> scenario for the pérennisation sprint (#214 / #209): a single run takes
> **every** adversity at once and must still reach a clean terminal state, with
> each incident leaving a **visible cause**. It proves the sprint invariant
> holds under combined stress, not just one fault at a time:
>
> > A valid pipeline on a healthy daemon always reaches a clean terminal state
> > (Completed, or Failed with a visible cause) without manual intervention.
> > Never a silent stall.
>
> The three adversities injected, in order:
> - **Duplicate completion** on an already-completed node is a **no-op** (#198):
>   downstream is not re-spawned, no second iteration appears.
> - **Forbidden mid-run edit** (changing the type of a running node) is
>   **rejected with a visible message** (#211 / ADR-0007), and not persisted.
> - **Session killed** out-of-band makes the node **Failed with a cause naming
>   the session** (#202/#213); the run then settles to a clean terminal state
>   (#214) — never stuck `running`.

## Setup

- Maestro daemon running on the user's repo (default `http://127.0.0.1:5172`;
  the live dev daemon uses `6172`). tmux runs on the per-port socket
  `maestro-<port>`.
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- `claude` available on `PATH`.
- A three-worker pipeline `run-of-hell-scenario.yaml` in `.maestro/pipelines/`.
  If absent, the agent creates it before driving the UI:

  ```yaml
  name: run-of-hell-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 150 }
    - id: alpha
      name: alpha
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 200, y: 0 }
    - id: beta
      name: beta
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 200, y: 150 }
    - id: gamma
      name: gamma
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 200, y: 300 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 500, y: 150 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: alpha, port: in }
    - source: { node: start, port: user_prompt }
      target: { node: beta, port: in }
    - source: { node: start, port: user_prompt }
      target: { node: gamma, port: in }
    - source: { node: alpha, port: out }
      target: { node: end, port: result }
  ```

  Prompts under `.maestro/pipelines/run-of-hell-scenario.prompts/`, each keeping
  the session busy (`Sleep for 5 minutes (run \`sleep 300\`). Do nothing else.`)
  for `alpha.md`, `beta.md`, `gamma.md`. Only `alpha` feeds `End`; `beta` and
  `gamma` are the nodes we abuse.

## Steps

1. Open the UI; confirm **`Daemon: connected`**.
2. Launch a run of `run-of-hell-scenario` with input `run-of-hell`. Capture the
   `run_id`. Within ~2 s all three of `alpha`, `beta`, `gamma` reach
   **`running`**. The run is **`running`**.

### Adversity 1 — duplicate completion is a no-op (#198)

3. Complete `alpha` cleanly. Write its artifact and POST `done`:

   ```bash
   mkdir -p .maestro/runs/<run_id>/worktree/.maestro/artifacts/alpha/iter-1/out
   echo '# Out' > .maestro/runs/<run_id>/worktree/.maestro/artifacts/alpha/iter-1/out/output.md
   curl -s -X POST "http://127.0.0.1:<port>/runs/<run_id>/nodes/alpha/done" \
     -H 'content-type: application/json' -d '{}'
   ```

   Assert `alpha` reads **`completed`** and the End `result` port fires (the
   run's terminal completion now depends only on `beta`/`gamma` settling).
4. **Inject a duplicate completion**: POST `done` for `alpha` **again**:

   ```bash
   curl -s -o /dev/null -w '%{http_code}\n' -X POST \
     "http://127.0.0.1:<port>/runs/<run_id>/nodes/alpha/done" \
     -H 'content-type: application/json' -d '{}'
   ```

   Assert it is a **no-op**: the call does not create a second iteration of
   `alpha` (no `iter-2`), does not re-spawn any downstream node, and the run
   state is unchanged. Over HTTP `GET /runs/<run_id>` shows `alpha` still a
   single completed iteration. (A 200 no-op or an explicit rejection both pass;
   what must NOT happen is a duplicate spawn or a state regression.)

### Adversity 2 — forbidden mid-run edit is rejected with a message (#211)

5. Click the still-running `beta` node to open the **Node Inspector**. Switch
   its type from **doc-only** to **code-mutating** and save (`Ctrl+S` / Save).
6. Assert the save is **rejected with a visible message**: the save-error modal
   (`data-testid="save-error-modal"`) appears and its message
   (`data-testid="save-error-message"`) contains
   **`cannot change type of node 'beta'`** and mentions the live session
   (`running`). Take a screenshot.
7. Assert the edit was **not persisted**: re-reading
   `.maestro/runs/<run_id>/pipeline.yaml` still shows `type: doc-only` for
   `beta`. Dismiss the modal and revert the type in the UI.

### Adversity 3 — session killed out-of-band → node Failed → run settles (#202/#214)

8. Kill `beta`'s and `gamma`'s sessions out-of-band (tmux crash / OOM):

   ```bash
   tmux -L maestro-<port> kill-session -t "maestro-<run_id>-beta-iter-1"
   tmux -L maestro-<port> kill-session -t "maestro-<run_id>-gamma-iter-1"
   ```

9. Trigger one detector cycle (or wait ~30 s for the background tick). Refresh
   the run view and assert:
   - `beta` and `gamma` both transition to **`failed`** (red), **not** stuck
     `running`. Each inspector shows a failure cause containing `session_died`
     and the dead session name.
10. Assert the run reaches a **clean terminal state** without manual help:
    - With `alpha` Completed (End fired) and `beta`/`gamma` Failed and no node
      live, the run does **not** sit `running` forever. It is reconciled to a
      terminal state with a **visible cause**. Over HTTP `GET /runs/<run_id>`
      reports a terminal `status` (`completed` if the End port fired before the
      failures settled, or `failed` with a `run_stalled` reason if the wedge
      reconciled first) — in **either** case it is terminal, never `running`.
11. Assert the overarching invariant: every adversity left a trace. The
    duplicate completion produced no phantom iteration; the forbidden edit left
    a save-error message and an unchanged snapshot; each killed session left a
    Failed node naming it. No incident produced a silent stall.

## Cleanup

- Kill any sessions still alive:
  `tmux -L maestro-<port> kill-session -t "maestro-<run_id>-<node>-iter-1"` for
  each of `alpha`, `beta`, `gamma`.
- Remove the worktree:
  `git worktree remove --force .maestro/runs/<run_id>/worktree`.
- Delete `.maestro/pipelines/run-of-hell-scenario.yaml` and its `.prompts/` dir
  if the agent created them in Setup.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 3: alpha completed cleanly; End result port fired",
    "step 4: duplicate node_done on alpha was a no-op — no iter-2, no downstream re-spawn, no state regression",
    "step 6: type change on running beta rejected; save-error-message contains \"cannot change type of node 'beta'\" and mentions the live session",
    "step 7: run snapshot still has type: doc-only for beta (rejected edit not persisted)",
    "step 9: beta and gamma turned Failed with session_died causes naming the dead sessions",
    "step 10: the run reached a terminal state (never left running) with a visible cause",
    "step 11: every adversity left a visible trace; no silent stall"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
