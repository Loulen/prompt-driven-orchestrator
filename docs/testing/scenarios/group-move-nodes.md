# Scenario — `group-move-nodes`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent drives a real
> browser and emits the verdict format below. Asserts the #232 fix: box-/
> additive-selecting several nodes on the edit canvas and dragging them moves
> **every** selected node, persists all their positions to disk, and leaves
> un-selected nodes untouched. Pre-fix, only the grabbed node persisted — every
> other selected node snapped back, and Save silently dropped their moves.

## Setup

- Daemon running on the user's repo (default `http://127.0.0.1:5172`).
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- A test pipeline named `group-move-scenario.yaml` seeded in `.pdo/pipelines/`.
  If it isn't already there, the agent creates it before driving the UI:

  ```yaml
  # Inputs are emergent (#149): work nodes declare OUTPUTS only; inputs derive
  # from incoming edges. Only End keeps a declared `result` input (structural).
  # Three work nodes are stacked vertically with clear space so a drag never
  # overlaps a neighbour.
  name: group-move-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 300 }
    - id: alpha
      name: alpha
      type: doc-only
      outputs:
        - name: out
      view: { x: 240, y: 100 }
    - id: beta
      name: beta
      type: doc-only
      outputs:
        - name: out
      view: { x: 240, y: 320 }
    - id: gamma
      name: gamma
      type: doc-only
      outputs:
        - name: out
      view: { x: 240, y: 540 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 520, y: 300 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: alpha, port: out }
    - source: { node: alpha, port: out }
      target: { node: beta, port: out }
    - source: { node: beta, port: out }
      target: { node: gamma, port: out }
    - source: { node: gamma, port: out }
      target: { node: end, port: result }
  ```

## Steps the agent executes — group move

1. Open the UI, confirm the **`Daemon: connected`** label is visible in the
   status bar.
2. Click the `group-move-scenario` row in the **Library** sidebar. The canvas
   renders five nodes (`Start`, `alpha`, `beta`, `gamma`, `End`).
3. Record the on-canvas position of `alpha`, `beta`, and `gamma` (read each
   `.react-flow__node[data-id="…"]`'s `transform: translate(Xpx, Ypx)` — the
   translate values are FLOW units and mirror the stored `view`). Call these
   `alpha0`, `beta0`, `gamma0`.
4. **Multi-select `alpha` and `beta`.** Either:
   - **Additive click:** click `alpha`, then hold **Control** (Linux/Windows;
     **Meta/⌘** on macOS) and click `beta`. Both should now show the green
     selection ring; **or**
   - **Box-select:** hold **Shift** and drag a marquee that encloses only
     `alpha` and `beta` (not `gamma`).
5. Assert **both `alpha` and `beta` show the accent selection ring** during the
   selection (the ring lights on every selected node, not just the
   last-clicked one — the #232 highlight fix). `gamma` shows **no** ring.
6. **Drag the group:** grab `beta` and drag it by a clear delta (e.g. ~+170px
   x, ~-60px y on screen). Use an incremental drag (several small mouse moves
   with a short wait between, then release) — xyflow's d3-drag ignores a single
   atomic move. `alpha` should visibly ride along with `beta`.
7. After releasing, record the new on-canvas positions `alpha1`, `beta1`,
   `gamma1`. Assert:
   - `beta` moved: `beta1 ≠ beta0`.
   - **`alpha` moved by the same delta as `beta`** (`alpha1 - alpha0 ≈
     beta1 - beta0`). This is the core regression: pre-fix `alpha` snapped back
     to `alpha0` after the drop.
   - `gamma` did **not** move (`gamma1 ≈ gamma0`).
8. Assert the tab title shows the dirty indicator **`•`** and the **Save**
   button is **enabled**.
9. Click **Save** (or press `Ctrl+S` / `Cmd+S`). Assert the dirty indicator
   clears and **"Saved …"** appears (`data-testid="saved-ago"`).
10. Reload the page (`F5` / `navigate_page`). Re-open `group-move-scenario`.
11. Read `.pdo/pipelines/group-move-scenario.yaml` from disk. Assert:
    - `alpha`'s `view:` and `beta`'s `view:` both shifted from the seed by the
      **same delta** (the flow-unit delta of the drag; allow ±1 for rounding).
    - `gamma`'s `view:` is **unchanged** from the seed (`{ x: 240, y: 540 }`).
    - This proves every selected node's move was persisted, not just the
      grabbed one — pre-fix only `beta`'s `view:` would have changed.

## Negative control — single-node drag (no regression)

12. With the pipeline still open, click `gamma` alone (no modifier) so only
    `gamma` is selected (the ring lights on `gamma`, clears on `alpha`/`beta`).
13. Drag `gamma` by a clear delta. Release.
14. Assert `gamma` moved and **neither `alpha` nor `beta` moved**.
15. Click **Save**. Reload. Read the YAML and assert only `gamma`'s `view:`
    changed from step 11's state; `alpha`/`beta` are untouched. A single drag
    still persists exactly one node — the fix didn't widen the blast radius.

## Cleanup

- Delete `.pdo/pipelines/group-move-scenario.yaml`.
- Delete `.pdo/pipelines/group-move-scenario.prompts/` if it was created.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: status bar shows 'Daemon: connected'",
    "step 5: both alpha and beta show the selection ring; gamma does not",
    "step 7: alpha moved by the same delta as beta; gamma did not move",
    "step 8: tab shows dirty indicator '•', Save button enabled",
    "step 9: dirty indicator cleared, 'Saved …' visible",
    "step 11: on disk, alpha.view and beta.view shifted by the same delta; gamma.view unchanged",
    "step 14: single-node drag moved only gamma; alpha/beta unmoved",
    "step 15: on disk, only gamma.view changed on the single drag"
  ],
  "anomalies": [
    "<optional — e.g. external-edit conflict dialog surfaced on save during a live run>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
