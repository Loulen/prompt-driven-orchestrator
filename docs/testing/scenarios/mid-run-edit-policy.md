# Scenario — `mid-run-edit-policy`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format below. Asserts the #211 / #206 input-validation
> pair: (1) launching a run on a pipeline with a dangling port is refused with
> an explicit error and no run is created; (2) mid-run, a dangerous edit
> (changing the type of a running node) is rejected with a **visible message
> that says why** (ADR-0007 amended), while a safe edit in the **same run**
> (adding a node + edge, editing an unspawned node's prompt) applies normally.

## Setup

- Daemon running on the user's repo (default `http://127.0.0.1:5172`).
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- A test pipeline named `mid-run-edit-scenario.yaml` seeded in
  `.pdo/pipelines/`. If it isn't already there, the agent creates it
  before driving the UI:

  ```yaml
  name: mid-run-edit-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: worker
      name: worker
      type: doc-only
      outputs:
        - name: result
      view: { x: 200, y: 100 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 600, y: 100 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: worker, port: task }
  ```

- Seed `.pdo/pipelines/mid-run-edit-scenario.prompts/worker.md` with a
  prompt that keeps the session busy long enough to edit mid-run, e.g.
  `Wait for further instructions. Do not call pdo complete.`

## Part 1 — launch refusal on a dangling port (#211)

1. Copy the pipeline to `.pdo/pipelines/dangling-port-scenario.yaml`,
   renaming `name:` to `dangling-port-scenario` and changing the first edge's
   source port to a typo: `source: { node: start, port: user_promppt }`.
2. Open the UI, confirm **`Daemon: connected`**, and launch a run on
   `dangling-port-scenario` with input `dangling launch test`.
3. Assert the launch **fails** and the UI surfaces an error message that names
   the dangling reference — it must contain both **`user_promppt`** and the
   edge's nodes (`start`, `worker`). No run may appear in the Runs list for
   `dangling-port-scenario`. Take a screenshot.
4. Cross-check over HTTP: `POST /runs` with
   `{"pipeline": "dangling-port-scenario", "input": "x"}` returns **400** and
   an `error` containing `dangling edge reference`.

## Part 2 — dangerous edit mid-run is rejected with a visible message

5. Launch a run on `mid-run-edit-scenario` with input `mid-run edit test`.
   Wait until the `worker` node shows status **running** on the canvas.
6. Click the `worker` node to open the **Node Inspector**. Switch its type
   from **doc-only** to **code-mutating** and save (`Ctrl+S` / Save button).
7. Assert the save is **rejected with a visible message**: the save-error
   modal (`data-testid="save-error-modal"`) appears and its message
   (`data-testid="save-error-message"`) contains
   **`cannot change type of node 'worker'`** and mentions the live session
   (`running`). Take a screenshot.
8. Dismiss the modal. Revert the type to **doc-only** (the rejected edit must
   not have been persisted: re-reading
   `.pdo/runs/<run-id>/pipeline.yaml` still shows `type: doc-only` for
   `worker`).

## Part 3 — safe edit in the same run applies normally

9. With the same run still live, add a new node `reviewer` (doc-only) on the
   canvas and draw an edge `worker.result → reviewer` (drop on body — the
   emergent input lands on the card, #149).
10. In the `reviewer` inspector, type the prompt
    `MARKER_<timestamp>_SAFE_EDIT` (reviewer is not spawned yet — prompt edit
    of an unspawned node is a safe edit).
11. Save. Assert the save **succeeds**: dirty indicator `•` clears, no
    save-error modal appears.
12. Assert persistence: `.pdo/runs/<run-id>/pipeline.yaml` now contains
    the `reviewer` node and the `worker → reviewer` edge, and
    `.pdo/runs/<run-id>/prompts/reviewer.md` equals the marker. The
    template `.pdo/pipelines/mid-run-edit-scenario.yaml` also contains
    `reviewer` (auto-sync montant, ADR-0007).
13. Complete or stop the run (e.g. stop the `worker` node) so cleanup can
    proceed.

## Cleanup

- Stop/cleanup the run launched in Part 2 if still live.
- Delete `.pdo/pipelines/dangling-port-scenario.yaml`.
- Delete `.pdo/pipelines/mid-run-edit-scenario.yaml` and
  `.pdo/pipelines/mid-run-edit-scenario.prompts/`.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 3: launch on dangling-port-scenario refused; UI error names 'user_promppt' and the edge; no run created",
    "step 4: POST /runs returns 400 with 'dangling edge reference'",
    "step 7: type change on running worker rejected; save-error-message contains \"cannot change type of node 'worker'\" and mentions the live session",
    "step 8: run snapshot still has type: doc-only for worker",
    "step 11: safe edit (add reviewer + edge + prompt) saved without error in the same run",
    "step 12: run snapshot + template contain reviewer; reviewer.md equals marker"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
