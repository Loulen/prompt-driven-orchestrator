# Scenario — `manager-unstick-loop`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies that the Pipeline
> Manager can **route a loop region by id** to unstick a stalled run (issue #152,
> PRD #143, ADR-0011 § *Loops*, design screen 04):
>
> - A run is driven into a bounded region's **"exhausted — unrouted"** blocked
>   state (the explicit halt from `loop-region-review-loop`, never a silent
>   stall).
> - The run overlay on the exhausted-unrouted region offers a **"route from
>   manager"** affordance.
> - Via the manager, the region is **ended** (fire its completion) **or bumped**
>   (run N more iterations) **by region id** — emitting the corresponding
>   control-flow event into the log.
> - `resume_run` continues the run: after `end_region` the run leaves the region
>   and proceeds to `End`; after `bump_region` the region runs the extra laps.
> - The whole recovery happens without restarting the daemon or editing the
>   database.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser (single always-interactive canvas — no
  Edit/Run pencil toggle, per ADR-0011).
- `claude` available on `PATH`.
- A pipeline `manager-unstick-loop.yaml` exists in `.maestro/pipelines/`. If it
  isn't already there, the agent creates it before driving the UI. It is the
  same bounded-region review loop as `loop-region-review-loop`, deliberately
  wired with **no `iter ≥ max` exit edge**, so reaching `max_iter` blocks
  "exhausted — unrouted" — the state the manager must route:

  ```yaml
  name: manager-unstick-loop
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
    - id: impl
      name: implementer
      type: code-mutating
      outputs:
        - name: code
          side: right
      view: { x: 280, y: 200 }
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
      view: { x: 560, y: 200 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      outputs: []
      view: { x: 880, y: 200 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: impl, port: task }
    - source: { node: impl, port: code }
      target: { node: rev, port: code }
    # Exit edge: reviewer PASSes => leave the region for End.
    - source: { node: rev, port: review }
      target: { node: end, port: result }
      when:
        verdict: { in: [PASS, APPROVED] }
    # Continuation back-edge: anything else => loop again at the entry (impl).
    - source: { node: rev, port: review }
      target: { node: impl, port: task }
      else: true
  loops:
    - id: review_loop
      kind: bounded
      members: [impl, rev]
      max_iter: 2
  ```

  `max_iter` is **2** so the region exhausts quickly. There is **no `iter ≥ max`
  exit edge** — at `iter 2` the continue-condition is still true and nothing
  routes the exit, so the region blocks "exhausted — unrouted".

## Steps

1. **Open the pipeline** — navigate to `http://127.0.0.1:5172` and select
   `manager-unstick-loop`. The region `review_loop` renders as a translucent box
   (members `impl`, `rev`) with a `↻ max 2` header.
2. **Create a run that never PASSes** — click "New Run", select
   `manager-unstick-loop`, enter a prompt that asks the implementer to do work
   the reviewer will **always reject** (never PASS/APPROVED), and start the run.
3. **Drive to exhaustion** — let the region iterate `↻ 1/2 → 2/2`. On lap 2 the
   reviewer again returns a non-PASS verdict; with no `iter ≥ max` exit edge, the
   region enters the explicit **"exhausted — unrouted"** blocked state. Verify:
   - The region box shows the `exhausted — unrouted` block (blocked accent).
   - The run is `Halted`/blocked with an "exhausted" / "unrouted" reason — **not**
     `Completed`, **not** a silent stall.
4. **See the manager affordance** — the run overlay on the exhausted-unrouted
   region offers a **"route from manager"** affordance (button on the region's
   blocked overlay). Take a screenshot.
5. **Route the region by id (end it)** — trigger "route from manager" and choose
   **end** the region. This issues an `end_region` command for region id
   `review_loop` and a `resume_run`. Confirm via the run's event log
   (`curl http://127.0.0.1:5172/runs/<run_id>/events`, or the events panel) that:
   - A `command_issued` event with `command: end_region` and
     `region_id: review_loop` was appended.
   - A `resume_run` followed (the run leaves `Halted`).
6. **Run proceeds** — after ending the region, the run **leaves the region** and
   reaches `end`, completing (or advances to whatever follows the region). The
   region box no longer shows "exhausted — unrouted"; the run status is no longer
   blocked. **No daemon restart and no DB edit were needed.**
7. **(Variant) Bump instead of end** — in a fresh run driven to the same
   exhausted-unrouted block, choose **bump** (run N more iterations, e.g. +2)
   instead. Confirm a `command_issued` event with `command: bump_region`,
   `region_id: review_loop`, `additional_iter: 2`, followed by `resume_run`; the
   region resumes iterating (`↻ 3/4 …`) instead of staying blocked.

## Verdict format

```
scenario: manager-unstick-loop
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- A run reaches the explicit **"exhausted — unrouted"** blocked state on a
  bounded region (never a silent stall, never a silent completion).
- The run overlay on the exhausted-unrouted region offers a **"route from
  manager"** affordance.
- Routing the region **by id** from the manager emits the corresponding
  control-flow event into the log: `end_region` (fire completion) or
  `bump_region` (run N more iterations), each carrying `region_id`.
- `resume_run` continues the run after the route: ending leaves the region and
  proceeds; bumping resumes iterating — without restarting the daemon or editing
  the database.
- No console errors or rendering glitches.
