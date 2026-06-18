# Scenario — `collection-fanout-merge`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies the **collection
> loop region** (ADR-0011 / #151) that replaces the `ForEach` node: a region is a
> named entry of the `loops:` block with `kind: collection` + `over: <field>`
> naming a list in the entering artifact's frontmatter (no `ForEach` node). The
> region fans the member out **in parallel**, one lap per item; the region's
> outgoing edges fire **once, on the barrier** — when every item finishes —
> preserving `done → Merge` convergence (ADR-0006). A single-member collection
> renders as a compact badge `⇉ N items` on the member's card.

## Setup

- PDO daemon running on the user's repo (`pdo daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser (single always-interactive canvas — there is
  no Edit/Run pencil toggle; the canvas is editable at all times per ADR-0011).
- `claude` available on `PATH`.
- A pipeline `collection-fanout-merge.yaml` exists in `.pdo/pipelines/`. If
  it isn't already there, the agent creates it before driving the UI. This is the
  collection-region form an old `ForEach` fan-out migrates into (the body is a
  single-member named region; the barrier converges via `Merge`):

  ```yaml
  name: collection-fanout-merge
  version: "1.0"
  nodes:
    - id: triage
      name: triage
      type: doc-only
      outputs:
        - name: plan
          side: right
          frontmatter:
            issues:
              type: list
      view: { x: 0, y: 200 }
    - id: fixer
      name: fixer
      type: code-mutating
      outputs:
        - name: fix
          side: right
      view: { x: 320, y: 200 }
    - id: merge
      name: merge
      type: merge
      inputs:
        - name: branches
          side: left
          repeated: true
      outputs:
        - name: merged
          side: right
      view: { x: 640, y: 200 }
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
          side: right
      view: { x: -300, y: 200 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      view: { x: 940, y: 200 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: triage, port: task }
    # Entering edge: triage's list-typed `plan` feeds the collection member.
    - source: { node: triage, port: plan }
      target: { node: fixer, port: in }
    # Barrier edge: when every item's fixer finishes, ONE edge fires into Merge.
    - source: { node: fixer, port: fix }
      target: { node: merge, port: branches }
    - source: { node: merge, port: merged }
      target: { node: end, port: result }
  loops:
    - id: per-issue
      kind: collection
      over: issues
      members: [fixer]
  ```

  Note there is **no `ForEach` node** — the fan-out is the `loops:` region with
  `kind: collection` and `over: issues`. The single member `fixer` is
  code-mutating, so each item forks its own sub-worktree and the parallel laps
  converge through the `Merge` node (ADR-0006).

## Steps

1. **Open the pipeline** — navigate to `http://127.0.0.1:5172` and select
   `collection-fanout-merge` from the pipelines list. The canvas is always
   interactive (no pencil toggle).
2. **Verify the collection badge** — confirm the canvas draws the collection
   region as a **compact badge** on the `fixer` card (single member), reading
   `⇉ N items` (before a run the cap is unknown, so it reads `⇉ items` or
   `⇉ over issues`). It is the `⇉` glyph (fan-out), **not** the `↻` loop glyph,
   and **not** a `ForEach` node anywhere on the canvas.
3. **Create a new run** — click "New Run", select `collection-fanout-merge`,
   enter a prompt that makes the triage node emit a list of two or three issues
   in its `plan` frontmatter (`issues: [...]`), and start the run.
4. **Observe the fan-out** — `triage` runs first; when it completes, the
   collection region resolves `over: issues` from `triage`'s `plan` frontmatter
   and fans `fixer` out **in parallel**, one lap per item. The badge updates to
   `⇉ N items` where `N` is the number of issues. Each lap's `fixer` artifact is
   stamped with its item index (`.../fixer/iter-1/...`, `.../fixer/iter-2/...`,
   `...`); because `fixer` is code-mutating, each lap runs in its **own
   sub-worktree**.
5. **Observe the barrier into Merge** — the region's outgoing edge
   (`fixer:fix → merge:branches`) does **not** fire per item. It fires **once**,
   when **all** items have finished (the barrier). At that point the `Merge` node
   spawns with one pooled `branches` input per item and resolves the parallel
   code changes (ADR-0006).
6. **Observe completion** — after `Merge` completes, `merge:merged → end:result`
   fires and the run reaches `end` and **completes**. No item-lap is left
   `running`, and the run is `Completed` (not `Halted`, not stalled).
7. **(Optional) Empty collection** — start a second run whose triage emits an
   **empty** `issues: []`. The barrier fires **immediately** with zero
   item-artifacts: `fixer` never spawns, `Merge` resolves an empty fan-out, and
   the run still reaches `end` and completes (never a silent stall).

## Verdict format

```
scenario: collection-fanout-merge
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- The fan-out renders as a **collection region** — a single-member compact badge
  `⇉ N items` on the `fixer` card — with **no `ForEach` node** on the canvas.
- The region resolves `over: issues` from the entering artifact and fans the
  member out **in parallel**, one lap per item, each lap stamped with its item
  index; code-mutating laps each fork a **sub-worktree**.
- The region's outgoing edge is a **barrier**: it fires **once**, when every item
  finishes, into the `Merge` node — never once per item.
- The parallel branches **converge through `Merge`** (ADR-0006) and the run
  reaches `end` and **completes**.
- An **empty** collection fires the barrier **immediately** (zero item-artifacts)
  and the run still completes — never a silent stall.
- No console errors or rendering glitches.
