# Scenario — `region-create-and-destroy`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies the **region
> inspector**, **live `max_iter` edit**, and **destroy-loop confirmation**
> (ADR-0011 / #150) on top of the bounded loop region (#148). Closing a cycle
> materializes a bounded region; clicking the region header opens an inspector
> and the `max_iter` bound is editable from both the header and the inspector,
> applying live to a running region; deleting the edge that removes the region's
> **last** cycle pops a confirmation popup, and on confirm the `loops:` entry
> (with its bound and iteration state) is removed. Deleting a non-last cycle edge
> pops nothing.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser (single always-interactive canvas — there is
  no Edit/Run pencil toggle; the canvas is editable at all times per ADR-0011).
- `claude` available on `PATH`.
- A pipeline `region-create-and-destroy.yaml` exists in `.maestro/pipelines/`. If
  it isn't already there, the agent creates it before driving the UI. It starts
  **without** a `loops:` block and **without** the back-edge — the agent draws
  the back-edge in the UI so the region auto-materializes (the create half):

  ```yaml
  name: region-create-and-destroy
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
    # Exit edge: reviewer PASSes ⇒ leave the (about-to-exist) region for End.
    - source: { node: rev, port: review }
      target: { node: end, port: result }
      when:
        verdict: { in: [PASS, APPROVED] }
  ```

  Note there is **no `loops:` block yet** and **no back-edge** (`rev → impl`).
  The forward chain `start → impl → rev → end` is acyclic, so no region exists at
  open time.

## Steps

1. **Open the pipeline** — navigate to `http://127.0.0.1:5172` and select
   `region-create-and-destroy` from the pipelines list. The canvas is always
   interactive (no pencil toggle). Confirm there is **no** region box and **no**
   `↻` header on the canvas yet.
2. **Close a cycle → a bounded region appears** — draw an edge from
   `rev` (output `review`) onto the `impl` node body (the continuation back-edge
   `rev → impl`). The drawn cycle auto-materializes a **bounded loop region**:
   - A **translucent box** appears enclosing both members (`impl` and `rev`),
     since the region has ≥ 2 members.
   - A region **header** appears showing the `↻` glyph, a `max` label, and the
     editable bound (the default `max_iter` is **5**), plus the generated region
     id (a `loop-<hash>` slug).
   - No region existed before this edge; exactly one exists now.
3. **Click the region header → inspector opens** — click the region header. The
   right panel opens the **region inspector**, showing the region id, its kind
   (`bounded loop region`), the editable `max_iter` field, and the member list
   (`implementer`, `reviewer`).
4. **Edit `max_iter` from the header** — in the region header, change the bound
   from `5` to `3`. The header now reads `↻ max 3`. Confirm the inspector's
   `max_iter` field also reads `3` (header and inspector are the same bound) and
   that the canvas marks the pipeline dirty / the change persists on save (the
   serialized `loops:` entry carries `max_iter: 3`).
5. **Edit `max_iter` from the inspector** — change the inspector's `max_iter`
   field to `4`. The header counter follows to `↻ max 4`. The edit round-trips
   the same `loops:` entry (header and inspector edit one source).
6. **Live edit applies to a running region** — set `max_iter` to `2`, then start
   a run whose reviewer never PASSes. Watch the region exhaust at `↻ 2/2`. While
   the run is **still live** (e.g. just after lap 1, header at `↻ 1/2`), open the
   region inspector and raise `max_iter` to `4` (you may not lower it below the
   current lap). Save the run-scoped edit. The running region now respects the
   new bound — it continues past iteration 2 (equivalent to `extend_cycle`),
   advancing toward `↻ .../4` instead of exhausting at 2. (Lowering the bound
   below the current lap is rejected by the daemon — a consistency guard, not a
   prescriptive block.)
7. **Delete a non-last cycle edge → no popup** *(only if a second cycle is
   present; skip if the loop has a single cycle)*. With just the single back-edge
   this loop has one cycle, so this sub-step is a no-op here — the next step
   exercises the last-cycle case directly. (If a second back-edge had been drawn,
   deleting one would leave the loop intact with **no** confirmation popup.)
8. **Delete the last cycle edge → confirmation popup** — right-click the
   `rev → impl` back-edge and choose **Delete edge**. Because this back-edge is
   the region's **last** cycle, a **confirmation popup** appears warning that it
   will **destroy** the loop, naming the region id (e.g. "destroy this loop —
   `loop-<hash>`").
   - **Cancel** the popup first: confirm **nothing** changes — the edge, the
     region box, and the `↻` header are all still present.
   - Trigger the delete again and **confirm**: the back-edge is removed, the
     region box and `↻` header disappear, and the `loops:` entry (with its bound
     and iteration state) is gone. The graph is back to the acyclic
     `start → impl → rev → end` chain.

## Verdict format

```
scenario: region-create-and-destroy
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- Drawing the `rev → impl` back-edge **auto-materializes** a bounded region
  (translucent box + `↻` header with the default `max_iter` 5) where none existed
  before.
- Clicking the region header opens a **region inspector** showing the region id,
  kind, members, and an editable `max_iter`.
- `max_iter` is editable from **both** the header and the inspector, and the two
  edit a **single** bound (a change in one is reflected in the other and in the
  serialized `loops:` entry).
- Raising `max_iter` on a **running** region applies **live** (extends the loop);
  the daemon rejects only a lower-than-current-lap value (consistency guard).
- Deleting the back-edge that removes the region's **last** cycle pops a
  **confirmation** naming the loop; **cancel** leaves everything unchanged;
  **confirm** removes the `loops:` entry (bound + iteration state) along with the
  edge. Deleting a non-last cycle edge would pop **nothing**.
- No console errors or rendering glitches.
```
