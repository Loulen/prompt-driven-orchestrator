# Scenario — `process-lifecycle-resilience`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Asserts issue #213
> (Pérennisation C): killing a NodeRun's tmux session mid-run surfaces a
> **Failed** node with a cause naming the session, the rest of the run settles
> cleanly, AND a nominal run is **never** disturbed by the detector/reaper. Also
> spot-checks reap-on-terminal (#205): a completed node's session is gone but its
> pane is still inspectable from a persisted snapshot.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172` (the live dev daemon uses `6172`).
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- `claude` available on `PATH`.
- A two-node pipeline `lifecycle-resilience-scenario.yaml` in
  `.maestro/pipelines/`. If absent, the agent creates it before driving the UI:

  ```yaml
  name: lifecycle-resilience-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: slow
      name: slow
      type: doc-only
      outputs:
        - name: out
          side: top
      view: { x: 200, y: 100 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      view: { x: 400, y: 100 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: slow, port: user_prompt }
    - source: { node: slow, port: out }
      target: { node: end, port: result }
  ```

  And `.maestro/pipelines/lifecycle-resilience-scenario.prompts/slow.md`:

  ```
  Sleep for 5 minutes (run the shell command `sleep 300`) and only THEN reply
  with `MAESTRO_LIFECYCLE_OK` and call `maestro complete`. Do nothing else in
  the meantime. This long delay gives the operator time to kill your session.
  ```

## Part A — kill a session mid-run → node Failed with cause

1. Open the UI; confirm **`Daemon: connected`**.
2. Launch a run of `lifecycle-resilience-scenario` with any input (e.g.
   `resilience`). Capture the `run_id`. Within ~2 s the `slow` node animates to
   **`running`**.
3. From a shell, confirm the session exists:

   ```bash
   tmux ls | grep "maestro-<run_id>-slow-iter-1"
   ```

4. Kill the session out-of-band (models a tmux/OOM crash, #202):

   ```bash
   tmux kill-session -t "maestro-<run_id>-slow-iter-1"
   ```

5. Wait up to one detector cycle (~30 s; the live daemon's stale detector runs
   on a 30 s tick). Refresh the run view. Assert:
   - The `slow` node transitions to **`failed`** (red), **not** stuck on
     `running`.
   - Selecting the node, the inspector shows a **failure cause** whose text
     **names the dead session** — it contains
     `session_died` and the string `maestro-<run_id>-slow-iter-1`.
6. Assert the run **settles cleanly**: it does not stay perpetually `running`
   with a phantom live node. The Failed `slow` node no longer holds an admission
   slot (see Part C). The End node's `result` port is **not** marked received
   (the run did not falsely complete).

## Part B — a nominal run is never disturbed

1. Launch a **second** run of the same pipeline (input `nominal`). Capture
   `run_id_2`. Do **not** touch its session.
2. Leave it running across **at least two** detector cycles (~70 s). The `slow`
   node prompt keeps the session alive (`sleep 300`).
3. Assert throughout:
   - `run_id_2`'s `slow` node stays **`running`** the entire time — the detector
     never marks a live-session node Failed (no false positive).
   - Its tmux session `maestro-<run_id_2>-slow-iter-1` stays alive (verify with
     `tmux ls`).
4. Unblock it: from the pane, interrupt the sleep and let the agent reply, OR
   write the output and Mark complete:

   ```bash
   mkdir -p .maestro/runs/<run_id_2>/worktree/.maestro/artifacts/slow/iter-1
   echo '# Out' > .maestro/runs/<run_id_2>/worktree/.maestro/artifacts/slow/iter-1/out.md
   ```

   Then click **Mark complete**. Assert the node reads **`completed`** and the
   run completes normally — exactly as a run with no detector interference would.

### Reap-on-terminal spot-check (#205)

5. After `run_id_2`'s `slow` node is **completed**, assert:
   - Its session is **gone promptly** (not lingering for the 1 h TTL):

     ```bash
     tmux has-session -t "maestro-<run_id_2>-slow-iter-1"; echo "exit=$?"
     # exit=1 → session correctly reaped on completion
     ```

   - A pane snapshot was persisted for post-mortem inspection:

     ```bash
     ls .maestro/runs/<run_id_2>/nodes/slow/pane-iter-1.snapshot
     ```

   - The terminal preview in the UI for the completed node still renders content
     (served from the snapshot). The `/pane` response reports
     `"source": "snapshot"`:

     ```bash
     curl -s "http://127.0.0.1:5172/runs/<run_id_2>/nodes/slow/pane?iter=1" | grep -o '"source":"[a-z]*"'
     # "source":"snapshot"
     ```

## Part C — slot accounting recovers (admission)

1. After Part A's `slow` node is Failed, its admission slot is freed. Confirm a
   fresh run is still admitted normally: launch a third run; its entry node
   reaches `running` (not stuck in `waiting`), proving the Failed zombie no
   longer burns a slot (#202).

## Cleanup

- Kill any sessions still alive:
  `tmux kill-session -t maestro-<run_id>-slow-iter-1` (and the others).
- Remove worktrees:
  `git worktree remove --force .maestro/runs/<run_id>/worktree` per run.
- Delete `.maestro/pipelines/lifecycle-resilience-scenario.yaml` and its
  `.prompts/` dir if the agent created them in Setup.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "A4: session killed out-of-band",
    "A5: 'slow' node turned Failed within one detector cycle; cause names the session",
    "A6: run settled cleanly; End result not falsely received",
    "B3: nominal run's 'slow' node stayed running across two detector cycles (no false positive)",
    "B4: nominal run completed normally",
    "B5: completed node's session reaped promptly; pane snapshot persisted; /pane source=snapshot",
    "C1: fresh run admitted after the zombie slot freed"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
