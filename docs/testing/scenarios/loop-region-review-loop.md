# Scenario — `loop-region-review-loop`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies the **bounded loop
> region** (ADR-0011 / #148) that replaces the `Loop` node: a review loop is a
> named entry of the `loops:` block (no `Switch`, no `Loop` node) drawn on the
> single, always-interactive canvas. The region renders as a translucent box with
> a `↻ X/Y` header; the back-edge and exit are conditional edges with always-
> visible midpoint pills; the loop exits early on the PASS edge and, when the
> verdict never passes, blocks **"exhausted — unrouted"** at `max_iter` (never a
> silent stall).

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser (single always-interactive canvas — there is
  no Edit/Run pencil toggle; the canvas is editable at all times per ADR-0011).
- `claude` available on `PATH`.
- A pipeline `loop-region-review-loop.yaml` exists in `.maestro/pipelines/`. If
  it isn't already there, the agent creates it before driving the UI. This is the
  bounded-region form an old `Loop`+`Switch` review loop migrates into (the body
  is a named region; routing lives on the edges):

  ```yaml
  name: loop-region-review-loop
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
    # Exit edge: reviewer PASSes ⇒ leave the region for End.
    - source: { node: rev, port: review }
      target: { node: end, port: result }
      when:
        verdict: { in: [PASS, APPROVED] }
    # Continuation back-edge: anything else ⇒ loop again at the entry (impl).
    - source: { node: rev, port: review }
      target: { node: impl, port: task }
      else: true
  loops:
    - id: review_loop
      kind: bounded
      members: [impl, rev]
      max_iter: 3
  ```

  Note there is **no `Switch` node** and **no `Loop` node** — the loop is the
  `loops:` region, and the back-edge/exit are conditional edges. The cycle
  `rev → impl` (the `else` back-edge) closes the region; the `verdict ∈ {PASS,
  APPROVED}` edge leaves it.

## Steps

1. **Open the pipeline** — navigate to `http://127.0.0.1:5172` and select
   `loop-region-review-loop` from the pipelines list. The canvas is always
   interactive (no pencil toggle).
2. **Verify the region renders** — confirm the canvas draws the bounded region:
   - A **translucent box** enclosing both members (`impl` and `rev`), since the
     region has ≥ 2 members. (A single-member region would render as a compact
     badge instead.)
   - A region **header** showing the `↻ X/Y` counter — before any run, `↻ 0/3`
     or `↻ max 3` (the cap reads 3).
   - No `Switch` node and no `Loop` node anywhere on the canvas.
3. **Verify the condition pills** — the two edges leaving `rev:review` each show
   an **always-visible midpoint pill** (no hover/select needed):
   - `rev → end`: pill reads the `verdict ∈ {PASS, APPROVED}` guard.
   - `rev → impl`: pill reads `else` (the continuation back-edge).
   - Unconditional edges (`start → impl`, `impl → rev`) show **no** pill.
4. **Create a new run** — click "New Run", select `loop-region-review-loop`,
   enter a prompt that asks the implementer to do work the reviewer will reject a
   couple of times before accepting, and start the run.
5. **Observe lap 1** — the region header advances to `↻ 1/3`. `impl` spawns and
   runs; when it completes, `rev` spawns. The member artifacts are stamped with
   `iter 1` (`.../impl/iter-1/...`, `.../rev/iter-1/...`).
6. **Observe re-entry (coalesced)** — when `rev` completes with a non-PASS
   verdict, the `else` back-edge fires and the region re-enters: header advances
   to `↻ 2/3`, `impl` re-spawns **once** at `iter 2` (a single entry-spawn per
   lap — no double-spawn even though the loop re-entered). Non-member nodes stay
   at `iter 1`.
7. **Observe early-PASS exit** — on a lap where `rev` completes with `verdict:
   PASS` (or `APPROVED`), the guarded `rev → end` edge fires, the region is left
   **before** reaching `max_iter`, and the run reaches `end` and completes. The
   header stops at the lap it exited on (e.g. `↻ 2/3`), not `3/3`.
8. **Observe `max_iter` exhaustion (separate run)** — start a second run whose
   reviewer never PASSes. The region advances `↻ 1/3 → 2/3 → 3/3`. At `iter 3`
   (`max_iter`) with the continue-condition still true and no `iter ≥ max` exit
   edge wired, the region enters the explicit **"exhausted — unrouted"** blocked
   state on the canvas (a halt the Pipeline Manager can route) — **not** a silent
   stall and **not** a silent completion. Verify the run is `Halted`/blocked with
   an "exhausted" / "unrouted" reason, not `Completed`.

## Verdict format

```
scenario: loop-region-review-loop
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- The review loop renders as a **bounded region** — translucent box (≥ 2
  members) with a `↻ X/Y` header — with **no `Switch` and no `Loop` node** on the
  canvas.
- The back-edge and exit are **conditional edges** with always-visible midpoint
  pills (`when:` for the PASS exit, `else` for the continuation back-edge);
  unconditional edges show none.
- The region's iteration counter is **region-wide**: it advances by exactly one
  per lap, the entry (`impl`) re-spawns **once** per lap (no double-spawn), and
  member artifacts are stamped with the region `iter` while non-members stay at
  `iter 1`.
- The loop **exits early** via the PASS edge before reaching `max_iter`.
- At `max_iter` with the continue-condition still true and no matching exit
  edge, the region blocks **"exhausted — unrouted"** — an explicit, diagnosable
  halt, never a silent stall and never a silent completion.
- No console errors or rendering glitches.
