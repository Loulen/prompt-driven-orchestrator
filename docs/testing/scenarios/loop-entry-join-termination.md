# Scenario — `loop-entry-join-termination`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies the **canonical
> scheduler** (#210, fixing #194/#199): a pipeline whose bounded region entry
> is fed by a **join** (external feeder + back-edge) terminates cleanly in the
> nominal case, and the shape that used to **stall silently** at the loop-entry
> join now runs to completion. Also verifies that `end_region` **closes** the
> region (routes its exit or halts "ended — unrouted") instead of starting a
> phantom lap past `max_iter`.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser.
- `claude` available on `PATH`.
- A pipeline `loop-entry-join-termination.yaml` exists in `.maestro/pipelines/`.
  If it isn't already there, the agent creates it before driving the UI. This is
  the forensic run-9c8d123 shape in miniature: a doc-only feeder (`griller`)
  outside the region feeds the region entry (`impl`), and the back-edge
  `rev → impl` makes the entry a **join** (feeder + back-edge) — the exact
  precondition that stalled the old scheduler:

  ```yaml
  name: loop-entry-join-termination
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
          side: right
      view: { x: 0, y: 200 }
    - id: griller
      name: griller
      type: doc-only
      outputs:
        - name: plan
          side: right
      view: { x: 240, y: 200 }
    - id: impl
      name: implementer
      type: code-mutating
      outputs:
        - name: code
          side: right
      view: { x: 520, y: 200 }
    - id: rev
      name: reviewer
      type: doc-only
      outputs:
        - name: review
          side: right
          frontmatter:
            verdict:
              type: enum
              allowed: [PASS, APPROVED, FAIL, NEEDS_WORK]
      view: { x: 800, y: 200 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      outputs: []
      view: { x: 1080, y: 200 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: griller, port: task }
    # External feeder into the region entry — half of the loop-entry join.
    - source: { node: griller, port: plan }
      target: { node: impl, port: task }
    - source: { node: impl, port: code }
      target: { node: rev, port: code }
    # Exit edge: reviewer PASSes ⇒ leave the region for End.
    - source: { node: rev, port: review }
      target: { node: end, port: result }
      when:
        verdict: { in: [PASS, APPROVED] }
    # Continuation back-edge into the entry — the other half of the join.
    - source: { node: rev, port: review }
      target: { node: impl, port: task }
      else: true
  loops:
    - id: review_loop
      kind: bounded
      members: [impl, rev]
      max_iter: 3
  ```

  Under the pre-#210 scheduler this pipeline **stalled right after `griller`
  completed**: the entry `impl` never spawned because the back-edge
  `rev → impl` was counted as an unsatisfiable upstream precondition (zero
  events, run stuck `Running` — the forensic 9c8d123 signature).

## Steps

1. **Open the pipeline** — navigate to `http://127.0.0.1:5172` and select
   `loop-entry-join-termination`.
2. **Nominal run terminates** — create a run with a prompt that asks the
   implementer to do trivial work and the reviewer to **reject once, then
   PASS** (e.g. "reviewer: answer FAIL on iteration 1, PASS on iteration 2").
   Observe:
   - `griller` completes, then **`impl` spawns at iter 1** — no stall at the
     loop-entry join (the old scheduler never spawned it).
   - The region laps `↻ 1/3 → 2/3`; on the PASS lap the run reaches `end` and
     the Run status is **Completed**.
   - `griller` is **never re-spawned** by the laps (it stays at iter 1) and no
     member NodeRun ever shows `iter > 3`.
3. **Input resolution survives a failed iteration** — create a second run
   whose prompt asks the griller to `maestro fail` on its first iteration
   (after writing its plan), then complete normally when restarted. Restart the
   failed griller (Manager or `restart_node`). When `impl` spawns, open its
   prompt/IO (`/runs/<run>/nodes/impl/io`) and verify its `task` input path
   points at the griller's **latest completed** iteration directory (e.g.
   `griller/iter-2/plan/...`), not the failed `iter-1` artifact.
4. **`end_region` closes, never a phantom lap** — create a third run whose
   reviewer **never PASSes**. Let the region exhaust `↻ 3/3` and block
   **"exhausted — unrouted"**. From the Pipeline Manager (or
   `POST /runs/<run>/commands` with `{"kind": "end_region", "region_id":
   "review_loop"}`), end the region. Observe:
   - **No node spawns** from the command: `impl` is NOT re-spawned, no NodeRun
     appears with `iter 4`, and `griller` is not re-spawned.
   - With no matching exit edge wired, the run halts explicitly with an
     **"ended — unrouted"** (or stays in an explicit halted state) — never a
     new lap, never a silent stall.

## Verdict format

```
scenario: loop-entry-join-termination
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- The loop-entry join (external feeder + back-edge into the region entry)
  **spawns the entry** as soon as the feeder completes — the run never sits
  `Running` with zero events (#194 stall fixed).
- The nominal bounded-loop run **terminates** (`Completed`) via the PASS exit.
- Input resolution reads the **latest completed** upstream iteration: a failed
  iteration's artifact is never consumed (#194).
- The external feeder is **never re-spawned** by region laps, and no member
  NodeRun ever exceeds `max_iter` (#195/#199).
- `end_region` **closes** the region: no entry re-spawn, no `iter > max_iter`
  spawn, explicit "ended/exhausted — unrouted" halt when no exit edge matches
  (#199).
- No console errors.
