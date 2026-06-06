# Scenario — `inspector-pooled-inputs`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format below. Asserts the node inspector (#153) surfaces
> what the canvas hides: the node **ID**, the role/prompt editor, the declared
> **output port schemas**, and the **derived inputs list with pooling spelled
> out** — two same-named incoming edges show as one logical pooled input that
> lists every contributing source node (CONTEXT.md § Node; inputs are emergent,
> #149).

## Setup

- Daemon running on the user's repo (default `http://127.0.0.1:5172`).
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- A test pipeline named `inspector-pooled-scenario.yaml` seeded in
  `.maestro/pipelines/`. If it isn't already there, the agent creates it before
  driving the UI. Two reviewers each produce a `review` document; both wire into
  the single `implementer` node, so the implementer's `review` input is
  **pooled** (one logical list input fed by two sources). The implementer
  declares a `diff` output with a frontmatter schema.

  ```yaml
  # Inputs are emergent (#149): regular nodes declare OUTPUTS only. The
  # implementer's `review` input below is NOT declared — it is derived from the
  # two incoming `review` edges and pools them into one list.
  name: inspector-pooled-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: sec
      name: security-reviewer
      type: doc-only
      outputs:
        - name: review
      view: { x: 200, y: 0 }
    - id: perf
      name: perf-reviewer
      type: doc-only
      outputs:
        - name: review
      view: { x: 200, y: 200 }
    - id: impl
      name: implementer
      type: code-mutating
      outputs:
        - name: diff
          frontmatter:
            verdict:
              type: enum
              allowed: [PASS, FAIL]
            files_changed:
              type: int
      view: { x: 450, y: 100 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 700, y: 100 }
  edges:
    - source: { node: sec, port: review }
      target: { node: impl, port: review }
    - source: { node: perf, port: review }
      target: { node: impl, port: review }
  ```

## Steps the agent executes

1. Open the UI, confirm the **`Daemon: connected`** label is visible in the
   status bar.
2. Click the `inspector-pooled-scenario` row in the **Pipelines** sidebar. The
   canvas renders the nodes (`Start`, `security-reviewer`, `perf-reviewer`,
   `implementer`, `End`).
3. Click the `implementer` node. The **Node Inspector** opens on the right.
4. **ID** — assert the inspector shows the node id `impl` under the **Identity**
   section (the canvas card itself does not show the id — #149 slim card; the
   inspector does — #153).
5. **Prompt editor** — assert a `Prompt` textarea is present with placeholder
   `Enter the node's role prompt...`.
6. **Pooled input** — assert the **Inputs** section shows **one** pooled input
   named `review` (`data-testid="pooled-input-review"`), and that this single
   row lists **both** source nodes — its text contains **`security-reviewer`**
   **and** **`perf-reviewer`** (the canonical `review ← security-reviewer,
   perf-reviewer` reading). Assert there is **no** second, separate `review`
   input row — the two edges pooled into one.
7. **Inputs are read-only** — assert the pooled input row has **no** editable
   name input, no side picker, and no delete button (inputs are emergent,
   derived from edges; the inspector does not let you edit them directly).
8. **Output schema** — assert the **Outputs** section shows the `diff` output
   card (`data-testid="output-port-card-diff"`). Expand it if collapsed and
   assert its schema lists the field **`verdict`** of type **`enum`** with
   allowed values **`PASS`** and **`FAIL`**, and the field **`files_changed`**
   of type **`int`**.
9. Take a screenshot of the inspector showing the pooled input + output schema.

## Validate against the model on disk

10. Read `.maestro/pipelines/inspector-pooled-scenario.yaml` and confirm both
    edges target `impl.review` (the source of the pooling), and that `impl`
    declares the `diff` output with the `verdict` enum + `files_changed` int
    frontmatter — i.e. what the inspector rendered matches the pipeline file.

## Cleanup

- Delete `.maestro/pipelines/inspector-pooled-scenario.yaml`.
- Delete `.maestro/pipelines/inspector-pooled-scenario.prompts/` if it was
  created during the run.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: status bar shows 'Daemon: connected'",
    "step 4: inspector Identity section shows node id 'impl'",
    "step 5: Prompt textarea present with placeholder 'Enter the node's role prompt...'",
    "step 6: single pooled input 'review' lists both security-reviewer and perf-reviewer; no duplicate review row",
    "step 7: pooled input row is read-only (no name input / side picker / delete)",
    "step 8: diff output card shows verdict enum [PASS, FAIL] and files_changed int",
    "step 10: pipeline YAML on disk has both edges → impl.review and the diff frontmatter schema"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
