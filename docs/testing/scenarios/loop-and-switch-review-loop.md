# Scenario — `loop-and-switch-review-loop`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies that the Loop node
> iterates correctly, respects `max_iter`, handles `break` signals, and
> integrates with Switch for branch-within-loop patterns.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser.
- `claude` available on `PATH`.
- A pipeline `review-loop-scenario.yaml` exists in `.maestro/pipelines/`. If it
  isn't already there, the agent creates it before driving the UI:

  ```yaml
  name: review-loop-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
      view: { x: 0, y: 200 }
    - id: loop1
      name: review-loop
      type: loop
      inputs:
        - name: in
          side: left
        - name: break
          side: left
      outputs:
        - name: body
          side: right
        - name: done
          side: right
      max_iter: 3
      view: { x: 250, y: 200 }
    - id: impl1
      name: implementer
      type: code-mutating
      inputs:
        - name: in
          side: left
      outputs:
        - name: out
          side: right
      view: { x: 500, y: 150 }
    - id: reviewer
      name: reviewer
      type: doc-only
      inputs:
        - name: in
          side: left
      outputs:
        - name: out
          side: right
      view: { x: 750, y: 150 }
    - id: sw1
      name: quality-gate
      type: switch
      inputs:
        - name: in
          side: left
      outputs:
        - name: pass
          side: right
          when: "$review_verdict == 'pass'"
        - name: fail
          side: bottom
      view: { x: 1000, y: 150 }
    - id: end
      name: End
      type: end
      inputs:
        - name: in
          side: left
      outputs: []
      view: { x: 1250, y: 200 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: loop1, port: in }
    - source: { node: loop1, port: body }
      target: { node: impl1, port: in }
    - source: { node: impl1, port: out }
      target: { node: reviewer, port: in }
    - source: { node: reviewer, port: out }
      target: { node: sw1, port: in }
    - source: { node: sw1, port: pass }
      target: { node: loop1, port: break }
    - source: { node: sw1, port: fail }
      target: { node: loop1, port: in }
    - source: { node: loop1, port: done }
      target: { node: end, port: in }
  ```

## Steps

1. **Open Edit mode** — click the pencil icon in the top bar.
2. **Load the pipeline** — select `review-loop-scenario` from the pipelines
   list. Verify:
   - The Loop node renders with the loop icon (↻) and a blue left border.
   - The iter badge reads **"↻ max 3"**.
   - The Switch node renders with its branch ports.
3. **Inspect the Loop node** — click the Loop node. The right panel should show
   **Loop Inspector** with:
   - Identity section: ID = `loop1`, display name = `review-loop`.
   - Configuration section: `max_iter` input showing `3`.
   - Ports section: 4 fixed ports (`in`, `break`, `body`, `done`) with
     `SidePicker` controls.
   - Help text about fixed port names.
4. **Edit max_iter** — change `max_iter` to `5`, press Enter. Verify the iter
   badge on the canvas updates to **"↻ max 5"**. Reset back to `3`.
5. **Switch to Run mode** — click the pencil icon to exit edit mode.
6. **Create a new run** — click "New Run", select `review-loop-scenario`, enter
   a prompt, and start the run.
7. **Observe iteration 1** — the DAG canvas should show:
   - The Loop node with status `running` and iter badge **"↻ 1/3"**.
   - `impl1` node spawned and running.
   - After `impl1` completes, `reviewer` spawned.
   - After `reviewer` completes, `sw1` evaluates.
8. **Observe routing** — depending on the `$review_verdict` variable:
   - If **fail**: `sw1` routes to `loop1:in`, triggering iteration 2. The iter
     badge should update to **"↻ 2/3"**.
   - If **pass**: `sw1` routes to `loop1:break`, firing `LoopBreakReceived` then
     `LoopDone`, and the `end` node completes the run.
9. **Observe max_iter cap** — if the loop reaches iteration 3 without a break,
   verify:
   - `LoopMaxReached` event emitted.
   - `LoopDone` event emitted.
   - The `done` port fires, spawning downstream nodes toward `end`.
   - The run completes.

## Verdict format

```
scenario: loop-and-switch-review-loop
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- Loop node renders correctly in both edit and run modes.
- Loop Inspector shows correct fields and accepts edits.
- Loop iterates body subgraph (impl1 → reviewer → sw1) on each cycle.
- Break signal from switch terminates the loop early.
- max_iter cap terminates the loop when reached.
- Run completes via the `done` → `end` edge after loop termination.
- No console errors or rendering glitches.
